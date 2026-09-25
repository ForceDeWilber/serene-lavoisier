pub mod alpha;
pub mod dynamic_pricing;
pub mod sniper;
pub use alpha::*;
pub use dynamic_pricing::*;
pub use sniper::*;

use crate::model::{Fill, Order, OrderSide, Symbol};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration as StdDuration;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryLot {
    pub lot_id: Uuid,
    pub buy_order_id: Uuid,
    pub buy_client_order_id: String,
    pub buy_price: Decimal,
    pub qty: Decimal,
    pub counter_sell_order_id: Option<Uuid>,
    pub counter_sell_client_order_id: Option<String>,
    pub target_sell_price: Decimal,
    pub is_closed: bool,
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
    pub lots: Vec<InventoryLot>,
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
            lots: Vec::new(),
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
        let min_clip = dec!(1.00);

        // Calculate allocatable fiat: if dynamic pricing is enabled and fiat is sufficient, reserve cash_reserve_pct
        let max_allocatable_fiat = if let Some(fiat) = free_fiat {
            let min_budget_for_full_grid = min_clip * Decimal::from(self.config.rungs_per_side);
            if self.dynamic_pricing.config.enabled && fiat >= min_budget_for_full_grid {
                (fiat * (dec!(1.0) - self.dynamic_pricing.config.cash_reserve_pct)).round_dp(2)
            } else {
                fiat
            }
        } else {
            Decimal::MAX
        };

        // Calculate dynamic order clip with inventory weighting based on allocatable fiat
        let order_clip = free_fiat.map_or(self.config.order_size_gbp, |_| {
            self.dynamic_pricing.calculate_order_clip_with_inventory(
                max_allocatable_fiat,
                self.config.rungs_per_side,
                self.config.order_size_gbp,
                self.inventory_base,
                mid_price,
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
        // Strictly respect cash_reserve_pct so capital is never 100% exhausted on a dip
        if order_clip >= dec!(1.00) && self.config.mode.as_deref() != Some("WIND_DOWN") {
            let mut current_buy_multiplier = one;
            let mut allocated_buy_fiat = Decimal::ZERO;

            for rung_idx in 1..=self.config.rungs_per_side {
                let remaining_fiat = max_allocatable_fiat - allocated_buy_fiat;

                if remaining_fiat < min_clip {
                    break;
                }

                // Progressive expanding geometric spacing factor
                let geo_factor = one + self.dynamic_pricing.config.geometric_spacing_ratio * Decimal::from(rung_idx - 1);
                let step_at_rung = dynamic_step * geo_factor;
                current_buy_multiplier *= one - step_at_rung;
                let rung_price = round_price_for(effective_center * current_buy_multiplier);
                if rung_price <= Decimal::ZERO {
                    continue;
                }

                let rung_clip = order_clip.min(remaining_fiat);
                if rung_clip < min_clip {
                    break;
                }

                let qty_dp = if self.config.symbol.base == "XRP" { 5 } else { 6 };
                let step = if self.config.symbol.base == "XRP" { dec!(0.00001) } else { dec!(0.000001) };
                let mut qty = (rung_clip / rung_price).round_dp(qty_dp);
                if qty <= Decimal::ZERO {
                    continue;
                }

                // If rounding qty caused notional to drop below min_clip, try bumping by smallest step if fiat allows
                while (qty * rung_price).round_dp(2) < min_clip && (qty + step) * rung_price <= remaining_fiat {
                    qty += step;
                }

                // If rounding qty caused notional to exceed remaining fiat, decrement to stay within budget
                while qty * rung_price > remaining_fiat && qty > step {
                    qty -= step;
                }

                let actual_notional = (rung_price * qty).round_dp(2);
                if actual_notional < min_clip {
                    break;
                }

                let order = Order::new_limit_post_only(
                    &self.config.runner_id,
                    self.config.symbol.clone(),
                    OrderSide::Buy,
                    rung_price,
                    qty,
                );
                new_orders.push(order);
                allocated_buy_fiat += actual_notional;
            }
        }

        // Generate SELL rungs above effective reservation center price
        // Only place sells if we hold unallocated base inventory (i.e. not already guarded by active sell orders)
        let resting_sell_qty: Decimal = self
            .active_orders
            .values()
            .filter(|o| o.side == OrderSide::Sell)
            .map(|o| o.qty)
            .sum();

        let raw_base = available_base.unwrap_or(self.inventory_base);
        let base_avail = (raw_base - resting_sell_qty).max(Decimal::ZERO);
        if base_avail > Decimal::ZERO {
            let mut current_sell_multiplier = one;
            let mut allocated_sell_base = Decimal::ZERO;

            let total_inv_val = (base_avail * effective_center).round_dp(2);
            let min_clip = dec!(1.00);

            if total_inv_val >= min_clip {
                let rungs_dec = Decimal::from(self.config.rungs_per_side.max(1));
                let per_rung = (total_inv_val / rungs_dec).round_dp(2);
                let sell_clip = per_rung.min(self.config.order_size_gbp).max(min_clip);

                let min_profit_multiplier = dec!(1.0015);
                let min_profit_floor_fiat = dec!(0.01);
                let dust = dec!(0.000001);

                let active_sell_cids: std::collections::HashSet<String> = self
                    .active_orders
                    .values()
                    .filter(|o| o.side == OrderSide::Sell)
                    .map(|o| o.client_order_id.clone())
                    .collect();

                let mut unassigned_lots: Vec<(usize, Decimal)> = self
                    .lots
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| !l.is_closed && l.counter_sell_client_order_id.as_ref().map_or(true, |cid| !active_sell_cids.contains(cid)))
                    .map(|(idx, l)| (idx, l.qty))
                    .collect();

                for rung_idx in 1..=self.config.rungs_per_side {
                    let geo_factor = one + self.dynamic_pricing.config.geometric_spacing_ratio * Decimal::from(rung_idx - 1);
                    let step_at_rung = dynamic_step * geo_factor;
                    current_sell_multiplier *= one + step_at_rung;

                    let remaining_base = base_avail - allocated_sell_base;
                    if remaining_base <= Decimal::ZERO {
                        break;
                    }

                    let qty_dp = if self.config.symbol.base == "XRP" { 5 } else { 6 };
                    let desired_qty = (sell_clip / effective_center).round_dp(qty_dp);
                    let qty = desired_qty.min(remaining_base);
                    if qty <= Decimal::ZERO {
                        break;
                    }

                    // Check minimum order size on Revolut X (>= 1.00 in quote currency)
                    let (final_qty, is_sweep) = if (qty * effective_center).round_dp(2) < min_clip {
                        if (remaining_base * effective_center).round_dp(2) >= min_clip {
                            (remaining_base, true)
                        } else {
                            break;
                        }
                    } else {
                        (qty, false)
                    };

                    // Allocate open lots to this sell rung and compute strict cost floor
                    let mut allocated_lots_for_order = Vec::new();
                    let mut max_lot_buy_price = None;
                    let mut needed_lot_qty = final_qty;

                    for (lot_idx, remaining_in_lot) in unassigned_lots.iter_mut() {
                        if needed_lot_qty <= dust {
                            break;
                        }
                        if *remaining_in_lot > dust {
                            let take = needed_lot_qty.min(*remaining_in_lot);
                            allocated_lots_for_order.push(*lot_idx);
                            let lot_buy_price = self.lots[*lot_idx].buy_price;
                            max_lot_buy_price = Some(max_lot_buy_price.map_or(lot_buy_price, |p: Decimal| p.max(lot_buy_price)));
                            *remaining_in_lot -= take;
                            needed_lot_qty -= take;
                        }
                    }

                    // Strict Zero-Loss Floor: Ensure sell price >= max_lot_buy_price (or cost_basis) * (1 + 0.15%) + Penny Shield
                    let reference_price = max_lot_buy_price
                        .or(self.config.cost_basis)
                        .unwrap_or(effective_center);
                    let floor_price = round_price_for(reference_price * min_profit_multiplier);

                    let (scale_factor, min_tick) = price_scale_and_tick(reference_price);
                    let raw_delta = min_profit_floor_fiat / final_qty;
                    let required_delta = (raw_delta * scale_factor).ceil() / scale_factor;
                    let penny_floor = reference_price + required_delta.max(min_tick);

                    let min_viable_sell_price = floor_price.max(penny_floor);
                    let base_ref = effective_center.max(reference_price);
                    let grid_target_price = round_price_for(base_ref * current_sell_multiplier);
                    let rung_price = grid_target_price.max(min_viable_sell_price);

                    if rung_price <= Decimal::ZERO {
                        break;
                    }

                    allocated_sell_base += final_qty;

                    let order = Order::new_limit_post_only(
                        &self.config.runner_id,
                        self.config.symbol.clone(),
                        OrderSide::Sell,
                        rung_price,
                        final_qty,
                    );

                    // Assign this order ID to all allocated lots
                    for &idx in &allocated_lots_for_order {
                        self.lots[idx].counter_sell_order_id = Some(order.id);
                        self.lots[idx].counter_sell_client_order_id = Some(order.client_order_id.clone());
                        self.lots[idx].target_sell_price = rung_price;
                    }

                    new_orders.push(order);

                    if is_sweep {
                        break;
                    }
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
        let min_profit_floor_fiat = dec!(0.01);

        match fill.side {
            OrderSide::Buy => {
                self.inventory_base += fill.qty;
                // Place counter SELL order 1 dynamic step above fill price
                let mut counter_price = round_price_for(fill.price * (one + dynamic_step));

                // Hard Penny Evaporation Shield:
                // Ensure the gross profit (counter_price - fill.price) * fill.qty is at least £0.01 (1 penny).
                if fill.qty > Decimal::ZERO {
                    let (scale_factor, min_tick) = price_scale_and_tick(fill.price);
                    let raw_delta = min_profit_floor_fiat / fill.qty;
                    let required_delta = (raw_delta * scale_factor).ceil() / scale_factor;
                    let min_viable_price = fill.price + required_delta.max(min_tick);
                    if counter_price < min_viable_price {
                        info!(
                            "[{}] Penny Evaporation Shield: bumping counter SELL price from £{} to £{} (guarantee >= £0.01 profit on qty {})",
                            self.config.runner_id, counter_price, min_viable_price, fill.qty
                        );
                        counter_price = min_viable_price;
                    }
                }

                let order = Order::new_limit_post_only(
                    &self.config.runner_id,
                    self.config.symbol.clone(),
                    OrderSide::Sell,
                    counter_price,
                    fill.qty,
                );

                // Register Lot in lot tracker
                let lot = InventoryLot {
                    lot_id: Uuid::new_v4(),
                    buy_order_id: fill.order_id,
                    buy_client_order_id: fill.client_order_id.clone(),
                    buy_price: fill.price,
                    qty: fill.qty,
                    counter_sell_order_id: Some(order.id),
                    counter_sell_client_order_id: Some(order.client_order_id.clone()),
                    target_sell_price: counter_price,
                    is_closed: false,
                };
                self.lots.push(lot);

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

                let mut remaining_fill_qty = fill.qty;
                let mut total_cost = Decimal::ZERO;
                let mut total_matched_qty = Decimal::ZERO;
                let dust = dec!(0.000001);

                // 1. First, match lots registered to this specific counter_sell_client_order_id
                for lot in self.lots.iter_mut() {
                    if !lot.is_closed && lot.counter_sell_client_order_id.as_deref() == Some(&fill.client_order_id) {
                        let take = remaining_fill_qty.min(lot.qty);
                        total_cost += take * lot.buy_price;
                        total_matched_qty += take;
                        lot.qty -= take;
                        remaining_fill_qty -= take;
                        if lot.qty <= dust {
                            lot.is_closed = true;
                        }
                        if remaining_fill_qty <= dust {
                            break;
                        }
                    }
                }

                // 2. If remaining fill qty > 0 (e.g. order covered multiple lots or was unmatched), match oldest open lots (FIFO)
                if remaining_fill_qty > dust {
                    for lot in self.lots.iter_mut() {
                        if !lot.is_closed {
                            let take = remaining_fill_qty.min(lot.qty);
                            total_cost += take * lot.buy_price;
                            total_matched_qty += take;
                            lot.qty -= take;
                            remaining_fill_qty -= take;
                            if lot.qty <= dust {
                                lot.is_closed = true;
                            }
                            if remaining_fill_qty <= dust {
                                break;
                            }
                        }
                    }
                }

                // 3. If any fill qty was unallocated to open lots (e.g. pre-existing inventory), use cost_basis or max open lot or fill.price
                if remaining_fill_qty > dust {
                    let fallback_cost = self.config.cost_basis
                        .or_else(|| self.lots.iter().filter(|l| !l.is_closed).map(|l| l.buy_price).max())
                        .unwrap_or(fill.price);
                    total_cost += remaining_fill_qty * fallback_cost;
                    total_matched_qty += remaining_fill_qty;
                }

                let effective_buy_price = if total_matched_qty > Decimal::ZERO {
                    total_cost / total_matched_qty
                } else {
                    fill.price
                };

                let profit = (fill.price - effective_buy_price) * fill.qty - fill.fee;
                self.realized_pnl += profit;

                info!(
                    "[{}] Grid SELL filled @ £{} (effective buy @ £{:.4}, qty: {}) -> True Realized Profit: £{:.4} (Total PnL: £{:.4})",
                    self.config.runner_id,
                    fill.price,
                    effective_buy_price,
                    fill.qty,
                    profit,
                    self.realized_pnl
                );

                if self.config.mode.as_deref() == Some("WIND_DOWN") {
                    info!(
                        "[{}] Grid SELL filled @ £{} -> profit: £{:.4} -> No counter BUY (Winding Down)",
                        self.config.runner_id, fill.price, profit
                    );
                    return None;
                }

                // Place counter BUY order 1 dynamic step below fill price,
                // also respecting penny shield so the buy-back discount earns at least 1 penny.
                let mut counter_price = round_price_for(fill.price * (one - dynamic_step));
                if fill.qty > Decimal::ZERO {
                    let (scale_factor, min_tick) = price_scale_and_tick(fill.price);
                    let raw_delta = min_profit_floor_fiat / fill.qty;
                    let required_delta = (raw_delta * scale_factor).ceil() / scale_factor;
                    let max_viable_buy = fill.price - required_delta.max(min_tick);
                    if counter_price > max_viable_buy && max_viable_buy > Decimal::ZERO {
                        info!(
                            "[{}] Penny Evaporation Shield: lowering counter BUY price from £{} to £{} (guarantee >= £0.01 discount on qty {})",
                            self.config.runner_id, counter_price, max_viable_buy, fill.qty
                        );
                        counter_price = max_viable_buy;
                    }
                }

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

    /// Checks if market mid-price drifted beyond the dynamic rebalance threshold
    pub fn needs_rebalance(
        &self,
        current_mid: Decimal,
        elapsed_since_fill: Option<StdDuration>,
        oracle_lead_pct: Option<Decimal>,
    ) -> bool {
        if let Some(center) = self.center_price {
            if center > Decimal::ZERO {
                let drift = (current_mid - center).abs() / center;
                let dynamic_threshold = self.dynamic_pricing.calculate_dynamic_rebalance_threshold(
                    self.config.rebalance_threshold_pct,
                    elapsed_since_fill,
                    self.inventory_base,
                    oracle_lead_pct,
                );
                return drift >= dynamic_threshold;
            }
        }
        false
    }

    /// Exposes the current effective dynamic rebalance threshold for telemetry and logging
    pub fn current_rebalance_threshold(
        &self,
        elapsed_since_fill: Option<StdDuration>,
        oracle_lead_pct: Option<Decimal>,
    ) -> Decimal {
        self.dynamic_pricing.calculate_dynamic_rebalance_threshold(
            self.config.rebalance_threshold_pct,
            elapsed_since_fill,
            self.inventory_base,
            oracle_lead_pct,
        )
    }
}

#[inline]
pub fn price_decimals_for(price: Decimal) -> u32 {
    if price < dec!(10.0) {
        4
    } else {
        2
    }
}

#[inline]
pub fn round_price_for(price: Decimal) -> Decimal {
    price.round_dp(price_decimals_for(price))
}

#[inline]
pub fn price_scale_and_tick(price: Decimal) -> (Decimal, Decimal) {
    if price < dec!(10.0) {
        (dec!(10000.0), dec!(0.0001))
    } else {
        (dec!(100.0), dec!(0.01))
    }
}

