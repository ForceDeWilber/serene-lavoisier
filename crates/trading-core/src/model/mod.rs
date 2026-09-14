use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

    pub fn xrp_gbp() -> Self {
        Self::new("XRP", "GBP")
    }

    pub fn btc_usd() -> Self {
        Self::new("BTC", "USD")
    }

    pub fn eth_usd() -> Self {
        Self::new("ETH", "USD")
    }

    pub fn sol_usd() -> Self {
        Self::new("SOL", "USD")
    }

    pub fn parse(s: &str) -> Option<Self> {
        if let Some((b, q)) = s.split_once('/') {
            Some(Self::new(b, q))
        } else if let Some((b, q)) = s.split_once('-') {
            Some(Self::new(b, q))
        } else {
            None
        }
    }

    pub fn as_slash(&self) -> String {
        format!("{}/{}", self.base, self.quote)
    }

    pub fn as_dash(&self) -> String {
        format!("{}-{}", self.base, self.quote)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairConfig {
    pub symbol: Symbol,
    pub envelope_capital: Decimal,
    pub grid_step_pct: Decimal,
    pub grid_rungs: usize,
    pub order_size_fiat: Decimal,
    pub rebalance_threshold_pct: Decimal,
    pub sniper_enabled: bool,
    pub sniper_order_size_fiat: Decimal,
    pub sniper_hurdle_pct: Decimal,
    pub is_active: bool,
}

impl PairConfig {
    pub fn default_for(symbol: Symbol) -> Self {
        let is_sol = symbol.base == "SOL";
        Self {
            symbol,
            envelope_capital: Decimal::from(500),
            grid_step_pct: if is_sol { rust_decimal_macros::dec!(0.0060) } else { rust_decimal_macros::dec!(0.0040) },
            grid_rungs: 5,
            order_size_fiat: Decimal::from(50),
            rebalance_threshold_pct: if is_sol { rust_decimal_macros::dec!(0.015) } else { rust_decimal_macros::dec!(0.012) },
            sniper_enabled: true,
            sniper_order_size_fiat: Decimal::from(50),
            sniper_hurdle_pct: rust_decimal_macros::dec!(0.0011),
            is_active: true,
        }
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
        let client_order_id = id.to_string();
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

    pub fn new_limit_taker(
        runner_id: impl Into<String>,
        symbol: Symbol,
        side: OrderSide,
        price: Decimal,
        qty: Decimal,
    ) -> Self {
        let now = Utc::now();
        let id = Uuid::new_v4();
        let client_order_id = id.to_string();
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
            post_only: false,
            status: OrderStatus::New,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn new_market(
        runner_id: impl Into<String>,
        symbol: Symbol,
        side: OrderSide,
        qty: Decimal,
    ) -> Self {
        let now = Utc::now();
        let id = Uuid::new_v4();
        let client_order_id = id.to_string();
        Self {
            id,
            client_order_id,
            runner_id: runner_id.into(),
            symbol,
            side,
            order_type: OrderType::Market,
            price: Decimal::ZERO,
            qty,
            filled_qty: Decimal::ZERO,
            post_only: false,
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
        if self.bid > Decimal::ZERO && self.ask > Decimal::ZERO {
            (self.bid + self.ask) / Decimal::from(2)
        } else if self.last > Decimal::ZERO {
            self.last
        } else if self.bid > Decimal::ZERO {
            self.bid
        } else {
            self.ask
        }
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
