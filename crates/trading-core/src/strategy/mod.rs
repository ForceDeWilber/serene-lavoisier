pub mod dynamic_pricing;
pub mod sniper;
pub use dynamic_pricing::*;
pub use sniper::*;

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
    #[serde(default)]
    pub dynamic_pricing: DynamicPricingConfig,
    pub mode: Option<String>,
    #[serde(default)]
    pub cost_basis: Option<Decimal>,
}

pub struct GeometricGridStrategy {
    pub config: GridConfig,
    pub center_price: Option<Decimal>,
    pub effective_center: Option<Decimal>,
    pub active_orders: HashMap<Uuid, Order>,
    pub inventory_base: Decimal,
    pub realized_pnl: Decimal,
    pub total_trades: usize,
    pub dynamic_pricing: DynamicPriceModel,
}

impl GeometricGridStrategy {
    pub fn new(config: GridConfig) -> Self {
        let mut dynamic_pricing = DynamicPriceModel::new(config.dynamic_pricing.clone());
        dynamic_pricing.config.base_step_pct = config.step_pct;
        Self {
            config,
            center_price: None,
            effective_center: None,
            active_orders: HashMap::new(),
            inventory_base: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
            total_trades: 0,
            dynamic_pricing,
        }
    }

    /// Records price tick to update rolling volatility
    pub fn record_tick(&mut self, mid_price: Decimal, now: chrono::DateTime<chrono::Utc>) {
        self.dynamic_pricing.record_sample(mid_price, now);
    }

    /// Generates initial or rebalanced grid orders with dynamic pricing, inventory skewing, and capital clipping
    pub fn initialize_grid(
        &mut self,
        mid_price: Decimal,
        free_fiat: Option<Decimal>,
        available_base: Option<Decimal>,
    ) -> Vec<Order> {
        self.center_price = Some(mid_price);

        if let Some(base) = available_base {
            self.inventory_base = base;
        }

        // Calculate Avellaneda-Stoikov skewed reservation center price
        let effective_center = self
            .dynamic_pricing
            .calculate_reservation_price(mid_price, self.inventory_base);
        self.effective_center = Some(effective_center);

        let dynamic_step = self.dynamic_pricing.calculate_dynamic_step();

        // Calculate dynamic order clip
        let order_clip = free_fiat.map_or(self.config.order_size_gbp, |fiat| {
            self.dynamic_pricing.calculate_order_clip(
                fiat,
                self.config.rungs_per_side,
                self.config.order_size_gbp,
            )
        });

        info!(
            "[{}] Initializing Dynamic Grid: mid=£{}, effective_center=£{}, step={:.4}%, clip=£{}, inv={} {}",
            self.config.runner_id,
            mid_price,
            effective_center,
            dynamic_step * dec!(100.0),
            order_clip,
            self.inventory_base,
            self.config.symbol.base
        );

        let mut new_orders = Vec::new();
        let one = dec!(1.0);

        // Generate BUY rungs below effective reservation center price (unless winding down)
        if order_clip >= dec!(1.00) && self.config.mode.as_deref() != Some("WIND_DOWN") {
            let mut current_buy_multiplier = one;
            for _ in 1..=self.config.rungs_per_side {
                current_buy_multiplier *= one - dynamic_step;
                let rung_price = (effective_center * current_buy_multiplier).round_dp(2);
                if rung_price <= Decimal::ZERO {
                    continue;
                }
                let qty = (order_clip / rung_price).round_dp(6);
                if qty <= Decimal::ZERO {
                    continue;
                }

                let order = Order::new_limit_post_only(
                    &self.config.runner_id,
                    self.config.symbol.clone(),
                    OrderSide::Buy,
                    rung_price,
                    qty,
                );
                new_orders.push(order);
            }
        }

        // Generate SELL rungs above effective reservation center price
        // Only place sells if we hold positive base inventory on spot!
        let base_avail = available_base.unwrap_or(self.inventory_base);
        if base_avail > Decimal::ZERO {
            let mut current_sell_multiplier = one;
            let mut allocated_sell_base = Decimal::ZERO;

            let total_inv_val = (base_avail * effective_center).round_dp(2);
            let min_clip = dec!(1.00);

            if total_inv_val >= min_clip {
                let sell_clip = if order_clip >= min_clip {
                    order_clip.min(total_inv_val)
                } else {
                    let rungs_dec = Decimal::from(self.config.rungs_per_side.max(1));
                    let per_rung = (total_inv_val / rungs_dec).round_dp(2);
                    per_rung.min(self.config.order_size_gbp).max(min_clip)
                };

                // Strict No-Loss Floor: Ensure sell price is at or above cost basis (+0.15% minimum profit margin)
                let min_profit_multiplier = dec!(1.0015);
                let floor_price = self.config.cost_basis.map(|cb| (cb * min_profit_multiplier).round_dp(2));

                for _ in 1..=self.config.rungs_per_side {
                    current_sell_multiplier *= one + dynamic_step;
                    let mut rung_price = (effective_center * current_sell_multiplier).round_dp(2);

                    if let Some(floor) = floor_price {
                        if rung_price < floor {
                            rung_price = floor;
                        }
                    }

                    if rung_price <= Decimal::ZERO {
                        continue;
                    }

                    let desired_qty = (sell_clip / rung_price).round_dp(6);
                    let remaining_base = base_avail - allocated_sell_base;
                    if remaining_base <= Decimal::ZERO {
                        break;
                    }

                    let qty = desired_qty.min(remaining_base);
                    if qty <= Decimal::ZERO {
                        break;
                    }

                    // Check minimum order size on Revolut X (>= 1.00 in quote currency)
                    if (qty * rung_price).round_dp(2) < min_clip {
                        if (remaining_base * rung_price).round_dp(2) >= min_clip {
                            let sweep_qty = remaining_base;
                            allocated_sell_base += sweep_qty;
                            let order = Order::new_limit_post_only(
                                &self.config.runner_id,
                                self.config.symbol.clone(),
                                OrderSide::Sell,
                                rung_price,
                                sweep_qty,
                            );
                            new_orders.push(order);
                        }
                        break;
                    }

                    allocated_sell_base += qty;

                    let order = Order::new_limit_post_only(
                        &self.config.runner_id,
                        self.config.symbol.clone(),
                        OrderSide::Sell,
                        rung_price,
                        qty,
                    );
                    new_orders.push(order);
                }
            }
        }

        new_orders
    }

    /// Handles a fill event: updates inventory and generates a counter-order
    /// Note: Does NOT insert into active_orders directly; runner registers it after successful submission.
    pub fn on_fill(&mut self, fill: &Fill) -> Option<Order> {
        self.active_orders.remove(&fill.order_id);
        self.total_trades += 1;

        let one = dec!(1.0);
        let dynamic_step = self.dynamic_pricing.calculate_dynamic_step();

        match fill.side {
            OrderSide::Buy => {
                self.inventory_base += fill.qty;
                // Place counter SELL order 1 dynamic step above fill price
                let counter_price = (fill.price * (one + dynamic_step)).round_dp(2);
                let order = Order::new_limit_post_only(
                    &self.config.runner_id,
                    self.config.symbol.clone(),
                    OrderSide::Sell,
                    counter_price,
                    fill.qty,
                );
                info!(
                    "[{}] Grid BUY filled @ £{} -> counter SELL @ £{} (step: {:.3}%, inventory: {} {})",
                    self.config.runner_id,
                    fill.price,
                    counter_price,
                    dynamic_step * dec!(100.0),
                    self.inventory_base,
                    self.config.symbol.base
                );
                Some(order)
            }
            OrderSide::Sell => {
                self.inventory_base -= fill.qty;
                // Realized profit calculation: (Sell Price - Buy Price) * Qty ~ step * notional
                let profit = fill.price * fill.qty * dynamic_step;
                self.realized_pnl += profit;

                if self.config.mode.as_deref() == Some("WIND_DOWN") {
                    info!(
                        "[{}] Grid SELL filled @ £{} -> profit: £{:.4} -> No counter BUY (Winding Down)",
                        self.config.runner_id, fill.price, profit
                    );
                    return None;
                }

                // Place counter BUY order 1 dynamic step below fill price
                let counter_price = (fill.price * (one - dynamic_step)).round_dp(2);
                let order = Order::new_limit_post_only(
                    &self.config.runner_id,
                    self.config.symbol.clone(),
                    OrderSide::Buy,
                    counter_price,
                    fill.qty,
                );
                info!(
                    "[{}] Grid SELL filled @ £{} -> profit: £{:.4} -> counter BUY @ £{} (Total PnL: £{:.4})",
                    self.config.runner_id, fill.price, profit, counter_price, self.realized_pnl
                );
                Some(order)
            }
        }
    }

    /// Registers an order in active_orders once successfully validated and submitted
    pub fn register_active_order(&mut self, order: Order) {
        self.active_orders.insert(order.id, order);
    }

    /// Removes an order from active_orders (e.g. upon fill or cancellation)
    pub fn remove_active_order(&mut self, order_id: &Uuid) -> Option<Order> {
        self.active_orders.remove(order_id)
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
