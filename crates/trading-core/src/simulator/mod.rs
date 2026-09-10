use crate::model::{Fill, MarketTick, Order, OrderSide, OrderStatus, Symbol};
use chrono::Utc;
use rust_decimal::Decimal;
use std::collections::HashMap;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct VirtualWallet {
    pub gbp: Decimal,
    pub btc: Decimal,
    pub eth: Decimal,
    pub sol: Decimal,
}

impl VirtualWallet {
    pub fn new(initial_gbp: Decimal) -> Self {
        Self {
            gbp: initial_gbp,
            btc: Decimal::ZERO,
            eth: Decimal::ZERO,
            sol: Decimal::ZERO,
        }
    }

    pub fn get_balance(&self, currency: &str) -> Decimal {
        match currency.to_uppercase().as_str() {
            "GBP" => self.gbp,
            "BTC" => self.btc,
            "ETH" => self.eth,
            "SOL" => self.sol,
            _ => Decimal::ZERO,
        }
    }

    pub fn credit(&mut self, currency: &str, amount: Decimal) {
        match currency.to_uppercase().as_str() {
            "GBP" => self.gbp += amount,
            "BTC" => self.btc += amount,
            "ETH" => self.eth += amount,
            "SOL" => self.sol += amount,
            _ => {}
        }
    }

    pub fn debit(&mut self, currency: &str, amount: Decimal) -> bool {
        match currency.to_uppercase().as_str() {
            "GBP" if self.gbp >= amount => {
                self.gbp -= amount;
                true
            }
            "BTC" if self.btc >= amount => {
                self.btc -= amount;
                true
            }
            "ETH" if self.eth >= amount => {
                self.eth -= amount;
                true
            }
            "SOL" if self.sol >= amount => {
                self.sol -= amount;
                true
            }
            _ => false,
        }
    }
}

pub struct PaperExecutionSimulator {
    wallet: Mutex<VirtualWallet>,
    resting_orders: Mutex<HashMap<Uuid, Order>>,
}

impl PaperExecutionSimulator {
    pub fn new(initial_gbp: Decimal) -> Self {
        Self {
            wallet: Mutex::new(VirtualWallet::new(initial_gbp)),
            resting_orders: Mutex::new(HashMap::new()),
        }
    }

    pub async fn get_wallet(&self) -> VirtualWallet {
        self.wallet.lock().await.clone()
    }

    pub async fn submit_order(&self, order: Order) -> Result<Order, String> {
        let mut orders = self.resting_orders.lock().await;
        info!(
            "[{}] [PAPER-SUBMIT] {} {} {} @ {} (client_id: {})",
            order.runner_id, order.side, order.qty, order.symbol, order.price, order.client_order_id
        );
        orders.insert(order.id, order.clone());
        Ok(order)
    }

    pub async fn cancel_order(&self, order_id: Uuid) -> Option<Order> {
        let mut orders = self.resting_orders.lock().await;
        if let Some(mut ord) = orders.remove(&order_id) {
            ord.status = OrderStatus::Canceled;
            ord.updated_at = Utc::now();
            info!(
                "[{}] [PAPER-CANCEL] Canceled order {} ({})",
                ord.runner_id, ord.id, ord.client_order_id
            );
            Some(ord)
        } else {
            None
        }
    }

    pub async fn cancel_all_buys_for_symbol(&self, symbol: &Symbol) -> Vec<Order> {
        let mut orders = self.resting_orders.lock().await;
        let mut canceled = Vec::new();
        let ids: Vec<Uuid> = orders
            .iter()
            .filter(|(_, ord)| ord.symbol == *symbol && ord.side == OrderSide::Buy)
            .map(|(id, _)| *id)
            .collect();

        for id in ids {
            if let Some(mut ord) = orders.remove(&id) {
                ord.status = OrderStatus::Canceled;
                ord.updated_at = Utc::now();
                warn!(
                    "[{}] [LAG-FILTER-CANCEL] Instantly canceled resting BUY order {} @ {}",
                    ord.runner_id, ord.id, ord.price
                );
                canceled.push(ord);
            }
        }
        canceled
    }

    pub async fn cancel_all_orders(&self) -> Vec<Order> {
        let mut orders = self.resting_orders.lock().await;
        let mut canceled = Vec::new();
        for (_, mut ord) in orders.drain() {
            ord.status = OrderStatus::Canceled;
            ord.updated_at = Utc::now();
            canceled.push(ord);
        }
        canceled
    }

    pub async fn get_resting_orders(&self) -> Vec<Order> {
        self.resting_orders.lock().await.values().cloned().collect()
    }

    pub async fn process_tick(&self, tick: &MarketTick) -> Vec<Fill> {
        let mut orders = self.resting_orders.lock().await;
        let mut fills = Vec::new();
        let mut filled_ids = Vec::new();
        let mut wallet = self.wallet.lock().await;

        for (id, order) in orders.iter_mut() {
            if order.symbol != tick.symbol || !order.is_active() {
                continue;
            }

            let mut filled = false;
            match order.side {
                OrderSide::Buy => {
                    // Maker buy limit fills when market ask or last dips down to or below our bid
                    if tick.last <= order.price || tick.ask <= order.price {
                        filled = true;
                    }
                }
                OrderSide::Sell => {
                    // Maker sell limit fills when market bid or last reaches up to or above our ask
                    if tick.last >= order.price || tick.bid >= order.price {
                        filled = true;
                    }
                }
            }

            if filled {
                let fill_qty = order.remaining_qty();
                let fill_price = order.price; // Maker limit order execution price
                let fill_cost = fill_price * fill_qty;

                match order.side {
                    OrderSide::Buy => {
                        wallet.debit(&order.symbol.quote, fill_cost);
                        wallet.credit(&order.symbol.base, fill_qty);
                    }
                    OrderSide::Sell => {
                        wallet.debit(&order.symbol.base, fill_qty);
                        wallet.credit(&order.symbol.quote, fill_cost);
                    }
                }

                order.filled_qty = order.qty;
                order.status = OrderStatus::Filled;
                order.updated_at = Utc::now();

                let fill = Fill {
                    order_id: order.id,
                    client_order_id: order.client_order_id.clone(),
                    runner_id: order.runner_id.clone(),
                    symbol: order.symbol.clone(),
                    side: order.side,
                    price: fill_price,
                    qty: fill_qty,
                    fee: Decimal::ZERO, // 0.00% Revolut X Maker Fee!
                    timestamp: Utc::now(),
                };

                info!(
                    "[{}] [PAPER-FILL] FILLED {} {} {} @ {} (Fee: £0.00)",
                    order.runner_id, order.side, fill_qty, order.symbol, fill_price
                );

                fills.push(fill);
                filled_ids.push(*id);
            }
        }

        for id in filled_ids {
            orders.remove(&id);
        }

        fills
    }
}
