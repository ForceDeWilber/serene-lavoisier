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
use tracing::{error, info};
use trading_core::execution::ExecutionClient;
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
pub struct LiveRevolutClient {
    base_url: String,
    client: Client,
    signer: Ed25519Signer,
    rate_limiter: TokenBucketRateLimiter,
}

impl LiveRevolutClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        private_key_source: &str,
    ) -> Result<Self, String> {
        let signer = Ed25519Signer::from_file_or_hex(api_key, private_key_source).map_err(|e| e.to_string())?;
        let client = Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .tcp_nodelay(true)
            .build()
            .map_err(|e| e.to_string())?;

        let rate_limiter = TokenBucketRateLimiter::new(30.0, 900.0);

        Ok(Self {
            base_url: base_url.into(),
            client,
            signer,
            rate_limiter,
        })
    }

    async fn send_order(&self, order: &Order, is_post_only: bool) -> Result<Order, String> {
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
        let resp = self
            .client
            .post(&url)
            .header("X-Revx-API-Key", self.signer.api_key())
            .header("X-Revx-Timestamp", timestamp.to_string())
            .header("X-Revx-Signature", signature)
            .header(header::ACCEPT, "application/json")
            .header(header::CONTENT_TYPE, "application/json")
            .body(body_str)
            .send()
            .await
            .map_err(|e| format!("HTTP order POST error: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            error!("Revolut X order submission failed [{status}]: {err_text}");
            return Err(format!("Revolut X rejected order [{status}]: {err_text}"));
        }

        info!(
            "[LIVE-ORDER-PLACED] {} {} {} @ £{} ({})",
            order.side, order.qty, order.symbol, order.price, order.client_order_id
        );
        Ok(order.clone())
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
        let path = format!("/api/1.0/orders/{}", client_order_id);
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
            return Err(format!("Revolut X cancel failed [{status}]: {err_text}"));
        }

        info!("[LIVE-CANCEL] Canceled order on Revolut X: {}", client_order_id);
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
            return Err(format!("Revolut X bulk cancel failed [{status}]: {err_text}"));
        }

        info!("[LIVE-BULK-CANCEL] All active orders canceled on Revolut X");
        Ok(1)
    }

    async fn get_balance(&self, currency: &str) -> Result<Decimal, String> {
        let balances = self.get_balances().await?;
        Ok(balances.get(currency).copied().unwrap_or(Decimal::ZERO))
    }

    async fn get_balances(&self) -> Result<HashMap<String, Decimal>, String> {
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

        #[derive(Deserialize)]
        struct BalanceEntry {
            pub currency: Option<String>,
            #[serde(default)]
            pub available: Option<String>,
        }

        let entries: Vec<BalanceEntry> = resp.json().await.map_err(|e| format!("Failed to parse balances JSON: {}", e))?;
        let mut map = HashMap::new();
        for entry in entries {
            if let (Some(curr), Some(avail_str)) = (entry.currency, entry.available) {
                if let Ok(d) = Decimal::from_str(&avail_str) {
                    map.insert(curr.to_uppercase(), d);
                }
            }
        }

        Ok(map)
    }

    async fn get_active_orders(&self) -> Result<Vec<Order>, String> {
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
            let sym = if sym_str.contains("BTC") {
                Symbol::btc_gbp()
            } else if sym_str.contains("ETH") {
                Symbol::eth_gbp()
            } else {
                Symbol::new("BTC", "GBP")
            };

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
                runner_id: if sym_str.contains("BTC") { "runner_btc".into() } else { "runner_eth".into() },
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

        Ok(orders)
    }

    async fn get_bbo(&self, symbol: &Symbol) -> Result<(Decimal, Decimal), String> {
        let pair = symbol.as_dash();
        let url = format!("https://revx.revolut.com/api/2.0/public/order-book/{}", pair);

        let resp = self
            .client
            .get(&url)
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
            .first()
            .and_then(|l| Decimal::from_str(&l.price).ok())
            .ok_or_else(|| "Empty bids in Revolut X order book".to_string())?;

        let best_ask = data
            .asks
            .first()
            .and_then(|l| Decimal::from_str(&l.price).ok())
            .ok_or_else(|| "Empty asks in Revolut X order book".to_string())?;

        Ok((best_bid, best_ask))
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
}

