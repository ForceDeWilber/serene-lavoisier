use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicPricingConfig {
    pub enabled: bool,
    pub base_step_pct: Decimal,          // e.g. 0.0040 (0.40%)
    pub min_step_pct: Decimal,           // e.g. 0.0015 (0.15%)
    pub max_step_pct: Decimal,           // e.g. 0.0150 (1.50%)
    pub baseline_volatility_pct: Decimal,// e.g. 0.0010 (0.10% standard deviation per 60s)
    pub inventory_gamma: Decimal,        // e.g. 0.10 (risk aversion factor for Avellaneda-Stoikov)
    pub max_inventory_skew_pct: Decimal, // e.g. 0.0150 (1.50% max center drift due to inventory)
    pub min_clip_fiat: Decimal,          // e.g. 1.00 (Revolut X order floor)
    pub max_clip_fiat: Decimal,          // e.g. 100.00
    pub volatility_window_secs: i64,     // e.g. 120s
}

impl Default for DynamicPricingConfig {
    fn default() -> Self {
        let min_step = std::env::var("GRID_MIN_STEP_PCT")
            .ok()
            .and_then(|v| v.parse::<Decimal>().ok())
            .unwrap_or(dec!(0.0009)); // 0.09% minimum spread for 0.00% maker fee on Revolut X

        Self {
            enabled: true,
            base_step_pct: dec!(0.0040),
            min_step_pct: min_step,
            max_step_pct: dec!(0.0150),
            baseline_volatility_pct: dec!(0.0010),
            inventory_gamma: dec!(0.08),
            max_inventory_skew_pct: dec!(0.0150),
            min_clip_fiat: dec!(1.00),
            max_clip_fiat: dec!(100.00),
            volatility_window_secs: 120,
        }
    }
}

#[derive(Debug, Clone)]
struct PriceTick {
    timestamp: DateTime<Utc>,
    price: Decimal,
}

/// Computes rolling volatility and calculates inventory-skewed, volatility-adaptive grid rungs
#[derive(Debug)]
pub struct DynamicPriceModel {
    pub config: DynamicPricingConfig,
    samples: VecDeque<PriceTick>,
}

impl DynamicPriceModel {
    pub fn new(config: DynamicPricingConfig) -> Self {
        Self {
            config,
            samples: VecDeque::new(),
        }
    }

    /// Records a new mid-price sample and trims outdated records
    pub fn record_sample(&mut self, price: Decimal, now: DateTime<Utc>) {
        if price <= Decimal::ZERO {
            return;
        }

        self.samples.push_back(PriceTick {
            timestamp: now,
            price,
        });

        let cutoff = now - Duration::seconds(self.config.volatility_window_secs);
        while let Some(front) = self.samples.front() {
            if front.timestamp < cutoff {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    /// Calculates rolling return volatility over the sample window
    pub fn calculate_volatility(&self) -> Decimal {
        if self.samples.len() < 3 {
            return self.config.baseline_volatility_pct;
        }

        let mut returns = Vec::with_capacity(self.samples.len() - 1);
        for i in 1..self.samples.len() {
            let p_prev = self.samples[i - 1].price;
            let p_curr = self.samples[i].price;
            if p_prev > Decimal::ZERO {
                let ret = (p_curr - p_prev) / p_prev;
                returns.push(ret);
            }
        }

        if returns.is_empty() {
            return self.config.baseline_volatility_pct;
        }

        let n = Decimal::from(returns.len());
        let mean = returns.iter().copied().sum::<Decimal>() / n;
        let variance = returns
            .iter()
            .map(|&r| (r - mean) * (r - mean))
            .sum::<Decimal>()
            / n;

        // Approximate sqrt using f64 conversion for sub-microsecond speed
        let var_f64 = variance.to_string().parse::<f64>().unwrap_or(0.0);
        let std_dev = var_f64.sqrt();
        let vol = Decimal::from_str_exact(&format!("{:.6}", std_dev))
            .unwrap_or(self.config.baseline_volatility_pct);

        vol.max(dec!(0.0001))
    }

    /// Calculates volatility-adjusted grid step percentage
    pub fn calculate_dynamic_step(&self) -> Decimal {
        if !self.config.enabled {
            return self.config.base_step_pct;
        }

        let current_vol = self.calculate_volatility();
        let ratio = current_vol / self.config.baseline_volatility_pct;
        let adjusted_step = self.config.base_step_pct * ratio;

        adjusted_step
            .max(self.config.min_step_pct)
            .min(self.config.max_step_pct)
    }

    /// Calculates Avellaneda-Stoikov inventory reservation price:
    /// P_reservation = mid_price * (1 - skew_pct)
    /// where skew_pct = clamp(q * gamma * vol, -max_skew, +max_skew)
    /// Positive inventory (long base asset) shades price down to stimulate sells.
    /// Negative or flat inventory shades price up to incentivize buys.
    pub fn calculate_reservation_price(&self, mid_price: Decimal, inventory_base: Decimal) -> Decimal {
        if !self.config.enabled || mid_price <= Decimal::ZERO {
            return mid_price;
        }

        let vol = self.calculate_volatility();
        let raw_skew = inventory_base * self.config.inventory_gamma * vol;
        let skew_pct = raw_skew
            .max(-self.config.max_inventory_skew_pct)
            .min(self.config.max_inventory_skew_pct);

        let reservation = mid_price * (dec!(1.0) - skew_pct);
        reservation.round_dp(2)
    }

    /// Slices available free capital into dynamic rung clips
    pub fn calculate_order_clip(
        &self,
        free_fiat: Decimal,
        active_rungs: usize,
        configured_clip: Decimal,
    ) -> Decimal {
        if active_rungs == 0 {
            return configured_clip;
        }

        let rungs_dec = Decimal::from(active_rungs);
        let budget_per_rung = (free_fiat / rungs_dec).round_dp(2);

        // Respect min and max clips
        let clip = budget_per_rung
            .min(configured_clip)
            .max(self.config.min_clip_fiat)
            .min(self.config.max_clip_fiat);

        if free_fiat < self.config.min_clip_fiat {
            Decimal::ZERO
        } else {
            clip
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_pricing_volatility_scaling() {
        let config = DynamicPricingConfig::default();
        let mut model = DynamicPriceModel::new(config);
        let now = Utc::now();

        // Baseline (no samples yet)
        assert_eq!(model.calculate_dynamic_step(), dec!(0.0040));

        // Inject low-volatility ticks
        model.record_sample(dec!(60000.0), now - Duration::seconds(30));
        model.record_sample(dec!(60005.0), now - Duration::seconds(20));
        model.record_sample(dec!(60002.0), now - Duration::seconds(10));
        model.record_sample(dec!(60004.0), now);

        let quiet_step = model.calculate_dynamic_step();
        assert!(quiet_step <= dec!(0.0040), "Quiet market should tighten step");

        // Inject turbulent breakout ticks
        model.record_sample(dec!(60500.0), now + Duration::seconds(2));
        model.record_sample(dec!(59700.0), now + Duration::seconds(4));
        model.record_sample(dec!(61200.0), now + Duration::seconds(6));

        let volatile_step = model.calculate_dynamic_step();
        assert!(volatile_step > dec!(0.0040), "Turbulent market should widen step");
    }

    #[test]
    fn test_inventory_skew_reservation_price() {
        let config = DynamicPricingConfig::default();
        let model = DynamicPriceModel::new(config);
        let mid = dec!(60000.0);

        // Flat inventory: reservation == mid
        let p_flat = model.calculate_reservation_price(mid, dec!(0.0));
        assert_eq!(p_flat, mid);

        // Long inventory: reservation shifts down (encouraging sells)
        let p_long = model.calculate_reservation_price(mid, dec!(2.5));
        assert!(p_long < mid, "Long inventory should shift reservation price downward");

        // Short / flat inventory: reservation shifts up
        let p_short = model.calculate_reservation_price(mid, dec!(-2.5));
        assert!(p_short > mid, "Short inventory should shift reservation price upward");
    }

    #[test]
    fn test_dynamic_clip_sizing() {
        let config = DynamicPricingConfig::default();
        let model = DynamicPriceModel::new(config);

        // Sufficient capital: $500 free fiat / 5 rungs -> $100 -> clamped to configured $50 clip
        let clip1 = model.calculate_order_clip(dec!(500.0), 5, dec!(50.0));
        assert_eq!(clip1, dec!(50.0));

        // Constrained capital: $16.68 free fiat / 5 rungs -> $3.34 per rung
        let clip2 = model.calculate_order_clip(dec!(16.68), 5, dec!(50.0));
        assert_eq!(clip2, dec!(3.34));

        // Exhausted capital: $0.50 free fiat (< min clip $1.00) -> 0
        let clip3 = model.calculate_order_clip(dec!(0.50), 5, dec!(50.0));
        assert_eq!(clip3, dec!(0.0));
    }
}
