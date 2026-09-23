use crate::model::{EnvelopeBalance, Order, OrderSide, Symbol};
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

#[derive(Error, Debug)]
pub enum RiskError {
    #[error("Circuit breaker is tripped: {0}")]
    CircuitBreakerTripped(String),
    #[error("Capital envelope exhausted: required {required} {currency}, available {available} {currency}")]
    EnvelopeExhausted {
        currency: String,
        required: Decimal,
        available: Decimal,
    },
    #[error("Adverse selection triggered: rapid price dump detected ({pct_drop}% drop), buy orders blocked")]
    AdverseSelectionBlocked { pct_drop: Decimal },
    #[error("Rate limit exceeded: Revolut X token bucket is depleted")]
    RateLimitDepleted,
}

/// Rolling window price sample for adverse selection detection
#[derive(Debug, Clone)]
struct PriceSample {
    timestamp: DateTime<Utc>,
    price: Decimal,
}

/// Detects sudden downward price velocity on Kraken to cancel toxic Revolut X bids
#[derive(Debug)]
pub struct AdverseSelectionLagFilter {
    samples: VecDeque<PriceSample>,
    window_duration: Duration,
    max_drop_threshold_pct: Decimal, // e.g. 0.006 = 0.6%
}

impl AdverseSelectionLagFilter {
    pub fn new(window_seconds: i64, max_drop_threshold_pct: Decimal) -> Self {
        Self {
            samples: VecDeque::new(),
            window_duration: Duration::seconds(window_seconds),
            max_drop_threshold_pct,
        }
    }

    pub fn record_price(&mut self, price: Decimal, now: DateTime<Utc>) -> bool {
        self.samples.push_back(PriceSample { timestamp: now, price });

        // Evict expired samples
        let cutoff = now - self.window_duration;
        while let Some(front) = self.samples.front() {
            if front.timestamp < cutoff {
                self.samples.pop_front();
            } else {
                break;
            }
        }

        // Check if there is an adverse rapid dump from the recent peak in window
        if let Some(peak) = self.samples.iter().map(|s| s.price).max() {
            if peak > Decimal::ZERO {
                let drop_pct = (peak - price) / peak;
                if drop_pct >= self.max_drop_threshold_pct {
                    warn!(
                        "[RISK-TRIGGER] Adverse selection triggered: price dropped {:.3}% (peak: {}, current: {}) within {}s",
                        drop_pct * dec!(100),
                        peak,
                        price,
                        self.window_duration.num_seconds()
                    );
                    return true;
                }
            }
        }
        false
    }
}

/// Rolling 2-hour window circuit breaker monitoring portfolio drawdown
#[derive(Debug)]
pub struct CircuitBreaker {
    tripped: Arc<AtomicBool>,
    max_drawdown_pct: Decimal, // e.g. 0.05 = 5.0%
    peak_equity: Decimal,
    last_reset: DateTime<Utc>,
}

impl CircuitBreaker {
    pub fn new(max_drawdown_pct: Decimal) -> Self {
        Self {
            tripped: Arc::new(AtomicBool::new(false)),
            max_drawdown_pct,
            peak_equity: Decimal::ZERO,
            last_reset: Utc::now(),
        }
    }

    pub fn is_tripped(&self) -> bool {
        self.tripped.load(Ordering::SeqCst)
    }

    pub fn trip(&self, reason: &str) {
        self.tripped.store(true, Ordering::SeqCst);
        error!("[RISK-TRIGGER] CRITICAL: Global circuit breaker TRIPPED! Reason: {}", reason);
    }

    pub fn reset(&mut self, initial_equity: Decimal) {
        self.peak_equity = initial_equity;
        self.tripped.store(false, Ordering::SeqCst);
        self.last_reset = Utc::now();
        info!("[RISK-RESET] Global circuit breaker manually RESET. Baseline peak equity set to £{}", initial_equity);
    }

    pub fn update_equity(&mut self, current_equity: Decimal, now: DateTime<Utc>) -> bool {
        if self.is_tripped() {
            return true;
        }

        // Reset peak every 2 hours
        if now - self.last_reset > Duration::hours(2) {
            self.peak_equity = current_equity;
            self.last_reset = now;
            return false;
        }

        if current_equity > self.peak_equity {
            self.peak_equity = current_equity;
            return false;
        }

        if self.peak_equity > Decimal::ZERO {
            let drawdown = (self.peak_equity - current_equity) / self.peak_equity;
            if drawdown >= self.max_drawdown_pct {
                self.trip(&format!(
                    "Portfolio drawdown of {:.2}% exceeded max threshold of {:.2}% (Peak: {}, Current: {})",
                    drawdown * dec!(100),
                    self.max_drawdown_pct * dec!(100),
                    self.peak_equity,
                    current_equity
                ));
                return true;
            }
        }
        false
    }
}

/// Central Risk Engine orchestrating envelopes, circuit breakers, and adverse selection
pub struct CentralRiskEngine {
    circuit_breaker: Mutex<CircuitBreaker>,
    envelopes: Mutex<HashMap<String, EnvelopeBalance>>, // key: "{runner_id}:{currency}"
    lag_filters: Mutex<HashMap<String, AdverseSelectionLagFilter>>, // key: symbol.as_slash()
    max_drawdown_pct: Decimal,
    lag_drop_threshold_pct: Decimal,
}

impl CentralRiskEngine {
    pub fn new(max_drawdown_pct: Decimal, lag_drop_threshold_pct: Decimal) -> Self {
        Self {
            circuit_breaker: Mutex::new(CircuitBreaker::new(max_drawdown_pct)),
            envelopes: Mutex::new(HashMap::new()),
            lag_filters: Mutex::new(HashMap::new()),
            max_drawdown_pct,
            lag_drop_threshold_pct,
        }
    }

    pub fn max_drawdown_pct(&self) -> Decimal {
        self.max_drawdown_pct
    }

    pub fn lag_drop_threshold_pct(&self) -> Decimal {
        self.lag_drop_threshold_pct
    }

    pub async fn register_envelope(&self, runner_id: &str, currency: &str, amount: Decimal) {
        let mut map = self.envelopes.lock().await;
        let key = format!("{}:{}", runner_id, currency.to_uppercase());
        map.insert(key, EnvelopeBalance::new(runner_id, currency, amount));
    }

    pub async fn get_envelope(&self, runner_id: &str, currency: &str) -> Option<EnvelopeBalance> {
        let map = self.envelopes.lock().await;
        let key = format!("{}:{}", runner_id, currency.to_uppercase());
        map.get(&key).cloned()
    }

    pub async fn record_kraken_tick(&self, symbol: &Symbol, price: Decimal, now: DateTime<Utc>) -> bool {
        let mut filters = self.lag_filters.lock().await;
        let filter = filters.entry(symbol.as_slash()).or_insert_with(|| {
            AdverseSelectionLagFilter::new(60, self.lag_drop_threshold_pct)
        });
        filter.record_price(price, now)
    }

    /// Validates an order intent before execution
    pub async fn validate_order(&self, order: &Order) -> Result<(), RiskError> {
        // 1. Check Circuit Breaker
        let cb = self.circuit_breaker.lock().await;
        if cb.is_tripped() {
            warn!("[RISK-REJECT] Order {} rejected: Global circuit breaker is active", order.client_order_id);
            return Err(RiskError::CircuitBreakerTripped(
                "Global circuit breaker is active. All order placement is halted.".into(),
            ));
        }
        drop(cb);

        // 2. Check Capital Envelope
        match order.side {
            OrderSide::Buy => {
                let required_currency = &order.symbol.quote;
                let required_amount = (order.price * order.qty).round_dp(2);
                let epsilon = dec!(0.005); // 0.5p tolerance for micro-decimal discrepancy

                let mut envelopes = self.envelopes.lock().await;
                let key = format!("{}:{}", order.runner_id, required_currency);
                let envelope = envelopes.get_mut(&key).ok_or_else(|| {
                    warn!(
                        "[RISK-REJECT] Order {} rejected: No capital envelope found for {}:{}",
                        order.client_order_id, order.runner_id, required_currency
                    );
                    RiskError::EnvelopeExhausted {
                        currency: required_currency.clone(),
                        required: required_amount,
                        available: Decimal::ZERO,
                    }
                })?;

                if envelope.available + epsilon < required_amount {
                    warn!(
                        "[RISK-REJECT] Order {} rejected: Envelope exhausted for {}. Required: {} {}, Available: {} {}",
                        order.client_order_id, order.runner_id, required_amount, required_currency, envelope.available, required_currency
                    );
                    return Err(RiskError::EnvelopeExhausted {
                        currency: required_currency.clone(),
                        required: required_amount,
                        available: envelope.available,
                    });
                }

                // Reserve the quote capital up to available
                let lock_amount = required_amount.min(envelope.available);
                envelope.lock(lock_amount).map_err(|_| RiskError::EnvelopeExhausted {
                    currency: required_currency.clone(),
                    required: lock_amount,
                    available: envelope.available,
                })?;
                debug!(
                    "[RISK-LOCK] Locked {} {} in envelope {} for order {}",
                    lock_amount, required_currency, key, order.client_order_id
                );
            }
            OrderSide::Sell => {
                // Spot Sell Orders: Liquidates base crypto inventory back into quote fiat.
                // If an explicit base envelope is configured, enforce and lock it;
                // Otherwise, permit the spot sell since it returns fiat rather than consuming it.
                let required_currency = &order.symbol.base;
                let mut envelopes = self.envelopes.lock().await;
                let key = format!("{}:{}", order.runner_id, required_currency);
                if let Some(envelope) = envelopes.get_mut(&key) {
                    if envelope.available < order.qty {
                        warn!(
                            "[RISK-REJECT] Spot sell order {} rejected: Base envelope exhausted for {}. Required: {} {}, Available: {} {}",
                            order.client_order_id, order.runner_id, order.qty, required_currency, envelope.available, required_currency
                        );
                        return Err(RiskError::EnvelopeExhausted {
                            currency: required_currency.clone(),
                            required: order.qty,
                            available: envelope.available,
                        });
                    }
                    envelope.lock(order.qty).map_err(|_| RiskError::EnvelopeExhausted {
                        currency: required_currency.clone(),
                        required: order.qty,
                        available: envelope.available,
                    })?;
                    debug!(
                        "[RISK-LOCK] Locked {} {} in envelope {} for order {}",
                        order.qty, required_currency, key, order.client_order_id
                    );
                }
            }
        }

        debug!("[RISK-CHECK] Order {} passed risk validation", order.client_order_id);
        Ok(())
    }

    pub async fn release_order_capital(&self, order: &Order) {
        let currency = match order.side {
            OrderSide::Buy => &order.symbol.quote,
            OrderSide::Sell => &order.symbol.base,
        };
        let amount = match order.side {
            OrderSide::Buy => (order.price * order.remaining_qty()).round_dp(2),
            OrderSide::Sell => order.remaining_qty(),
        };

        let mut envelopes = self.envelopes.lock().await;
        let key = format!("{}:{}", order.runner_id, currency);
        if let Some(env) = envelopes.get_mut(&key) {
            env.unlock(amount);
            debug!(
                "[RISK-RELEASE] Released {} {} back to envelope {} for order {}",
                amount, currency, key, order.client_order_id
            );
        }
    }

    /// Releases locked order capital upon fill completion so envelopes rotate properly
    pub async fn settle_fill(&self, fill: &crate::model::Fill) {
        let mut envelopes = self.envelopes.lock().await;

        match fill.side {
            OrderSide::Buy => {
                let key = format!("{}:{}", fill.runner_id, fill.symbol.quote);
                if let Some(env) = envelopes.get_mut(&key) {
                    let amount = (fill.price * fill.qty).round_dp(2);
                    env.unlock(amount);
                }
            }
            OrderSide::Sell => {
                let base_key = format!("{}:{}", fill.runner_id, fill.symbol.base);
                if let Some(env) = envelopes.get_mut(&base_key) {
                    env.unlock(fill.qty);
                }

                // SELL fill returns quote currency proceeds (e.g. GBP) into the runner's quote envelope
                let quote_key = format!("{}:{}", fill.runner_id, fill.symbol.quote);
                let quote_proceeds = (fill.price * fill.qty).round_dp(2);
                if let Some(env) = envelopes.get_mut(&quote_key) {
                    env.available += quote_proceeds;
                    env.allocated += quote_proceeds;
                    info!(
                        "[RISK-SETTLE] Credited {:.2} {} back to envelope {} from SELL fill {}",
                        quote_proceeds, fill.symbol.quote, quote_key, fill.client_order_id
                    );
                }
            }
        }
    }

    pub async fn is_circuit_breaker_tripped(&self) -> bool {
        self.circuit_breaker.lock().await.is_tripped()
    }

    pub async fn trip_circuit_breaker(&self, reason: &str) {
        self.circuit_breaker.lock().await.trip(reason);
    }

    pub async fn reset_circuit_breaker(&self, current_equity: Decimal) {
        self.circuit_breaker.lock().await.reset(current_equity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_risk_engine_validates_spot_sell_order() {
        let risk = CentralRiskEngine::new(dec!(0.05), dec!(0.006));
        let runner_id = "runner_btc_gbp";

        // Register only GBP quote envelope
        risk.register_envelope(runner_id, "GBP", dec!(500.0)).await;

        // Buy order for 0.001 BTC @ £50,000 (£50) succeeds
        let buy_order = Order::new_limit_post_only(runner_id, Symbol::btc_gbp(), OrderSide::Buy, dec!(50000.0), dec!(0.001));
        assert!(risk.validate_order(&buy_order).await.is_ok());

        // Sell order for 0.001 BTC @ £51,000 succeeds even with no BTC base envelope
        let sell_order = Order::new_limit_post_only(runner_id, Symbol::btc_gbp(), OrderSide::Sell, dec!(51000.0), dec!(0.001));
        assert!(risk.validate_order(&sell_order).await.is_ok());
    }

    #[tokio::test]
    async fn test_risk_engine_settle_fill_releases_locked_capital() {
        let risk = CentralRiskEngine::new(dec!(0.05), dec!(0.006));
        let runner_id = "runner_btc_gbp";

        // Register £100 envelope
        risk.register_envelope(runner_id, "GBP", dec!(100.0)).await;

        let buy_order = Order::new_limit_post_only(runner_id, Symbol::btc_gbp(), OrderSide::Buy, dec!(50000.0), dec!(0.001));
        assert!(risk.validate_order(&buy_order).await.is_ok());

        let env_before = risk.get_envelope(runner_id, "GBP").await.unwrap();
        assert_eq!(env_before.available, dec!(50.0));
        assert_eq!(env_before.locked, dec!(50.0));

        let fill = crate::model::Fill {
            order_id: buy_order.id,
            client_order_id: buy_order.client_order_id.clone(),
            runner_id: runner_id.to_string(),
            symbol: Symbol::btc_gbp(),
            side: OrderSide::Buy,
            price: dec!(50000.0),
            qty: dec!(0.001),
            fee: dec!(0.0),
            timestamp: chrono::Utc::now(),
        };

        risk.settle_fill(&fill).await;

        let env_after = risk.get_envelope(runner_id, "GBP").await.unwrap();
        assert_eq!(env_after.locked, dec!(0.0));
        assert_eq!(env_after.available, dec!(100.0));
    }

    #[tokio::test]
    async fn test_risk_engine_envelope_micro_precision_tolerance() {
        let risk = CentralRiskEngine::new(dec!(0.05), dec!(0.006));
        let runner_id = "runner_sol_gbp";

        // Recreate exact live incident:
        // Available: 15.52993406 GBP
        // Order: price 86.91, qty 0.178690 -> raw 15.52997991 GBP, rounded 15.53 GBP
        // Difference is 0.00004585 GBP (< 0.5p epsilon)
        risk.register_envelope(runner_id, "GBP", dec!(15.52993406)).await;

        let buy_order = Order::new_limit_post_only(runner_id, Symbol::sol_gbp(), OrderSide::Buy, dec!(86.91), dec!(0.178690));
        assert!(risk.validate_order(&buy_order).await.is_ok(), "Order should pass with epsilon tolerance");

        let env = risk.get_envelope(runner_id, "GBP").await.unwrap();
        assert_eq!(env.available, dec!(0.0), "All remaining available capital should be locked");
        assert_eq!(env.locked, dec!(15.52993406));
    }

    #[tokio::test]
    async fn test_risk_engine_settle_fill_on_sell_credits_quote_envelope() {
        let risk = CentralRiskEngine::new(dec!(0.05), dec!(0.006));
        let runner_id = "runner_xrp_gbp";

        // Register small £2.00 initial quote envelope
        risk.register_envelope(runner_id, "GBP", dec!(2.00)).await;

        // Spot sell fill occurs: 12.4 XRP @ £1.15 = £14.26 GBP proceeds
        let sell_fill = crate::model::Fill {
            order_id: uuid::Uuid::new_v4(),
            client_order_id: "test-sell-fill".to_string(),
            runner_id: runner_id.to_string(),
            symbol: Symbol::new("XRP", "GBP"),
            side: OrderSide::Sell,
            price: dec!(1.15),
            qty: dec!(12.4),
            fee: dec!(0.0),
            timestamp: chrono::Utc::now(),
        };

        risk.settle_fill(&sell_fill).await;

        let env_after = risk.get_envelope(runner_id, "GBP").await.unwrap();
        // Envelope should now have £2.00 + £14.26 = £16.26 available!
        assert_eq!(env_after.available, dec!(16.26));
        assert_eq!(env_after.allocated, dec!(16.26));
    }
}

