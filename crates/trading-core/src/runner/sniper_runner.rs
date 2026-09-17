use crate::execution::ExecutionClient;
use crate::model::{MarketTick, Order, OrderSide, Symbol};
use crate::risk::CentralRiskEngine;
use crate::strategy::{LeadLagSniperStrategy, SniperConfig};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::{Arc, RwLock};
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
    pub revolut_best_bid: Option<Decimal>,
    pub revolut_best_ask: Option<Decimal>,
    pub current_dislocation_pct: Option<Decimal>,
    pub kraken_price: Option<Decimal>,
}

#[derive(Debug, Clone, Default)]
pub struct CachedMarketState {
    pub bbo: Option<(Decimal, Decimal)>,
    pub free_quote: Decimal,
    pub kraken_price: Option<Decimal>,
    pub current_dislocation_pct: Option<Decimal>,
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
    telemetry_tx: Option<watch::Sender<SniperTelemetry>>,
    cached_state: Arc<RwLock<CachedMarketState>>,
    brain: Option<Arc<crate::brain::EngineBrain>>,
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
            telemetry_tx: None,
            cached_state: Arc::new(RwLock::new(CachedMarketState::default())),
            brain: None,
        }
    }

    pub fn with_brain(mut self, brain: Arc<crate::brain::EngineBrain>) -> Self {
        self.brain = Some(brain);
        self
    }

    pub fn with_telemetry_channel(mut self, tx: watch::Sender<SniperTelemetry>) -> Self {
        self.telemetry_tx = Some(tx);
        self
    }

    fn publish_telemetry(&self) {
        if let Some(ref tx) = self.telemetry_tx {
            let _ = tx.send(self.get_telemetry());
        }
    }

    pub async fn run(mut self) {
        info!("[{}] Lead-Lag Momentum Sniper Runner active for {}", self.runner_id, self.symbol);
        self.publish_telemetry();

        let cached_clone = self.cached_state.clone();
        let client_poll = self.execution_client.clone();
        let sym_poll = self.symbol.clone();
        let telem_tx_clone = self.telemetry_tx.clone();
        let brain_clone = self.brain.clone();

        // Stagger poll intervals based on runner symbol to prevent simultaneous burst requests
        let initial_offset = match sym_poll.base.as_str() {
            "BTC" => 0,
            "ETH" => 1200,
            "SOL" => 2400,
            _ => 3600,
        };

        // Background poller to refresh Revolut BBO every 4.0s without hammering Revolut API
        let _poller = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(initial_offset)).await;
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(4000));
            loop {
                interval.tick().await;

                let total_balances = client_poll.get_balances().await.unwrap_or_default();
                let total_quote = total_balances.get(&sym_poll.quote).copied().unwrap_or(Decimal::ZERO);

                // If quote currency has no funded balance (e.g. USD is 0.00), skip order book polling
                if total_quote < dec!(1.00) {
                    tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
                    continue;
                }

                let raw_quote = client_poll.get_balance(&sym_poll.quote).await.unwrap_or(Decimal::ZERO);
                let free_quote = if let Some(ref b) = brain_clone {
                    b.get_sniper_allocation(raw_quote, &[sym_poll.clone()])
                } else {
                    raw_quote
                };

                let bbo = client_poll.get_bbo(&sym_poll).await.ok();
                let mut updated = false;
                if let Ok(mut state) = cached_clone.write() {
                    if let Some(valid_bbo) = bbo {
                        state.bbo = Some(valid_bbo);
                    }
                    state.free_quote = free_quote;
                    if let (Some((_, ask)), Some(k_mid)) = (state.bbo, state.kraken_price) {
                        if ask > Decimal::ZERO {
                            state.current_dislocation_pct = Some(((k_mid - ask) / ask * dec!(100.0)).round_dp(4));
                        }
                    }
                    updated = true;
                }
                if updated {
                    if let Some(ref tx) = telem_tx_clone {
                        let (bid, ask, k_price, disloc) = if let Ok(state) = cached_clone.read() {
                            (
                                state.bbo.map(|b| b.0),
                                state.bbo.map(|b| b.1),
                                state.kraken_price,
                                state.current_dislocation_pct,
                            )
                        } else {
                            (None, None, None, None)
                        };
                        let _ = tx.send_modify(|curr| {
                            curr.revolut_best_bid = bid;
                            curr.revolut_best_ask = ask;
                            curr.current_dislocation_pct = disloc;
                            curr.kraken_price = k_price;
                        });
                    }
                }
            }
        });

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

                    // Update cached kraken price and recalculate dislocation immediately
                    if let Ok(mut state) = self.cached_state.write() {
                        state.kraken_price = Some(kraken_mid);
                        if let Some((_, ask)) = state.bbo {
                            if ask > Decimal::ZERO {
                                state.current_dislocation_pct = Some(((kraken_mid - ask) / ask * dec!(100.0)).round_dp(4));
                            }
                        }
                    }
                    self.publish_telemetry();

                    // Read cached Revolut X BBO and free balance
                    let (rev_bid, rev_ask, free_quote) = {
                        if let Ok(state) = self.cached_state.read() {
                            match state.bbo {
                                Some((bid, ask)) => (bid, ask, state.free_quote),
                                None => continue,
                            }
                        } else {
                            continue;
                        }
                    };

                    if let Some(opp) = self.strategy.evaluate_dislocation(kraken_mid, rev_bid, rev_ask, free_quote) {
                        self.in_flight = true;
                        info!(
                            "[SNIPER-OPPORTUNITY] [{}] Dislocation on {}: Kraken @ £{} vs Revolut Ask @ £{} (gross: +{}%, est net: £{})",
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
                                    "[ORDER-FILL] [{}] Snipe Leg 1 FILLED: Bought {} {} @ £{} in {}ms ({})",
                                    self.runner_id, filled_snipe.qty, self.symbol, filled_snipe.price, lead_ms, filled_snipe.client_order_id
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
                                let sym_scratch = self.symbol.clone();
                                let opp_qty = opp.qty;

                                match self.execution_client.submit_post_only_order(&exit_order).await {
                                    Ok(_) => {
                                        info!(
                                            "[ORDER-DISPATCH] [{}] Placed Leg 2 profit exit Maker rung @ £{} for {} {} (Scratch watchdog: {}ms, {})",
                                            self.runner_id, opp.target_price, opp.qty, self.symbol, timeout_ms, exit_order.client_order_id
                                        );

                                        // Spawn watchdog to scratch trade if maker exit doesn't fill
                                        tokio::spawn(async move {
                                            tokio::time::sleep(tokio::time::Duration::from_millis(timeout_ms)).await;
                                            if let Ok(active) = client_clone.get_active_orders().await {
                                                if active.iter().any(|o| o.client_order_id == exit_ord_id) {
                                                    warn!("[SNIPER-SCRATCH] [{}] Snipe exit timed out after {}ms! Scratching unhedged position...", runner_tag, timeout_ms);
                                                    match client_clone.cancel_order(&exit_ord_id).await {
                                                        Ok(_) => info!("[ORDER-CANCEL] [{}] Canceled timed-out maker exit {}", runner_tag, exit_ord_id),
                                                        Err(e) => error!("[ORDER-CANCEL-FAIL] [{}] Failed to cancel timed-out maker exit {}: {}", runner_tag, exit_ord_id, e),
                                                    }
                                                    // Liquidate unhedged inventory immediately at top bid
                                                    match client_clone.get_bbo(&sym_scratch).await {
                                                        Ok((bbo_bid, _)) if bbo_bid > Decimal::ZERO => {
                                                            let scratch_order = Order::new_limit_taker(
                                                                &runner_tag,
                                                                sym_scratch.clone(),
                                                                OrderSide::Sell,
                                                                bbo_bid,
                                                                opp_qty,
                                                            );
                                                            info!("[SNIPER-SCRATCH-DISPATCH] [{}] Submitting market liquidation sell for {} {} @ £{} ({})", runner_tag, opp_qty, sym_scratch, bbo_bid, scratch_order.client_order_id);
                                                            match client_clone.submit_taker_order(&scratch_order).await {
                                                                Ok(filled) => info!("[SNIPER-SCRATCH-FILLED] [{}] Liquidated unhedged position: {} {} @ £{}", runner_tag, filled.qty, sym_scratch, filled.price),
                                                                Err(e) => error!("[SNIPER-SCRATCH-FAILED] [{}] Emergency liquidation failed: {}", runner_tag, e),
                                                            }
                                                        }
                                                        Ok((bbo_bid, _)) => error!("[SNIPER-SCRATCH-FAILED] [{}] Top bid is zero (£{}), cannot liquidate", runner_tag, bbo_bid),
                                                        Err(e) => error!("[SNIPER-SCRATCH-FAILED] [{}] Failed to fetch BBO for liquidation: {}", runner_tag, e),
                                                    }
                                                }
                                            }
                                        });

                                        let taker_fee = (opp.entry_price * opp.qty * dec!(0.0009)).round_dp(2);
                                        self.strategy.record_snipe_result(opp.estimated_profit_gbp, taker_fee, lead_ms);
                                        self.publish_telemetry();
                                    }
                                    Err(e) => {
                                        error!("[ORDER-REJECT] [{}] Failed to place profit exit order: {}", self.runner_id, e);
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("[ORDER-REJECT] [{}] Taker snipe order rejected or missed window: {}", self.runner_id, e);
                            }
                        }

                        self.in_flight = false;
                    }
                }

                // Dynamic parameter updates from control plane
                Ok(()) = self.tuning_rx.changed() => {
                    let update = self.tuning_rx.borrow().clone();
                    self.strategy.enabled = update.enabled;
                    if let Some(imp) = update.impulse_threshold_pct {
                        self.strategy.config.impulse_threshold_pct = imp;
                        info!("[SNIPER-TUNE] [{}] Dynamically updated impulse_threshold_pct to {:.4}%", self.runner_id, imp);
                    }
                    if let Some(size) = update.order_size_gbp {
                        self.strategy.config.order_size_gbp = size;
                        info!("[SNIPER-TUNE] [{}] Dynamically updated order_size_gbp to £{}", self.runner_id, size);
                    }
                    info!("[SNIPER-TUNE] [{}] Sniper state set to enabled={}", self.runner_id, update.enabled);
                    self.publish_telemetry();
                }
            }
        }
    }

    pub fn get_telemetry(&self) -> SniperTelemetry {
        let (bid, ask, k_price, disloc) = if let Ok(state) = self.cached_state.read() {
            (
                state.bbo.map(|b| b.0),
                state.bbo.map(|b| b.1),
                state.kraken_price,
                state.current_dislocation_pct,
            )
        } else {
            (None, None, None, None)
        };

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
            revolut_best_bid: bid,
            revolut_best_ask: ask,
            current_dislocation_pct: disloc,
            kraken_price: k_price,
        }
    }
}
