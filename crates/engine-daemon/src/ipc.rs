use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{error, info, warn};
use trading_core::execution::ExecutionClient;
use trading_core::model::PairConfig;
use trading_core::risk::CentralRiskEngine;
use crate::manager::RunnerManager;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use tokio::net::UnixListener;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerTelemetryDto {
    pub runner_id: String,
    pub symbol: String,
    pub center_price: Option<Decimal>,
    pub inventory_base: Decimal,
    pub realized_pnl: Decimal,
    pub total_trades: usize,
    pub active_orders_count: usize,
    pub is_paused: bool,
    #[serde(default)]
    pub effective_center: Option<Decimal>,
    #[serde(default)]
    pub dynamic_step_pct: Option<Decimal>,
    #[serde(default)]
    pub rolling_volatility_pct: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SniperTelemetryDto {
    pub runner_id: String,
    pub symbol: String,
    pub enabled: bool,
    pub impulse_threshold_pct: Decimal,
    pub order_size_gbp: Decimal,
    pub total_snipes: usize,
    pub successful_snipes: usize,
    pub total_sniper_profit_gbp: Decimal,
    pub total_fees_paid_gbp: Decimal,
    pub average_lead_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderTelemetryDto {
    pub id: String,
    pub client_order_id: String,
    pub runner_id: String,
    pub symbol: String,
    pub side: String,
    pub price: Decimal,
    pub qty: Decimal,
    pub value_gbp: Decimal,
    pub created_at: String,
    pub rung_level: i32,
    pub distance_pct: Decimal,
    pub is_live: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcRequest {
    GetTelemetry,
    AddPair {
        config: PairConfig,
    },
    ListPairs,
    TuneRunner {
        runner_id: String,
        paused: Option<bool>,
        step_pct: Option<Decimal>,
        rebalance_threshold_pct: Option<Decimal>,
        #[serde(default)]
        order_size_fiat: Option<Decimal>,
        #[serde(default)]
        dynamic_pricing_enabled: Option<bool>,
        #[serde(default)]
        inventory_gamma: Option<Decimal>,
    },
    TuneSniper {
        runner_id: String,
        enabled: Option<bool>,
        impulse_threshold_pct: Option<Decimal>,
        order_size_gbp: Option<Decimal>,
    },
    ToggleSniper {
        runner_id: String,
        enabled: Option<bool>,
    },
    EmergencyKillSwitch {
        reason: String,
    },
    ResetCircuitBreaker,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcResponse {
    Telemetry {
        trading_mode: String,
        circuit_breaker_tripped: bool,
        circuit_breaker_reason: String,
        balances: HashMap<String, Decimal>,
        runners: Vec<RunnerTelemetryDto>,
        snipers: Vec<SniperTelemetryDto>,
        resting_orders_count: usize,
        active_orders: Vec<OrderTelemetryDto>,
    },
    PairsList {
        pairs: Vec<PairConfig>,
    },
    Ack {
        message: String,
    },
    Error {
        error: String,
    },
}

pub struct IpcServer {
    socket_path: String,
    trading_mode: String,
    execution_client: Arc<dyn ExecutionClient>,
    risk_engine: Arc<CentralRiskEngine>,
    runner_manager: Arc<RunnerManager>,
}

impl IpcServer {
    pub fn new(
        socket_path: impl Into<String>,
        trading_mode: impl Into<String>,
        execution_client: Arc<dyn ExecutionClient>,
        risk_engine: Arc<CentralRiskEngine>,
        runner_manager: Arc<RunnerManager>,
    ) -> Self {
        Self {
            socket_path: socket_path.into(),
            trading_mode: trading_mode.into(),
            execution_client,
            risk_engine,
            runner_manager,
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let client = self.execution_client.clone();
        let risk = self.risk_engine.clone();
        let mode = self.trading_mode.clone();
        let manager = self.runner_manager.clone();

        #[cfg(unix)]
        {
            if !self.socket_path.contains(':') {
                let path = Path::new(&self.socket_path);
                if path.exists() {
                    let _ = std::fs::remove_file(path);
                }

                let listener = UnixListener::bind(&self.socket_path)?;
                info!("Unix Domain Socket IPC Server listening on {}", self.socket_path);

                let c = client.clone();
                let r = risk.clone();
                let m = mode.clone();
                let mgr = manager.clone();
                tokio::spawn(async move {
                    loop {
                        match listener.accept().await {
                            Ok((stream, _)) => {
                                let c_clone = c.clone();
                                let r_clone = r.clone();
                                let m_clone = m.clone();
                                let mgr_clone = mgr.clone();
                                tokio::spawn(async move {
                                    let (reader, writer) = stream.into_split();
                                    if let Err(err) = Self::handle_stream(reader, writer, c_clone, r_clone, m_clone, mgr_clone).await {
                                        error!("IPC Client error: {}", err);
                                    }
                                });
                            }
                            Err(e) => {
                                error!("IPC accept failed: {}", e);
                                break;
                            }
                        }
                    }
                });

                return Ok(());
            }
        }

        // TCP fallback (Windows or host:port)
        let addr = if self.socket_path.contains(':') {
            self.socket_path.clone()
        } else {
            "127.0.0.1:9099".to_string()
        };

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("TCP IPC Server listening on {}", addr);

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let c_clone = client.clone();
                        let r_clone = risk.clone();
                        let m_clone = mode.clone();
                        let mgr_clone = manager.clone();
                        tokio::spawn(async move {
                            let (reader, writer) = stream.into_split();
                            if let Err(err) = Self::handle_stream(reader, writer, c_clone, r_clone, m_clone, mgr_clone).await {
                                error!("TCP IPC Client error: {}", err);
                            }
                        });
                    }
                    Err(e) => {
                        error!("TCP IPC accept failed: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    async fn handle_stream<R, W>(
        reader: R,
        mut writer: W,
        client: Arc<dyn ExecutionClient>,
        risk: Arc<CentralRiskEngine>,
        mode: String,
        manager: Arc<RunnerManager>,
    ) -> anyhow::Result<()>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut buf_reader = BufReader::new(reader);
        let mut line = String::new();

        while buf_reader.read_line(&mut line).await? > 0 {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                line.clear();
                continue;
            }

            let response = match serde_json::from_str::<IpcRequest>(trimmed) {
                Ok(req) => match req {
                    IpcRequest::GetTelemetry => {
                        let balances = client.get_balances().await.unwrap_or_default();
                        let active_orders_raw = client.get_active_orders().await.unwrap_or_default();
                        let cb_tripped = risk.is_circuit_breaker_tripped().await;

                        // Dynamic runner and sniper telemetry across all active pairs
                        let (runners, snipers) = manager.get_telemetry_dtos(&balances, &active_orders_raw).await;

                        let active_dtos: Vec<OrderTelemetryDto> = active_orders_raw
                            .iter()
                            .map(|o| {
                                let val = (o.price * o.qty).round_dp(2);
                                OrderTelemetryDto {
                                    id: o.client_order_id.clone(),
                                    client_order_id: o.client_order_id.clone(),
                                    runner_id: o.runner_id.clone(),
                                    symbol: o.symbol.as_slash(),
                                    side: o.side.to_string().to_uppercase(),
                                    price: o.price,
                                    qty: o.qty,
                                    value_gbp: val,
                                    created_at: o.created_at.format("%H:%M:%S").to_string(),
                                    rung_level: if o.side == trading_core::model::OrderSide::Buy { -1 } else { 1 },
                                    distance_pct: Decimal::ZERO,
                                    is_live: mode == "LIVE",
                                }
                            })
                            .collect();

                        IpcResponse::Telemetry {
                            trading_mode: mode.clone(),
                            circuit_breaker_tripped: cb_tripped,
                            circuit_breaker_reason: if cb_tripped { "Portfolio drawdown exceeded threshold" } else { "Normal" }.into(),
                            balances,
                            runners,
                            snipers,
                            resting_orders_count: active_dtos.len(),
                            active_orders: active_dtos,
                        }
                    }
                    IpcRequest::AddPair { config } => {
                        let sym = config.symbol.as_slash();
                        match manager.spawn_pair(config).await {
                            Ok(()) => IpcResponse::Ack {
                                message: format!("Pair {} dynamically spawned and registered", sym),
                            },
                            Err(e) => IpcResponse::Error {
                                error: format!("Failed to spawn pair {}: {}", sym, e),
                            },
                        }
                    }
                    IpcRequest::ListPairs => {
                        let pairs = manager.get_all_pairs().await;
                        IpcResponse::PairsList { pairs }
                    }
                    IpcRequest::TuneRunner {
                        runner_id,
                        paused,
                        step_pct,
                        rebalance_threshold_pct,
                        order_size_fiat,
                        dynamic_pricing_enabled,
                        inventory_gamma,
                    } => {
                        if manager
                            .tune_runner(
                                &runner_id,
                                paused,
                                step_pct,
                                rebalance_threshold_pct,
                                order_size_fiat,
                                dynamic_pricing_enabled,
                                inventory_gamma,
                            )
                            .await
                        {
                            IpcResponse::Ack {
                                message: format!("Tuned runner {}", runner_id),
                            }
                        } else {
                            IpcResponse::Error {
                                error: format!("Runner {} not found in active pairs", runner_id),
                            }
                        }
                    }
                    IpcRequest::TuneSniper {
                        runner_id,
                        enabled,
                        impulse_threshold_pct,
                        order_size_gbp,
                    } => {
                        if manager.tune_sniper(&runner_id, enabled, impulse_threshold_pct, order_size_gbp).await {
                            IpcResponse::Ack {
                                message: format!("Tuned sniper {}", runner_id),
                            }
                        } else {
                            IpcResponse::Error {
                                error: format!("Sniper {} not found in active pairs", runner_id),
                            }
                        }
                    }
                    IpcRequest::ToggleSniper { runner_id, enabled } => {
                        if let Some(state) = manager.toggle_sniper(&runner_id, enabled).await {
                            IpcResponse::Ack {
                                message: format!("Sniper {} enabled={}", runner_id, state),
                            }
                        } else {
                            IpcResponse::Error {
                                error: format!("Sniper {} not found in active pairs", runner_id),
                            }
                        }
                    }
                    IpcRequest::EmergencyKillSwitch { reason } => {
                        warn!("EMERGENCY KILL SWITCH TRIGGERED: {}", reason);
                        manager.emergency_kill_switch().await;
                        IpcResponse::Ack {
                            message: format!("Emergency kill switch executed: {}", reason),
                        }
                    }
                    IpcRequest::ResetCircuitBreaker => {
                        manager.reset_circuit_breaker().await;
                        IpcResponse::Ack {
                            message: "Circuit breaker reset. All pair runners resumed.".into(),
                        }
                    }
                },
                Err(err) => IpcResponse::Error {
                    error: format!("Invalid IPC request format: {}", err),
                },
            };

            let resp_bytes = serde_json::to_vec(&response)?;
            writer.write_all(&resp_bytes).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;

            line.clear();
        }

        Ok(())
    }
}
