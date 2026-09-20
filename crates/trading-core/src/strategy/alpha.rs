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
    pub plunge_threshold_pct: Decimal, // e.g. dec!(0.0035) = -0.35% in fast window
    pub surge_threshold_pct: Decimal,  // e.g. dec!(0.0035) = +0.35% in fast window
    pub fast_window_secs: i64,         // e.g. 3 seconds
    pub slow_window_secs: i64,         // e.g. 15 seconds
    pub stabilization_secs: i64,       // e.g. 8 seconds of quiet after plunge
    pub max_stabilization_vol: Decimal,// e.g. dec!(0.0010) = 0.10% max oscillation
}

impl Default for AlphaConfig {
    fn default() -> Self {
        Self {
            plunge_threshold_pct: dec!(0.0035),
            surge_threshold_pct: dec!(0.0035),
            fast_window_secs: 3,
            slow_window_secs: 15,
            stabilization_secs: 8,
            max_stabilization_vol: dec!(0.0010),
        }
    }
}

pub struct AlphaMomentumEngine {
    config: AlphaConfig,
    samples: VecDeque<TickSample>,
    current_regime: MarketRegime,
    plunge_start_time: Option<DateTime<Utc>>,
    last_plunge_price: Option<Decimal>,
    stabilization_counter: usize,
}

impl AlphaMomentumEngine {
    pub fn new(config: AlphaConfig) -> Self {
        Self {
            config,
            samples: VecDeque::new(),
            current_regime: MarketRegime::Normal,
            plunge_start_time: None,
            last_plunge_price: None,
            stabilization_counter: 0,
        }
    }

    pub fn current_regime(&self) -> MarketRegime {
        self.current_regime
    }

    /// Ingests a new tick and evaluates the market regime.
    /// Returns (current_regime, should_cancel_bids, should_reanchor_bottom)
    pub fn record_tick(&mut self, price: Decimal, now: DateTime<Utc>) -> (MarketRegime, bool, bool) {
        if price <= Decimal::ZERO {
            return (self.current_regime, false, false);
        }

        self.samples.push_back(TickSample { timestamp: now, price });

        // Evict samples older than slow window
        let max_age = Duration::seconds(self.config.slow_window_secs);
        let cutoff = now - max_age;
        while let Some(front) = self.samples.front() {
            if front.timestamp < cutoff {
                self.samples.pop_front();
            } else {
                break;
            }
        }

        if self.samples.len() < 2 {
            return (self.current_regime, false, false);
        }

        // 1. Calculate fast window velocity (e.g. 3s ago vs now)
        let fast_cutoff = now - Duration::seconds(self.config.fast_window_secs);
        let fast_anchor_price = self.samples
            .iter()
            .find(|s| s.timestamp >= fast_cutoff)
            .map(|s| s.price)
            .unwrap_or(price);

        let fast_velocity = if fast_anchor_price > Decimal::ZERO {
            (price - fast_anchor_price) / fast_anchor_price
        } else {
            Decimal::ZERO
        };

        // 2. Check for Toxic Plunge
        if fast_velocity <= -self.config.plunge_threshold_pct {
            if self.current_regime != MarketRegime::ToxicPlunge {
                warn!(
                    "[ALPHA-SIGNAL] TOXIC PLUNGE DETECTED! Price dropped {:.2}% in {}s (from {} to {}). Signaling bid cancellation.",
                    fast_velocity * dec!(100),
                    self.config.fast_window_secs,
                    fast_anchor_price,
                    price
                );
                self.current_regime = MarketRegime::ToxicPlunge;
                self.plunge_start_time = Some(now);
                self.last_plunge_price = Some(price);
                self.stabilization_counter = 0;
                return (MarketRegime::ToxicPlunge, true, false);
            }
            self.last_plunge_price = Some(price.min(self.last_plunge_price.unwrap_or(price)));
            return (MarketRegime::ToxicPlunge, false, false);
        }

        // 3. If currently in Toxic Plunge, check for Stabilization & Safe Bottom Re-anchoring
        if self.current_regime == MarketRegime::ToxicPlunge {
            let stab_duration = self.plunge_start_time
                .map(|t| (now - t).num_seconds())
                .unwrap_or(0);

            // Calculate volatility in the last stabilization window
            let stab_cutoff = now - Duration::seconds(self.config.stabilization_secs);
            let stab_samples: Vec<Decimal> = self.samples
                .iter()
                .filter(|s| s.timestamp >= stab_cutoff)
                .map(|s| s.price)
                .collect();

            let is_flat = if stab_samples.len() >= 3 {
                let min_p = stab_samples.iter().min().copied().unwrap_or(price);
                let max_p = stab_samples.iter().max().copied().unwrap_or(price);
                let range = if min_p > Decimal::ZERO { (max_p - min_p) / min_p } else { Decimal::ONE };
                range <= self.config.max_stabilization_vol && fast_velocity > -dec!(0.0010)
            } else {
                false
            };

            if stab_duration >= self.config.stabilization_secs && is_flat {
                self.stabilization_counter += 1;
                if self.stabilization_counter >= 3 {
                    info!(
                        "[ALPHA-SIGNAL] MARKET STABILIZED at bottom (£{}). Re-anchoring grid and resuming bids.",
                        price
                    );
                    self.current_regime = MarketRegime::Normal;
                    self.plunge_start_time = None;
                    self.last_plunge_price = None;
                    self.stabilization_counter = 0;
                    return (MarketRegime::Stabilizing, false, true);
                }
            } else {
                self.stabilization_counter = 0;
            }

            // Keep bids cancelled while in plunge
            return (MarketRegime::ToxicPlunge, false, false);
        }

        // 4. Check for Bullish Surge
        if fast_velocity >= self.config.surge_threshold_pct {
            if self.current_regime != MarketRegime::BullishSurge {
                info!(
                    "[ALPHA-SIGNAL] BULLISH SURGE DETECTED! Price surged +{:.2}% in {}s. Skewing grid rungs upward.",
                    fast_velocity * dec!(100),
                    self.config.fast_window_secs
                );
                self.current_regime = MarketRegime::BullishSurge;
            }
            return (MarketRegime::BullishSurge, false, false);
        }

        self.current_regime = MarketRegime::Normal;
        (MarketRegime::Normal, false, false)
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
            ((max_p - min_p) / min_p * dec!(100)).round_dp(4)
        } else {
            dec!(0.0010)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toxic_plunge_and_stabilization() {
        let config = AlphaConfig {
            plunge_threshold_pct: dec!(0.0035), // -0.35%
            surge_threshold_pct: dec!(0.0035),
            fast_window_secs: 3,
            slow_window_secs: 15,
            stabilization_secs: 5,
            max_stabilization_vol: dec!(0.0010),
        };
        let mut engine = AlphaMomentumEngine::new(config);
        let mut now = Utc::now();

        // 1. Stable baseline at £100
        for _ in 0..5 {
            now = now + Duration::seconds(1);
            let (regime, cancel, reanchor) = engine.record_tick(dec!(100.0), now);
            assert_eq!(regime, MarketRegime::Normal);
            assert!(!cancel);
            assert!(!reanchor);
        }

        // 2. Sudden plunge to £99.50 (-0.50% drop within 1 second)
        now = now + Duration::seconds(1);
        let (regime, cancel, reanchor) = engine.record_tick(dec!(99.50), now);
        assert_eq!(regime, MarketRegime::ToxicPlunge);
        assert!(cancel, "Should signal bid cancel on toxic plunge");
        assert!(!reanchor);

        // 3. Stabilization at £99.50 for 6 seconds
        let mut did_reanchor = false;
        for _ in 0..8 {
            now = now + Duration::seconds(1);
            let (_, _, reanchor) = engine.record_tick(dec!(99.50), now);
            if reanchor {
                did_reanchor = true;
                break;
            }
        }
        assert!(did_reanchor, "Should signal safe re-anchor after market stabilizes");
        assert_eq!(engine.current_regime(), MarketRegime::Normal);
    }

    #[test]
    fn test_bullish_surge() {
        let config = AlphaConfig::default();
        let mut engine = AlphaMomentumEngine::new(config);
        let mut now = Utc::now();

        // Baseline
        engine.record_tick(dec!(100.0), now);
        now = now + Duration::seconds(1);
        engine.record_tick(dec!(100.0), now);

        // Surge +0.50% to £100.50
        now = now + Duration::seconds(1);
        let (regime, cancel, reanchor) = engine.record_tick(dec!(100.50), now);
        assert_eq!(regime, MarketRegime::BullishSurge);
        assert!(!cancel);
        assert!(!reanchor);
    }
}
