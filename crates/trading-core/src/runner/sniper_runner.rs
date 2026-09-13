use crate::execution::ExecutionClient;
use crate::model::{MarketTick, Order, OrderSide, Symbol};
use crate::risk::CentralRiskEngine;
use crate::strategy::{LeadLagSniperStrategy, SniperConfig};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;
use tokio::sync::{broadcast, watch};
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct SniperTuningUpdate {
    pub enabled: bool,
    pub impulse_threshold_pct: Option<Decimal>,
    pub order_size_gbp: Option<Decimal>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SniperTelemetry {
    pub runner_id: String,
    pub symbol: Symbol,
    pub enabled: bool,
    pub impulse_threshold_pct: Decimal,
    pub order_size_gbp: Decimal,
    pub total_snipes: usize,
    pub successful_snipes: usize,
    pub total_sniper_profit_gbp: Decimal,
    pub total_fees_paid_gbp: Decimal,
    pub average_lead_ms: u64,
}

pub struct SniperRunner {
    pub runner_id: String,
    pub symbol: Symbol,
    strategy: LeadLagSniperStrategy,
    execution_client: Arc<dyn ExecutionClient>,
    risk_engine: Arc<CentralRiskEngine>,
    tick_rx: broadcast::Receiver<MarketTick>,
    tuning_rx: watch::Receiver<SniperTuningUpdate>,
    in_flight: bool,
}

impl SniperRunner {
    pub fn new(
        config: SniperConfig,
        execution_client: Arc<dyn ExecutionClient>,
        risk_engine: Arc<CentralRiskEngine>,
        tick_rx: broadcast::Receiver<MarketTick>,
        tuning_rx: watch::Receiver<SniperTuningUpdate>,
    ) -> Self {
        let runner_id = config.runner_id.clone();
        let symbol = config.symbol.clone();
        Self {
            runner_id,
            symbol,
            strategy: LeadLagSniperStrategy::new(config),
            execution_client,
            risk_engine,
            tick_rx,
            tuning_rx,
            in_flight: false,
        }
    }

    pub async fn run(mut self) {
        info!("[{}] Lead-Lag Momentum Sniper Runner active for {}", self.runner_id, self.symbol);

        loop {
            tokio::select! {
                Ok(tick) = self.tick_rx.recv() => {
                    if tick.symbol != self.symbol || !self.strategy.enabled || self.in_flight {
                        continue;
                    }

                    // Check if circuit breaker is tripped
                    if self.risk_engine.is_circuit_breaker_tripped().await {
                        continue;
                    }

                    let kraken_mid = tick.mid_price();

                    // Query Revolut X BBO
                    if let Ok((rev_bid, rev_ask)) = self.execution_client.get_bbo(&self.symbol).await {
                        let free_gbp = self.execution_client.get_balance("GBP").await.unwrap_or(Decimal::ZERO);

                        if let Some(opp) = self.strategy.evaluate_dislocation(kraken_mid, rev_bid, rev_ask, free_gbp) {
                            self.in_flight = true;
                            info!(
                                "⚡ [{}] SNIPE OPPORTUNITY DETECTED on {}: Kraken @ £{} vs Revolut Ask @ £{} (gross: +{}%, est net: £{})",
                                self.runner_id, self.symbol, kraken_mid, rev_ask, opp.gross_edge_pct, opp.estimated_profit_gbp
                            );

                            let start_instant = std::time::Instant::now();

                            // Leg 1: Taker order to lift stale ask
                            let snipe_order = Order::new_limit_taker(
                                &self.runner_id,
                                self.symbol.clone(),
                                opp.side,
                                opp.entry_price,
                                opp.qty,
                            );

                            match self.execution_client.submit_taker_order(&snipe_order).await {
                                Ok(filled_snipe) => {
                                    let lead_ms = start_instant.elapsed().as_millis() as u64;
                                    info!(
                                        "⚡ [{}] SNIPE FILLED: Bought {} {} @ £{} in {}ms",
                                        self.runner_id, filled_snipe.qty, self.symbol, filled_snipe.price, lead_ms
                                    );

                                    // Leg 2: Immediate Maker Profit Exit limit sell at target price
                                    let exit_order = Order::new_limit_post_only(
                                        &self.runner_id,
                                        self.symbol.clone(),
                                        OrderSide::Sell,
                                        opp.target_price,
                                        opp.qty,
                                    );

                                    let client_clone = self.execution_client.clone();
                                    let exit_ord_id = exit_order.client_order_id.clone();
                                    let timeout_ms = self.strategy.config.scratch_timeout_ms;
                                    let runner_tag = self.runner_id.clone();

                                    match self.execution_client.submit_post_only_order(&exit_order).await {
                                        Ok(_) => {
                                            info!(
                                                "[{}] Placed profit exit Maker rung @ £{} for {} (Scratch watchdog: {}ms)",
                                                self.runner_id, opp.target_price, opp.qty, timeout_ms
                                            );

                                            // Spawn watchdog to scratch trade if maker exit doesn't fill
                                            tokio::spawn(async move {
                                                tokio::time::sleep(tokio::time::Duration::from_millis(timeout_ms)).await;
                                                if let Ok(active) = client_clone.get_active_orders().await {
                                                    if active.iter().any(|o| o.client_order_id == exit_ord_id) {
                                                        warn!("[{}] Snipe exit timed out after {}ms! Scratching order...", runner_tag, timeout_ms);
                                                        let _ = client_clone.cancel_order(&exit_ord_id).await;
                                                    }
                                                }
                                            });

                                            let taker_fee = (opp.entry_price * opp.qty * dec!(0.0009)).round_dp(2);
                                            self.strategy.record_snipe_result(opp.estimated_profit_gbp, taker_fee, lead_ms);
                                        }
                                        Err(e) => {
                                            error!("[{}] Failed to place profit exit order: {}", self.runner_id, e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!("[{}] Taker snipe order rejected or missed window: {}", self.runner_id, e);
                                }
                            }

                            self.in_flight = false;
                        }
                    }
                }

                Ok(()) = self.tuning_rx.changed() => {
                    let update = self.tuning_rx.borrow().clone();
                    self.strategy.enabled = update.enabled;
                    if let Some(impulse) = update.impulse_threshold_pct {
                        self.strategy.config.impulse_threshold_pct = impulse;
                        info!("[{}] Dynamically updated sniper impulse_threshold_pct to {:.4}%", self.runner_id, impulse);
                    }
                    if let Some(size) = update.order_size_gbp {
                        self.strategy.config.order_size_gbp = size;
                        info!("[{}] Dynamically updated sniper order_size_gbp to £{:.2}", self.runner_id, size);
                    }
                }
            }
        }
    }

    pub fn get_telemetry(&self) -> SniperTelemetry {
        SniperTelemetry {
            runner_id: self.runner_id.clone(),
            symbol: self.symbol.clone(),
            enabled: self.strategy.enabled,
            impulse_threshold_pct: self.strategy.config.impulse_threshold_pct,
            order_size_gbp: self.strategy.config.order_size_gbp,
            total_snipes: self.strategy.total_snipes,
            successful_snipes: self.strategy.successful_snipes,
            total_sniper_profit_gbp: self.strategy.total_sniper_profit_gbp,
            total_fees_paid_gbp: self.strategy.total_taker_fees_paid_gbp,
            average_lead_ms: self.strategy.average_lead_advantage_ms,
        }
    }
}
