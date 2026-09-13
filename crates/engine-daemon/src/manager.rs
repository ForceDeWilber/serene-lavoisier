use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch, RwLock};
use tracing::info;

use crate::ipc::{RunnerTelemetryDto, SniperTelemetryDto};
use trading_core::db::DbStore;
use trading_core::execution::ExecutionClient;
use trading_core::model::{MarketTick, PairConfig, Symbol};
use trading_core::risk::CentralRiskEngine;
use trading_core::runner::{GridRunner, RunnerTuningUpdate, SniperRunner, SniperTuningUpdate};
use trading_core::simulator::PaperExecutionSimulator;
use trading_core::strategy::{GridConfig, SniperConfig};

pub struct PairHandle {
    pub config: PairConfig,
    pub grid_runner_id: String,
    pub sniper_runner_id: String,
    pub grid_tune_tx: watch::Sender<RunnerTuningUpdate>,
    pub sniper_tune_tx: watch::Sender<SniperTuningUpdate>,
}

pub struct RunnerManager {
    execution_client: Arc<dyn ExecutionClient>,
    risk_engine: Arc<CentralRiskEngine>,
    paper_sim: Option<Arc<PaperExecutionSimulator>>,
    db_store: Option<Arc<DbStore>>,
    kraken_sub_tx: mpsc::UnboundedSender<Vec<Symbol>>,
    tick_broadcast: broadcast::Sender<MarketTick>,
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
            pairs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Spawns GridRunner & SniperRunner for a trading pair without interrupting other pairs
    pub async fn spawn_pair(&self, config: PairConfig) -> anyhow::Result<()> {
        let sym_slash = config.symbol.as_slash();

        // Check if already active
        {
            let map = self.pairs.read().await;
            if map.contains_key(&sym_slash) {
                info!("Pair {} is already active in runner manager", sym_slash);
                return Ok(());
            }
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
        });

        let (sniper_tune_tx, sniper_tune_rx) = watch::channel(SniperTuningUpdate {
            enabled: config.sniper_enabled,
            impulse_threshold_pct: Some(config.sniper_hurdle_pct),
            order_size_gbp: Some(config.sniper_order_size_fiat),
        });

        // 4. Create Grid Runner
        let grid_config = GridConfig {
            runner_id: grid_id.clone(),
            symbol: config.symbol.clone(),
            step_pct: config.grid_step_pct,
            rungs_per_side: config.grid_rungs,
            order_size_gbp: config.order_size_fiat,
            rebalance_threshold_pct: config.rebalance_threshold_pct,
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
        );

        // 6. Spawn independent Tokio tasks
        tokio::spawn(async move {
            grid_runner.run().await;
        });

        tokio::spawn(async move {
            sniper_runner.run().await;
        });

        // 7. Store pair handle
        let handle = PairHandle {
            config: config.clone(),
            grid_runner_id: grid_id,
            sniper_runner_id: sniper_id,
            grid_tune_tx,
            sniper_tune_tx,
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
    ) -> bool {
        let map = self.pairs.read().await;
        for handle in map.values() {
            if handle.grid_runner_id == runner_id {
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
                let _ = handle.grid_tune_tx.send(curr);
                return true;
            }
        }
        false
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
            if handle.sniper_runner_id == runner_id {
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
            if handle.sniper_runner_id == runner_id {
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
        let map = self.pairs.read().await;
        for handle in map.values() {
            let mut curr = handle.grid_tune_tx.borrow().clone();
            curr.paused = true;
            let _ = handle.grid_tune_tx.send(curr);
        }
    }

    pub async fn reset_circuit_breaker(&self) {
        self.risk_engine.reset_circuit_breaker(Decimal::ZERO).await;
        let map = self.pairs.read().await;
        for handle in map.values() {
            let mut curr = handle.grid_tune_tx.borrow().clone();
            curr.paused = false;
            let _ = handle.grid_tune_tx.send(curr);
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
            let is_paused = handle.grid_tune_tx.borrow().paused;

            runners.push(RunnerTelemetryDto {
                runner_id: handle.grid_runner_id.clone(),
                symbol: sym_slash.clone(),
                center_price: None,
                inventory_base: inv_base,
                realized_pnl: Decimal::ZERO,
                total_trades: 0,
                active_orders_count: active_count,
                is_paused,
            });

            let sniper_state = handle.sniper_tune_tx.borrow().clone();
            snipers.push(SniperTelemetryDto {
                runner_id: handle.sniper_runner_id.clone(),
                symbol: sym_slash,
                enabled: sniper_state.enabled,
                impulse_threshold_pct: sniper_state.impulse_threshold_pct.unwrap_or(handle.config.sniper_hurdle_pct),
                order_size_gbp: sniper_state.order_size_gbp.unwrap_or(handle.config.sniper_order_size_fiat),
                total_snipes: 0,
                successful_snipes: 0,
                total_sniper_profit_gbp: Decimal::ZERO,
                total_fees_paid_gbp: Decimal::ZERO,
                average_lead_ms: 450,
            });
        }

        (runners, snipers)
    }

    pub async fn get_all_pairs(&self) -> Vec<PairConfig> {
        let map = self.pairs.read().await;
        map.values().map(|h| h.config.clone()).collect()
    }
}
