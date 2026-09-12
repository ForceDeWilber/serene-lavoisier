pub mod rate_limiter;
pub mod signer;

use async_trait::async_trait;
use chrono::Utc;
use rate_limiter::TokenBucketRateLimiter;
use reqwest::{header, Client};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use signer::Ed25519Signer;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{error, info};
use trading_core::model::Order;
use trading_core::simulator::PaperExecutionSimulator;

#[derive(Debug, Serialize, Deserialize)]
pub struct RevolutOrderPayload {
    pub client_order_id: String,
    pub symbol: String, // e.g. "BTC-GBP"
    pub side: String,   // "BUY" or "SELL"
    pub r#type: String, // "LIMIT"
    pub price: String,
    pub quantity: String,
    pub time_in_force: String, // "GTC"
    pub execution_instructions: Vec<String>, // ["post_only"]
}

#[async_trait]
pub trait RevolutExecutionClient: Send + Sync {
    async fn submit_post_only_order(&self, order: &Order) -> Result<Order, String>;
    async fn cancel_order(&self, client_order_id: &str) -> Result<(), String>;
    async fn get_balance(&self, currency: &str) -> Result<Decimal, String>;
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
        private_key_hex: &str,
    ) -> Result<Self, String> {
        let signer = Ed25519Signer::from_hex(api_key, private_key_hex).map_err(|e| e.to_string())?;
        let client = Client::builder()
            .http2_prior_knowledge()
            .pool_idle_timeout(std::time::Duration::from_secs(90))
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
}

#[async_trait]
impl RevolutExecutionClient for LiveRevolutClient {
    async fn submit_post_only_order(&self, order: &Order) -> Result<Order, String> {
        self.rate_limiter.acquire().await;

        let payload = RevolutOrderPayload {
            client_order_id: order.client_order_id.clone(),
            symbol: order.symbol.as_dash(),
            side: order.side.to_string(),
            r#type: "LIMIT".into(),
            price: order.price.to_string(),
            quantity: order.qty.to_string(),
            time_in_force: "GTC".into(),
            execution_instructions: vec!["post_only".into()],
        };

        let body_str = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
        let timestamp = Utc::now().timestamp_millis();
        let path = "/api/v1/orders";
        let signature = self.signer.sign_payload(timestamp, "POST", path, &body_str);

        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .post(&url)
            .header("X-Revx-API-Key", self.signer.api_key())
            .header("X-Revx-Timestamp", timestamp.to_string())
            .header("X-Revx-Signature", signature)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body_str)
            .send()
            .await
            .map_err(|e| format!("HTTP request error: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            error!("Revolut X order submission failed [{status}]: {err_text}");
            return Err(format!("Revolut X API rejected order [{status}]: {err_text}"));
        }

        info!("[LIVE-SUBMIT] Successfully placed post_only order on Revolut X: {}", order.client_order_id);
        Ok(order.clone())
    }

    async fn cancel_order(&self, client_order_id: &str) -> Result<(), String> {
        self.rate_limiter.acquire().await;

        let timestamp = Utc::now().timestamp_millis();
        let path = format!("/api/v1/orders/{}", client_order_id);
        let signature = self.signer.sign_payload(timestamp, "DELETE", &path, "");

        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .delete(&url)
            .header("X-Revx-API-Key", self.signer.api_key())
            .header("X-Revx-Timestamp", timestamp.to_string())
            .header("X-Revx-Signature", signature)
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

    async fn get_balance(&self, currency: &str) -> Result<Decimal, String> {
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
            .send()
            .await
            .map_err(|e| format!("HTTP balance request error: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Revolut X balance fetch failed [HTTP {}]", resp.status()));
        }

        #[derive(Deserialize)]
        struct BalanceEntry {
            currency: String,
            #[serde(default)]
            available: Option<String>,
        }

        let entries: Vec<BalanceEntry> = resp.json().await.map_err(|e| format!("Failed to parse balances JSON: {}", e))?;
        for entry in entries {
            if entry.currency.eq_ignore_ascii_case(currency) {
                if let Some(avail_str) = entry.available {
                    return Decimal::from_str(&avail_str).map_err(|e| e.to_string());
                }
            }
        }

        // Return Decimal::ZERO if currency is not present in account list
        Ok(Decimal::ZERO)
    }
}

/// Paper Trading adapter that routes orders into the in-memory simulator
pub struct PaperRevolutClient {
    simulator: Arc<PaperExecutionSimulator>,
}

impl PaperRevolutClient {
    pub fn new(simulator: Arc<PaperExecutionSimulator>) -> Self {
        Self { simulator }
    }
}

#[async_trait]
impl RevolutExecutionClient for PaperRevolutClient {
    async fn submit_post_only_order(&self, order: &Order) -> Result<Order, String> {
        self.simulator.submit_order(order.clone()).await
    }

    async fn cancel_order(&self, client_order_id: &str) -> Result<(), String> {
        // Find order by client_order_id in simulator
        let orders = self.simulator.get_resting_orders().await;
        if let Some(ord) = orders.iter().find(|o| o.client_order_id == client_order_id) {
            self.simulator.cancel_order(ord.id).await;
        }
        Ok(())
    }

    async fn get_balance(&self, currency: &str) -> Result<Decimal, String> {
        let wallet = self.simulator.get_wallet().await;
        Ok(wallet.get_balance(currency))
    }
}
