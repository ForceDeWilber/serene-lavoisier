pub mod rate_limiter;
pub mod signer;

use async_trait::async_trait;
use chrono::Utc;
use rate_limiter::TokenBucketRateLimiter;
use reqwest::{header, Client};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use signer::Ed25519Signer;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::Instant;
use tracing::{error, info};
use trading_core::execution::{ExecutionClient, HistoricalOrder};
use trading_core::model::{Order, OrderSide, OrderStatus, OrderType, Symbol};
use trading_core::simulator::PaperExecutionSimulator;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct LimitConfiguration {
    pub base_size: String,
    pub price: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub execution_instructions: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OrderConfiguration {
    pub limit: LimitConfiguration,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RevolutOrderPayload {
    pub client_order_id: String,
    pub symbol: String, // e.g. "BTC-GBP"
    pub side: String,   // "BUY" or "SELL"
    pub order_configuration: OrderConfiguration,
}

/// Live HTTP/2 client for Revolut X with Ed25519 signing and rate limiting
#[derive(Debug, Clone)]
struct CachedBalanceSheet {
    total: HashMap<String, Decimal>,
    available: HashMap<String, Decimal>,
    reserved: HashMap<String, Decimal>,
}

pub struct LiveRevolutClient {
    base_url: String,
    client: Client,
    signer: Ed25519Signer,
    rate_limiter: TokenBucketRateLimiter,
    cached_balances: RwLock<Option<(Instant, CachedBalanceSheet)>>,
    cached_active_orders: RwLock<Option<(Instant, Vec<Order>)>>,
    cached_historical_orders: RwLock<Option<(Instant, Vec<HistoricalOrder>)>>,
    cached_bbo: RwLock<HashMap<String, (Instant, (Decimal, Decimal))>>,
}

impl LiveRevolutClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        private_key_source: &str,
    ) -> Result<Self, String> {
        let signer = Ed25519Signer::from_file_or_hex(api_key, private_key_source).map_err(|e| e.to_string())?;

        let mut default_headers = header::HeaderMap::new();
        default_headers.insert(header::ACCEPT, header::HeaderValue::from_static("application/json"));
        default_headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"),
        );

        let client = Client::builder()
            .default_headers(default_headers)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .tcp_nodelay(true)
            .build()
            .map_err(|e| e.to_string())?;

        // Capacity 5.0 bursts with 8.0 tokens/sec refill to comfortably stay beneath Revolut X limits
        let rate_limiter = TokenBucketRateLimiter::new(5.0, 480.0);

        Ok(Self {
            base_url: base_url.into(),
            client,
            signer,
            rate_limiter,
            cached_balances: RwLock::new(None),
            cached_active_orders: RwLock::new(None),
            cached_historical_orders: RwLock::new(None),
            cached_bbo: RwLock::new(HashMap::new()),
        })
    }

    /// Dispatches a raw post-only or aggressive limit order directly to Revolut X venue
    pub async fn send_order(&self, order: &Order, is_post_only: bool) -> Result<Order, String> {
        self.rate_limiter.acquire().await;

        let execution_instructions = if is_post_only {
            vec!["post_only".to_string()]
        } else {
            vec![]
        };

        let payload = RevolutOrderPayload {
            client_order_id: order.client_order_id.clone(),
            symbol: order.symbol.as_dash(),
            side: order.side.to_string().to_uppercase(),
            order_configuration: OrderConfiguration {
                limit: LimitConfiguration {
                    base_size: format!("{:.8}", order.qty),
                    price: format!("{:.2}", order.price),
                    execution_instructions,
                },
            },
        };

        let body_str = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
        let timestamp = Utc::now().timestamp_millis();
        let path = "/api/1.0/orders";
        let signature = self.signer.sign_payload(timestamp, "POST", path, &body_str);

        let url = format!("{}{}", self.base_url, path);
        let mut resp = self
            .client
            .post(&url)
            .header("X-Revx-API-Key", self.signer.api_key())
            .header("X-Revx-Timestamp", timestamp.to_string())
            .header("X-Revx-Signature", signature)
            .header(header::ACCEPT, "application/json")
            .header(header::CONTENT_TYPE, "application/json")
            .body(body_str.clone())
            .send()
            .await
            .map_err(|e| format!("HTTP order POST error: {}", e))?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let jitter_ms = 100 + (Utc::now().timestamp_millis() % 250) as u64;
            let backoff_ms = 400 + jitter_ms;
            tracing::warn!("[RATE-LIMIT] Revolut X 429 on send_order, backing off {backoff_ms}ms and retrying once...");
            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            self.rate_limiter.acquire().await;
            let ts2 = Utc::now().timestamp_millis();
            let sig2 = self.signer.sign_payload(ts2, "POST", path, &body_str);
            resp = self
                .client
                .post(&url)
                .header("X-Revx-API-Key", self.signer.api_key())
                .header("X-Revx-Timestamp", ts2.to_string())
                .header("X-Revx-Signature", sig2)
                .header(header::ACCEPT, "application/json")
                .header(header::CONTENT_TYPE, "application/json")
                .body(body_str)
                .send()
                .await
                .map_err(|e| format!("HTTP order POST retry error: {}", e))?;
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            error!("[ORDER-REJECT] Revolut X rejected order [{status}]: {err_text}");
            return Err(format!("Revolut X rejected order [{status}]: {err_text}"));
        }

        // Invalidate cached balances, active orders, and historical orders immediately
        *self.cached_balances.write().await = None;
        *self.cached_active_orders.write().await = None;
        *self.cached_historical_orders.write().await = None;

        info!(
            "[ORDER-DISPATCH] [VENUE-ACK] Revolut X placed order: {} {} {} @ £{} ({})",
            order.side, order.qty, order.symbol, order.price, order.client_order_id
        );
        Ok(order.clone())
    }

    async fn fetch_balance_sheet(&self) -> Result<CachedBalanceSheet, String> {
        {
            let cache = self.cached_balances.read().await;
            if let Some((cached_at, ref sheet)) = *cache {
                if cached_at.elapsed() < Duration::from_millis(1500) {
                    return Ok(sheet.clone());
                }
            }
        }

        self.rate_limiter.acquire().await;

        let timestamp = Utc::now().timestamp_millis();
        let path = "/api/1.0/balances";
        let signature = self.signer.sign_payload(timestamp, "GET", path, "");

        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .header("X-Revx-API-Key", self.signer.api_key())
            .header("X-Revx-Timestamp", timestamp.to_string())
            .header("X-Revx-Signature", signature)
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| format!("HTTP balance request error: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Revolut X balance fetch failed [HTTP {}]", resp.status()));
        }

        #[derive(Deserialize, Debug)]
        struct BalanceEntry {
            pub currency: Option<String>,
            #[serde(default)]
            pub available: Option<String>,
            #[serde(default)]
            pub reserved: Option<String>,
            #[serde(default)]
            pub total: Option<String>,
        }

        let entries: Vec<BalanceEntry> = resp.json().await.map_err(|e| format!("Failed to parse balances JSON: {}", e))?;
        let mut total_map = HashMap::new();
        let mut avail_map = HashMap::new();
        let mut resvd_map = HashMap::new();
        for entry in entries {
            if let Some(curr) = entry.currency {
                let curr_up = curr.to_uppercase();
                let avail = entry.available.as_deref().and_then(|s| Decimal::from_str(s).ok()).unwrap_or(Decimal::ZERO);
                let resvd = entry.reserved.as_deref().and_then(|s| Decimal::from_str(s).ok()).unwrap_or(Decimal::ZERO);
                let tot = entry.total.as_deref().and_then(|s| Decimal::from_str(s).ok()).unwrap_or(avail + resvd);
                total_map.insert(curr_up.clone(), tot);
                avail_map.insert(curr_up.clone(), avail);
                resvd_map.insert(curr_up, resvd);
            }
        }

        let sheet = CachedBalanceSheet {
            total: total_map,
            available: avail_map,
            reserved: resvd_map,
        };

        *self.cached_balances.write().await = Some((Instant::now(), sheet.clone()));
        Ok(sheet)
    }

    async fn fetch_historical_orders(&self, limit: usize) -> Result<Vec<HistoricalOrder>, String> {
        {
            let cache = self.cached_historical_orders.read().await;
            if let Some((cached_at, ref orders)) = *cache {
                if cached_at.elapsed() < Duration::from_millis(1500) {
                    return Ok(orders.clone());
                }
            }
        }

        self.rate_limiter.acquire().await;

        let timestamp = Utc::now().timestamp_millis();
        let query_limit = limit.max(1).min(100);
        let path = format!("/api/1.0/orders/historical?limit={}", query_limit);
        let signature = self.signer.sign_payload(timestamp, "GET", &path, "");

        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .header("X-Revx-API-Key", self.signer.api_key())
            .header("X-Revx-Timestamp", timestamp.to_string())
            .header("X-Revx-Signature", signature)
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| format!("HTTP historical orders request error: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Revolut X historical orders fetch failed [HTTP {}]", resp.status()));
        }

        #[derive(Deserialize, Debug)]
        struct HistoricalOrderEntry {
            pub id: Option<String>,
            pub client_order_id: Option<String>,
            pub symbol: Option<String>,
            pub side: Option<String>,
            pub status: Option<String>,
            pub price: Option<String>,
            pub quantity: Option<String>,
            pub filled_quantity: Option<String>,
            pub average_fill_price: Option<String>,
        }

        let body_bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        let parsed_items: Vec<HistoricalOrderEntry> = if let Ok(wrapped) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
            if let Some(arr) = wrapped.get("data").and_then(|d| d.as_array()) {
                serde_json::from_value(serde_json::Value::Array(arr.clone())).unwrap_or_default()
            } else if wrapped.is_array() {
                serde_json::from_value(wrapped).unwrap_or_default()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let mut orders = Vec::new();
        for item in parsed_items {
            let id = item.id.unwrap_or_default();
            let cid = item.client_order_id.unwrap_or_else(|| id.clone());
            let sym = item.symbol.unwrap_or_default();
            let side = if item.side.as_deref().unwrap_or("").eq_ignore_ascii_case("SELL") {
                OrderSide::Sell
            } else {
                OrderSide::Buy
            };
            let status = item.status.unwrap_or_else(|| "UNKNOWN".to_string()).to_lowercase();
            let price = item.price.and_then(|p| Decimal::from_str(&p).ok()).unwrap_or(Decimal::ZERO);
            let quantity = item.quantity.and_then(|q| Decimal::from_str(&q).ok()).unwrap_or(Decimal::ZERO);
            let filled_quantity = item.filled_quantity.and_then(|q| Decimal::from_str(&q).ok()).unwrap_or(Decimal::ZERO);
            let average_fill_price = item.average_fill_price.and_then(|p| Decimal::from_str(&p).ok());

            orders.push(HistoricalOrder {
                id,
                client_order_id: cid,
                symbol: sym,
                side,
                status,
                price,
                quantity,
                filled_quantity,
                average_fill_price,
            });
        }

        *self.cached_historical_orders.write().await = Some((Instant::now(), orders.clone()));
        Ok(orders)
    }
}

#[async_trait]
impl ExecutionClient for LiveRevolutClient {
    async fn submit_post_only_order(&self, order: &Order) -> Result<Order, String> {
        self.send_order(order, true).await
    }

    async fn submit_taker_order(&self, order: &Order) -> Result<Order, String> {
        self.send_order(order, false).await
    }

    async fn cancel_order(&self, client_order_id: &str) -> Result<(), String> {
        self.rate_limiter.acquire().await;

        let timestamp = Utc::now().timestamp_millis();
        let path = format!("/api/1.0/orders?client_order_id={}", client_order_id);
        let signature = self.signer.sign_payload(timestamp, "DELETE", &path, "");

        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .delete(&url)
            .header("X-Revx-API-Key", self.signer.api_key())
            .header("X-Revx-Timestamp", timestamp.to_string())
            .header("X-Revx-Signature", signature)
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| format!("HTTP cancel request error: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            error!("[ORDER-CANCEL-FAIL] Revolut X cancel failed [{status}] for {}: {err_text}", client_order_id);
            return Err(format!("Revolut X cancel failed [{status}]: {err_text}"));
        }

        // Invalidate cached active orders, balances, and historical orders immediately
        *self.cached_active_orders.write().await = None;
        *self.cached_balances.write().await = None;
        *self.cached_historical_orders.write().await = None;

        info!("[ORDER-CANCEL] [VENUE-ACK] Canceled order on Revolut X: {}", client_order_id);
        Ok(())
    }

    async fn cancel_all_orders(&self) -> Result<usize, String> {
        self.rate_limiter.acquire().await;

        let timestamp = Utc::now().timestamp_millis();
        let path = "/api/1.0/orders";
        let signature = self.signer.sign_payload(timestamp, "DELETE", path, "");

        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .delete(&url)
            .header("X-Revx-API-Key", self.signer.api_key())
            .header("X-Revx-Timestamp", timestamp.to_string())
            .header("X-Revx-Signature", signature)
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| format!("HTTP bulk cancel error: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            error!("[ORDER-CANCEL-FAIL] Revolut X bulk cancel failed [{status}]: {err_text}");
            return Err(format!("Revolut X bulk cancel failed [{status}]: {err_text}"));
        }

        // Invalidate cached active orders, balances, and historical orders immediately
        *self.cached_active_orders.write().await = None;
        *self.cached_balances.write().await = None;
        *self.cached_historical_orders.write().await = None;

        info!("[ORDER-CANCEL] [VENUE-ACK] All active orders canceled on Revolut X");
        Ok(1)
    }

    async fn get_balance(&self, currency: &str) -> Result<Decimal, String> {
        let sheet = self.fetch_balance_sheet().await?;
        Ok(sheet.available.get(currency).copied().unwrap_or(Decimal::ZERO))
    }

    async fn get_balances(&self) -> Result<HashMap<String, Decimal>, String> {
        let sheet = self.fetch_balance_sheet().await?;
        Ok(sheet.total)
    }

    async fn get_available_balances(&self) -> Result<HashMap<String, Decimal>, String> {
        let sheet = self.fetch_balance_sheet().await?;
        Ok(sheet.available)
    }

    async fn get_reserved_balances(&self) -> Result<HashMap<String, Decimal>, String> {
        let sheet = self.fetch_balance_sheet().await?;
        Ok(sheet.reserved)
    }

    async fn get_active_orders(&self) -> Result<Vec<Order>, String> {
        {
            let cache = self.cached_active_orders.read().await;
            if let Some((cached_at, ref orders)) = *cache {
                if cached_at.elapsed() < Duration::from_millis(1500) {
                    return Ok(orders.clone());
                }
            }
        }

        self.rate_limiter.acquire().await;

        let timestamp = Utc::now().timestamp_millis();
        let path = "/api/1.0/orders/active";
        let signature = self.signer.sign_payload(timestamp, "GET", path, "");

        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .header("X-Revx-API-Key", self.signer.api_key())
            .header("X-Revx-Timestamp", timestamp.to_string())
            .header("X-Revx-Signature", signature)
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| format!("HTTP active orders request error: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Revolut X active orders fetch failed [HTTP {}]", resp.status()));
        }

        #[derive(Deserialize)]
        struct ActiveOrderEntry {
            pub id: Option<String>,
            pub client_order_id: Option<String>,
            pub symbol: Option<String>,
            pub side: Option<String>,
            pub price: Option<String>,
            pub quantity: Option<String>,
        }

        let body_bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        let parsed_items: Vec<ActiveOrderEntry> = if let Ok(wrapped) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
            if let Some(arr) = wrapped.get("data").and_then(|d| d.as_array()) {
                serde_json::from_value(serde_json::Value::Array(arr.clone())).unwrap_or_default()
            } else if wrapped.is_array() {
                serde_json::from_value(wrapped).unwrap_or_default()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let mut orders = Vec::new();
        for item in parsed_items {
            let cid = item.client_order_id.or(item.id.clone()).unwrap_or_else(|| Uuid::new_v4().to_string());
            let sym_str = item.symbol.unwrap_or_default();
            let (base, quote) = if sym_str.contains('-') {
                let parts: Vec<&str> = sym_str.split('-').collect();
                (parts[0].to_uppercase(), parts.get(1).map(|s| s.to_uppercase()).unwrap_or_else(|| "GBP".to_string()))
            } else if sym_str.contains('/') {
                let parts: Vec<&str> = sym_str.split('/').collect();
                (parts[0].to_uppercase(), parts.get(1).map(|s| s.to_uppercase()).unwrap_or_else(|| "GBP".to_string()))
            } else if sym_str.contains("BTC") {
                ("BTC".to_string(), "GBP".to_string())
            } else if sym_str.contains("ETH") {
                ("ETH".to_string(), "GBP".to_string())
            } else if sym_str.contains("SOL") {
                ("SOL".to_string(), "GBP".to_string())
            } else {
                ("BTC".to_string(), "GBP".to_string())
            };
            let sym = Symbol::new(&base, &quote);
            let runner_id = format!("runner_{}_{}", base.to_lowercase(), quote.to_lowercase());

            let side = if item.side.as_deref().unwrap_or("").eq_ignore_ascii_case("SELL") {
                OrderSide::Sell
            } else {
                OrderSide::Buy
            };

            let price = item.price.and_then(|p| Decimal::from_str(&p).ok()).unwrap_or(Decimal::ZERO);
            let qty = item.quantity.and_then(|q| Decimal::from_str(&q).ok()).unwrap_or(Decimal::ZERO);

            let ord = Order {
                id: Uuid::new_v4(),
                client_order_id: cid,
                runner_id,
                symbol: sym,
                side,
                order_type: OrderType::Limit,
                price,
                qty,
                filled_qty: Decimal::ZERO,
                post_only: true,
                status: OrderStatus::New,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            orders.push(ord);
        }

        *self.cached_active_orders.write().await = Some((Instant::now(), orders.clone()));
        Ok(orders)
    }

    async fn get_historical_orders(&self, limit: usize) -> Result<Vec<HistoricalOrder>, String> {
        self.fetch_historical_orders(limit).await
    }

    async fn get_bbo(&self, symbol: &Symbol) -> Result<(Decimal, Decimal), String> {
        let pair = symbol.as_dash();
        {
            let cache = self.cached_bbo.read().await;
            if let Some((cached_at, bbo)) = cache.get(&pair) {
                if cached_at.elapsed() < Duration::from_millis(3000) {
                    return Ok(*bbo);
                }
            }
        }

        self.rate_limiter.acquire().await;

        let url = format!("{}/api/2.0/public/order-book/{}", self.base_url.trim_end_matches('/'), pair);

        let resp = self
            .client
            .get(&url)
            .header(header::ACCEPT, "application/json")
            .timeout(std::time::Duration::from_millis(3000))
            .send()
            .await
            .map_err(|e| format!("HTTP public book error: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Revolut X public book failed [HTTP {}]", resp.status()));
        }

        #[derive(Deserialize)]
        struct Level {
            pub price: String,
        }

        #[derive(Deserialize)]
        struct BookData {
            #[serde(default)]
            pub bids: Vec<Level>,
            #[serde(default)]
            pub asks: Vec<Level>,
        }

        #[derive(Deserialize)]
        struct BookResponse {
            pub data: Option<BookData>,
        }

        let resp_json: BookResponse = resp.json().await.map_err(|e| format!("Failed to parse book JSON: {}", e))?;
        let data = resp_json.data.ok_or_else(|| "Missing 'data' in order book response".to_string())?;

        let best_bid = data
            .bids
            .iter()
            .filter_map(|l| Decimal::from_str(&l.price).ok())
            .max()
            .ok_or_else(|| "Empty bids in Revolut X order book".to_string())?;

        let best_ask = data
            .asks
            .iter()
            .filter_map(|l| Decimal::from_str(&l.price).ok())
            .min()
            .ok_or_else(|| "Empty asks in Revolut X order book".to_string())?;

        let bbo = (best_bid, best_ask);
        self.cached_bbo.write().await.insert(pair, (Instant::now(), bbo));
        Ok(bbo)
    }
}

/// Paper Trading adapter that routes orders into the in-memory simulator
pub struct PaperRevolutClient {
    simulator: Arc<PaperExecutionSimulator>,
    simulated_bbo: tokio::sync::RwLock<HashMap<String, (Decimal, Decimal)>>,
}

impl PaperRevolutClient {
    pub fn new(simulator: Arc<PaperExecutionSimulator>) -> Self {
        let mut initial_bbo = HashMap::new();
        initial_bbo.insert("BTC-GBP".to_string(), (Decimal::from(57870), Decimal::from(57880)));
        initial_bbo.insert("ETH-GBP".to_string(), (Decimal::from(1818), Decimal::from(1819)));

        Self {
            simulator,
            simulated_bbo: tokio::sync::RwLock::new(initial_bbo),
        }
    }

    pub async fn update_bbo(&self, symbol: &str, bid: Decimal, ask: Decimal) {
        let mut bbo = self.simulated_bbo.write().await;
        bbo.insert(symbol.to_string(), (bid, ask));
    }
}

#[async_trait]
impl ExecutionClient for PaperRevolutClient {
    async fn submit_post_only_order(&self, order: &Order) -> Result<Order, String> {
        self.simulator.submit_order(order.clone()).await
    }

    async fn submit_taker_order(&self, order: &Order) -> Result<Order, String> {
        // Taker order in simulator executes immediately
        info!(
            "[{}] [PAPER-TAKER-SNIPE] {} {} {} @ £{}",
            order.runner_id, order.side, order.qty, order.symbol, order.price
        );
        let mut executed_order = order.clone();
        executed_order.filled_qty = order.qty;
        executed_order.status = OrderStatus::Filled;
        executed_order.updated_at = Utc::now();
        Ok(executed_order)
    }

    async fn cancel_order(&self, client_order_id: &str) -> Result<(), String> {
        let orders = self.simulator.get_resting_orders().await;
        if let Some(ord) = orders.iter().find(|o| o.client_order_id == client_order_id) {
            self.simulator.cancel_order(ord.id).await;
        }
        Ok(())
    }

    async fn cancel_all_orders(&self) -> Result<usize, String> {
        let canceled = self.simulator.cancel_all_orders().await;
        Ok(canceled.len())
    }

    async fn get_balance(&self, currency: &str) -> Result<Decimal, String> {
        let wallet = self.simulator.get_wallet().await;
        Ok(wallet.get_balance(currency))
    }

    async fn get_balances(&self) -> Result<HashMap<String, Decimal>, String> {
        let wallet = self.simulator.get_wallet().await;
        let mut map = HashMap::new();
        map.insert("GBP".to_string(), wallet.gbp);
        map.insert("BTC".to_string(), wallet.btc);
        map.insert("ETH".to_string(), wallet.eth);
        map.insert("SOL".to_string(), wallet.sol);
        Ok(map)
    }

    async fn get_active_orders(&self) -> Result<Vec<Order>, String> {
        let orders = self.simulator.get_resting_orders().await;
        Ok(orders)
    }

    async fn get_historical_orders(&self, limit: usize) -> Result<Vec<HistoricalOrder>, String> {
        let orders = self.simulator.get_resting_orders().await;
        let mut hist = Vec::new();
        for o in orders.into_iter().take(limit) {
            hist.push(HistoricalOrder {
                id: o.id.to_string(),
                client_order_id: o.client_order_id,
                symbol: o.symbol.as_dash(),
                side: o.side,
                status: if o.status == OrderStatus::Filled {
                    "filled".to_string()
                } else if o.status == OrderStatus::Canceled {
                    "cancelled".to_string()
                } else {
                    "open".to_string()
                },
                price: o.price,
                quantity: o.qty,
                filled_quantity: o.filled_qty,
                average_fill_price: Some(o.price),
            });
        }
        Ok(hist)
    }

    async fn get_bbo(&self, symbol: &Symbol) -> Result<(Decimal, Decimal), String> {
        let bbo_map = self.simulated_bbo.read().await;
        if let Some(pair) = bbo_map.get(&symbol.as_dash()) {
            Ok(*pair)
        } else {
            Ok((Decimal::from(50000), Decimal::from(50010)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_live_revolut_auth_and_balances() {
        dotenvy::from_filename("../../.env").ok();
        dotenvy::dotenv().ok();
        let api_key = std::env::var("REVOLUT_API_KEY").expect("Missing REVOLUT_API_KEY");
        let priv_path = "../../backend/credentials/revolut_private.pem";
        let base_url = "https://revx.revolut.com";

        let client = LiveRevolutClient::new(base_url, api_key, priv_path).expect("Client init failed");
        let balances = client.get_balances().await;
        println!("\n>>> LIVE BALANCES RESULT: {:?}\n", balances);
        assert!(balances.is_ok(), "Balances failed: {:?}", balances.err());

        let orders = client.get_active_orders().await;
        println!("\n>>> LIVE ACTIVE ORDERS RESULT: {:?}\n", orders);
        assert!(orders.is_ok(), "Orders failed: {:?}", orders.err());
    }

    #[tokio::test]
    #[ignore]
    async fn test_probe_revolut_endpoints() {
        dotenvy::from_filename("../../.env").ok();
        dotenvy::dotenv().ok();
        let api_key = std::env::var("REVOLUT_API_KEY").expect("Missing REVOLUT_API_KEY");
        let priv_path = "../../backend/credentials/revolut_private.pem";
        let base_url = "https://revx.revolut.com";

        let client = LiveRevolutClient::new(base_url, api_key, priv_path).expect("Client init failed");
        
        for path in &["/api/1.0/orders", "/api/1.0/orders?state=FILLED", "/api/1.0/trades", "/api/1.0/orders/active"] {
            let timestamp = chrono::Utc::now().timestamp_millis();
            let sig = client.signer.sign_payload(timestamp, "GET", path, "");
            let url = format!("{}{}", base_url, path);
            let resp = client.client.get(&url)
                .header("X-Revx-API-Key", client.signer.api_key())
                .header("X-Revx-Timestamp", timestamp.to_string())
                .header("X-Revx-Signature", sig)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await;
            if let Ok(r) = resp {
                let status = r.status();
                let text = r.text().await.unwrap_or_default();
                let snippet = if text.len() > 300 { &text[..300] } else { &text };
                println!("[PROBE] {} -> HTTP {} : {}", path, status, snippet);
            }
        }
    }

    #[test]
    fn test_revolut_active_order_symbol_and_runner_parsing() {
        // Verify parsing logic for SOL-GBP, BTC-USD, and ETH/GBP
        let test_cases = vec![
            ("SOL-GBP", "SOL", "GBP", "runner_sol_gbp"),
            ("BTC-USD", "BTC", "USD", "runner_btc_usd"),
            ("ETH/GBP", "ETH", "GBP", "runner_eth_gbp"),
        ];

        for (sym_str, expected_base, expected_quote, expected_runner) in test_cases {
            let (base, quote) = if sym_str.contains('-') {
                let parts: Vec<&str> = sym_str.split('-').collect();
                (parts[0].to_uppercase(), parts.get(1).map(|s| s.to_uppercase()).unwrap_or_else(|| "GBP".to_string()))
            } else if sym_str.contains('/') {
                let parts: Vec<&str> = sym_str.split('/').collect();
                (parts[0].to_uppercase(), parts.get(1).map(|s| s.to_uppercase()).unwrap_or_else(|| "GBP".to_string()))
            } else {
                ("BTC".to_string(), "GBP".to_string())
            };
            let sym = Symbol::new(&base, &quote);
            let runner_id = format!("runner_{}_{}", base.to_lowercase(), quote.to_lowercase());

            assert_eq!(sym.base, expected_base);
            assert_eq!(sym.quote, expected_quote);
            assert_eq!(runner_id, expected_runner);
        }
    }

    #[test]
    fn test_revolut_get_bbo_url_resolution() {
        let base_url = "https://sandbox-revx.revolut.com/";
        let sym = Symbol::sol_usd();
        let pair = sym.as_dash();
        let url = format!("{}/api/2.0/public/order-book/{}", base_url.trim_end_matches('/'), pair);
        assert_eq!(url, "https://sandbox-revx.revolut.com/api/2.0/public/order-book/SOL-USD");
    }
}


