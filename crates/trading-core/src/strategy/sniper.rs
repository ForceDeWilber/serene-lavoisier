use crate::model::{OrderSide, Symbol};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SniperConfig {
    pub runner_id: String,
    pub symbol: Symbol,
    pub impulse_threshold_pct: Decimal, // e.g. dec!(0.0011) = 0.11%
    pub min_net_edge_pct: Decimal,      // e.g. dec!(0.0002) = 0.02%
    pub revolut_taker_fee_pct: Decimal, // e.g. dec!(0.0009) = 0.09%
    pub order_size_gbp: Decimal,        // e.g. dec!(50.0)
    pub scratch_timeout_ms: u64,        // e.g. 800ms
}

impl Default for SniperConfig {
    fn default() -> Self {
        Self {
            runner_id: "sniper_btc".into(),
            symbol: Symbol::btc_gbp(),
            impulse_threshold_pct: dec!(0.0011), // 0.11% hurdle
            min_net_edge_pct: dec!(0.0002),      // 0.02% net target
            revolut_taker_fee_pct: dec!(0.0009), // 0.09% taker fee
            order_size_gbp: dec!(50.0),
            scratch_timeout_ms: 800,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnipeOpportunity {
    pub side: OrderSide,
    pub entry_price: Decimal,
    pub target_price: Decimal,
    pub qty: Decimal,
    pub gross_edge_pct: Decimal,
    pub net_edge_pct: Decimal,
    pub estimated_profit_gbp: Decimal,
}

pub struct LeadLagSniperStrategy {
    pub config: SniperConfig,
    pub enabled: bool,
    pub total_snipes: usize,
    pub successful_snipes: usize,
    pub total_sniper_profit_gbp: Decimal,
    pub total_taker_fees_paid_gbp: Decimal,
    pub average_lead_advantage_ms: u64,
}

impl LeadLagSniperStrategy {
    pub fn new(config: SniperConfig) -> Self {
        Self {
            config,
            enabled: true,
            total_snipes: 0,
            successful_snipes: 0,
            total_sniper_profit_gbp: Decimal::ZERO,
            total_taker_fees_paid_gbp: Decimal::ZERO,
            average_lead_advantage_ms: 450,
        }
    }

    /// Evaluates if a Kraken tick dislocates against Revolut X BBO enough to justify an aggressive taker snipe
    pub fn evaluate_dislocation(
        &self,
        kraken_price: Decimal,
        rev_bid: Decimal,
        rev_ask: Decimal,
        free_gbp: Decimal,
    ) -> Option<SnipeOpportunity> {
        if !self.enabled || kraken_price <= Decimal::ZERO || rev_ask <= Decimal::ZERO || rev_bid <= Decimal::ZERO {
            return None;
        }

        // Bullish Surge: Kraken jumped UP higher than Revolut X ask
        if kraken_price > rev_ask {
            let gross_edge = (kraken_price - rev_ask) / rev_ask;
            let net_edge = gross_edge - self.config.revolut_taker_fee_pct;

            if gross_edge >= self.config.impulse_threshold_pct && net_edge >= self.config.min_net_edge_pct {
                let size_gbp = self.config.order_size_gbp.min(free_gbp);
                if size_gbp < dec!(2.0) {
                    return None;
                }

                let qty = (size_gbp / rev_ask).round_dp(6);
                let gross_profit = (size_gbp * gross_edge).round_dp(2);
                let taker_fee = (size_gbp * self.config.revolut_taker_fee_pct).round_dp(2);
                let net_profit = (gross_profit - taker_fee).max(dec!(0.01));

                return Some(SnipeOpportunity {
                    side: OrderSide::Buy,
                    entry_price: rev_ask,
                    target_price: kraken_price,
                    qty,
                    gross_edge_pct: (gross_edge * dec!(100.0)).round_dp(3),
                    net_edge_pct: (net_edge * dec!(100.0)).round_dp(3),
                    estimated_profit_gbp: net_profit,
                });
            }
        }

        None
    }

    pub fn record_snipe_result(&mut self, net_profit: Decimal, fee: Decimal, lead_ms: u64) {
        self.total_snipes += 1;
        if net_profit > Decimal::ZERO {
            self.successful_snipes += 1;
        }
        self.total_sniper_profit_gbp += net_profit;
        self.total_taker_fees_paid_gbp += fee;
        self.average_lead_advantage_ms = (self.average_lead_advantage_ms * 85 + lead_ms * 15) / 100;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_sniper_dislocation_hurdle() {
        let config = SniperConfig {
            runner_id: "sniper_btc".into(),
            symbol: Symbol::btc_gbp(),
            impulse_threshold_pct: dec!(0.0011), // 0.11%
            min_net_edge_pct: dec!(0.0002),      // 0.02%
            revolut_taker_fee_pct: dec!(0.0009), // 0.09%
            order_size_gbp: dec!(50.0),
            scratch_timeout_ms: 800,
        };
        let strategy = LeadLagSniperStrategy::new(config);

        let rev_bid = dec!(57870.0);
        let rev_ask = dec!(57880.0);
        let free_gbp = dec!(500.0);

        // Case 1: Kraken is at 57,890 (only +0.017% move, below 0.11% hurdle)
        let opp1 = strategy.evaluate_dislocation(dec!(57890.0), rev_bid, rev_ask, free_gbp);
        assert!(opp1.is_none(), "Expected no snipe when gross edge is only 0.017%");

        // Case 2: Kraken spikes to 57,960 (+0.138% move, exceeds 0.11% hurdle and covers 0.09% fee)
        let opp2 = strategy.evaluate_dislocation(dec!(57960.0), rev_bid, rev_ask, free_gbp);
        assert!(opp2.is_some(), "Expected snipe when gross edge is 0.138%");
        let snipe = opp2.unwrap();
        assert_eq!(snipe.side, OrderSide::Buy);
        assert_eq!(snipe.entry_price, rev_ask);
        assert_eq!(snipe.target_price, dec!(57960.0));
        assert!(snipe.net_edge_pct > dec!(0.02));
        assert!(snipe.estimated_profit_gbp > Decimal::ZERO);
    }
}
