mod ipc;
mod manager;

use dotenvy::dotenv;
use ipc::IpcServer;
use manager::RunnerManager;
use kraken_client::KrakenWsMultiplexer;
use revolut_client::{LiveRevolutClient, PaperRevolutClient};
use rust_decimal_macros::dec;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use trading_core::execution::ExecutionClient;
use trading_core::risk::CentralRiskEngine;
use trading_core::simulator::PaperExecutionSimulator;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Initialize logging: Dual-layer (Terminal Stdout + Non-blocking Rolling File Appender)
    let logs_dir = std::path::Path::new("logs");
    if !logs_dir.exists() {
        let _ = std::fs::create_dir_all(logs_dir);
    }
    let file_appender = tracing_appender::rolling::daily("logs", "engine.log");
    let (non_blocking_file, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,kraken_client=info,trading_core=info,revolut_client=info,engine_daemon=info".into());

    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_ansi(true);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking_file)
        .with_ansi(false);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .init();

    dotenv().ok();

    let trading_mode = std::env::var("TRADING_MODE").unwrap_or_else(|_| "PAPER".into()).to_uppercase();

    info!("============================================================");
    info!("Starting Multi-Venue Algorithmic Crypto Trading System (UK)");
    info!("Asymmetric Venue Model: Revolut X (Execution) + Kraken Pro WS (Oracle)");
    info!("System Architecture: Pure Rust Decision & Execution Engine");
    info!("Execution Mode: {}", if trading_mode == "LIVE" { "🔴 LIVE REAL CAPITAL (Revolut X HTTP/2)" } else { "🧪 PAPER TRADING (Virtual Simulator)" });
    info!("Dynamic Pair Architecture: Enabled (Database-Driven)");
    info!("============================================================");

    // 2. Risk Engine
    let max_drawdown_pct = dec!(0.05); // 5.0% rolling circuit breaker
    let lag_drop_threshold_pct = dec!(0.006); // 0.6% adverse selection drop threshold
    let risk_engine = Arc::new(CentralRiskEngine::new(max_drawdown_pct, lag_drop_threshold_pct));

    let db_path = if trading_mode == "LIVE" { "trading_live.db" } else { "trading_paper.db" };
    let db_store = match trading_core::db::DbStore::new(db_path).await {
        Ok(db) => Some(Arc::new(db)),
        Err(e) => {
            error!("Failed to initialize database: {}", e);
            None
        }
    };

    // 3. Execution Client Selection (Polymorphic: Live vs Paper)
    let (execution_client, paper_sim): (Arc<dyn ExecutionClient>, Option<Arc<PaperExecutionSimulator>>) = if trading_mode == "LIVE" {
        let api_key = std::env::var("REVOLUT_API_KEY").unwrap_or_default();
        let priv_path = std::env::var("REVOLUT_PRIVATE_KEY_PATH").unwrap_or_else(|_| "backend/credentials/revolut_private.pem".into());
        let base_url = std::env::var("REVOLUT_BASE_URL").unwrap_or_else(|_| "https://revx.revolut.com".into());

        if api_key.is_empty() || (!std::path::Path::new(&priv_path).exists() && !priv_path.contains("-----BEGIN")) {
            error!("FATAL: TRADING_MODE=LIVE requested, but REVOLUT_API_KEY or revolut_private.pem is missing!");
            error!("Refusing to run live execution without valid credentials.");
            std::process::exit(1);
        }

        let live_client = LiveRevolutClient::new(base_url, api_key, &priv_path)
            .map_err(|e| anyhow::anyhow!("Failed to initialize LiveRevolutClient: {}", e))?;

        let client_arc: Arc<dyn ExecutionClient> = Arc::new(live_client);

        // Verify live credentials and balances
        match client_arc.get_balances().await {
            Ok(bals) => {
                info!("✅ [LIVE AUTH SUCCESS] Revolut X Balances: GBP: £{:.2}, USD: ${:.2}, BTC: {:.6}, ETH: {:.6}, SOL: {:.4}",
                    bals.get("GBP").unwrap_or(&rust_decimal::Decimal::ZERO),
                    bals.get("USD").unwrap_or(&rust_decimal::Decimal::ZERO),
                    bals.get("BTC").unwrap_or(&rust_decimal::Decimal::ZERO),
                    bals.get("ETH").unwrap_or(&rust_decimal::Decimal::ZERO),
                    bals.get("SOL").unwrap_or(&rust_decimal::Decimal::ZERO)
                );
            }
            Err(e) => {
                error!("❌ [LIVE AUTH FAILED] Could not fetch balances from Revolut X: {}", e);
            }
        }

        // Rehydrate active resting orders on boot
        match client_arc.get_active_orders().await {
            Ok(orders) => {
                info!("🔄 [REHYDRATION] Successfully rehydrated {} resting orders directly from Revolut X book", orders.len());
            }
            Err(e) => {
                warn!("⚠️ [REHYDRATION] Notice fetching active orders: {}", e);
            }
        }

        (client_arc, None)
    } else {
        let total_paper_capital = dec!(3000.00);
        let simulator = Arc::new(PaperExecutionSimulator::new(total_paper_capital));
        let paper_client = Arc::new(PaperRevolutClient::new(simulator.clone()));
        (paper_client, Some(simulator))
    };

    // 4. Load Active Pairs from Database
    let active_pair_configs = if let Some(ref db) = db_store {
        db.get_active_pair_configs().await.unwrap_or_default()
    } else {
        Vec::new()
    };
    info!("Loaded {} active pair configuration(s) from SQLite", active_pair_configs.len());

    let initial_symbols: Vec<trading_core::model::Symbol> = active_pair_configs.iter().map(|p| p.symbol.clone()).collect();

    // 5. Kraken WebSocket v2 Multiplexer (Informational Oracle)
    let kraken_ws_url = std::env::var("KRAKEN_WS_URL").unwrap_or_else(|_| "wss://ws.kraken.com/v2".into());
    let (kraken_multiplexer, _primary_rx) = KrakenWsMultiplexer::new(&kraken_ws_url, initial_symbols);
    let kraken_sub_tx = kraken_multiplexer.subscribe_sender();
    let tick_broadcast = kraken_multiplexer.tick_sender();

    // Spawn Kraken WS ingest task
    tokio::spawn(async move {
        kraken_multiplexer.run().await;
    });

    // 6. Dynamic Runner Manager
    let runner_manager = Arc::new(RunnerManager::new(
        execution_client.clone(),
        risk_engine.clone(),
        paper_sim.clone(),
        db_store.clone(),
        kraken_sub_tx,
        tick_broadcast,
    ));

    // 7. Spawn runners for all loaded active pairs
    for config in active_pair_configs {
        let sym_slash = config.symbol.as_slash();
        if let Err(e) = runner_manager.spawn_pair(config).await {
            error!("Failed to spawn runners for pair {}: {}", sym_slash, e);
        }
    }

    // 8. IPC Telemetry Server (Unix Domain Socket on Unix, TCP on Windows)
    let default_sock = if cfg!(windows) {
        "127.0.0.1:9099".to_string()
    } else {
        "/tmp/trading_engine.sock".to_string()
    };
    let socket_path = std::env::var("ENGINE_UDS_PATH").unwrap_or(default_sock);
    let ipc_server = IpcServer::new(
        socket_path,
        trading_mode.clone(),
        execution_client.clone(),
        risk_engine.clone(),
        runner_manager.clone(),
    );
    ipc_server.run().await?;

    // 9. Background Heartbeat Logger (throttled to 60s to maintain clean paper trail)
    let client_heartbeat = execution_client.clone();
    let risk_heartbeat = risk_engine.clone();
    let mode_str = trading_mode.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let bals = client_heartbeat.get_balances().await.unwrap_or_default();
            let resting = client_heartbeat.get_active_orders().await.unwrap_or_default();
            let cb_tripped = risk_heartbeat.is_circuit_breaker_tripped().await;

            info!(
                "[HEARTBEAT] Mode: {} | Balances: [GBP: £{:.2}, USD: ${:.2}, BTC: {:.6}, ETH: {:.6}, SOL: {:.4}] | Active Orders: {} | Circuit Breaker: {}",
                mode_str,
                bals.get("GBP").unwrap_or(&dec!(0.0)),
                bals.get("USD").unwrap_or(&dec!(0.0)),
                bals.get("BTC").unwrap_or(&dec!(0.0)),
                bals.get("ETH").unwrap_or(&dec!(0.0)),
                bals.get("SOL").unwrap_or(&dec!(0.0)),
                resting.len(),
                if cb_tripped { "TRIPPED" } else { "NORMAL" }
            );
        }
    });

    // 10. Wait for termination signal
    tokio::signal::ctrl_c().await?;
    warn!("Shutdown signal received. Canceling all active resting orders across venues...");
    let _ = execution_client.cancel_all_orders().await;
    info!("Successfully canceled active orders. System shut down cleanly.");

    Ok(())
}
