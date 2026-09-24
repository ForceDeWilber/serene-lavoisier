use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketRegime {
    Normal,
    ToxicPlunge,
    Stabilizing,
    BullishSurge,
}

#[derive(Debug, Clone)]
struct TickSample {
    timestamp: DateTime<Utc>,
    price: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlphaConfig {
    pub plunge_threshold_pct: Decimal, // e.g. dec!(0.0015) = -0.15% dislocation or velocity
    pub surge_threshold_pct: Decimal,  // e.g. dec!(0.0020) = +0.20% dislocation or velocity
    pub fast_window_secs: i64,         // e.g. 3 seconds
    pub slow_window_secs: i64,         // e.g. 15 seconds
    pub stabilization_secs: i64,       // e.g. 6 seconds of quiet after plunge
    pub max_stabilization_vol: Decimal,// e.g. dec!(0.0010) = 0.10% max oscillation
    pub surge_transmission_alpha: Decimal, // e.g. dec!(0.80) = 80% momentum pass-through
}

impl Default for AlphaConfig {
    fn default() -> Self {
        Self {
            plunge_threshold_pct: dec!(0.0015), // 0.15% dislocation hurdle
            surge_threshold_pct: dec!(0.0020),  // 0.20% surge hurdle
            fast_window_secs: 3,
            slow_window_secs: 15,
            stabilization_secs: 6,
            max_stabilization_vol: dec!(0.0010),
            surge_transmission_alpha: dec!(0.80),
        }
    }
}

/// Global Consensus Oracle and Alpha Momentum Engine
/// Ingests Binance (USDT), Kraken (GBP), and Revolut X (Mid) feeds
/// Computes real-time Fair Value Consensus and classifies the market into 3 operational regimes
pub struct AlphaMomentumEngine {
    pub config: AlphaConfig,
    samples: VecDeque<TickSample>,
    current_regime: MarketRegime,
    pub is_hibernating: bool,
    plunge_start_time: Option<DateTime<Utc>>,
    last_plunge_price: Option<Decimal>,
    stabilization_counter: usize,

    // Real-time multi-exchange oracle state
    pub latest_binance_usdt: Option<(Decimal, DateTime<Utc>)>,
    pub latest_kraken_gbp: Option<(Decimal, DateTime<Utc>)>,
    pub latest_revolut_mid: Option<(Decimal, DateTime<Utc>)>,
    pub dynamic_fx_usd_to_gbp: Decimal,
    pub last_fair_value_gbp: Option<Decimal>,
    pub last_dislocation_pct: Option<Decimal>,
    pub predicted_surge_peak_gbp: Option<Decimal>,
}

impl AlphaMomentumEngine {
    pub fn new(config: AlphaConfig) -> Self {
        Self {
            config,
            samples: VecDeque::new(),
            current_regime: MarketRegime::Normal,
            is_hibernating: false,
            plunge_start_time: None,
            last_plunge_price: None,
            stabilization_counter: 0,
            latest_binance_usdt: None,
            latest_kraken_gbp: None,
            latest_revolut_mid: None,
            dynamic_fx_usd_to_gbp: dec!(0.76), // Baseline GBP/USD ~ 1.315 -> 0.760
            last_fair_value_gbp: None,
            last_dislocation_pct: None,
            predicted_surge_peak_gbp: None,
        }
    }

    pub fn current_regime(&self) -> MarketRegime {
        self.current_regime
    }

    pub fn is_hibernating(&self) -> bool {
        self.is_hibernating
    }

    pub fn current_fair_value_gbp(&self) -> Option<Decimal> {
        self.last_fair_value_gbp
    }

    pub fn current_dislocation_pct(&self) -> Option<Decimal> {
        self.last_dislocation_pct
    }

    pub fn predicted_surge_peak_gbp(&self) -> Option<Decimal> {
        self.predicted_surge_peak_gbp
    }

    /// Records high-frequency Binance USDT tick (e.g. XRP/USDT)
    pub fn record_binance_tick(&mut self, price_usdt: Decimal, now: DateTime<Utc>) -> (MarketRegime, bool, bool) {
        if price_usdt <= Decimal::ZERO {
            return (self.current_regime, false, false);
        }

        self.latest_binance_usdt = Some((price_usdt, now));
        self.samples.push_back(TickSample { timestamp: now, price: price_usdt });

        // Update dynamic FX if Kraken tick is recent (< 5s)
        if let Some((kraken_p, k_ts)) = self.latest_kraken_gbp {
            if (now - k_ts).num_seconds().abs() < 5 && kraken_p > Decimal::ZERO {
                let implied_fx = kraken_p / price_usdt;
                if implied_fx > dec!(0.50) && implied_fx < dec!(1.10) {
                    // Smooth FX update (95% previous, 5% instantaneous)
                    self.dynamic_fx_usd_to_gbp = (self.dynamic_fx_usd_to_gbp * dec!(0.95) + implied_fx * dec!(0.05)).round_dp(4);
                }
            }
        }

        self.prune_samples(now);
        self.evaluate_regime(now)
    }

    /// Records native Kraken GBP tick (e.g. XRP/GBP)
    pub fn record_kraken_tick(&mut self, price_gbp: Decimal, now: DateTime<Utc>) -> (MarketRegime, bool, bool) {
        if price_gbp <= Decimal::ZERO {
            return (self.current_regime, false, false);
        }

        self.latest_kraken_gbp = Some((price_gbp, now));

        // Update dynamic FX if Binance tick is recent (< 5s)
        if let Some((binance_p, b_ts)) = self.latest_binance_usdt {
            if (now - b_ts).num_seconds().abs() < 5 && binance_p > Decimal::ZERO {
                let implied_fx = price_gbp / binance_p;
                if implied_fx > dec!(0.50) && implied_fx < dec!(1.10) {
                    self.dynamic_fx_usd_to_gbp = (self.dynamic_fx_usd_to_gbp * dec!(0.95) + implied_fx * dec!(0.05)).round_dp(4);
                }
            }
        }

        self.evaluate_regime(now)
    }

    /// Records local Revolut X BBO mid-price
    pub fn record_revolut_tick(&mut self, mid_price: Decimal, now: DateTime<Utc>) -> (MarketRegime, bool, bool) {
        if mid_price <= Decimal::ZERO {
            return (self.current_regime, false, false);
        }

        self.latest_revolut_mid = Some((mid_price, now));
        self.evaluate_regime(now)
    }

    /// Legacy single-price tick record method for backward compatibility
    pub fn record_tick(&mut self, price: Decimal, now: DateTime<Utc>) -> (MarketRegime, bool, bool) {
        self.record_binance_tick(price, now)
    }

    fn prune_samples(&mut self, now: DateTime<Utc>) {
        let max_age = Duration::seconds(self.config.slow_window_secs);
        let cutoff = now - max_age;
        while let Some(front) = self.samples.front() {
            if front.timestamp < cutoff {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    /// Calculates Global Fair Value as the median of active venue prices in GBP
    pub fn calculate_fair_value(&self, now: DateTime<Utc>) -> Option<Decimal> {
        let max_staleness = Duration::seconds(30);
        let mut prices = Vec::new();

        // 1. Binance converted to GBP
        if let Some((p_usdt, ts)) = self.latest_binance_usdt {
            if now - ts <= max_staleness && p_usdt > Decimal::ZERO {
                let p_gbp = (p_usdt * self.dynamic_fx_usd_to_gbp).round_dp(4);
                prices.push(p_gbp);
            }
        }

        // 2. Kraken GBP
        if let Some((p_gbp, ts)) = self.latest_kraken_gbp {
            if now - ts <= max_staleness && p_gbp > Decimal::ZERO {
                prices.push(p_gbp);
            }
        }

        // 3. Revolut Mid GBP
        if let Some((p_rev, ts)) = self.latest_revolut_mid {
            if now - ts <= max_staleness && p_rev > Decimal::ZERO {
                prices.push(p_rev);
            }
        }

        if prices.is_empty() {
            return None;
        }

        prices.sort();
        let n = prices.len();
        if n == 1 {
            Some(prices[0])
        } else if n == 2 {
            Some(((prices[0] + prices[1]) / dec!(2.0)).round_dp(4))
        } else {
            // Median of 3 prices
            Some(prices[n / 2])
        }
    }

    /// Evaluates current market regime and signals necessary action:
    /// Returns `(current_regime, should_cancel_bids, should_reanchor_bottom)`
    pub fn evaluate_regime(&mut self, now: DateTime<Utc>) -> (MarketRegime, bool, bool) {
        let fair_val = self.calculate_fair_value(now);
        self.last_fair_value_gbp = fair_val;

        let revolut_mid = self.latest_revolut_mid.map(|(p, _)| p);

        // Dislocation = (Fair_Value - Revolut_Mid) / Revolut_Mid
        // Negative -> Global price is lower than Revolut (Plunge incoming)
        // Positive -> Global price is higher than Revolut (Surge incoming)
        let dislocation = match (fair_val, revolut_mid) {
            (Some(fv), Some(rev)) if rev > Decimal::ZERO => {
                let d = (fv - rev) / rev;
                self.last_dislocation_pct = Some((d * dec!(100.0)).round_dp(4));
                Some(d)
            }
            _ => None,
        };

        // Fast window velocity on Binance leader
        let fast_velocity = self.calculate_fast_velocity(now);

        // 1. Check for Toxic Plunge (Adverse Selection Defense)
        // Triggers if Global Fair Value is <= -plunge_threshold_pct below Revolut
        // OR Binance velocity dropped <= -plunge_threshold_pct within fast window
        let is_disloc_plunge = dislocation.map_or(false, |d| d <= -self.config.plunge_threshold_pct);
        let is_velocity_plunge = fast_velocity <= -self.config.plunge_threshold_pct;

        if is_disloc_plunge || is_velocity_plunge {
            if self.current_regime != MarketRegime::ToxicPlunge {
                warn!(
                    "[ALPHA-SHIELD] TOXIC PLUNGE DETECTED! Dislocation: {:.3}%, Binance Velocity: {:.3}% in {}s. HIBERNATING ALL BUY RUNGS.",
                    dislocation.unwrap_or(Decimal::ZERO) * dec!(100.0),
                    fast_velocity * dec!(100.0),
                    self.config.fast_window_secs
                );
                self.current_regime = MarketRegime::ToxicPlunge;
                self.is_hibernating = true;
                self.plunge_start_time = Some(now);
                self.last_plunge_price = fair_val.or(revolut_mid);
                self.stabilization_counter = 0;
                return (MarketRegime::ToxicPlunge, true, false);
            }
            return (MarketRegime::ToxicPlunge, false, false);
        }

        // 2. Stabilization after plunge: Check for safe bottom re-anchoring
        if self.current_regime == MarketRegime::ToxicPlunge {
            let stab_duration = self.plunge_start_time
                .map(|t| (now - t).num_seconds())
                .unwrap_or(0);

            let is_disloc_recovered = dislocation.map_or(true, |d| d > -dec!(0.0010));
            let is_velocity_flat = fast_velocity > -dec!(0.0010);

            if stab_duration >= self.config.stabilization_secs && is_disloc_recovered && is_velocity_flat {
                self.stabilization_counter += 1;
                if self.stabilization_counter >= 3 {
                    info!(
                        "[ALPHA-SHIELD] MARKET STABILIZED! Global Fair Value £{} in consensus with Revolut. Lifting hibernation and resuming buy rungs.",
                        fair_val.unwrap_or_default()
                    );
                    self.current_regime = MarketRegime::Normal;
                    self.is_hibernating = false;
                    self.plunge_start_time = None;
                    self.last_plunge_price = None;
                    self.stabilization_counter = 0;
                    return (MarketRegime::Stabilizing, false, true);
                }
            } else {
                self.stabilization_counter = 0;
            }

            // Maintain hibernation while in plunge
            self.is_hibernating = true;
            return (MarketRegime::ToxicPlunge, false, false);
        }

        // 3. Bullish Surge (Surge Harvest & Snipe)
        let is_disloc_surge = dislocation.map_or(false, |d| d >= self.config.surge_threshold_pct);
        let is_velocity_surge = fast_velocity >= self.config.surge_threshold_pct;

        if is_disloc_surge || is_velocity_surge {
            if let Some(rev) = revolut_mid {
                let lead_pct = dislocation.unwrap_or(fast_velocity).max(Decimal::ZERO);
                let surge_peak = (rev * (dec!(1.0) + lead_pct * self.config.surge_transmission_alpha)).round_dp(4);
                self.predicted_surge_peak_gbp = Some(surge_peak);
            }

            if self.current_regime != MarketRegime::BullishSurge {
                info!(
                    "[ALPHA-SIGNAL] BULLISH SURGE DETECTED! Dislocation: {:.3}%, Predicted Peak: £{}. Widening counter-sells.",
                    dislocation.unwrap_or(Decimal::ZERO) * dec!(100.0),
                    self.predicted_surge_peak_gbp.unwrap_or_default()
                );
                self.current_regime = MarketRegime::BullishSurge;
            }
            return (MarketRegime::BullishSurge, false, false);
        }

        // 4. Equilibrium Consensus
        self.current_regime = MarketRegime::Normal;
        self.is_hibernating = false;
        self.predicted_surge_peak_gbp = None;
        (MarketRegime::Normal, false, false)
    }

    fn calculate_fast_velocity(&self, now: DateTime<Utc>) -> Decimal {
        if self.samples.len() < 2 {
            return Decimal::ZERO;
        }

        let fast_cutoff = now - Duration::seconds(self.config.fast_window_secs);
        let fast_anchor_price = self.samples
            .iter()
            .find(|s| s.timestamp >= fast_cutoff)
            .map(|s| s.price)
            .unwrap_or_else(|| self.samples.back().map(|s| s.price).unwrap_or(Decimal::ZERO));

        let current_price = self.samples.back().map(|s| s.price).unwrap_or(Decimal::ZERO);

        if fast_anchor_price > Decimal::ZERO && current_price > Decimal::ZERO {
            (current_price - fast_anchor_price) / fast_anchor_price
        } else {
            Decimal::ZERO
        }
    }

    /// Calculates rolling volatility percentage over the slow window
    pub fn rolling_volatility_pct(&self) -> Decimal {
        if self.samples.len() < 3 {
            return dec!(0.0010);
        }

        let prices: Vec<Decimal> = self.samples.iter().map(|s| s.price).collect();
        let min_p = prices.iter().min().copied().unwrap_or(Decimal::ONE);
        let max_p = prices.iter().max().copied().unwrap_or(Decimal::ONE);

        if min_p > Decimal::ZERO {
            ((max_p - min_p) / min_p * dec!(100.0)).round_dp(4)
        } else {
            dec!(0.0010)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_fair_value_median() {
        let mut engine = AlphaMomentumEngine::new(AlphaConfig::default());
        let now = Utc::now();

        // 1. Ingest Binance XRP @ $1.50 -> £1.1400 (using fx 0.7600)
        engine.record_binance_tick(dec!(1.50), now);
        // 2. Ingest Kraken XRP @ £1.1410
        engine.record_kraken_tick(dec!(1.1410), now);
        // 3. Ingest Revolut XRP Mid @ £1.1420
        engine.record_revolut_tick(dec!(1.1420), now);

        let fair_val = engine.calculate_fair_value(now).unwrap();
        // Prices in GBP: [1.1400, 1.1410, 1.1420]. Median should be exactly £1.1410
        assert_eq!(fair_val, dec!(1.1410));
    }

    #[test]
    fn test_dislocation_plunge_and_hibernation() {
        let config = AlphaConfig {
            plunge_threshold_pct: dec!(0.0015), // 0.15%
            surge_threshold_pct: dec!(0.0020),  // 0.20%
            fast_window_secs: 3,
            slow_window_secs: 15,
            stabilization_secs: 5,
            max_stabilization_vol: dec!(0.0010),
            surge_transmission_alpha: dec!(0.80),
        };
        let mut engine = AlphaMomentumEngine::new(config);
        let mut now = Utc::now();

        // Equilibrium baseline @ £1.1400
        engine.record_binance_tick(dec!(1.50), now); // $1.50 * 0.76 = £1.1400
        engine.record_kraken_tick(dec!(1.1400), now);
        let (regime, cancel, _) = engine.record_revolut_tick(dec!(1.1400), now);
        assert_eq!(regime, MarketRegime::Normal);
        assert!(!cancel);
        assert!(!engine.is_hibernating());

        // Global venues dump! Binance drops to $1.49 (-0.67%) -> £1.1324. Kraken drops to £1.1330.
        // Revolut order book is lagging, sitting at £1.1400.
        now = now + Duration::seconds(1);
        let (b_regime, b_cancel, _) = engine.record_binance_tick(dec!(1.49), now);
        assert_eq!(b_regime, MarketRegime::ToxicPlunge);
        assert!(b_cancel, "Must signal cancel bids on plunge");
        assert!(engine.is_hibernating(), "Must be in hibernation mode");

        engine.record_kraken_tick(dec!(1.1330), now);
        let (regime, _, reanchor) = engine.record_revolut_tick(dec!(1.1400), now);

        // Fair value is ~£1.1330. Dislocation vs Revolut £1.1400 is ~ -0.61%
        assert_eq!(regime, MarketRegime::ToxicPlunge);
        assert!(!reanchor);
        assert!(engine.is_hibernating(), "Must be in hibernation mode");

        // Stabilization: Market stabilizes at £1.1330 for 6 seconds
        let mut did_reanchor = false;
        for _ in 0..8 {
            now = now + Duration::seconds(1);
            engine.record_binance_tick(dec!(1.4908), now);
            engine.record_kraken_tick(dec!(1.1330), now);
            let (r, _, reanch) = engine.record_revolut_tick(dec!(1.1330), now);
            if reanch {
                did_reanchor = true;
                assert_eq!(r, MarketRegime::Stabilizing);
                break;
            }
        }

        assert!(did_reanchor, "Must reanchor and resume after market stabilizes");
        assert!(!engine.is_hibernating(), "Hibernation must be lifted after stabilization");
    }

    #[test]
    fn test_surge_and_predicted_peak() {
        let config = AlphaConfig::default();
        let mut engine = AlphaMomentumEngine::new(config);
        let mut now = Utc::now();

        // Baseline £1.1400
        engine.record_binance_tick(dec!(1.50), now);
        engine.record_kraken_tick(dec!(1.1400), now);
        engine.record_revolut_tick(dec!(1.1400), now);

        // Binance & Kraken surge +0.50% to £1.1457 while Revolut lags at £1.1400
        now = now + Duration::seconds(1);
        engine.record_binance_tick(dec!(1.5075), now); // $1.5075 * 0.76 = £1.1457
        engine.record_kraken_tick(dec!(1.1460), now);
        let (regime, cancel, reanchor) = engine.record_revolut_tick(dec!(1.1400), now);

        assert_eq!(regime, MarketRegime::BullishSurge);
        assert!(!cancel);
        assert!(!reanchor);
        assert!(engine.predicted_surge_peak_gbp().is_some());
        let peak = engine.predicted_surge_peak_gbp().unwrap();
        assert!(peak > dec!(1.1400), "Peak should be projected higher than current mid");
    }

    #[test]
    fn test_slow_dislocation_without_binance_velocity_spike() {
        let config = AlphaConfig {
            plunge_threshold_pct: dec!(0.0015), // 0.15%
            surge_threshold_pct: dec!(0.0020),
            fast_window_secs: 3,
            slow_window_secs: 15,
            stabilization_secs: 5,
            max_stabilization_vol: dec!(0.0010),
            surge_transmission_alpha: dec!(0.80),
        };
        let mut engine = AlphaMomentumEngine::new(config);
        let now = Utc::now();

        // Baseline: Kraken and Binance at £1.1350, but Revolut lagging high at £1.1400 (dislocation = -0.44%)
        engine.record_binance_tick(dec!(1.4934), now); // $1.4934 * 0.76 = ~£1.1350
        engine.record_kraken_tick(dec!(1.1350), now);
        
        // Fast velocity on Binance is 0.0 because there's only 1 sample, but Revolut mid is 1.1400
        let (regime, cancel, _) = engine.record_revolut_tick(dec!(1.1400), now);

        assert_eq!(regime, MarketRegime::ToxicPlunge, "Dislocation alone must trigger plunge shield");
        assert!(cancel, "Must signal cancel bids even without fast velocity spike");
        assert!(engine.is_hibernating());
    }
}

