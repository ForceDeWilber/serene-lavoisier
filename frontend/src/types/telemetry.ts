export interface RunnerTelemetry {
  runner_id: string;
  symbol: string;
  current_price?: number;
  center_price: number | null;
  inventory_base: number;
  inventory_value_gbp?: number;
  realized_pnl: number;
  total_trades: number;
  active_orders_count: number;
  is_paused: boolean;
  step_pct?: number;
  rebalance_threshold_pct?: number;
}

export interface RestingOrder {
  id: string;
  runner_id: string;
  symbol: string;
  side: "BUY" | "SELL";
  price: number;
  qty: number;
  value_gbp: number;
  created_at: string;
  rung_level: number;
  distance_pct: number;
}

export interface LiveTradeEvent {
  id: string | number;
  timestamp?: number | string;
  time_str?: string;
  runner_id?: string;
  symbol: string;
  side?: "BUY" | "SELL" | string;
  action?: string;
  price: number;
  qty: number;
  value_gbp?: number;
  fee_gbp?: number;
  fx_rate?: number;
  profit?: number;
  pnl_gbp?: number;
  pnl_pct?: number;
  strategy?: string;
  note?: string;
}

export interface MarketPriceInfo {
  price: number | null;
  status?: string;
  disclaimer?: string | null;
  high24h?: number | null;
  low24h?: number | null;
  change24h?: number | null;
  yesterday_close?: number | null;
  timestamp?: string | null;
}

export interface VenueRadarItem {
  symbol: string;
  status?: string;
  disclaimer?: string | null;
  kraken_price: number | null;
  revolut_best_bid: number | null;
  revolut_best_ask: number | null;
  revolut_spread_gbp: number;
  revolut_spread_pct: number;
  buy_opportunity_pct: number;
  sell_opportunity_pct: number;
  current_dislocation_pct: number;
  in_snipe_zone: boolean;
  direction: string;
  lead_advantage_ms: number;
}

export interface SniperTelemetry {
  enabled: boolean;
  status: string;
  impulse_threshold_pct: number;
  snipe_order_size_gbp: number;
  min_net_edge_pct: number;
  revolut_taker_fee_pct: number;
  revolut_maker_fee_pct?: number;
  scratch_timeout_ms?: number;
  total_snipes: number;
  successful_snipes: number;
  win_rate_pct: number;
  total_sniper_profit_gbp: number;
  total_taker_fees_paid_gbp: number;
  average_lead_advantage_ms: number;
  radar: Record<string, VenueRadarItem>;
  recent_snipes: any[];
}

export interface CapitalManagement {
  balance_source: string;
  starting_balance_gbp: number;
  total_deposited_cash_gbp?: number;
  settled_cash_gbp: number;
  cumulative_profit_gbp: number;
  profit_lock_pct: number;
  locked_profit_gbp: number;
  unlocked_profit_gbp: number;
  active_trading_power_gbp: number;
  expansion_ratio: number;
  rungs_per_side: number;
  split_btc_pct: number;
  split_eth_pct: number;
  split_sol_pct?: number;
  unrealized_pnl_gbp?: number;
  crypto_holdings_value_gbp?: number;
  allocations: {
    trading_power_gbp: number;
    expansion_ratio: number;
    runner_btc: {
      envelope_gbp: number;
      split_pct: number;
      rungs_per_side: number;
      order_size_gbp: number;
    };
    runner_eth: {
      envelope_gbp: number;
      split_pct: number;
      rungs_per_side: number;
      order_size_gbp: number;
    };
    runner_sol?: {
      envelope_gbp: number;
      split_pct: number;
      rungs_per_side: number;
      order_size_gbp: number;
    };
    sniper: {
      order_size_gbp: number;
    };
  };
}

export interface EngineActivityItem {
  id: string;
  time: string;
  pair: string;
  event: string;
  spread_eval: string;
  status: string;
  disclaimer?: string | null;
}

export interface EngineActivity {
  title: string;
  status: string;
  status_code: string;
  summary: string;
  timestamp: string;
  oracle_latency_ms: number;
  drawdown_pct: number;
  circuit_breaker: string;
  pairs_monitored: number;
  target_hurdle_pct: number;
  activities?: EngineActivityItem[];
}

export interface EngineDecision {
  id: string;
  timestamp: string;
  category: string;
  badge: string;
  title: string;
  detail: string;
  status: string;
}

export interface TelemetryPayload {
  mode?: "paper" | "live";
  is_live?: boolean;
  status?: string;
  status_message?: string;
  authenticated?: boolean;
  can_trade?: boolean;
  latency_ms?: number;
  host_mode?: string;
  circuit_breaker_tripped: boolean;
  circuit_breaker_reason?: string;
  balances: Record<string, number>;
  available_balances?: Record<string, number>;
  reserved_balances?: Record<string, number>;
  portfolio?: {
    total_equity_gbp: number;
    total_deposited_cash_gbp?: number;
    initial_budget_gbp: number;
    total_pnl_gbp: number;
    total_pnl_pct: number;
    total_realized_pnl_gbp: number;
    unrealized_pnl_gbp?: number;
    crypto_holdings_value_gbp?: number;
    total_fee_savings_gbp: number;
  };
  capital_management?: CapitalManagement;
  market_prices?: Record<string, MarketPriceInfo>;
  runners: RunnerTelemetry[];
  sniper?: SniperTelemetry;
  engine_activity?: EngineActivity;
  engine_decisions?: EngineDecision[];
  resting_orders?: RestingOrder[];
  resting_orders_count: number;
  live_trades?: LiveTradeEvent[];
  timestamp?: number;
  // Extracted from brain telemetry mapping
  brain?: {
    total_settled_cash: number;
    sniper_reserve_fiat: number;
    grid_pool_fiat: number;
    pair_allocations: Record<string, number>;
    status: string;
    decision_summary: string;
  };
}
