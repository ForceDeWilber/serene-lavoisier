pub mod sniper_runner;
pub use sniper_runner::*;

use crate::execution::ExecutionClient;
use crate::model::{MarketTick, Order, OrderSide, Symbol};
use crate::risk::{CentralRiskEngine, RiskError};
use crate::simulator::PaperExecutionSimulator;
use crate::strategy::{GeometricGridStrategy, GridConfig};
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::{broadcast, watch};
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct RunnerTuningUpdate {
    pub paused: bool,
    pub step_pct: Option<Decimal>,
    pub rebalance_threshold_pct: Option<Decimal>,
    pub order_size_fiat: Option<Decimal>,
    pub dynamic_pricing_enabled: Option<bool>,
    pub inventory_gamma: Option<Decimal>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RunnerTelemetry {
    pub runner_id: String,
    pub symbol: Symbol,
    pub center_price: Option<Decimal>,
    pub effective_center: Option<Decimal>,
    pub dynamic_step_pct: Decimal,
    pub rolling_volatility_pct: Decimal,
    pub inventory_base: Decimal,
    pub realized_pnl: Decimal,
    pub total_trades: usize,
    pub active_orders_count: usize,
    pub is_paused: bool,
}

pub struct GridRunner {
    pub runner_id: String,
    pub symbol: Symbol,
    strategy: GeometricGridStrategy,
    risk_engine: Arc<CentralRiskEngine>,
    execution_client: Arc<dyn ExecutionClient>,
    simulator: Option<Arc<PaperExecutionSimulator>>,
    tick_rx: broadcast::Receiver<MarketTick>,
    tuning_rx: watch::Receiver<RunnerTuningUpdate>,
    is_paused: bool,
    db: Option<Arc<crate::db::DbStore>>,
    telemetry_tx: Option<watch::Sender<RunnerTelemetry>>,
    brain: Option<Arc<crate::brain::EngineBrain>>,
    last_init_attempt: Option<tokio::time::Instant>,
}

impl GridRunner {
    pub fn new(
        config: GridConfig,
        risk_engine: Arc<CentralRiskEngine>,
        execution_client: Arc<dyn ExecutionClient>,
        simulator: Option<Arc<PaperExecutionSimulator>>,
        tick_rx: broadcast::Receiver<MarketTick>,
        tuning_rx: watch::Receiver<RunnerTuningUpdate>,
        db: Option<Arc<crate::db::DbStore>>,
    ) -> Self {
        let runner_id = config.runner_id.clone();
        let symbol = config.symbol.clone();
        Self {
            runner_id,
            symbol,
            strategy: GeometricGridStrategy::new(config),
            risk_engine,
            execution_client,
            simulator,
            tick_rx,
            tuning_rx,
            is_paused: false,
            db,
            telemetry_tx: None,
            brain: None,
            last_init_attempt: None,
        }
    }

    pub fn with_brain(mut self, brain: Arc<crate::brain::EngineBrain>) -> Self {
        self.brain = Some(brain);
        self
    }

    pub fn with_telemetry_channel(mut self, tx: watch::Sender<RunnerTelemetry>) -> Self {
        self.telemetry_tx = Some(tx);
        self
    }

    fn publish_telemetry(&self) {
        if let Some(ref tx) = self.telemetry_tx {
            let snap = RunnerTelemetry {
                runner_id: self.runner_id.clone(),
                symbol: self.symbol.clone(),
                center_price: self.strategy.center_price,
                effective_center: self.strategy.effective_center,
                dynamic_step_pct: self.strategy.dynamic_pricing.calculate_dynamic_step(),
                rolling_volatility_pct: self.strategy.dynamic_pricing.calculate_volatility(),
                inventory_base: self.strategy.inventory_base,
                realized_pnl: self.strategy.realized_pnl,
                total_trades: self.strategy.total_trades,
                active_orders_count: self.strategy.active_orders.len(),
                is_paused: self.is_paused,
            };
            let _ = tx.send(snap);
        }
    }

    pub fn with_initial_active_orders(mut self, orders: Vec<Order>) -> Self {
        for order in orders {
            if order.symbol == self.symbol {
                info!(
                    "[{}] Rehydrated active resting order {} ({} {} @ {})",
                    self.runner_id, order.client_order_id, order.side, order.qty, order.price
                );
                self.strategy.register_active_order(order);
            }
        }
        self
    }

    pub async fn run(mut self) {
        info!("[{}] Grid Runner task started for {}", self.runner_id, self.symbol);
        self.publish_telemetry();

        let mut poll_interval = tokio::time::interval(std::time::Duration::from_secs(2));

        loop {
            tokio::select! {
                _ = poll_interval.tick() => {
                    // 1. Live Fill Polling (Only if not using simulator)
                    if self.simulator.is_none() && !self.is_paused && !self.strategy.active_orders.is_empty() {
                        if let Ok(active) = self.execution_client.get_active_orders().await {
                            let active_ids: std::collections::HashSet<_> = active.into_iter().map(|o| o.client_order_id).collect();
                            let mut fills_to_process = Vec::new();
                            let mut filled_ids = Vec::new();

                            for ord in self.strategy.active_orders.values() {
                                if !active_ids.contains(&ord.client_order_id) {
                                    let fill = crate::model::Fill {
                                        order_id: ord.id,
                                        client_order_id: ord.client_order_id.clone(),
                                        runner_id: ord.runner_id.clone(),
                                        symbol: ord.symbol.clone(),
                                        side: ord.side,
                                        price: ord.price,
                                        qty: ord.qty,
                                        fee: Decimal::ZERO,
                                        timestamp: chrono::Utc::now(),
                                    };
                                    fills_to_process.push(fill);
                                    filled_ids.push(ord.id);
                                }
                            }

                            for id in filled_ids {
                                self.strategy.remove_active_order(&id);
                            }

                            for fill in fills_to_process {
                                info!("[ORDER-FILL] [{}] Live venue fill confirmed: Order {} ({} {} @ £{})", self.runner_id, fill.client_order_id, fill.side, fill.qty, fill.price);
                                self.risk_engine.settle_fill(&fill).await;
                                if let Some(ref db) = self.db {
                                    db.update_order_status(&fill.client_order_id, "FILLED").await;
                                }
                                if let Some(counter_order) = self.strategy.on_fill(&fill) {
                                    info!(
                                        "[COUNTER-ORDER] [{}] Generated counter order {} for fill {}: {} {} @ £{}",
                                        self.runner_id, counter_order.client_order_id, fill.client_order_id, counter_order.side, counter_order.qty, counter_order.price
                                    );
                                    match self.risk_engine.validate_order(&counter_order).await {
                                        Ok(()) => {
                                            match self.submit_and_save(&counter_order).await {
                                                Ok(submitted) => {
                                                    info!("[ORDER-DISPATCH] [{}] Counter order placed: {} {} @ £{} ({})", self.runner_id, submitted.side, submitted.qty, submitted.price, submitted.client_order_id);
                                                    self.strategy.register_active_order(submitted);
                                                }
                                                Err(e) => {
                                                    error!("[ORDER-REJECT] [{}] Live submission of counter order failed: {}", self.runner_id, e);
                                                    self.risk_engine.release_order_capital(&counter_order).await;
                                                }
                                            }
                                        }
                                        Err(RiskError::CircuitBreakerTripped(reason)) => {
                                            error!("[RISK-TRIGGER] [{}] Counter order rejected by risk engine: {}", self.runner_id, reason);
                                            let _ = self.execution_client.cancel_all_orders().await;
                                            self.is_paused = true;
                                        }
                                        Err(err) => {
                                            warn!("[RISK-REJECT] [{}] Counter order risk check failed: {}", self.runner_id, err);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    self.publish_telemetry();
                }
                
                // Listen for market ticks multiplexed from Kraken WS
                Ok(tick) = self.tick_rx.recv() => {
                    if tick.symbol != self.symbol {
                        continue;
                    }

                    if self.is_paused {
                        continue;
                    }

                    let mid_price = tick.mid_price();

                    // Record tick in dynamic pricing model to track rolling volatility
                    self.strategy.record_tick(mid_price, tick.timestamp);

                    // 2. Adverse selection check on Kraken tick
                    let is_toxic_dump = self.risk_engine.record_kraken_tick(&self.symbol, mid_price, tick.timestamp).await;
                    if is_toxic_dump {
                        let buys_to_cancel: Vec<Order> = self.strategy.active_orders.values()
                            .filter(|o| o.side == OrderSide::Buy)
                            .cloned()
                            .collect();

                        warn!(
                            "[RISK-TRIGGER] [{}] Adverse selection lag filter triggered! Canceling {} resting BUY bids on Revolut X",
                            self.runner_id, buys_to_cancel.len()
                        );

                        for ord in buys_to_cancel {
                            let _ = self.execution_client.cancel_order(&ord.client_order_id).await;
                            info!("[ORDER-CANCEL] [{}] Canceled resting BUY order {} due to adverse selection plunge", self.runner_id, ord.client_order_id);
                            if let Some(ref db) = self.db {
                                db.update_order_status(&ord.client_order_id, "CANCELED").await;
                            }
                            self.strategy.active_orders.remove(&ord.id);
                            self.risk_engine.release_order_capital(&ord).await;
                        }
                    }

                    // 3. Process fills in paper simulator if present
                    if let Some(ref sim) = self.simulator {
                        let fills = sim.process_tick(&tick).await;
                        for fill in fills {
                            info!("[ORDER-FILL] [{}] Simulator fill: Order {} ({} {} @ £{})", self.runner_id, fill.client_order_id, fill.side, fill.qty, fill.price);
                            self.risk_engine.settle_fill(&fill).await;
                            if let Some(counter_order) = self.strategy.on_fill(&fill) {
                                info!(
                                    "[COUNTER-ORDER] [{}] Generated simulator counter order {} for fill {}: {} {} @ £{}",
                                    self.runner_id, counter_order.client_order_id, fill.client_order_id, counter_order.side, counter_order.qty, counter_order.price
                                );
                                match self.risk_engine.validate_order(&counter_order).await {
                                    Ok(()) => {
                                        if let Ok(submitted) = self.submit_and_save(&counter_order).await {
                                            info!("[ORDER-DISPATCH] [{}] Simulator counter order placed: {} {} @ £{} ({})", self.runner_id, submitted.side, submitted.qty, submitted.price, submitted.client_order_id);
                                            self.strategy.register_active_order(submitted);
                                        } else {
                                            self.risk_engine.release_order_capital(&counter_order).await;
                                        }
                                    }
                                    Err(RiskError::CircuitBreakerTripped(reason)) => {
                                        error!("[RISK-TRIGGER] [{}] Counter order rejected by risk engine: {}", self.runner_id, reason);
                                        let _ = self.execution_client.cancel_all_orders().await;
                                        self.is_paused = true;
                                    }
                                    Err(err) => {
                                        warn!("[RISK-REJECT] [{}] Counter order risk check failed: {}", self.runner_id, err);
                                    }
                                }
                            }
                        }
                    }

                    // 4. Initialize grid if not yet initialized or if active orders are empty
                    let should_init = (self.strategy.center_price.is_none() || self.strategy.active_orders.is_empty())
                        && mid_price > Decimal::ZERO
                        && self.last_init_attempt.map_or(true, |t| t.elapsed() >= tokio::time::Duration::from_secs(10));

                    if should_init {
                        self.last_init_attempt = Some(tokio::time::Instant::now());
                        let balances = self.execution_client.get_available_balances().await.ok();
                        let total_quote = balances.as_ref().map(|b| b.get(&self.symbol.quote).copied().unwrap_or(Decimal::ZERO)).unwrap_or(Decimal::ZERO);
                        let free_fiat = if let Some(ref brain) = self.brain {
                            let (sniper_res, _, allocations) = brain.partition_capital(total_quote, &[self.symbol.clone()]);
                            let pair_budget = allocations.get(&self.symbol.as_slash()).copied().unwrap_or(Decimal::ZERO);
                            info!(
                                "[BRAIN-PARTITION] [{}] Cash {} {:.2} partitioned: {:.2} to Sniper reserve, {:.2} to {}",
                                self.runner_id, self.symbol.quote, total_quote, sniper_res, pair_budget, self.symbol
                            );
                            self.risk_engine.register_envelope(&self.runner_id, &self.symbol.quote, pair_budget).await;
                            Some(pair_budget)
                        } else {
                            balances.as_ref().map(|b| b.get(&self.symbol.quote).copied().unwrap_or(Decimal::ZERO))
                        };
                        let available_base = balances.as_ref().map(|b| b.get(&self.symbol.base).copied().unwrap_or(Decimal::ZERO));

                        info!(
                            "[ORDER-INTENT] [{}] Initializing grid: mid={}{:.2}, {} rungs/side (allocated budget: {:?}, base balance: {:?})",
                            self.runner_id, if self.symbol.quote == "USD" { "$" } else { "£" }, mid_price, self.strategy.config.rungs_per_side, free_fiat, available_base
                        );
                        let initial_orders = self.strategy.initialize_grid(mid_price, free_fiat, available_base);
                        for order in initial_orders {
                            match self.risk_engine.validate_order(&order).await {
                                Ok(()) => {
                                    if let Ok(submitted) = self.submit_and_save(&order).await {
                                        info!("[ORDER-DISPATCH] [{}] Placed initial grid rung: {} {} @ £{} ({})", self.runner_id, submitted.side, submitted.qty, submitted.price, submitted.client_order_id);
                                        self.strategy.register_active_order(submitted);
                                    } else {
                                        self.risk_engine.release_order_capital(&order).await;
                                    }
                                }
                                Err(err) => {
                                    warn!("[RISK-REJECT] [{}] Initial grid order rejected by risk engine: {}", self.runner_id, err);
                                }
                            }
                        }
                    }

                    // 5. Check for rebalancing if price drifted significantly
                    if self.strategy.needs_rebalance(mid_price) {
                        info!(
                            "[ORDER-INTENT] [{}] Price drifted significantly from center ({:.2} -> {:.2}). Rebalancing grid (clearing {} orders)",
                            self.runner_id, self.strategy.center_price.unwrap_or_default(), mid_price, self.strategy.active_orders.len()
                        );
                        // Cancel existing orders
                        for (_, ord) in self.strategy.active_orders.drain() {
                            let _ = self.execution_client.cancel_order(&ord.client_order_id).await;
                            info!("[ORDER-CANCEL] [{}] Canceled resting order {} for grid rebalance", self.runner_id, ord.client_order_id);
                            if let Some(ref db) = self.db {
                                db.update_order_status(&ord.client_order_id, "CANCELED").await;
                            }
                            self.risk_engine.release_order_capital(&ord).await;
                        }
                        // Re-initialize grid with live balances
                        let balances = self.execution_client.get_available_balances().await.ok();
                        let total_quote = balances.as_ref().map(|b| b.get(&self.symbol.quote).copied().unwrap_or(Decimal::ZERO)).unwrap_or(Decimal::ZERO);
                        let free_fiat = if let Some(ref brain) = self.brain {
                            let (_, _, allocations) = brain.partition_capital(total_quote, &[self.symbol.clone()]);
                            let pair_budget = allocations.get(&self.symbol.as_slash()).copied().unwrap_or(Decimal::ZERO);
                            self.risk_engine.register_envelope(&self.runner_id, &self.symbol.quote, pair_budget).await;
                            Some(pair_budget)
                        } else {
                            balances.as_ref().map(|b| b.get(&self.symbol.quote).copied().unwrap_or(Decimal::ZERO))
                        };
                        let available_base = balances.as_ref().map(|b| b.get(&self.symbol.base).copied().unwrap_or(Decimal::ZERO));

                        let new_orders = self.strategy.initialize_grid(mid_price, free_fiat, available_base);
                        for order in new_orders {
                            match self.risk_engine.validate_order(&order).await {
                                Ok(()) => {
                                    if let Ok(submitted) = self.submit_and_save(&order).await {
                                        info!("[ORDER-DISPATCH] [{}] Placed rebalance grid rung: {} {} @ £{} ({})", self.runner_id, submitted.side, submitted.qty, submitted.price, submitted.client_order_id);
                                        self.strategy.register_active_order(submitted);
                                    } else {
                                        self.risk_engine.release_order_capital(&order).await;
                                    }
                                }
                                Err(err) => {
                                    warn!("[RISK-REJECT] [{}] Rebalance grid order rejected by risk engine: {}", self.runner_id, err);
                                }
                            }
                        }
                    }
                    self.publish_telemetry();
                }

                // Dynamic parameter updates from control plane
                Ok(()) = self.tuning_rx.changed() => {
                    let update = self.tuning_rx.borrow().clone();
                    self.is_paused = update.paused;
                    if let Some(step) = update.step_pct {
                        self.strategy.config.step_pct = step;
                        self.strategy.dynamic_pricing.config.base_step_pct = step;
                        info!("[RUNNER-TUNE] [{}] Dynamically updated step_pct to {:.4}%", self.runner_id, step);
                    }
                    if let Some(rebal) = update.rebalance_threshold_pct {
                        self.strategy.config.rebalance_threshold_pct = rebal;
                        info!("[RUNNER-TUNE] [{}] Dynamically updated rebalance_threshold_pct to {:.4}%", self.runner_id, rebal);
                    }
                    if let Some(size) = update.order_size_fiat {
                        self.strategy.config.order_size_gbp = size;
                        info!("[RUNNER-TUNE] [{}] Dynamically updated order_size_fiat to £{}", self.runner_id, size);
                    }
                    if let Some(dyn_enabled) = update.dynamic_pricing_enabled {
                        self.strategy.dynamic_pricing.config.enabled = dyn_enabled;
                        info!("[RUNNER-TUNE] [{}] Dynamically set dynamic_pricing enabled={}", self.runner_id, dyn_enabled);
                    }
                    if let Some(gamma) = update.inventory_gamma {
                        self.strategy.dynamic_pricing.config.inventory_gamma = gamma;
                        info!("[RUNNER-TUNE] [{}] Dynamically updated inventory_gamma to {:.4}", self.runner_id, gamma);
                    }
                    if let Some(mode) = update.mode {
                        self.strategy.config.mode = Some(mode.clone());
                        info!("[RUNNER-TUNE] [{}] Dynamically updated mode to {}", self.runner_id, mode);
                    }
                }
            }
        }
    }

    pub fn get_telemetry(&self) -> RunnerTelemetry {
        RunnerTelemetry {
            runner_id: self.runner_id.clone(),
            symbol: self.symbol.clone(),
            center_price: self.strategy.center_price,
            effective_center: self.strategy.effective_center,
            dynamic_step_pct: self.strategy.dynamic_pricing.calculate_dynamic_step(),
            rolling_volatility_pct: self.strategy.dynamic_pricing.calculate_volatility(),
            inventory_base: self.strategy.inventory_base,
            realized_pnl: self.strategy.realized_pnl,
            total_trades: self.strategy.total_trades,
            active_orders_count: self.strategy.active_orders.len(),
            is_paused: self.is_paused,
        }
    }

    async fn submit_and_save(&self, order: &Order) -> Result<Order, String> {
        let res = self.execution_client.submit_post_only_order(order).await;
        match &res {
            Ok(_) => {
                if let Some(ref db) = self.db {
                    db.save_order(order, "OPEN").await;
                }
            }
            Err(e) => {
                error!("[ORDER-REJECT] [{}] Order submission failed for {}: {}", self.runner_id, order.client_order_id, e);
            }
        }
        res
    }
}
