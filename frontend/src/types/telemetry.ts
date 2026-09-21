export interface RunnerTelemetry {
  runner_id: string;
  symbol: string;
  current_price?: number | string | null;
  center_price: number | string | null;
  inventory_base: number | string;
  inventory_value_gbp?: number | string;
  realized_pnl: number | string;
  total_trades: number;
  active_orders_count: number;
  is_paused: boolean;
  step_pct?: number | string;
  rebalance_threshold_pct?: number | string;
  effective_center?: number | string | null;
  dynamic_step_pct?: number | string | null;
  rolling_volatility_pct?: number | string | null;
}

export interface RestingOrder {
  id: string;
  client_order_id?: string;
  runner_id: string;
  symbol: string;
  side: "BUY" | "SELL" | string;
  price: number | string;
  qty: number | string;
  value_gbp: number | string;
  created_at: string;
  rung_level: number;
  distance_pct: number | string;
  is_live?: boolean;
}

export interface LiveTradeEvent {
  id: string | number;
  timestamp?: number | string;
  time_str?: string;
  runner_id?: string;
  symbol: string;
  side?: "BUY" | "SELL" | string;
  action?: string;
  price: number | string;
  qty: number | string;
  value_gbp?: number | string;
  fee_gbp?: number | string;
  fx_rate?: number | string;
  profit?: number | string;
  pnl_gbp?: number | string;
  pnl_pct?: number | string;
  strategy?: string;
  note?: string;
}

export interface MarketPriceInfo {
  price: number | string | null;
  status?: string;
  disclaimer?: string | null;
  high24h?: number | string | null;
  low24h?: number | string | null;
  change24h?: number | string | null;
  yesterday_close?: number | string | null;
  timestamp?: string | null;
}

export interface VenueRadarItem {
  symbol: string;
  status?: string;
  disclaimer?: string | null;
  kraken_price: number | string | null;
  revolut_best_bid: number | string | null;
  revolut_best_ask: number | string | null;
  revolut_spread_gbp: number | string;
  revolut_spread_pct: number | string;
  buy_opportunity_pct: number | string;
  sell_opportunity_pct: number | string;
  current_dislocation_pct: number | string;
  in_snipe_zone: boolean;
  direction: string;
  lead_advantage_ms: number;
}

export interface SniperTelemetry {
  enabled: boolean;
  status: string;
  impulse_threshold_pct: number | string;
  snipe_order_size_gbp: number | string;
  min_net_edge_pct: number | string;
  revolut_taker_fee_pct: number | string;
  revolut_maker_fee_pct?: number | string;
  scratch_timeout_ms?: number;
  total_snipes: number;
  successful_snipes: number;
  win_rate_pct: number | string;
  total_sniper_profit_gbp: number | string;
  total_taker_fees_paid_gbp: number | string;
  average_lead_advantage_ms: number;
  radar: Record<string, VenueRadarItem>;
  recent_snipes: any[];
}

export interface TransferRecord {
  id: string;
  type: "receive" | "send" | string;
  status: string;
  currency: string;
  amount: number | string;
  amount_gbp: number | string;
  created_date?: number;
  processed_date?: number;
}

export interface TransferAuditInfo {
  status: string;
  deposits_count: number;
  withdrawals_count: number;
  total_deposits_gbp: number | string;
  total_withdrawals_gbp: number | string;
  net_deposited_cash_gbp: number | string;
  last_audit_timestamp?: number;
  recent_transfers?: TransferRecord[];
}

export interface CapitalManagement {
  balance_source: string;
  starting_balance_gbp: number | string;
  total_deposited_cash_gbp?: number | string;
  net_deposited_cash_gbp?: number | string;
  total_deposits_gbp?: number | string;
  total_withdrawals_gbp?: number | string;
  settled_cash_gbp: number | string;
  cumulative_profit_gbp: number | string;
  total_realized_pnl_pct?: number | string;
  profit_lock_pct: number | string;
  locked_profit_gbp: number | string;
  unlocked_profit_gbp: number | string;
  active_trading_power_gbp: number | string;
  expansion_ratio: number | string;
  rungs_per_side: number;
  split_btc_pct: number | string;
  split_eth_pct: number | string;
  split_sol_pct?: number | string;
  unrealized_pnl_gbp?: number | string;
  crypto_holdings_value_gbp?: number | string;
  transfer_audit?: TransferAuditInfo;
  allocations: {
    trading_power_gbp: number | string;
    expansion_ratio: number | string;
    runner_btc: {
      envelope_gbp: number | string;
      split_pct: number | string;
      rungs_per_side: number;
      order_size_gbp: number | string;
    };
    runner_eth: {
      envelope_gbp: number | string;
      split_pct: number | string;
      rungs_per_side: number;
      order_size_gbp: number | string;
    };
    runner_sol?: {
      envelope_gbp: number | string;
      split_pct: number | string;
      rungs_per_side: number;
      order_size_gbp: number | string;
    };
    sniper: {
      order_size_gbp: number | string;
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
  balances: Record<string, number | string>;
  available_balances?: Record<string, number | string>;
  reserved_balances?: Record<string, number | string>;
  portfolio?: {
    total_equity_gbp: number | string;
    total_deposited_cash_gbp?: number | string;
    net_deposited_cash_gbp?: number | string;
    initial_budget_gbp: number | string;
    total_pnl_gbp: number | string;
    total_pnl_pct: number | string;
    total_realized_pnl_gbp: number | string;
    total_realized_pnl_pct?: number | string;
    unrealized_pnl_gbp?: number | string;
    crypto_holdings_value_gbp?: number | string;
    total_fee_savings_gbp: number | string;
    transfer_audit?: TransferAuditInfo;
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
  brain?: {
    total_settled_cash: number | string;
    sniper_reserve_fiat: number | string;
    grid_pool_fiat: number | string;
    pair_allocations: Record<string, number | string>;
    status: string;
    decision_summary: string;
  };
}
