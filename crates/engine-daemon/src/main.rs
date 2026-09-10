mod ipc;

use dotenvy::dotenv;
use ipc::IpcServer;
use rust_decimal_macros::dec;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use kraken_client::KrakenWsMultiplexer;
use trading_core::model::Symbol;
use trading_core::risk::CentralRiskEngine;
use trading_core::runner::{GridRunner, RunnerTuningUpdate};
use trading_core::simulator::PaperExecutionSimulator;
use trading_core::strategy::GridConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Initialize logging
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,kraken_client=info,trading_core=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenv().ok();

    info!("============================================================");
    info!("Starting Multi-Venue Algorithmic Crypto Trading System (UK)");
    info!("Asymmetric Venue Model: Revolut X (0% Maker) + Kraken Pro WS");
    info!("Execution Mode: PAPER TRADING (Virtual Simulator)");
    info!("============================================================");

    // 2. Risk Engine & Capital Envelopes
    let max_drawdown_pct = dec!(0.05); // 5.0% rolling circuit breaker
    let lag_drop_threshold_pct = dec!(0.006); // 0.6% adverse selection drop threshold
    let risk_engine = Arc::new(CentralRiskEngine::new(max_drawdown_pct, lag_drop_threshold_pct));

    let btc_envelope_gbp = dec!(500.00);
    let eth_envelope_gbp = dec!(500.00);

    risk_engine.register_envelope("runner_btc", "GBP", btc_envelope_gbp).await;
    risk_engine.register_envelope("runner_eth", "GBP", eth_envelope_gbp).await;

    info!(
        "Capital Envelopes initialized: runner_btc = £{}, runner_eth = £{}",
        btc_envelope_gbp, eth_envelope_gbp
    );

    // 3. In-memory Paper Execution Simulator
    let total_paper_capital = btc_envelope_gbp + eth_envelope_gbp;
    let simulator = Arc::new(PaperExecutionSimulator::new(total_paper_capital));

    // 4. Kraken WebSocket v2 Multiplexer
    let kraken_ws_url = std::env::var("KRAKEN_WS_URL").unwrap_or_else(|_| "wss://ws.kraken.com/v2".into());
    let symbols = vec![Symbol::btc_gbp(), Symbol::eth_gbp()];

    let (kraken_multiplexer, _primary_rx) = KrakenWsMultiplexer::new(&kraken_ws_url, symbols.clone());
    let btc_tick_rx = kraken_multiplexer.subscribe_receiver();
    let eth_tick_rx = kraken_multiplexer.subscribe_receiver();

    // Spawn Kraken WS ingest task
    tokio::spawn(async move {
        kraken_multiplexer.run().await;
    });

    // 5. Runner 1: BTC/GBP Geometric Grid
    let btc_config = GridConfig {
        runner_id: "runner_btc".into(),
        symbol: Symbol::btc_gbp(),
        step_pct: dec!(0.0040), // 0.40% geometric step
        rungs_per_side: 5,
        order_size_gbp: dec!(50.0),
        rebalance_threshold_pct: dec!(0.012),
    };
    let (btc_tune_tx, btc_tune_rx) = watch::channel(RunnerTuningUpdate {
        paused: false,
        step_pct: None,
        rebalance_threshold_pct: None,
    });
    let btc_runner = GridRunner::new(
        btc_config,
        risk_engine.clone(),
        simulator.clone(),
        btc_tick_rx,
        btc_tune_rx,
    );

    // 6. Runner 2: ETH/GBP Geometric Grid
    let eth_config = GridConfig {
        runner_id: "runner_eth".into(),
        symbol: Symbol::eth_gbp(),
        step_pct: dec!(0.0040), // 0.40% geometric step
        rungs_per_side: 5,
        order_size_gbp: dec!(50.0),
        rebalance_threshold_pct: dec!(0.012),
    };
    let (eth_tune_tx, eth_tune_rx) = watch::channel(RunnerTuningUpdate {
        paused: false,
        step_pct: None,
        rebalance_threshold_pct: None,
    });
    let eth_runner = GridRunner::new(
        eth_config,
        risk_engine.clone(),
        simulator.clone(),
        eth_tick_rx,
        eth_tune_rx,
    );

    // 7. Unix Domain Socket IPC Server
    let socket_path = std::env::var("ENGINE_UDS_PATH").unwrap_or_else(|_| "/tmp/trading_engine.sock".into());
    let ipc_server = IpcServer::new(
        socket_path,
        risk_engine.clone(),
        simulator.clone(),
        btc_tune_tx,
        eth_tune_tx,
    );
    ipc_server.run().await?;

    // Spawn runners
    tokio::spawn(async move {
        btc_runner.run().await;
    });

    tokio::spawn(async move {
        eth_runner.run().await;
    });

    // 8. Telemetry Heartbeat Logger
    let sim_telemetry = simulator.clone();
    let risk_telemetry = risk_engine.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            let wallet = sim_telemetry.get_wallet().await;
            let resting = sim_telemetry.get_resting_orders().await;
            let cb_tripped = risk_telemetry.is_circuit_breaker_tripped().await;

            info!(
                "[TELEMETRY] Balances: [GBP: £{:.2}, BTC: {:.6}, ETH: {:.6}] | Active Resting Orders: {} | Circuit Breaker: {}",
                wallet.gbp,
                wallet.btc,
                wallet.eth,
                resting.len(),
                if cb_tripped { "TRIPPED" } else { "NORMAL" }
            );
        }
    });

    // 8. Wait for termination signal
    tokio::signal::ctrl_c().await?;
    warn!("Shutdown signal received. Canceling all active resting orders...");
    let canceled = simulator.cancel_all_orders().await;
    info!("Successfully canceled {} orders across all venues. System shut down cleanly.", canceled.len());

    Ok(())
}
