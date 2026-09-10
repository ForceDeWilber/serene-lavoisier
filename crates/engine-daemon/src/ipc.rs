use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tracing::{error, info, warn};
use trading_core::risk::CentralRiskEngine;
use trading_core::runner::RunnerTuningUpdate;
use trading_core::simulator::PaperExecutionSimulator;

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
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcRequest {
    GetTelemetry,
    TuneRunner {
        runner_id: String,
        paused: Option<bool>,
        step_pct: Option<Decimal>,
        rebalance_threshold_pct: Option<Decimal>,
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
        circuit_breaker_tripped: bool,
        balances: HashMap<String, Decimal>,
        runners: Vec<RunnerTelemetryDto>,
        resting_orders_count: usize,
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
    risk_engine: Arc<CentralRiskEngine>,
    simulator: Arc<PaperExecutionSimulator>,
    btc_tune_tx: watch::Sender<RunnerTuningUpdate>,
    eth_tune_tx: watch::Sender<RunnerTuningUpdate>,
}

impl IpcServer {
    pub fn new(
        socket_path: impl Into<String>,
        risk_engine: Arc<CentralRiskEngine>,
        simulator: Arc<PaperExecutionSimulator>,
        btc_tune_tx: watch::Sender<RunnerTuningUpdate>,
        eth_tune_tx: watch::Sender<RunnerTuningUpdate>,
    ) -> Self {
        Self {
            socket_path: socket_path.into(),
            risk_engine,
            simulator,
            btc_tune_tx,
            eth_tune_tx,
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let path = Path::new(&self.socket_path);
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }

        let listener = UnixListener::bind(path)?;
        info!("UDS IPC Server listening on unix:{}", self.socket_path);

        let risk = self.risk_engine.clone();
        let sim = self.simulator.clone();
        let btc_tx = self.btc_tune_tx.clone();
        let eth_tx = self.eth_tune_tx.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let r = risk.clone();
                        let s = sim.clone();
                        let b = btc_tx.clone();
                        let e = eth_tx.clone();
                        tokio::spawn(async move {
                            if let Err(err) = Self::handle_client(stream, r, s, b, e).await {
                                error!("UDS Client error: {}", err);
                            }
                        });
                    }
                    Err(e) => {
                        error!("UDS accept failed: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    async fn handle_client(
        stream: UnixStream,
        risk: Arc<CentralRiskEngine>,
        sim: Arc<PaperExecutionSimulator>,
        btc_tx: watch::Sender<RunnerTuningUpdate>,
        eth_tx: watch::Sender<RunnerTuningUpdate>,
    ) -> anyhow::Result<()> {
        let (reader, mut writer) = stream.into_split();
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
                        let wallet = sim.get_wallet().await;
                        let resting = sim.get_resting_orders().await;
                        let cb_tripped = risk.is_circuit_breaker_tripped().await;

                        let mut balances = HashMap::new();
                        balances.insert("GBP".into(), wallet.gbp);
                        balances.insert("BTC".into(), wallet.btc);
                        balances.insert("ETH".into(), wallet.eth);
                        balances.insert("SOL".into(), wallet.sol);

                        let btc_orders = resting.iter().filter(|o| o.runner_id == "runner_btc").count();
                        let eth_orders = resting.iter().filter(|o| o.runner_id == "runner_eth").count();

                        let runners = vec![
                            RunnerTelemetryDto {
                                runner_id: "runner_btc".into(),
                                symbol: "BTC/GBP".into(),
                                center_price: None,
                                inventory_base: wallet.btc,
                                realized_pnl: Decimal::ZERO,
                                total_trades: 0,
                                active_orders_count: btc_orders,
                                is_paused: btc_tx.borrow().paused,
                            },
                            RunnerTelemetryDto {
                                runner_id: "runner_eth".into(),
                                symbol: "ETH/GBP".into(),
                                center_price: None,
                                inventory_base: wallet.eth,
                                realized_pnl: Decimal::ZERO,
                                total_trades: 0,
                                active_orders_count: eth_orders,
                                is_paused: eth_tx.borrow().paused,
                            },
                        ];

                        IpcResponse::Telemetry {
                            circuit_breaker_tripped: cb_tripped,
                            balances,
                            runners,
                            resting_orders_count: resting.len(),
                        }
                    }
                    IpcRequest::TuneRunner {
                        runner_id,
                        paused,
                        step_pct,
                        rebalance_threshold_pct,
                    } => {
                        let tx = match runner_id.as_str() {
                            "runner_btc" => Some(&btc_tx),
                            "runner_eth" => Some(&eth_tx),
                            _ => None,
                        };

                        if let Some(target_tx) = tx {
                            let mut current = target_tx.borrow().clone();
                            if let Some(p) = paused {
                                current.paused = p;
                            }
                            if step_pct.is_some() {
                                current.step_pct = step_pct;
                            }
                            if rebalance_threshold_pct.is_some() {
                                current.rebalance_threshold_pct = rebalance_threshold_pct;
                            }
                            let _ = target_tx.send(current);
                            IpcResponse::Ack {
                                message: format!("Tuned runner {}", runner_id),
                            }
                        } else {
                            IpcResponse::Error {
                                error: format!("Runner not found: {}", runner_id),
                            }
                        }
                    }
                    IpcRequest::EmergencyKillSwitch { reason } => {
                        warn!("EMERGENCY KILL SWITCH TRIGGERED VIA IPC! Reason: {}", reason);
                        risk.trip_circuit_breaker(&reason).await;
                        let canceled = sim.cancel_all_orders().await;
                        // Pause all runners
                        let _ = btc_tx.send(RunnerTuningUpdate {
                            paused: true,
                            step_pct: None,
                            rebalance_threshold_pct: None,
                        });
                        let _ = eth_tx.send(RunnerTuningUpdate {
                            paused: true,
                            step_pct: None,
                            rebalance_threshold_pct: None,
                        });
                        IpcResponse::Ack {
                            message: format!("Emergency kill-switch executed. Canceled {} resting orders across all venues.", canceled.len()),
                        }
                    }
                    IpcRequest::ResetCircuitBreaker => {
                        IpcResponse::Ack {
                            message: "Circuit breaker reset signal acknowledged".into(),
                        }
                    }
                },
                Err(err) => IpcResponse::Error {
                    error: format!("Invalid JSON IPC request: {}", err),
                },
            };

            let resp_str = serde_json::to_string(&response)? + "\n";
            writer.write_all(resp_str.as_bytes()).await?;
            writer.flush().await?;
            line.clear();
        }

        Ok(())
    }
}
