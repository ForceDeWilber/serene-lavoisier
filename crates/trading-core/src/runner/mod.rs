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
}

#[derive(Debug, Clone)]
pub struct RunnerTelemetry {
    pub runner_id: String,
    pub symbol: Symbol,
    pub center_price: Option<Decimal>,
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
        }
    }

    pub async fn run(mut self) {
        info!("[{}] Grid Runner task started for {}", self.runner_id, self.symbol);

        let mut poll_interval = tokio::time::interval(std::time::Duration::from_secs(2));

        loop {
            tokio::select! {
                _ = poll_interval.tick() => {
                    // 1. Live Fill Polling (Only if not using simulator)
                    if self.simulator.is_none() && !self.is_paused && !self.strategy.active_orders.is_empty() {
                        if let Ok(active) = self.execution_client.get_active_orders().await {
                            let active_ids: std::collections::HashSet<_> = active.into_iter().map(|o| o.client_order_id).collect();
                            let mut fills_to_process = Vec::new();

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
                                }
                            }

                            for fill in fills_to_process {
                                info!("[{}] Order {} FILLED on live venue", self.runner_id, fill.client_order_id);
                                if let Some(ref db) = self.db {
                                    db.update_order_status(&fill.client_order_id, "FILLED").await;
                                }
                                if let Some(counter_order) = self.strategy.on_fill(&fill) {
                                    match self.risk_engine.validate_order(&counter_order).await {
                                        Ok(()) => {
                                            let _ = self.submit_and_save(&counter_order).await;
                                        }
                                        Err(RiskError::CircuitBreakerTripped(reason)) => {
                                            error!("[{}] Risk rejection: {}", self.runner_id, reason);
                                            let _ = self.execution_client.cancel_all_orders().await;
                                            self.is_paused = true;
                                        }
                                        Err(err) => {
                                            warn!("[{}] Counter order risk check failed: {}", self.runner_id, err);
                                        }
                                    }
                                }
                            }
                        }
                    }
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

                    // 2. Adverse selection check on Kraken tick
                    let is_toxic_dump = self.risk_engine.record_kraken_tick(&self.symbol, mid_price, tick.timestamp).await;
                    if is_toxic_dump {
                        warn!(
                            "[{}] Adverse selection lag filter triggered! Canceling resting BUY bids on Revolut X",
                            self.runner_id
                        );
                        let buys_to_cancel: Vec<Order> = self.strategy.active_orders.values()
                            .filter(|o| o.side == OrderSide::Buy)
                            .cloned()
                            .collect();

                        for ord in buys_to_cancel {
                            let _ = self.execution_client.cancel_order(&ord.client_order_id).await;
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
                            if let Some(counter_order) = self.strategy.on_fill(&fill) {
                                match self.risk_engine.validate_order(&counter_order).await {
                                    Ok(()) => {
                                        let _ = self.submit_and_save(&counter_order).await;
                                    }
                                    Err(RiskError::CircuitBreakerTripped(reason)) => {
                                        error!("[{}] Risk rejection: {}", self.runner_id, reason);
                                        let _ = self.execution_client.cancel_all_orders().await;
                                        self.is_paused = true;
                                    }
                                    Err(err) => {
                                        warn!("[{}] Counter order risk check failed: {}", self.runner_id, err);
                                    }
                                }
                            }
                        }
                    }

                    // 4. Initialize grid if not yet initialized
                    if self.strategy.center_price.is_none() && mid_price > Decimal::ZERO {
                        let initial_orders = self.strategy.initialize_grid(mid_price);
                        for order in initial_orders {
                            match self.risk_engine.validate_order(&order).await {
                                Ok(()) => {
                                    let _ = self.submit_and_save(&order).await;
                                }
                                Err(err) => {
                                    warn!("[{}] Initial grid order rejected by risk engine: {}", self.runner_id, err);
                                }
                            }
                        }
                    }

                    // 4. Check for rebalancing if price drifted significantly
                    if self.strategy.needs_rebalance(mid_price) {
                        info!(
                            "[{}] Price drifted significantly from center ({:.2} -> {:.2}). Rebalancing grid.",
                            self.runner_id, self.strategy.center_price.unwrap_or_default(), mid_price
                        );
                        // Cancel existing orders
                        for (_, ord) in self.strategy.active_orders.drain() {
                            let _ = self.execution_client.cancel_order(&ord.client_order_id).await;
                            if let Some(ref db) = self.db {
                                db.update_order_status(&ord.client_order_id, "CANCELED").await;
                            }
                            self.risk_engine.release_order_capital(&ord).await;
                        }
                        // Re-initialize grid
                        let new_orders = self.strategy.initialize_grid(mid_price);
                        for order in new_orders {
                            if self.risk_engine.validate_order(&order).await.is_ok() {
                                let _ = self.submit_and_save(&order).await;
                            }
                        }
                    }
                }

                // Dynamic parameter updates from control plane
                Ok(()) = self.tuning_rx.changed() => {
                    let update = self.tuning_rx.borrow().clone();
                    self.is_paused = update.paused;
                    if let Some(step) = update.step_pct {
                        self.strategy.config.step_pct = step;
                        info!("[{}] Dynamically updated step_pct to {:.4}%", self.runner_id, step);
                    }
                    if let Some(rebal) = update.rebalance_threshold_pct {
                        self.strategy.config.rebalance_threshold_pct = rebal;
                        info!("[{}] Dynamically updated rebalance_threshold_pct to {:.4}%", self.runner_id, rebal);
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
            inventory_base: self.strategy.inventory_base,
            realized_pnl: self.strategy.realized_pnl,
            total_trades: self.strategy.total_trades,
            active_orders_count: self.strategy.active_orders.len(),
            is_paused: self.is_paused,
        }
    }

    async fn submit_and_save(&self, order: &Order) -> Result<Order, String> {
        let res = self.execution_client.submit_post_only_order(order).await;
        if res.is_ok() {
            if let Some(ref db) = self.db {
                db.save_order(order, "OPEN").await;
            }
        }
        res
    }
}
