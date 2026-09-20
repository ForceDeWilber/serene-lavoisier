use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch, RwLock};
use tracing::{error, info, warn};

use crate::ipc::{RunnerTelemetryDto, SniperTelemetryDto};
use trading_core::db::DbStore;
use trading_core::execution::ExecutionClient;
use trading_core::model::{MarketTick, PairConfig, Symbol};
use trading_core::risk::CentralRiskEngine;
use trading_core::runner::{
    GridRunner, RunnerTelemetry, RunnerTuningUpdate, SniperRunner, SniperTelemetry, SniperTuningUpdate,
};
use trading_core::simulator::PaperExecutionSimulator;
use trading_core::strategy::{GridConfig, SniperConfig};

pub struct PairHandle {
    pub config: PairConfig,
    pub grid_runner_id: String,
    pub sniper_runner_id: String,
    pub grid_tune_tx: watch::Sender<RunnerTuningUpdate>,
    pub sniper_tune_tx: watch::Sender<SniperTuningUpdate>,
    pub grid_telemetry_rx: watch::Receiver<RunnerTelemetry>,
    pub sniper_telemetry_rx: watch::Receiver<SniperTelemetry>,
}

pub struct RunnerManager {
    execution_client: Arc<dyn ExecutionClient>,
    risk_engine: Arc<CentralRiskEngine>,
    paper_sim: Option<Arc<PaperExecutionSimulator>>,
    db_store: Option<Arc<DbStore>>,
    kraken_sub_tx: mpsc::UnboundedSender<Vec<Symbol>>,
    tick_broadcast: broadcast::Sender<MarketTick>,
    pub brain: Arc<trading_core::brain::EngineBrain>,
    pairs: Arc<RwLock<HashMap<String, PairHandle>>>, // key: symbol.as_slash() e.g. "BTC/USD"
}

impl RunnerManager {
    pub fn new(
        execution_client: Arc<dyn ExecutionClient>,
        risk_engine: Arc<CentralRiskEngine>,
        paper_sim: Option<Arc<PaperExecutionSimulator>>,
        db_store: Option<Arc<DbStore>>,
        kraken_sub_tx: mpsc::UnboundedSender<Vec<Symbol>>,
        tick_broadcast: broadcast::Sender<MarketTick>,
    ) -> Self {
        Self {
            execution_client,
            risk_engine,
            paper_sim,
            db_store,
            kraken_sub_tx,
            tick_broadcast,
            brain: Arc::new(trading_core::brain::EngineBrain::default()),
            pairs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Spawns GridRunner & SniperRunner for a trading pair without interrupting other pairs
    pub async fn spawn_pair(&self, config: PairConfig) -> anyhow::Result<()> {
        let sym_slash = config.symbol.as_slash();

        // Check if already active: if so, dynamically tune and persist updated config
        {
            let map = self.pairs.read().await;
            if let Some(handle) = map.get(&sym_slash) {
                info!("Pair {} is already active, updating tuning parameters", sym_slash);
                let mut curr_grid = handle.grid_tune_tx.borrow().clone();
                curr_grid.step_pct = Some(config.grid_step_pct);
                curr_grid.rebalance_threshold_pct = Some(config.rebalance_threshold_pct);
                curr_grid.order_size_fiat = Some(config.order_size_fiat);
                curr_grid.paused = !config.is_active;
                let _ = handle.grid_tune_tx.send(curr_grid);

                let mut curr_sniper = handle.sniper_tune_tx.borrow().clone();
                curr_sniper.enabled = config.sniper_enabled;
                curr_sniper.impulse_threshold_pct = Some(config.sniper_hurdle_pct);
                curr_sniper.order_size_gbp = Some(config.sniper_order_size_fiat);
                let _ = handle.sniper_tune_tx.send(curr_sniper);

                if config.is_active {
                    self.brain.register_active_pair(config.symbol.clone());
                } else {
                    self.brain.remove_active_pair(&config.symbol);
                }

                if let Some(ref db) = self.db_store {
                    let _ = db.upsert_pair_config(&config).await;
                }
                return Ok(());
            }
        }

        if config.is_active {
            self.brain.register_active_pair(config.symbol.clone());
        }

        info!("Spawning dynamic execution runners for pair: {}", sym_slash);

        // Derive consistent runner IDs
        let slug = format!("{}_{}", config.symbol.base.to_lowercase(), config.symbol.quote.to_lowercase());
        let grid_id = format!("runner_{}", slug);
        let sniper_id = format!("sniper_{}", slug);

        // 1. Register Capital Envelopes in Risk Engine (using pair's quote currency)
        self.risk_engine.register_envelope(&grid_id, &config.symbol.quote, config.envelope_capital).await;
        self.risk_engine.register_envelope(&sniper_id, &config.symbol.quote, dec!(100.0)).await;

        // 2. Hot-subscribe Kraken WS v2 Oracle
        let _ = self.kraken_sub_tx.send(vec![config.symbol.clone()]);

        // 3. Strategy Tuning Channels
        let (grid_tune_tx, grid_tune_rx) = watch::channel(RunnerTuningUpdate {
            paused: !config.is_active,
            step_pct: Some(config.grid_step_pct),
            rebalance_threshold_pct: Some(config.rebalance_threshold_pct),
            order_size_fiat: Some(config.order_size_fiat),
            dynamic_pricing_enabled: Some(true),
            inventory_gamma: Some(dec!(0.08)),
            mode: None,
        });

        let (sniper_tune_tx, sniper_tune_rx) = watch::channel(SniperTuningUpdate {
            enabled: config.sniper_enabled,
            impulse_threshold_pct: Some(config.sniper_hurdle_pct),
            order_size_gbp: Some(config.sniper_order_size_fiat),
        });

        // 4. Create Grid Runner
        let cost_basis = match config.symbol.base.as_str() {
            "BTC" => {
                if config.symbol.quote == "GBP" { Some(dec!(57019.97)) } else { Some(dec!(76350.00)) }
            }
            "ETH" => {
                if config.symbol.quote == "GBP" { Some(dec!(1845.43)) } else { Some(dec!(2470.00)) }
            }
            "SOL" => {
                if config.symbol.quote == "GBP" { Some(dec!(74.79)) } else { Some(dec!(100.10)) }
            }
            _ => None,
        };

        let grid_config = GridConfig {
            runner_id: grid_id.clone(),
            symbol: config.symbol.clone(),
            step_pct: config.grid_step_pct,
            rungs_per_side: config.grid_rungs,
            order_size_gbp: config.order_size_fiat,
            rebalance_threshold_pct: config.rebalance_threshold_pct,
            dynamic_pricing: trading_core::strategy::DynamicPricingConfig::default(),
            mode: None,
            cost_basis,
        };

        let grid_runner = GridRunner::new(
            grid_config,
            self.risk_engine.clone(),
            self.execution_client.clone(),
            self.paper_sim.clone(),
            self.tick_broadcast.subscribe(),
            grid_tune_rx,
            self.db_store.clone(),
        );

        let (grid_telem_tx, grid_telem_rx) = watch::channel(RunnerTelemetry::default());
        let (sniper_telem_tx, sniper_telem_rx) = watch::channel(SniperTelemetry {
            runner_id: sniper_id.clone(),
            symbol: config.symbol.clone(),
            enabled: config.sniper_enabled,
            impulse_threshold_pct: config.sniper_hurdle_pct,
            order_size_gbp: config.sniper_order_size_fiat,
            total_snipes: 0,
            successful_snipes: 0,
            total_sniper_profit_gbp: Decimal::ZERO,
            total_fees_paid_gbp: Decimal::ZERO,
            average_lead_ms: 0,
            revolut_best_bid: None,
            revolut_best_ask: None,
            current_dislocation_pct: None,
            kraken_price: None,
        });

        let initial_orders = self.execution_client.get_active_orders().await.unwrap_or_default();
        let grid_runner = grid_runner
            .with_brain(self.brain.clone())
            .with_initial_active_orders(initial_orders)
            .with_telemetry_channel(grid_telem_tx);

        // 5. Create Sniper Runner
        let sniper_config = SniperConfig {
            runner_id: sniper_id.clone(),
            symbol: config.symbol.clone(),
            impulse_threshold_pct: config.sniper_hurdle_pct,
            min_net_edge_pct: dec!(0.0002),
            revolut_taker_fee_pct: dec!(0.0009),
            order_size_gbp: config.sniper_order_size_fiat,
            scratch_timeout_ms: 800,
        };

        let sniper_runner = SniperRunner::new(
            sniper_config,
            self.execution_client.clone(),
            self.risk_engine.clone(),
            self.tick_broadcast.subscribe(),
            sniper_tune_rx,
        )
        .with_brain(self.brain.clone())
        .with_telemetry_channel(sniper_telem_tx);

        // 6. Spawn independent Tokio tasks
        if config.grid_rungs > 0 {
            tokio::spawn(async move {
                grid_runner.run().await;
            });
        }

        if config.sniper_enabled {
            tokio::spawn(async move {
                sniper_runner.run().await;
            });
        }

        // 7. Store pair handle
        let handle = PairHandle {
            config: config.clone(),
            grid_runner_id: grid_id,
            sniper_runner_id: sniper_id,
            grid_tune_tx,
            sniper_tune_tx,
            grid_telemetry_rx: grid_telem_rx,
            sniper_telemetry_rx: sniper_telem_rx,
        };

        {
            let mut map = self.pairs.write().await;
            map.insert(sym_slash.clone(), handle);
        }

        // 8. Persist to DB
        if let Some(ref db) = self.db_store {
            let _ = db.upsert_pair_config(&config).await;
        }

        info!("Successfully hot-spawned runners for {}", sym_slash);
        Ok(())
    }

    pub async fn tune_runner(
        &self,
        runner_id: &str,
        paused: Option<bool>,
        step_pct: Option<Decimal>,
        rebalance_threshold_pct: Option<Decimal>,
        order_size_fiat: Option<Decimal>,
        dynamic_pricing_enabled: Option<bool>,
        inventory_gamma: Option<Decimal>,
    ) -> bool {
        let map = self.pairs.read().await;
        for handle in map.values() {
            if handle.grid_runner_id == runner_id
                || handle.grid_runner_id.starts_with(runner_id)
                || runner_id.starts_with(&handle.grid_runner_id)
                || handle.config.symbol.as_slash() == runner_id
                || handle.config.symbol.as_dash() == runner_id
                || runner_id.eq_ignore_ascii_case(&handle.config.symbol.as_slash())
            {
                let mut curr = handle.grid_tune_tx.borrow().clone();
                if let Some(p) = paused {
                    curr.paused = p;
                }
                if let Some(s) = step_pct {
                    curr.step_pct = Some(s);
                }
                if let Some(r) = rebalance_threshold_pct {
                    curr.rebalance_threshold_pct = Some(r);
                }
                if let Some(sz) = order_size_fiat {
                    curr.order_size_fiat = Some(sz);
                }
                if let Some(dyn_en) = dynamic_pricing_enabled {
                    curr.dynamic_pricing_enabled = Some(dyn_en);
                }
                if let Some(gamma) = inventory_gamma {
                    curr.inventory_gamma = Some(gamma);
                }
                let _ = handle.grid_tune_tx.send(curr);
                return true;
            }
        }
        false
    }

    pub async fn set_runner_mode(&self, runner_id: &str, mode: &str) -> bool {
        let map = self.pairs.read().await;
        for handle in map.values() {
            if handle.grid_runner_id == runner_id
                || handle.grid_runner_id.starts_with(runner_id)
                || runner_id.starts_with(&handle.grid_runner_id)
                || handle.config.symbol.as_slash() == runner_id
                || handle.config.symbol.as_dash() == runner_id
                || runner_id.eq_ignore_ascii_case(&handle.config.symbol.as_slash())
            {
                let mut curr = handle.grid_tune_tx.borrow().clone();
                curr.mode = Some(mode.to_string());
                let _ = handle.grid_tune_tx.send(curr);
                return true;
            }
        }
        false
    }

    pub async fn remove_pair(&self, runner_id: &str) -> bool {
        let mut map = self.pairs.write().await;
        // find symbol to remove
        let mut target_sym = None;
        let mut target_symbol_obj = None;
        for (sym, handle) in map.iter() {
            if handle.grid_runner_id == runner_id
                || handle.sniper_runner_id == runner_id
                || sym == runner_id
                || runner_id.eq_ignore_ascii_case(sym)
                || handle.config.symbol.as_slash() == runner_id
            {
                target_sym = Some(sym.clone());
                target_symbol_obj = Some(handle.config.symbol.clone());
                // Tell runners to pause/stop by tuning
                let mut curr_grid = handle.grid_tune_tx.borrow().clone();
                curr_grid.paused = true;
                let _ = handle.grid_tune_tx.send(curr_grid);
                
                let mut curr_sniper = handle.sniper_tune_tx.borrow().clone();
                curr_sniper.enabled = false;
                let _ = handle.sniper_tune_tx.send(curr_sniper);
                break;
            }
        }
        
        if let Some(sym) = target_sym {
            map.remove(&sym);
            if let Some(ref db) = self.db_store {
                let _ = db.delete_pair_config(&sym).await;
            }
            if let Some(symbol_obj) = target_symbol_obj {
                self.brain.remove_active_pair(&symbol_obj);
                if let Ok(active_orders) = self.execution_client.get_active_orders().await {
                    for ord in active_orders {
                        if ord.symbol == symbol_obj {
                            info!("[REMOVE-CANCEL] Canceling active order {} for removed pair {}", ord.client_order_id, sym);
                            let _ = self.execution_client.cancel_order(&ord.client_order_id).await;
                        }
                    }
                }
            }
            info!("[MANAGER] Runner {} ({}) cleanly unmapped and deregistered", runner_id, sym);
            true
        } else {
            warn!("[MANAGER] Runner {} not found for removal in pairs map", runner_id);
            false
        }
    }

    pub async fn liquidate_pair(&self, runner_id: &str) -> anyhow::Result<()> {
        let map = self.pairs.read().await;
        for handle in map.values() {
            if handle.grid_runner_id == runner_id
                || handle.sniper_runner_id == runner_id
                || handle.config.symbol.as_slash() == runner_id
                || runner_id.eq_ignore_ascii_case(&handle.config.symbol.as_slash())
            {
                // 1. Pause grid runner and disable sniper so no new orders are placed
                let mut curr = handle.grid_tune_tx.borrow().clone();
                curr.paused = true;
                let _ = handle.grid_tune_tx.send(curr);

                let mut curr_sniper = handle.sniper_tune_tx.borrow().clone();
                curr_sniper.enabled = false;
                let _ = handle.sniper_tune_tx.send(curr_sniper);

                // 2. Cancel resting orders for this pair to free reserved balances
                if let Ok(active_orders) = self.execution_client.get_active_orders().await {
                    for ord in active_orders {
                        if ord.symbol == handle.config.symbol || ord.runner_id == handle.grid_runner_id || ord.runner_id == handle.sniper_runner_id {
                            info!("[LIQUIDATE-CANCEL] Canceling resting order {} before liquidation", ord.client_order_id);
                            let _ = self.execution_client.cancel_order(&ord.client_order_id).await;
                        }
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;

                // 3. Get total base balance available to sell
                if let Ok(bals) = self.execution_client.get_balances().await {
                    let base = handle.config.symbol.base.clone();
                    if let Some(qty) = bals.get(&base) {
                        if *qty > dec!(0.0) {
                            let sell_price = if let Ok((best_bid, _)) = self.execution_client.get_bbo(&handle.config.symbol).await {
                                if best_bid > dec!(0.0) {
                                    // 2% discount below top bid ensures immediate aggressive taker fill
                                    (best_bid * dec!(0.98)).round_dp(2)
                                } else {
                                    dec!(0.01)
                                }
                            } else {
                                dec!(0.01)
                            };

                            let order = trading_core::model::Order::new_limit_taker(
                                &handle.grid_runner_id,
                                handle.config.symbol.clone(),
                                trading_core::model::OrderSide::Sell,
                                sell_price,
                                *qty,
                            );
                            info!("[LIQUIDATE-DISPATCH] [{}] Submitting taker liquidation sell for {} {} @ £{}", handle.grid_runner_id, qty, handle.config.symbol, sell_price);
                            match self.execution_client.submit_taker_order(&order).await {
                                Ok(filled) => info!("[LIQUIDATE-FILLED] [{}] Market liquidated: {} {} @ £{}", handle.grid_runner_id, filled.qty, handle.config.symbol, filled.price),
                                Err(e) => {
                                    error!("[LIQUIDATE-FAILED] [{}] Market liquidation failed: {}", handle.grid_runner_id, e);
                                    return Err(anyhow::anyhow!("Market liquidation failed: {}", e));
                                }
                            }
                        } else {
                            info!("[LIQUIDATE] [{}] Base balance for {} is 0, nothing to liquidate", handle.grid_runner_id, base);
                        }
                    }
                }
                return Ok(());
            }
        }
        Err(anyhow::anyhow!("Runner not found: {}", runner_id))
    }

    pub async fn tune_sniper(
        &self,
        runner_id: &str,
        enabled: Option<bool>,
        impulse_threshold_pct: Option<Decimal>,
        order_size_gbp: Option<Decimal>,
    ) -> bool {
        let map = self.pairs.read().await;
        for handle in map.values() {
            if handle.sniper_runner_id == runner_id
                || handle.sniper_runner_id.starts_with(runner_id)
                || runner_id.starts_with(&handle.sniper_runner_id)
            {
                let mut curr = handle.sniper_tune_tx.borrow().clone();
                if let Some(en) = enabled {
                    curr.enabled = en;
                }
                if let Some(imp) = impulse_threshold_pct {
                    curr.impulse_threshold_pct = Some(imp);
                }
                if let Some(sz) = order_size_gbp {
                    curr.order_size_gbp = Some(sz);
                }
                let _ = handle.sniper_tune_tx.send(curr);
                return true;
            }
        }
        false
    }

    pub async fn toggle_sniper(&self, runner_id: &str, enabled: Option<bool>) -> Option<bool> {
        let map = self.pairs.read().await;
        for handle in map.values() {
            if handle.sniper_runner_id == runner_id
                || handle.sniper_runner_id.starts_with(runner_id)
                || runner_id.starts_with(&handle.sniper_runner_id)
            {
                let mut curr = handle.sniper_tune_tx.borrow().clone();
                curr.enabled = enabled.unwrap_or(!curr.enabled);
                let state = curr.enabled;
                let _ = handle.sniper_tune_tx.send(curr);
                return Some(state);
            }
        }
        None
    }

    pub async fn emergency_kill_switch(&self) {
        let _ = self.execution_client.cancel_all_orders().await;
        self.risk_engine.trip_circuit_breaker("Emergency kill switch manually triggered").await;
        let map = self.pairs.read().await;
        for handle in map.values() {
            let mut curr = handle.grid_tune_tx.borrow().clone();
            curr.paused = true;
            let _ = handle.grid_tune_tx.send(curr);

            let mut curr_sniper = handle.sniper_tune_tx.borrow().clone();
            curr_sniper.enabled = false;
            let _ = handle.sniper_tune_tx.send(curr_sniper);
        }
    }

    pub async fn reset_circuit_breaker(&self) {
        self.risk_engine.reset_circuit_breaker(Decimal::ZERO).await;
        let map = self.pairs.read().await;
        for handle in map.values() {
            let mut curr = handle.grid_tune_tx.borrow().clone();
            curr.paused = false;
            let _ = handle.grid_tune_tx.send(curr);

            let mut curr_sniper = handle.sniper_tune_tx.borrow().clone();
            curr_sniper.enabled = true;
            let _ = handle.sniper_tune_tx.send(curr_sniper);
        }
    }

    /// Dynamically aggregates telemetry across all active pairs
    pub async fn get_telemetry_dtos(
        &self,
        balances: &HashMap<String, Decimal>,
        active_orders: &[trading_core::model::Order],
    ) -> (Vec<RunnerTelemetryDto>, Vec<SniperTelemetryDto>) {
        let map = self.pairs.read().await;
        let mut runners = Vec::new();
        let mut snipers = Vec::new();

        for handle in map.values() {
            let sym_slash = handle.config.symbol.as_slash();
            let base = &handle.config.symbol.base;
            let inv_base = balances.get(base).copied().unwrap_or(Decimal::ZERO);
            let active_count = active_orders.iter().filter(|o| o.symbol == handle.config.symbol).count();
            let grid_tune = handle.grid_tune_tx.borrow().clone();
            let is_paused = grid_tune.paused;

            let grid_snap = handle.grid_telemetry_rx.borrow().clone();
            let sniper_snap = handle.sniper_telemetry_rx.borrow().clone();

            runners.push(RunnerTelemetryDto {
                runner_id: handle.grid_runner_id.clone(),
                symbol: sym_slash.clone(),
                center_price: grid_snap.center_price,
                inventory_base: if grid_snap.inventory_base > Decimal::ZERO { grid_snap.inventory_base } else { inv_base },
                realized_pnl: grid_snap.realized_pnl,
                total_trades: grid_snap.total_trades,
                active_orders_count: active_count,
                is_paused,
                effective_center: grid_snap.effective_center,
                dynamic_step_pct: if grid_snap.dynamic_step_pct > Decimal::ZERO { Some(grid_snap.dynamic_step_pct) } else { grid_tune.step_pct },
                rolling_volatility_pct: if grid_snap.rolling_volatility_pct > Decimal::ZERO { Some(grid_snap.rolling_volatility_pct) } else { None },
                market_regime: grid_snap.market_regime,
            });

            snipers.push(SniperTelemetryDto {
                runner_id: handle.sniper_runner_id.clone(),
                symbol: sym_slash,
                enabled: sniper_snap.enabled,
                impulse_threshold_pct: sniper_snap.impulse_threshold_pct,
                order_size_gbp: sniper_snap.order_size_gbp,
                total_snipes: sniper_snap.total_snipes,
                successful_snipes: sniper_snap.successful_snipes,
                total_sniper_profit_gbp: sniper_snap.total_sniper_profit_gbp,
                total_fees_paid_gbp: sniper_snap.total_fees_paid_gbp,
                average_lead_ms: sniper_snap.average_lead_ms,
                revolut_best_bid: sniper_snap.revolut_best_bid,
                revolut_best_ask: sniper_snap.revolut_best_ask,
                current_dislocation_pct: sniper_snap.current_dislocation_pct,
                kraken_price: sniper_snap.kraken_price,
            });
        }

        (runners, snipers)
    }

    pub async fn get_all_pairs(&self) -> Vec<PairConfig> {
        let map = self.pairs.read().await;
        map.values().map(|h| h.config.clone()).collect()
    }
}
