use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::model::{Order, OrderSide, Symbol};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoricalOrder {
    pub id: String,
    pub client_order_id: String,
    pub symbol: String,
    pub side: OrderSide,
    pub status: String, // e.g. "filled", "cancelled", "expired"
    pub price: Decimal,
    pub quantity: Decimal,
    pub filled_quantity: Decimal,
    pub average_fill_price: Option<Decimal>,
}

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

    /// Get all account total balances (available + in orders)
    async fn get_balances(&self) -> Result<HashMap<String, Decimal>, String>;

    /// Get all account free available balances (unreserved)
    async fn get_available_balances(&self) -> Result<HashMap<String, Decimal>, String> {
        self.get_balances().await
    }

    /// Get all account reserved balances (locked in open resting orders)
    async fn get_reserved_balances(&self) -> Result<HashMap<String, Decimal>, String> {
        Ok(HashMap::new())
    }

    /// Get all currently active resting orders on the venue (for boot rehydration)
    async fn get_active_orders(&self) -> Result<Vec<Order>, String>;

    /// Get historical/completed orders from the venue to verify execution status (filled vs cancelled)
    async fn get_historical_orders(&self, _limit: usize) -> Result<Vec<HistoricalOrder>, String> {
        Ok(Vec::new())
    }

    /// Get top of the book Best Bid and Best Offer (best_bid, best_ask)
    async fn get_bbo(&self, symbol: &Symbol) -> Result<(Decimal, Decimal), String>;
}

