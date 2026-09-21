use chrono::{TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};
use trading_core::model::{MarketTick, Symbol};

#[derive(Debug, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct BinanceTradePayload {
    #[serde(default)]
    pub e: Option<String>, // Event type ("trade")
    #[serde(default)]
    pub E: Option<i64>,    // Event time
    #[serde(default)]
    pub s: Option<String>, // Symbol ("SOLUSDT")
    #[serde(default)]
    pub t: Option<i64>,    // Trade ID
    #[serde(default)]
    pub p: Option<String>, // Price
    #[serde(default)]
    pub q: Option<String>, // Quantity
    #[serde(default)]
    pub T: Option<i64>,    // Trade time
    #[serde(default)]
    pub m: Option<bool>,   // Is buyer the market maker?
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BinanceCombinedStreamMessage {
    #[serde(default)]
    pub stream: Option<String>,
    #[serde(default)]
    pub data: Option<BinanceTradePayload>,
}

pub struct BinanceWsMultiplexer {
    ws_base_url: String,
    symbols: Vec<Symbol>,
    tick_tx: broadcast::Sender<MarketTick>,
    sub_tx: tokio::sync::mpsc::UnboundedSender<Vec<Symbol>>,
    sub_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<Symbol>>,
}

impl BinanceWsMultiplexer {
    pub fn new(ws_base_url: impl Into<String>, symbols: Vec<Symbol>) -> (Self, broadcast::Receiver<MarketTick>) {
        let (tick_tx, tick_rx) = broadcast::channel(4096);
        let (sub_tx, sub_rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Self {
                ws_base_url: ws_base_url.into(),
                symbols,
                tick_tx,
                sub_tx,
                sub_rx,
            },
            tick_rx,
        )
    }

    pub fn new_with_sender(ws_base_url: impl Into<String>, symbols: Vec<Symbol>, tick_tx: broadcast::Sender<MarketTick>) -> Self {
        let (sub_tx, sub_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            ws_base_url: ws_base_url.into(),
            symbols,
            tick_tx,
            sub_tx,
            sub_rx,
        }
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

    /// Maps an internal Symbol (e.g. SOL/GBP) to Binance stream name (e.g. solusdt@trade)
    pub fn symbol_to_binance_stream(symbol: &Symbol) -> String {
        let base_lower = symbol.base.to_lowercase();
        format!("{}usdt@trade", base_lower)
    }

    /// Maps Binance symbol string (e.g. "SOLUSDT") back to matching internal Symbol with USDT quote
    pub fn match_binance_symbol(binance_sym: &str, active_symbols: &HashSet<Symbol>) -> Option<Symbol> {
        let sym_upper = binance_sym.to_uppercase();
        for s in active_symbols {
            let expected = format!("{}USDT", s.base.to_uppercase());
            if sym_upper == expected {
                return Some(Symbol::new(&s.base, "USDT"));
            }
        }
        None
    }

    /// Constructs the Binance WebSocket URL for current active symbols
    fn build_ws_url(base: &str, symbols: &HashSet<Symbol>) -> String {
        let trimmed = base.trim_end_matches('/');
        if symbols.is_empty() {
            return format!("{}/ws/solusdt@trade", trimmed);
        }

        let streams: Vec<String> = symbols
            .iter()
            .map(Self::symbol_to_binance_stream)
            .collect();

        if streams.len() == 1 {
            format!("{}/ws/{}", trimmed, streams[0])
        } else {
            format!("{}/stream?streams={}", trimmed, streams.join("/"))
        }
    }

    pub async fn run(mut self) {
        let mut backoff = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(15);
        let mut active_symbols: HashSet<Symbol> = self.symbols.drain(..).collect();
        if active_symbols.is_empty() {
            active_symbols.insert(Symbol::sol_gbp());
        }

        loop {
            let ws_url = Self::build_ws_url(&self.ws_base_url, &active_symbols);
            info!("Connecting to Binance WebSocket at {}", ws_url);

            match connect_async(&ws_url).await {
                Ok((ws_stream, response)) => {
                    info!("Successfully connected to Binance WS (HTTP {})", response.status());
                    backoff = Duration::from_secs(1); // Reset backoff on successful connect

                    let (mut write, mut read) = ws_stream.split();
                    let mut ping_interval = tokio::time::interval(Duration::from_secs(180));

                    loop {
                        tokio::select! {
                            _ = ping_interval.tick() => {
                                if let Err(e) = write.send(Message::Ping(vec![].into())).await {
                                    warn!("Failed to send ping to Binance WS: {}", e);
                                    break;
                                }
                            }

                            Some(new_symbols) = self.sub_rx.recv() => {
                                let mut needs_reconnect = false;
                                for s in new_symbols {
                                    if active_symbols.insert(s) {
                                        needs_reconnect = true;
                                    }
                                }
                                if needs_reconnect {
                                    info!("New symbols registered. Reconnecting Binance WebSocket with updated stream list...");
                                    break;
                                }
                            }

                            msg_opt = read.next() => {
                                match msg_opt {
                                    Some(Ok(Message::Text(text))) => {
                                        Self::handle_message(&text, &active_symbols, &self.tick_tx);
                                    }
                                    Some(Ok(Message::Ping(payload))) => {
                                        let _ = write.send(Message::Pong(payload)).await;
                                    }
                                    Some(Ok(Message::Close(frame))) => {
                                        warn!("Binance WS connection closed by server: {:?}", frame);
                                        break;
                                    }
                                    Some(Err(e)) => {
                                        warn!("Binance WS read error: {}", e);
                                        break;
                                    }
                                    None => {
                                        warn!("Binance WS stream ended unexpectedly");
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Binance WebSocket connection error: {}. Reconnecting in {}s...", e, backoff.as_secs());
                }
            }

            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(max_backoff);
        }
    }

    fn handle_message(text: &str, active_symbols: &HashSet<Symbol>, tick_tx: &broadcast::Sender<MarketTick>) {
        // Try parsing combined stream format first
        let trade = if let Ok(combined) = serde_json::from_str::<BinanceCombinedStreamMessage>(text) {
            if combined.data.is_some() {
                combined.data
            } else {
                serde_json::from_str::<BinanceTradePayload>(text).ok()
            }
        } else {
            serde_json::from_str::<BinanceTradePayload>(text).ok()
        };

        if let Some(t) = trade {
            if let (Some(ref sym_str), Some(ref price_str)) = (&t.s, &t.p) {
                if let Some(engine_sym) = Self::match_binance_symbol(sym_str, active_symbols) {
                    if let Ok(price) = Decimal::from_str(price_str) {
                        let timestamp = if let Some(ts_ms) = t.T {
                            Utc.timestamp_millis_opt(ts_ms).single().unwrap_or_else(Utc::now)
                        } else {
                            Utc::now()
                        };

                        let tick = MarketTick {
                            symbol: engine_sym,
                            bid: price,
                            ask: price,
                            last: price,
                            timestamp,
                        };

                        let _ = tick_tx.send(tick);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binance_stream_naming() {
        assert_eq!(BinanceWsMultiplexer::symbol_to_binance_stream(&Symbol::sol_gbp()), "solusdt@trade");
        assert_eq!(BinanceWsMultiplexer::symbol_to_binance_stream(&Symbol::btc_gbp()), "btcusdt@trade");
        assert_eq!(BinanceWsMultiplexer::symbol_to_binance_stream(&Symbol::eth_gbp()), "ethusdt@trade");
    }

    #[test]
    fn test_symbol_matching() {
        let mut set = HashSet::new();
        set.insert(Symbol::sol_gbp());
        set.insert(Symbol::btc_gbp());

        let matched = BinanceWsMultiplexer::match_binance_symbol("SOLUSDT", &set);
        assert_eq!(matched, Some(Symbol::new("SOL", "USDT")));

        let matched_btc = BinanceWsMultiplexer::match_binance_symbol("BTCUSDT", &set);
        assert_eq!(matched_btc, Some(Symbol::new("BTC", "USDT")));

        let unmatched = BinanceWsMultiplexer::match_binance_symbol("DOGEUSDT", &set);
        assert_eq!(unmatched, None);
    }

    #[test]
    fn test_handle_raw_trade_message() {
        let (tx, mut rx) = broadcast::channel(16);
        let mut set = HashSet::new();
        set.insert(Symbol::sol_gbp());

        let sample_json = r#"{
            "e": "trade",
            "E": 1789895926388,
            "s": "SOLUSDT",
            "t": 12345,
            "p": "108.24000000",
            "q": "1.50000000",
            "T": 1789895926388,
            "m": true
        }"#;

        BinanceWsMultiplexer::handle_message(sample_json, &set, &tx);
        let tick = rx.try_recv().expect("Expected MarketTick from trade message");
        assert_eq!(tick.symbol, Symbol::new("SOL", "USDT"));
        assert_eq!(tick.bid, Decimal::from_str("108.24000000").unwrap());
        assert_eq!(tick.ask, Decimal::from_str("108.24000000").unwrap());
    }

    #[test]
    fn test_handle_combined_stream_message() {
        let (tx, mut rx) = broadcast::channel(16);
        let mut set = HashSet::new();
        set.insert(Symbol::sol_gbp());

        let sample_json = r#"{
            "stream": "solusdt@trade",
            "data": {
                "e": "trade",
                "E": 1789895926388,
                "s": "SOLUSDT",
                "t": 12345,
                "p": "108.35000000",
                "q": "2.00000000",
                "T": 1789895926388,
                "m": false
            }
        }"#;

        BinanceWsMultiplexer::handle_message(sample_json, &set, &tx);
        let tick = rx.try_recv().expect("Expected MarketTick from combined stream message");
        assert_eq!(tick.symbol, Symbol::new("SOL", "USDT"));
        assert_eq!(tick.bid, Decimal::from_str("108.35000000").unwrap());
    }
}
