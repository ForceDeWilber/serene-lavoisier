use async_trait::async_trait;
use rust_decimal::Decimal;
use std::collections::HashMap;
use crate::model::{Order, Symbol};

#[async_trait]
pub trait ExecutionClient: Send + Sync {
    /// Submit a post-only maker limit order (Revolut X fee: 0.00%)
    async fn submit_post_only_order(&self, order: &Order) -> Result<Order, String>;

    /// Submit an aggressive taker limit order (Revolut X fee: 0.09%)
    async fn submit_taker_order(&self, order: &Order) -> Result<Order, String>;

    /// Cancel a single order by client_order_id or exchange_id
    async fn cancel_order(&self, client_order_id: &str) -> Result<(), String>;

    /// Bulk cancel all active orders across the venue (emergency kill switch)
    async fn cancel_all_orders(&self) -> Result<usize, String>;

    /// Get free available balance for a single currency
    async fn get_balance(&self, currency: &str) -> Result<Decimal, String>;

    /// Get all account balances (e.g. GBP, BTC, ETH)
    async fn get_balances(&self) -> Result<HashMap<String, Decimal>, String>;

    /// Get all currently active resting orders on the venue (for boot rehydration)
    async fn get_active_orders(&self) -> Result<Vec<Order>, String>;

    /// Get top of the book Best Bid and Best Offer (best_bid, best_ask)
    async fn get_bbo(&self, symbol: &Symbol) -> Result<(Decimal, Decimal), String>;
}
