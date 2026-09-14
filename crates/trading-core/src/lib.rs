pub mod db;
pub mod execution;
pub mod model;
pub mod risk;
pub mod runner;
pub mod simulator;
pub mod strategy;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use model::*;
    use risk::*;
    use rust_decimal_macros::dec;
    use simulator::*;
    use strategy::*;

    #[test]
    fn test_symbol_formatting() {
        let sym = Symbol::btc_gbp();
        assert_eq!(sym.as_slash(), "BTC/GBP");
        assert_eq!(sym.as_dash(), "BTC-GBP");
    }

    #[test]
    fn test_adverse_selection_lag_filter() {
        let mut filter = AdverseSelectionLagFilter::new(60, dec!(0.006)); // 0.6% threshold
        let now = Utc::now();

        // Normal fluctuating price: 50,000 -> 49,900 (0.2% drop)
        assert!(!filter.record_price(dec!(50000.0), now));
        assert!(!filter.record_price(dec!(49900.0), now + Duration::seconds(10)));

        // Sharp sudden dump: 49,900 -> 49,500 (1.0% drop from 50,000 peak)
        let is_dump = filter.record_price(dec!(49500.0), now + Duration::seconds(20));
        assert!(is_dump, "Expected adverse selection lag filter to trigger on 1.0% sudden drop");
    }

    #[test]
    fn test_circuit_breaker() {
        let mut cb = CircuitBreaker::new(dec!(0.05)); // 5% max drawdown
        let now = Utc::now();

        cb.reset(dec!(1000.0));
        assert!(!cb.is_tripped());

        // Price rises: equity 1050
        assert!(!cb.update_equity(dec!(1050.0), now));

        // Drawdown of 3% (1050 -> 1018.5): shouldn't trip
        assert!(!cb.update_equity(dec!(1020.0), now + Duration::minutes(10)));
        assert!(!cb.is_tripped());

        // Drawdown of 6% (1050 -> 987): should trip!
        assert!(cb.update_equity(dec!(980.0), now + Duration::minutes(30)));
        assert!(cb.is_tripped());
    }

    #[test]
    fn test_capital_envelope_locking() {
        let mut env = EnvelopeBalance::new("runner_btc", "GBP", dec!(500.0));
        assert_eq!(env.available, dec!(500.0));
        assert_eq!(env.locked, dec!(0.0));

        // Lock 200
        assert!(env.lock(dec!(200.0)).is_ok());
        assert_eq!(env.available, dec!(300.0));
        assert_eq!(env.locked, dec!(200.0));

        // Overdraw by 400 (only 300 available)
        assert!(env.lock(dec!(400.0)).is_err());

        // Unlock 100
        env.unlock(dec!(100.0));
        assert_eq!(env.available, dec!(400.0));
        assert_eq!(env.locked, dec!(100.0));
    }

    #[test]
    fn test_geometric_grid_spacing() {
        let config = GridConfig {
            runner_id: "test_runner".into(),
            symbol: Symbol::btc_gbp(),
            step_pct: dec!(0.005), // 0.5%
            rungs_per_side: 2,
            order_size_gbp: dec!(50.0),
            rebalance_threshold_pct: dec!(0.015),
            dynamic_pricing: DynamicPricingConfig {
                enabled: false, // test static baseline spacing
                ..Default::default()
            },
        };
        let mut strategy = GeometricGridStrategy::new(config);
        let center = dec!(50000.0);
        let orders = strategy.initialize_grid(center, Some(dec!(100.0)), Some(dec!(1.0)));

        // Expect 2 buys and 2 sells
        assert_eq!(orders.len(), 4);
        let buys: Vec<&Order> = orders.iter().filter(|o| o.side == OrderSide::Buy).collect();
        let sells: Vec<&Order> = orders.iter().filter(|o| o.side == OrderSide::Sell).collect();

        assert_eq!(buys.len(), 2);
        assert_eq!(sells.len(), 2);

        // Buy 1: 50,000 * (1 - 0.005) = 49,750
        assert_eq!(buys[0].price, dec!(49750.00));
        // Sell 1: 50,000 * (1 + 0.005) = 50,250
        assert_eq!(sells[0].price, dec!(50250.00));
    }

    #[test]
    fn test_dynamic_grid_inventory_skew() {
        let config = GridConfig {
            runner_id: "test_skew_runner".into(),
            symbol: Symbol::btc_gbp(),
            step_pct: dec!(0.005),
            rungs_per_side: 2,
            order_size_gbp: dec!(50.0),
            rebalance_threshold_pct: dec!(0.015),
            dynamic_pricing: DynamicPricingConfig::default(),
        };
        let mut strategy = GeometricGridStrategy::new(config);
        strategy.inventory_base = dec!(3.0); // positive inventory (long 3 BTC)
        let center = dec!(50000.0);

        let orders = strategy.initialize_grid(center, Some(dec!(100.0)), Some(dec!(3.0)));
        assert_eq!(orders.len(), 4);
        let buys: Vec<&Order> = orders.iter().filter(|o| o.side == OrderSide::Buy).collect();

        // Effective center should be skewed downward from 50,000 to stimulate sells
        assert!(strategy.effective_center.unwrap() < center);
        // Buy prices should be lower than symmetric 49,750
        assert!(buys[0].price < dec!(49750.00));
    }

    #[tokio::test]
    async fn test_paper_execution_matching() {
        let sim = PaperExecutionSimulator::new(dec!(1000.0));
        let symbol = Symbol::btc_gbp();

        // Submit Buy limit order @ £49,000 for 0.01 BTC (£490)
        let buy_order = Order::new_limit_post_only("runner_1", symbol.clone(), OrderSide::Buy, dec!(49000.0), dec!(0.01));
        let _ = sim.submit_order(buy_order).await;

        // Tick @ £50,000: No fill
        let tick1 = MarketTick {
            symbol: symbol.clone(),
            bid: dec!(49990.0),
            ask: dec!(50010.0),
            last: dec!(50000.0),
            timestamp: Utc::now(),
        };
        let fills1 = sim.process_tick(&tick1).await;
        assert_eq!(fills1.len(), 0);

        // Tick dips to £48,900: Buy order fills!
        let tick2 = MarketTick {
            symbol: symbol.clone(),
            bid: dec!(48890.0),
            ask: dec!(48910.0),
            last: dec!(48900.0),
            timestamp: Utc::now(),
        };
        let fills2 = sim.process_tick(&tick2).await;
        assert_eq!(fills2.len(), 1);
        assert_eq!(fills2[0].side, OrderSide::Buy);
        assert_eq!(fills2[0].price, dec!(49000.0)); // Filled at limit price

        // Wallet checks: 1000 - 490 = 510 GBP, 0.01 BTC
        let wallet = sim.get_wallet().await;
        assert_eq!(wallet.gbp, dec!(510.0));
        assert_eq!(wallet.btc, dec!(0.01));
    }
}
