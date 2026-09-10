use crate::model::{Fill, Order, OrderSide, Symbol};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridConfig {
    pub runner_id: String,
    pub symbol: Symbol,
    pub step_pct: Decimal,                // e.g. dec!(0.0040) = 0.40%
    pub rungs_per_side: usize,           // e.g. 5
    pub order_size_gbp: Decimal,         // e.g. dec!(50.0)
    pub rebalance_threshold_pct: Decimal,// e.g. dec!(0.012) = 1.2%
}

pub struct GeometricGridStrategy {
    pub config: GridConfig,
    pub center_price: Option<Decimal>,
    pub active_orders: HashMap<Uuid, Order>,
    pub inventory_base: Decimal,
    pub realized_pnl: Decimal,
    pub total_trades: usize,
}

impl GeometricGridStrategy {
    pub fn new(config: GridConfig) -> Self {
        Self {
            config,
            center_price: None,
            active_orders: HashMap::new(),
            inventory_base: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
            total_trades: 0,
        }
    }

    /// Generates initial grid orders around current mid price
    pub fn initialize_grid(&mut self, mid_price: Decimal) -> Vec<Order> {
        self.center_price = Some(mid_price);
        let mut new_orders = Vec::new();

        info!(
            "[{}] Initializing Geometric Grid around center price: £{}",
            self.config.runner_id, mid_price
        );

        let one = dec!(1.0);
        let step = self.config.step_pct;

        // Generate BUY rungs below center price
        let mut current_buy_multiplier = one;
        for _ in 1..=self.config.rungs_per_side {
            current_buy_multiplier *= one - step;
            let rung_price = (mid_price * current_buy_multiplier).round_dp(2);
            let qty = (self.config.order_size_gbp / rung_price).round_dp(6);

            let order = Order::new_limit_post_only(
                &self.config.runner_id,
                self.config.symbol.clone(),
                OrderSide::Buy,
                rung_price,
                qty,
            );
            self.active_orders.insert(order.id, order.clone());
            new_orders.push(order);
        }

        // Generate SELL rungs above center price
        let mut current_sell_multiplier = one;
        for _ in 1..=self.config.rungs_per_side {
            current_sell_multiplier *= one + step;
            let rung_price = (mid_price * current_sell_multiplier).round_dp(2);
            let qty = (self.config.order_size_gbp / rung_price).round_dp(6);

            let order = Order::new_limit_post_only(
                &self.config.runner_id,
                self.config.symbol.clone(),
                OrderSide::Sell,
                rung_price,
                qty,
            );
            self.active_orders.insert(order.id, order.clone());
            new_orders.push(order);
        }

        new_orders
    }

    /// Handles a fill event: updates inventory and spawns counter-rung order
    pub fn on_fill(&mut self, fill: &Fill) -> Option<Order> {
        self.active_orders.remove(&fill.order_id);
        self.total_trades += 1;

        let one = dec!(1.0);
        let step = self.config.step_pct;

        match fill.side {
            OrderSide::Buy => {
                self.inventory_base += fill.qty;
                // Place counter SELL order 1 step above fill price
                let counter_price = (fill.price * (one + step)).round_dp(2);
                let order = Order::new_limit_post_only(
                    &self.config.runner_id,
                    self.config.symbol.clone(),
                    OrderSide::Sell,
                    counter_price,
                    fill.qty,
                );
                info!(
                    "[{}] Grid BUY filled @ £{} -> placing counter SELL @ £{} (inventory: {} {})",
                    self.config.runner_id, fill.price, counter_price, self.inventory_base, self.config.symbol.base
                );
                self.active_orders.insert(order.id, order.clone());
                Some(order)
            }
            OrderSide::Sell => {
                self.inventory_base -= fill.qty;
                // Realized profit calculation: (Sell Price - Buy Price) * Qty ~ step_pct * notional
                let profit = fill.price * fill.qty * step;
                self.realized_pnl += profit;

                // Place counter BUY order 1 step below fill price
                let counter_price = (fill.price * (one - step)).round_dp(2);
                let order = Order::new_limit_post_only(
                    &self.config.runner_id,
                    self.config.symbol.clone(),
                    OrderSide::Buy,
                    counter_price,
                    fill.qty,
                );
                info!(
                    "[{}] Grid SELL filled @ £{} -> profit: £{:.4} -> placing counter BUY @ £{} (Total PnL: £{:.4})",
                    self.config.runner_id, fill.price, profit, counter_price, self.realized_pnl
                );
                self.active_orders.insert(order.id, order.clone());
                Some(order)
            }
        }
    }

    /// Checks if market mid-price drifted too far from grid center
    pub fn needs_rebalance(&self, current_mid: Decimal) -> bool {
        if let Some(center) = self.center_price {
            if center > Decimal::ZERO {
                let drift = (current_mid - center).abs() / center;
                return drift >= self.config.rebalance_threshold_pct;
            }
        }
        false
    }
}
