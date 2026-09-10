use crate::model::{MarketTick, Symbol};
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
    simulator: Arc<PaperExecutionSimulator>,
    tick_rx: broadcast::Receiver<MarketTick>,
    tuning_rx: watch::Receiver<RunnerTuningUpdate>,
    is_paused: bool,
}

impl GridRunner {
    pub fn new(
        config: GridConfig,
        risk_engine: Arc<CentralRiskEngine>,
        simulator: Arc<PaperExecutionSimulator>,
        tick_rx: broadcast::Receiver<MarketTick>,
        tuning_rx: watch::Receiver<RunnerTuningUpdate>,
    ) -> Self {
        let runner_id = config.runner_id.clone();
        let symbol = config.symbol.clone();
        Self {
            runner_id,
            symbol,
            strategy: GeometricGridStrategy::new(config),
            risk_engine,
            simulator,
            tick_rx,
            tuning_rx,
            is_paused: false,
        }
    }

    pub async fn run(mut self) {
        info!("[{}] Grid Runner task started for {}", self.runner_id, self.symbol);

        loop {
            tokio::select! {
                // Listen for market ticks multiplexed from Kraken WS
                Ok(tick) = self.tick_rx.recv() => {
                    if tick.symbol != self.symbol {
                        continue;
                    }

                    if self.is_paused {
                        continue;
                    }

                    let mid_price = tick.mid_price();

                    // 1. Adverse selection check on Kraken tick
                    let is_toxic_dump = self.risk_engine.record_kraken_tick(&self.symbol, mid_price, tick.timestamp).await;
                    if is_toxic_dump {
                        warn!(
                            "[{}] Adverse selection lag filter triggered! Canceling resting BUY bids on Revolut X",
                            self.runner_id
                        );
                        let canceled = self.simulator.cancel_all_buys_for_symbol(&self.symbol).await;
                        for ord in canceled {
                            self.strategy.active_orders.remove(&ord.id);
                            self.risk_engine.release_order_capital(&ord).await;
                        }
                    }

                    // 2. Process fills in paper simulator
                    let fills = self.simulator.process_tick(&tick).await;
                    for fill in fills {
                        if let Some(counter_order) = self.strategy.on_fill(&fill) {
                            match self.risk_engine.validate_order(&counter_order).await {
                                Ok(()) => {
                                    let _ = self.simulator.submit_order(counter_order).await;
                                }
                                Err(RiskError::CircuitBreakerTripped(reason)) => {
                                    error!("[{}] Risk rejection: {}", self.runner_id, reason);
                                    let _ = self.simulator.cancel_all_orders().await;
                                    self.is_paused = true;
                                }
                                Err(err) => {
                                    warn!("[{}] Counter order risk check failed: {}", self.runner_id, err);
                                }
                            }
                        }
                    }

                    // 3. Initialize grid if not yet initialized
                    if self.strategy.center_price.is_none() && mid_price > Decimal::ZERO {
                        let initial_orders = self.strategy.initialize_grid(mid_price);
                        for order in initial_orders {
                            match self.risk_engine.validate_order(&order).await {
                                Ok(()) => {
                                    let _ = self.simulator.submit_order(order).await;
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
                        for (id, _) in self.strategy.active_orders.drain() {
                            if let Some(ord) = self.simulator.cancel_order(id).await {
                                self.risk_engine.release_order_capital(&ord).await;
                            }
                        }
                        // Re-initialize grid
                        let new_orders = self.strategy.initialize_grid(mid_price);
                        for order in new_orders {
                            if self.risk_engine.validate_order(&order).await.is_ok() {
                                let _ = self.simulator.submit_order(order).await;
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
}
