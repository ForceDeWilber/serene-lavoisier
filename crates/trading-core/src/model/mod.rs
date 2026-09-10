use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol {
    pub base: String,
    pub quote: String,
}

impl Symbol {
    pub fn new(base: impl Into<String>, quote: impl Into<String>) -> Self {
        Self {
            base: base.into().to_uppercase(),
            quote: quote.into().to_uppercase(),
        }
    }

    pub fn btc_gbp() -> Self {
        Self::new("BTC", "GBP")
    }

    pub fn eth_gbp() -> Self {
        Self::new("ETH", "GBP")
    }

    pub fn sol_gbp() -> Self {
        Self::new("SOL", "GBP")
    }

    pub fn as_slash(&self) -> String {
        format!("{}/{}", self.base, self.quote)
    }

    pub fn as_dash(&self) -> String {
        format!("{}-{}", self.base, self.quote)
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.base, self.quote)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

impl OrderSide {
    pub fn opposite(&self) -> Self {
        match self {
            Self::Buy => Self::Sell,
            Self::Sell => Self::Buy,
        }
    }
}

impl fmt::Display for OrderSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Buy => write!(f, "BUY"),
            Self::Sell => write!(f, "SELL"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Limit,
    Market,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: Uuid,
    pub client_order_id: String,
    pub runner_id: String,
    pub symbol: Symbol,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub price: Decimal,
    pub qty: Decimal,
    pub filled_qty: Decimal,
    pub post_only: bool,
    pub status: OrderStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Order {
    pub fn new_limit_post_only(
        runner_id: impl Into<String>,
        symbol: Symbol,
        side: OrderSide,
        price: Decimal,
        qty: Decimal,
    ) -> Self {
        let now = Utc::now();
        let id = Uuid::new_v4();
        let client_order_id = format!("ord_{}", id.simple());
        Self {
            id,
            client_order_id,
            runner_id: runner_id.into(),
            symbol,
            side,
            order_type: OrderType::Limit,
            price,
            qty,
            filled_qty: Decimal::ZERO,
            post_only: true,
            status: OrderStatus::New,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn remaining_qty(&self) -> Decimal {
        self.qty - self.filled_qty
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status, OrderStatus::New | OrderStatus::PartiallyFilled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub order_id: Uuid,
    pub client_order_id: String,
    pub runner_id: String,
    pub symbol: Symbol,
    pub side: OrderSide,
    pub price: Decimal,
    pub qty: Decimal,
    pub fee: Decimal,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketTick {
    pub symbol: Symbol,
    pub bid: Decimal,
    pub ask: Decimal,
    pub last: Decimal,
    pub timestamp: DateTime<Utc>,
}

impl MarketTick {
    pub fn mid_price(&self) -> Decimal {
        (self.bid + self.ask) / Decimal::from(2)
    }

    pub fn spread(&self) -> Decimal {
        self.ask - self.bid
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2Level {
    pub price: Decimal,
    pub qty: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct L2OrderBook {
    pub symbol: Option<Symbol>,
    pub bids: Vec<L2Level>,
    pub asks: Vec<L2Level>,
    pub timestamp: DateTime<Utc>,
}

impl L2OrderBook {
    pub fn best_bid(&self) -> Option<Decimal> {
        self.bids.first().map(|l| l.price)
    }

    pub fn best_ask(&self) -> Option<Decimal> {
        self.asks.first().map(|l| l.price)
    }

    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid + ask) / Decimal::from(2)),
            _ => None,
        }
    }

    pub fn spread(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeBalance {
    pub runner_id: String,
    pub currency: String,
    pub allocated: Decimal,
    pub locked: Decimal,
    pub available: Decimal,
}

impl EnvelopeBalance {
    pub fn new(runner_id: impl Into<String>, currency: impl Into<String>, amount: Decimal) -> Self {
        Self {
            runner_id: runner_id.into(),
            currency: currency.into().to_uppercase(),
            allocated: amount,
            locked: Decimal::ZERO,
            available: amount,
        }
    }

    pub fn lock(&mut self, amount: Decimal) -> Result<(), &'static str> {
        if self.available < amount {
            return Err("Insufficient available balance in capital envelope");
        }
        self.available -= amount;
        self.locked += amount;
        Ok(())
    }

    pub fn unlock(&mut self, amount: Decimal) {
        let release = amount.min(self.locked);
        self.locked -= release;
        self.available += release;
    }
}
