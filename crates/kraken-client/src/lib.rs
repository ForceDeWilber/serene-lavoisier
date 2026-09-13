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
    #[serde(skip_serializing_if = "Option::is_none")]
    event_trigger: Option<String>,
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
    sub_tx: tokio::sync::mpsc::UnboundedSender<Vec<Symbol>>,
    sub_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<Symbol>>,
}

impl KrakenWsMultiplexer {
    pub fn new(ws_url: impl Into<String>, symbols: Vec<Symbol>) -> (Self, broadcast::Receiver<MarketTick>) {
        let (tick_tx, tick_rx) = broadcast::channel(1024);
        let (sub_tx, sub_rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Self {
                ws_url: ws_url.into(),
                symbols,
                tick_tx,
                sub_tx,
                sub_rx,
            },
            tick_rx,
        )
    }

    pub fn subscribe_receiver(&self) -> broadcast::Receiver<MarketTick> {
        self.tick_tx.subscribe()
    }

    pub fn tick_sender(&self) -> broadcast::Sender<MarketTick> {
        self.tick_tx.clone()
    }

    pub fn subscribe_sender(&self) -> tokio::sync::mpsc::UnboundedSender<Vec<Symbol>> {
        self.sub_tx.clone()
    }

    pub fn subscribe_symbol(&self, symbol: Symbol) {
        let _ = self.sub_tx.send(vec![symbol]);
    }

    pub async fn run(mut self) {
        let mut backoff = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(30);
        let mut active_symbols: std::collections::HashSet<Symbol> = self.symbols.drain(..).collect();

        loop {
            info!("Connecting to Kraken WebSocket v2 at {}", self.ws_url);
            match connect_async(&self.ws_url).await {
                Ok((ws_stream, response)) => {
                    info!("Successfully connected to Kraken WS (status: {})", response.status());
                    backoff = Duration::from_secs(1); // Reset backoff on success

                    let (mut write, mut read) = ws_stream.split();

                    // Send initial subscription request for all accumulated symbols
                    let symbol_strs: Vec<String> = active_symbols.iter().map(|s| s.as_slash()).collect();
                    if !symbol_strs.is_empty() {
                        let sub_msg = SubscribeRequest {
                            method: "subscribe".into(),
                            params: SubscribeParams {
                                channel: "ticker".into(),
                                symbol: symbol_strs.clone(),
                                event_trigger: Some("bbo".into()),
                            },
                        };

                        if let Ok(json) = serde_json::to_string(&sub_msg) {
                            info!("Subscribing to Kraken ticker for symbols: {:?}", symbol_strs);
                            if let Err(e) = write.send(Message::Text(json.into())).await {
                                error!("Failed to send subscribe message: {}", e);
                                continue;
                            }
                        }
                    }

                    // Message processing & dynamic subscription loop
                    loop {
                        tokio::select! {
                            Some(new_syms) = self.sub_rx.recv() => {
                                let mut to_sub = Vec::new();
                                for s in new_syms {
                                    if active_symbols.insert(s.clone()) {
                                        to_sub.push(s.as_slash());
                                    }
                                }
                                if !to_sub.is_empty() {
                                    let sub_msg = SubscribeRequest {
                                        method: "subscribe".into(),
                                        params: SubscribeParams {
                                            channel: "ticker".into(),
                                            symbol: to_sub.clone(),
                                            event_trigger: Some("bbo".into()),
                                        },
                                    };
                                    if let Ok(json) = serde_json::to_string(&sub_msg) {
                                        info!("[KRAKEN-WS] Hot-subscribing to symbols: {:?}", to_sub);
                                        let _ = write.send(Message::Text(json.into())).await;
                                    }
                                }
                            }
                            msg_opt = read.next() => {
                                match msg_opt {
                                    Some(Ok(Message::Text(text))) => {
                                        self.handle_message(&text);
                                    }
                                    Some(Ok(Message::Ping(payload))) => {
                                        let _ = write.send(Message::Pong(payload)).await;
                                    }
                                    Some(Ok(Message::Close(frame))) => {
                                        warn!("Kraken WS closed by remote: {:?}", frame);
                                        break;
                                    }
                                    Some(Err(e)) => {
                                        error!("Kraken WS error: {}", e);
                                        break;
                                    }
                                    None => {
                                        warn!("Kraken WS stream ended");
                                        break;
                                    }
                                    _ => {}
                                }
                            }
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
