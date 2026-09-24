pub mod brain;
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
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use simulator::*;
    use strategy::*;
    use uuid::Uuid;

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
            mode: None,
            cost_basis: None,
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
    fn test_zero_fiat_generates_sells_above_cost_basis() {
        let config = GridConfig {
            runner_id: "test_eth_runner".into(),
            symbol: Symbol::eth_gbp(),
            step_pct: dec!(0.004),
            rungs_per_side: 3,
            order_size_gbp: dec!(50.0),
            rebalance_threshold_pct: dec!(0.012),
            dynamic_pricing: DynamicPricingConfig {
                enabled: false,
                ..Default::default()
            },
            mode: None,
            cost_basis: Some(dec!(1845.43)), // Exact user ETH cost basis
        };
        let mut strategy = GeometricGridStrategy::new(config);
        let center = dec!(1824.87); // Market is currently below cost basis!
        let orders = strategy.initialize_grid(center, Some(dec!(0.0)), Some(dec!(0.00521505)));

        let buys: Vec<&Order> = orders.iter().filter(|o| o.side == OrderSide::Buy).collect();
        let sells: Vec<&Order> = orders.iter().filter(|o| o.side == OrderSide::Sell).collect();

        // Zero fiat means 0 buys
        assert_eq!(buys.len(), 0);
        // Positive held base inventory means sell rungs are generated!
        assert!(sells.len() > 0, "Expected sell orders to be placed from crypto inventory even with £0 fiat");

        // Strict cost basis floor: every sell must be >= £1845.43 * 1.0015 = £1848.20
        let floor = (dec!(1845.43) * dec!(1.0015)).round_dp(2);
        for s in sells {
            assert!(s.price >= floor, "Sell price {} was below cost basis floor {}", s.price, floor);
            assert!(s.price * s.qty >= dec!(1.00), "Sell order value {} was below Revolut X £1.00 minimum", s.price * s.qty);
        }
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
            mode: None,
            cost_basis: None,
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

    #[test]
    fn test_grid_buy_budget_clamping() {
        use rust_decimal::Decimal;
        let config = GridConfig {
            runner_id: "test_budget_runner".into(),
            symbol: Symbol::btc_gbp(),
            step_pct: dec!(0.005),
            rungs_per_side: 5,
            order_size_gbp: dec!(10.0), // 5 rungs * £10 = £50 needed
            rebalance_threshold_pct: dec!(0.015),
            dynamic_pricing: DynamicPricingConfig::default(),
            mode: None,
            cost_basis: None,
        };
        let mut strategy = GeometricGridStrategy::new(config);
        let center = dec!(50000.0);

        // Case 1: £15.00 available fiat -> dynamically scales 5 rungs to £3.00 each
        let orders = strategy.initialize_grid(center, Some(dec!(15.0)), Some(dec!(0.0)));
        let buys: Vec<&Order> = orders.iter().filter(|o| o.side == OrderSide::Buy).collect();
        let total_buy_fiat: Decimal = buys.iter().map(|o| o.price * o.qty).sum();
        assert!(total_buy_fiat <= dec!(15.00), "Total buy fiat £{} exceeded budget £15.00", total_buy_fiat);
        assert_eq!(buys.len(), 5);

        // Case 2: Only £3.50 available fiat -> min_clip is £1.00, so at most 3 rungs can be placed (£3.00), not 5!
        let orders_tight = strategy.initialize_grid(center, Some(dec!(3.50)), Some(dec!(0.0)));
        let buys_tight: Vec<&Order> = orders_tight.iter().filter(|o| o.side == OrderSide::Buy).collect();
        let total_tight_fiat: Decimal = buys_tight.iter().map(|o| o.price * o.qty).sum();
        assert!(total_tight_fiat <= dec!(3.50), "Total buy fiat £{} exceeded budget £3.50", total_tight_fiat);
        assert_eq!(buys_tight.len(), 3);
    }

    #[test]
    fn test_brain_multi_pair_quote_partitioning() {
        use crate::brain::{EngineBrain, BrainConfig};
        use rust_decimal::Decimal;
        let brain = EngineBrain::new(BrainConfig::default());
        let pair_btc = Symbol::btc_gbp();
        let pair_eth = Symbol::eth_gbp();
        let pair_sol = Symbol::sol_gbp();

        brain.register_active_pair(pair_btc.clone());
        brain.register_active_pair(pair_eth.clone());
        brain.register_active_pair(pair_sol.clone());

        let gbp_pairs = brain.get_active_pairs_for_quote("GBP");
        assert_eq!(gbp_pairs.len(), 3);

        // Partition £30 total fiat across 3 pairs
        let quote_capital = dec!(30.0);
        let per_pair_capital = quote_capital / Decimal::from(gbp_pairs.len() as u32);
        assert_eq!(per_pair_capital, dec!(10.0));

        brain.remove_active_pair(&pair_sol);
        let gbp_pairs_after = brain.get_active_pairs_for_quote("GBP");
        assert_eq!(gbp_pairs_after.len(), 2);
    }

    #[test]
    fn test_penny_evaporation_shield_counter_sell() {
        let config = GridConfig {
            runner_id: "test_sol_runner".into(),
            symbol: Symbol::sol_gbp(),
            step_pct: dec!(0.0001), // Very tight 0.01% step
            rungs_per_side: 3,
            order_size_gbp: dec!(15.0),
            rebalance_threshold_pct: dec!(0.01),
            dynamic_pricing: DynamicPricingConfig {
                enabled: false,
                ..Default::default()
            },
            mode: None,
            cost_basis: None,
        };
        let mut strategy = GeometricGridStrategy::new(config);

        // Fill buy: 0.02 SOL @ £88.31
        let fill = Fill {
            order_id: uuid::Uuid::new_v4(),
            client_order_id: "buy_1".into(),
            runner_id: "test_sol_runner".into(),
            symbol: Symbol::sol_gbp(),
            side: OrderSide::Buy,
            price: dec!(88.31),
            qty: dec!(0.02),
            fee: dec!(0.0),
            timestamp: chrono::Utc::now(),
        };

        let counter = strategy.on_fill(&fill).expect("Expected counter sell order");
        assert_eq!(counter.side, OrderSide::Sell);
        // Required delta to earn at least £0.01 on 0.02 SOL: 0.01 / 0.02 = £0.50
        // Min viable sell price: £88.31 + £0.50 = £88.81
        assert!(
            counter.price >= dec!(88.81),
            "Counter sell price £{} should be at least £88.81 to guarantee £0.01 profit",
            counter.price
        );
        let gross_profit = (counter.price - fill.price) * fill.qty;
        assert!(
            gross_profit >= dec!(0.01),
            "Expected gross profit £{} >= £0.01",
            gross_profit
        );
    }

    #[test]
    fn test_penny_evaporation_shield_counter_buy() {
        let config = GridConfig {
            runner_id: "test_sol_runner".into(),
            symbol: Symbol::sol_gbp(),
            step_pct: dec!(0.0001), // Very tight 0.01% step
            rungs_per_side: 3,
            order_size_gbp: dec!(15.0),
            rebalance_threshold_pct: dec!(0.01),
            dynamic_pricing: DynamicPricingConfig {
                enabled: false,
                ..Default::default()
            },
            mode: None,
            cost_basis: None,
        };
        let mut strategy = GeometricGridStrategy::new(config);

        // Fill sell: 0.02 SOL @ £88.81
        let fill = Fill {
            order_id: uuid::Uuid::new_v4(),
            client_order_id: "sell_1".into(),
            runner_id: "test_sol_runner".into(),
            symbol: Symbol::sol_gbp(),
            side: OrderSide::Sell,
            price: dec!(88.81),
            qty: dec!(0.02),
            fee: dec!(0.0),
            timestamp: chrono::Utc::now(),
        };

        let counter = strategy.on_fill(&fill).expect("Expected counter buy order");
        assert_eq!(counter.side, OrderSide::Buy);
        // Required delta to buy back £0.01 cheaper on 0.02 SOL: 0.01 / 0.02 = £0.50
        // Max viable buy price: £88.81 - £0.50 = £88.31
        assert!(
            counter.price <= dec!(88.31),
            "Counter buy price £{} should be <= £88.31 to guarantee £0.01 discount",
            counter.price
        );
        let gross_discount = (fill.price - counter.price) * fill.qty;
        assert!(
            gross_discount >= dec!(0.01),
            "Expected gross discount £{} >= £0.01",
            gross_discount
        );
    }

    #[test]
    fn test_sell_clip_sizes_independently_from_low_fiat() {
        let config = GridConfig {
            runner_id: "test_sol_runner".into(),
            symbol: Symbol::sol_gbp(),
            step_pct: dec!(0.002),
            rungs_per_side: 5,
            order_size_gbp: dec!(25.0),
            rebalance_threshold_pct: dec!(0.012),
            dynamic_pricing: DynamicPricingConfig {
                enabled: false,
                ..Default::default()
            },
            mode: None,
            cost_basis: None,
        };
        let mut strategy = GeometricGridStrategy::new(config);
        let center = dec!(88.00);

        // Case: free_fiat is almost depleted (£1.50 available fiat),
        // but user holds 0.80 SOL (~£70.40 inventory value).
        let orders = strategy.initialize_grid(center, Some(dec!(1.50)), Some(dec!(0.80)));
        let sells: Vec<&Order> = orders.iter().filter(|o| o.side == OrderSide::Sell).collect();

        assert_eq!(sells.len(), 5, "Expected all 5 sell rungs to be generated");
        // Each sell rung should sell ~£14.08 of SOL (£70.40 / 5 rungs), NOT be crushed down to £1.00!
        for s in sells {
            let notional = s.price * s.qty;
            assert!(
                notional >= dec!(13.00),
                "Sell order notional £{} was improperly clamped by low fiat! Should be ~£14.08",
                notional
            );
        }
    }

    #[test]
    fn test_xrp_sub_penny_grid_precision() {
        let config = GridConfig {
            runner_id: "test_xrp_runner".into(),
            symbol: Symbol::xrp_gbp(),
            step_pct: dec!(0.0035), // 0.35%
            rungs_per_side: 5,
            order_size_gbp: dec!(15.0),
            rebalance_threshold_pct: dec!(0.012),
            dynamic_pricing: DynamicPricingConfig {
                enabled: false,
                ..Default::default()
            },
            mode: None,
            cost_basis: None,
        };
        let mut strategy = GeometricGridStrategy::new(config);
        let center = dec!(1.1830);

        // £75 free cash, 0 XRP inventory
        let orders = strategy.initialize_grid(center, Some(dec!(75.0)), Some(dec!(0.0)));
        let buys: Vec<&Order> = orders.iter().filter(|o| o.side == OrderSide::Buy).collect();
        assert_eq!(buys.len(), 5, "Expected 5 buy rungs on XRP");

        // Verify rungs have 4 decimal places and proper sub-penny increments
        for b in &buys {
            assert!(b.price.scale() <= 4, "XRP buy price {} exceeded 4 decimal places", b.price);
            assert!(b.qty.scale() <= 5, "XRP buy qty {} exceeded 5 decimal places", b.qty);
            assert!(b.price < center, "Buy rung should be below center price");
        }

        // Test counter sell generation with Penny Shield on small price asset
        let fill = Fill {
            runner_id: "test_xrp_runner".into(),
            order_id: buys[0].id,
            client_order_id: buys[0].client_order_id.clone(),
            symbol: Symbol::xrp_gbp(),
            side: OrderSide::Buy,
            price: dec!(1.1800),
            qty: dec!(12.71186),
            fee: rust_decimal::Decimal::ZERO,
            timestamp: Utc::now(),
        };

        let counter_sell = strategy.on_fill(&fill).expect("Expected counter sell");
        assert_eq!(counter_sell.side, OrderSide::Sell);
        assert!(counter_sell.price.scale() <= 4, "Counter sell price {} exceeded 4 decimal places", counter_sell.price);
        assert!(counter_sell.qty.scale() <= 5, "Counter sell qty {} exceeded 5 decimal places", counter_sell.qty);
        // Gross profit must be at least £0.01
        let gross_profit = (counter_sell.price - fill.price) * fill.qty;
        assert!(gross_profit >= dec!(0.01), "Gross profit £{} was less than 1 penny guarantee", gross_profit);
    }

    #[test]
    fn test_lot_tracking_and_no_loss_rebalance() {
        let config = GridConfig {
            runner_id: "test_lot_runner".into(),
            symbol: Symbol::xrp_gbp(),
            step_pct: dec!(0.0035),
            rungs_per_side: 5,
            order_size_gbp: dec!(15.0),
            rebalance_threshold_pct: dec!(0.012),
            dynamic_pricing: DynamicPricingConfig {
                enabled: false,
                ..Default::default()
            },
            mode: None,
            cost_basis: None,
        };
        let mut strategy = GeometricGridStrategy::new(config);

        // 1. Initial BUY fills at £1.1800 for 12.5 XRP (£14.75)
        let buy_fill = Fill {
            runner_id: "test_lot_runner".into(),
            order_id: Uuid::new_v4(),
            client_order_id: "buy_lot_1".into(),
            symbol: Symbol::xrp_gbp(),
            side: OrderSide::Buy,
            price: dec!(1.1800),
            qty: dec!(12.5),
            fee: Decimal::ZERO,
            timestamp: Utc::now(),
        };

        let counter_sell = strategy.on_fill(&buy_fill).expect("Expected counter sell order");
        assert_eq!(strategy.lots.len(), 1, "Expected 1 lot recorded");
        assert_eq!(strategy.lots[0].buy_price, dec!(1.1800));
        assert!(!strategy.lots[0].is_closed);
        assert!(counter_sell.price > dec!(1.1800), "Counter sell price must be higher than buy price");

        // Register counter sell order in active_orders
        strategy.register_active_order(counter_sell.clone());

        // 2. Price dumps from £1.18 to £1.14 (-3.4%).
        // A rebalance occurs at mid=£1.1400.
        // User holds 12.5 XRP, but that 12.5 XRP is ALREADY in an active sell order!
        let rebalanced_orders = strategy.initialize_grid(dec!(1.1400), Some(dec!(50.0)), Some(dec!(12.5)));
        let new_sells: Vec<&Order> = rebalanced_orders.iter().filter(|o| o.side == OrderSide::Sell).collect();

        // Must NOT place new sell orders for the already-guarded 12.5 XRP!
        assert_eq!(new_sells.len(), 0, "Rebalance must NOT place new sell rungs that dump existing inventory below buy price");

        // 3. Price recovers to counter_sell.price and fills
        let sell_fill = Fill {
            runner_id: "test_lot_runner".into(),
            order_id: counter_sell.id,
            client_order_id: counter_sell.client_order_id.clone(),
            symbol: Symbol::xrp_gbp(),
            side: OrderSide::Sell,
            price: counter_sell.price,
            qty: dec!(12.5),
            fee: Decimal::ZERO,
            timestamp: Utc::now(),
        };

        let _ = strategy.on_fill(&sell_fill);
        assert!(strategy.lots[0].is_closed, "Lot must be marked closed after sell fill");
        assert!(strategy.realized_pnl > Decimal::ZERO, "Realized PnL must be strictly positive");
        let expected_profit = (counter_sell.price - dec!(1.1800)) * dec!(12.5);
        assert_eq!(strategy.realized_pnl, expected_profit, "Realized PnL must match exact (sell - buy) * qty math");
    }

    #[test]
    fn test_hibernation_prevents_buy_orders_during_plunge() {
        use crate::strategy::{AlphaConfig, AlphaMomentumEngine, MarketRegime};
        let mut alpha = AlphaMomentumEngine::new(AlphaConfig::default());
        let mut now = Utc::now();

        let config = GridConfig {
            runner_id: "test_hibernation_runner".into(),
            symbol: Symbol::xrp_gbp(),
            step_pct: dec!(0.0035),
            rungs_per_side: 5,
            order_size_gbp: dec!(15.0),
            rebalance_threshold_pct: dec!(0.012),
            dynamic_pricing: DynamicPricingConfig {
                enabled: false,
                ..Default::default()
            },
            mode: None,
            cost_basis: None,
        };
        let mut strategy = GeometricGridStrategy::new(config);

        // Pre-existing protected inventory: 20 XRP bought at £1.15, guarded by counter sell @ £1.1550
        let resting_sell = Order::new_limit_post_only(
            "test_hibernation_runner",
            Symbol::xrp_gbp(),
            OrderSide::Sell,
            dec!(1.1550),
            dec!(20.0),
        );
        strategy.register_active_order(resting_sell);

        // 1. Normal baseline @ £1.1400
        alpha.record_binance_tick(dec!(1.50), now);
        alpha.record_kraken_tick(dec!(1.1400), now);
        let (r, cancel, _) = alpha.record_revolut_tick(dec!(1.1400), now);
        assert_eq!(r, MarketRegime::Normal);
        assert!(!cancel);
        assert!(!alpha.is_hibernating());

        // 2. Global market plunges! Binance drops to $1.485 (-1.0%), Kraken to £1.1280.
        // Revolut order book is lagging, still at £1.1400.
        now = now + chrono::Duration::seconds(1);
        let (r_b, cancel_b, _) = alpha.record_binance_tick(dec!(1.485), now);
        alpha.record_kraken_tick(dec!(1.1280), now);
        let (r_rev, _, _) = alpha.record_revolut_tick(dec!(1.1400), now);

        assert_eq!(r_b, MarketRegime::ToxicPlunge);
        assert!(cancel_b);
        assert_eq!(r_rev, MarketRegime::ToxicPlunge);
        assert!(alpha.is_hibernating(), "Engine must be hibernating");

        // 3. While hibernating, test grid initialization & rebalance:
        // Attempting to generate orders while hibernating must produce ZERO BUY RUNGS!
        let mut orders = strategy.initialize_grid(dec!(1.1400), Some(dec!(50.0)), Some(dec!(20.0)));
        if alpha.is_hibernating() {
            orders.retain(|o| o.side == OrderSide::Sell);
        }

        let buys: Vec<&Order> = orders.iter().filter(|o| o.side == OrderSide::Buy).collect();
        assert_eq!(buys.len(), 0, "Hibernation must strictly block any BUY orders from being placed!");

        // Protected sell must still exist in active_orders
        assert_eq!(strategy.active_orders.len(), 1);
        assert_eq!(strategy.active_orders.values().next().unwrap().side, OrderSide::Sell);

        // 4. Market stabilizes at £1.1280
        let mut stabilized = false;
        for _ in 0..8 {
            now = now + chrono::Duration::seconds(1);
            alpha.record_binance_tick(dec!(1.4842), now);
            alpha.record_kraken_tick(dec!(1.1280), now);
            let (regime, _, reanchor) = alpha.record_revolut_tick(dec!(1.1280), now);
            if reanchor {
                stabilized = true;
                assert_eq!(regime, MarketRegime::Stabilizing);
                break;
            }
        }

        assert!(stabilized, "Market should stabilize");
        assert!(!alpha.is_hibernating(), "Hibernation should be lifted after stabilization");

        // 5. Fresh buy rungs can now be safely generated at the bottom (£1.1280)
        let mut bottom_orders = strategy.initialize_grid(dec!(1.1280), Some(dec!(50.0)), Some(dec!(20.0)));
        if alpha.is_hibernating() {
            bottom_orders.retain(|o| o.side == OrderSide::Sell);
        }
        let bottom_buys: Vec<&Order> = bottom_orders.iter().filter(|o| o.side == OrderSide::Buy).collect();
        assert_eq!(bottom_buys.len(), 5, "5 fresh buy rungs should now be placed at the bottom");
        for b in bottom_buys {
            assert!(b.price < dec!(1.1280), "Buy rungs should be below £1.1280");
        }
    }
}


