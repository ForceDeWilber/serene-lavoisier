use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};
use trading_core::model::{MarketTick, Symbol};

#[derive(Debug, Serialize)]
struct SubscribeRequest {
    method: String,
    params: SubscribeParams,
}

#[derive(Debug, Serialize)]
struct SubscribeParams {
    channel: String,
    symbol: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct KrakenTickerData {
    symbol: String,
    #[serde(default)]
    bid: Option<serde_json::Value>,
    #[serde(default)]
    ask: Option<serde_json::Value>,
    #[serde(default)]
    last: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct KrakenWsMessage {
    channel: Option<String>,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    data: Option<Vec<KrakenTickerData>>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    success: Option<bool>,
}

pub struct KrakenWsMultiplexer {
    ws_url: String,
    symbols: Vec<Symbol>,
    tick_tx: broadcast::Sender<MarketTick>,
}

impl KrakenWsMultiplexer {
    pub fn new(ws_url: impl Into<String>, symbols: Vec<Symbol>) -> (Self, broadcast::Receiver<MarketTick>) {
        let (tick_tx, tick_rx) = broadcast::channel(1024);
        (
            Self {
                ws_url: ws_url.into(),
                symbols,
                tick_tx,
            },
            tick_rx,
        )
    }

    pub fn subscribe_receiver(&self) -> broadcast::Receiver<MarketTick> {
        self.tick_tx.subscribe()
    }

    pub async fn run(self) {
        let mut backoff = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(30);

        loop {
            info!("Connecting to Kraken WebSocket v2 at {}", self.ws_url);
            match connect_async(&self.ws_url).await {
                Ok((ws_stream, response)) => {
                    info!("Successfully connected to Kraken WS (status: {})", response.status());
                    backoff = Duration::from_secs(1); // Reset backoff on success

                    let (mut write, mut read) = ws_stream.split();

                    // Send subscription request for tickers
                    let symbol_strs: Vec<String> = self.symbols.iter().map(|s| s.as_slash()).collect();
                    let sub_msg = SubscribeRequest {
                        method: "subscribe".into(),
                        params: SubscribeParams {
                            channel: "ticker".into(),
                            symbol: symbol_strs.clone(),
                        },
                    };

                    if let Ok(json) = serde_json::to_string(&sub_msg) {
                        info!("Subscribing to Kraken ticker for symbols: {:?}", symbol_strs);
                        if let Err(e) = write.send(Message::Text(json.into())).await {
                            error!("Failed to send subscribe message: {}", e);
                            continue;
                        }
                    }

                    // Message processing loop
                    while let Some(msg_result) = read.next().await {
                        match msg_result {
                            Ok(Message::Text(text)) => {
                                self.handle_message(&text);
                            }
                            Ok(Message::Ping(payload)) => {
                                let _ = write.send(Message::Pong(payload)).await;
                            }
                            Ok(Message::Close(frame)) => {
                                warn!("Kraken WS closed by remote: {:?}", frame);
                                break;
                            }
                            Err(e) => {
                                error!("Kraken WS error: {}", e);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to connect to Kraken WS: {}. Retrying in {:?}...", e, backoff);
                }
            }

            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(max_backoff);
        }
    }

    fn handle_message(&self, text: &str) {
        if let Ok(msg) = serde_json::from_str::<KrakenWsMessage>(text) {
            if let Some(channel) = msg.channel.as_deref() {
                if channel == "ticker" {
                    if let Some(data_items) = msg.data {
                        for item in data_items {
                            let (base, quote) = match item.symbol.split_once('/') {
                                Some((b, q)) => (b, q),
                                None => continue,
                            };
                            let symbol = Symbol::new(base, quote);

                            let parse_dec = |v: Option<&serde_json::Value>| -> Option<Decimal> {
                                match v? {
                                    serde_json::Value::Number(n) => Decimal::from_str(&n.to_string()).ok(),
                                    serde_json::Value::String(s) => Decimal::from_str(s).ok(),
                                    _ => None,
                                }
                            };

                            let bid = parse_dec(item.bid.as_ref()).unwrap_or_default();
                            let ask = parse_dec(item.ask.as_ref()).unwrap_or_default();
                            let last = parse_dec(item.last.as_ref()).unwrap_or(bid);

                            if bid > Decimal::ZERO || ask > Decimal::ZERO || last > Decimal::ZERO {
                                let tick = MarketTick {
                                    symbol,
                                    bid,
                                    ask,
                                    last,
                                    timestamp: Utc::now(),
                                };
                                debug!("Kraken Tick: {} | Bid: {} | Ask: {}", tick.symbol, tick.bid, tick.ask);
                                let _ = self.tick_tx.send(tick);
                            }
                        }
                    }
                } else if channel == "heartbeat" {
                    debug!("Kraken WS heartbeat received");
                }
            }
        }
    }
}
