use crate::model::Symbol;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainConfig {
    pub sniper_reserve_pct: Decimal,
    pub min_sniper_reserve_fiat: Decimal,
    pub min_order_floor: Decimal,
    pub pair_weights: HashMap<String, Decimal>,
}

impl Default for BrainConfig {
    fn default() -> Self {
        let mut weights = HashMap::new();
        weights.insert("BTC/GBP".to_string(), dec!(0.40));
        weights.insert("ETH/GBP".to_string(), dec!(0.30));
        weights.insert("SOL/GBP".to_string(), dec!(0.30));
        weights.insert("BTC/USD".to_string(), dec!(0.40));
        weights.insert("ETH/USD".to_string(), dec!(0.30));
        weights.insert("SOL/USD".to_string(), dec!(0.30));

        Self {
            sniper_reserve_pct: Decimal::ZERO,
            min_sniper_reserve_fiat: Decimal::ZERO,
            min_order_floor: dec!(1.00),
            pair_weights: weights,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainTelemetryDto {
    pub total_settled_cash: Decimal,
    pub sniper_reserve_fiat: Decimal,
    pub grid_pool_fiat: Decimal,
    pub pair_allocations: HashMap<String, Decimal>,
    pub status: String,
    pub decision_summary: String,
}

#[derive(Debug)]
pub struct EngineBrain {
    pub config: RwLock<BrainConfig>,
    pub last_known_cash: RwLock<HashMap<String, Decimal>>,
    pub active_pairs: RwLock<Vec<Symbol>>,
}

impl Default for EngineBrain {
    fn default() -> Self {
        Self::new(BrainConfig::default())
    }
}

impl EngineBrain {
    pub fn new(config: BrainConfig) -> Self {
        Self {
            config: RwLock::new(config),
            last_known_cash: RwLock::new(HashMap::new()),
            active_pairs: RwLock::new(Vec::new()),
        }
    }

    pub fn register_active_pair(&self, symbol: Symbol) {
        if let Ok(mut list) = self.active_pairs.write() {
            if !list.iter().any(|s| s == &symbol) {
                list.push(symbol);
            }
        }
    }

    pub fn remove_active_pair(&self, symbol: &Symbol) {
        if let Ok(mut list) = self.active_pairs.write() {
            list.retain(|s| s != symbol);
        }
    }

    pub fn get_active_pairs_for_quote(&self, quote_currency: &str) -> Vec<Symbol> {
        let quote_up = quote_currency.to_uppercase();
        let list = self.active_pairs
            .read()
            .ok()
            .map(|l| {
                l.iter()
                    .filter(|s| s.quote.to_uppercase() == quote_up)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if !list.is_empty() {
            list
        } else {
            // Safety fallback if pairs haven't registered yet
            if quote_up == "GBP" {
                vec![Symbol::btc_gbp(), Symbol::eth_gbp(), Symbol::sol_gbp()]
            } else {
                vec![Symbol::btc_usd(), Symbol::eth_usd(), Symbol::sol_usd()]
            }
        }
    }

    pub fn update_cash(&self, currency: &str, cash: Decimal) {
        if let Ok(mut map) = self.last_known_cash.write() {
            map.insert(currency.to_string(), cash);
        }
    }

    pub fn deduct_cash(&self, currency: &str, amount: Decimal) {
        if let Ok(mut map) = self.last_known_cash.write() {
            let entry = map.entry(currency.to_string()).or_insert(Decimal::ZERO);
            *entry = (*entry - amount).max(Decimal::ZERO);
        }
    }

    pub fn zero_cash(&self, currency: &str) {
        if let Ok(mut map) = self.last_known_cash.write() {
            map.insert(currency.to_string(), Decimal::ZERO);
        }
    }

    pub fn get_cash(&self, currency: &str) -> Decimal {
        self.last_known_cash
            .read()
            .ok()
            .and_then(|map| map.get(currency).copied())
            .unwrap_or(Decimal::ZERO)
    }

    pub fn partition_capital(
        &self,
        total_settled_cash: Decimal,
        active_pairs: &[Symbol],
    ) -> (Decimal, Decimal, HashMap<String, Decimal>) {
        if total_settled_cash <= Decimal::ZERO {
            return (Decimal::ZERO, Decimal::ZERO, HashMap::new());
        }

        let cfg = self.config.read().unwrap().clone();

        let sniper_reserve = if cfg.sniper_reserve_pct <= Decimal::ZERO || cfg.min_sniper_reserve_fiat <= Decimal::ZERO {
            Decimal::ZERO
        } else if total_settled_cash <= cfg.min_sniper_reserve_fiat {
            total_settled_cash
        } else {
            let proportional = total_settled_cash * cfg.sniper_reserve_pct;
            proportional.max(cfg.min_sniper_reserve_fiat).min(total_settled_cash)
        };

        let grid_pool = (total_settled_cash - sniper_reserve).max(Decimal::ZERO);

        let mut allocations = HashMap::new();
        if grid_pool >= cfg.min_order_floor && !active_pairs.is_empty() {
            let mut total_weight = Decimal::ZERO;
            for sym in active_pairs {
                let key = sym.as_slash();
                let w = cfg.pair_weights.get(&key).copied().unwrap_or(dec!(0.3333));
                total_weight += w;
            }

            if total_weight > Decimal::ZERO {
                for sym in active_pairs {
                    let key = sym.as_slash();
                    let w = cfg.pair_weights.get(&key).copied().unwrap_or(dec!(0.3333));
                    let raw_slice = (grid_pool * w / total_weight).round_dp(2);
                    let effective_slice = if raw_slice >= cfg.min_order_floor {
                        raw_slice
                    } else {
                        Decimal::ZERO
                    };
                    allocations.insert(key, effective_slice);
                }
            }
        }

        (sniper_reserve, grid_pool, allocations)
    }

    pub fn get_grid_allocation(
        &self,
        _runner_id: &str,
        total_cash: Decimal,
        symbol: &Symbol,
        active_pairs: &[Symbol],
    ) -> Decimal {
        let (_, _, allocations) = self.partition_capital(total_cash, active_pairs);
        allocations.get(&symbol.as_slash()).copied().unwrap_or(Decimal::ZERO)
    }

    pub fn get_sniper_allocation(&self, total_cash: Decimal, active_pairs: &[Symbol]) -> Decimal {
        let (sniper_reserve, _, _) = self.partition_capital(total_cash, active_pairs);
        sniper_reserve
    }

    pub fn get_telemetry(&self, total_cash: Decimal, active_pairs: &[Symbol]) -> BrainTelemetryDto {
        let (sniper_res, grid_pool, allocations) = self.partition_capital(total_cash, active_pairs);

        let (status, summary) = if total_cash <= Decimal::ZERO {
            ("UNFUNDED".to_string(), "Settled cash is £0.00. Standing by for capital deposit.".to_string())
        } else if grid_pool < dec!(1.00) {
            (
                "SNIPER_CONCENTRATED".to_string(),
                format!(
                    "Micro-capital mode: 100% of cash (£{:.2}) ring-fenced for guaranteed-alpha Sniper. Grid runners in STANDBY.",
                    total_cash
                ),
            )
        } else {
            (
                "OPTIMAL_PARTITIONED".to_string(),
                format!(
                    "Ring-fenced £{:.2} for Sniper; allocated £{:.2} across {} Grid pair(s) with zero venue collision.",
                    sniper_res,
                    grid_pool,
                    allocations.len()
                ),
            )
        };

        BrainTelemetryDto {
            total_settled_cash: total_cash,
            sniper_reserve_fiat: sniper_res,
            grid_pool_fiat: grid_pool,
            pair_allocations: allocations,
            status,
            decision_summary: summary,
        }
    }
}
