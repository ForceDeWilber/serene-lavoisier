import React, { useState } from "react";
import { TelemetryPayload } from "../../types/telemetry";
import { formatNum, formatGbp, formatPct, toNum } from "../../lib/format";

interface Props {
  telemetry: TelemetryPayload | null;
  onSyncRevolut: () => void;
  syncingRevolut: boolean;
}

export function PortfolioPane({ telemetry, onSyncRevolut, syncingRevolut }: Props) {
  const [showTransfers, setShowTransfers] = useState(false);

  const gbpBalance = toNum(telemetry?.balances?.GBP, 0);
  const usdBalance = toNum(telemetry?.balances?.USD, 0);
  const btcBalance = toNum(telemetry?.balances?.BTC, 0);
  const ethBalance = toNum(telemetry?.balances?.ETH, 0);
  const solBalance = toNum(telemetry?.balances?.SOL, 0);
  const xrpBalance = toNum(telemetry?.balances?.XRP, 0);

  const portfolio = telemetry?.portfolio;
  const capital = telemetry?.capital_management;
  const audit = portfolio?.transfer_audit || capital?.transfer_audit;

  const totalEquity = toNum(portfolio?.total_equity_gbp, gbpBalance);
  const depositedCash = toNum(
    audit?.net_deposited_cash_gbp ??
      portfolio?.net_deposited_cash_gbp ??
      portfolio?.total_deposited_cash_gbp ??
      portfolio?.initial_budget_gbp,
    0
  );

  const realizedPnLGbp = toNum(portfolio?.total_realized_pnl_gbp ?? capital?.cumulative_profit_gbp, 0);
  const realizedPnLPct = toNum(
    portfolio?.total_realized_pnl_pct,
    depositedCash > 0 ? (realizedPnLGbp / depositedCash) * 100 : 0
  );

  const netPnLGbp = toNum(portfolio?.total_pnl_gbp, totalEquity - depositedCash);
  const netPnLPct = toNum(portfolio?.total_pnl_pct, depositedCash > 0 ? (netPnLGbp / depositedCash) * 100 : 0);
  const unrealizedPnLGbp = toNum(portfolio?.unrealized_pnl_gbp, netPnLGbp - realizedPnLGbp);

  const brain = telemetry?.brain;
  const transfers = audit?.recent_transfers || [];

  return (
    <div className="flex flex-col bg-[#050505] border border-gray-800 font-mono text-xs overflow-hidden h-full">
      <div className="bg-gray-900 border-b border-gray-800 px-2 py-1 flex items-center justify-between">
        <div className="flex items-center gap-1.5">
          <span className="text-gray-300 font-semibold uppercase">PORTFOLIO_CAPITAL</span>
          {audit?.status && (
            <span
              className={`text-[9px] px-1 py-0.5 border ${
                audit.status === "ACTIVE" || audit.status === "SUCCESS"
                  ? "border-emerald-800 text-emerald-400 bg-emerald-950/40"
                  : "border-yellow-800 text-yellow-400 bg-yellow-950/40"
              }`}
            >
              AUDIT: {audit.status}
            </span>
          )}
        </div>
        <button
          onClick={onSyncRevolut}
          disabled={syncingRevolut}
          className="text-[10px] text-blue-400 hover:text-blue-300 underline disabled:opacity-50"
          title="Force immediate audit of Revolut X transfers & balances"
        >
          {syncingRevolut ? "AUDITING..." : "[SYNC_API]"}
        </button>
      </div>

      <div className="p-2 space-y-2.5 overflow-y-auto">
        {/* 4-Metric Grid: Realized Profit, Total Equity, Net PnL, Net Deposited */}
        <div className="grid grid-cols-2 gap-2">
          {/* Card 1: Realized Trading Profit (Hero Metric) */}
          <div className="border border-emerald-900/60 bg-emerald-950/20 p-2 relative overflow-hidden">
            <div className="flex justify-between items-center text-[10px] text-emerald-400/90 font-medium">
              <span>REALIZED_PROFIT</span>
              <span className="bg-emerald-900/80 text-emerald-300 px-1 text-[8px] tracking-wide">
                100% WIN
              </span>
            </div>
            <div className="text-lg font-bold text-emerald-400 mt-0.5">
              {formatGbp(realizedPnLGbp, 2, true)}
            </div>
            <div className="text-[10px] text-emerald-500/80 flex items-center justify-between">
              <span>({formatPct(realizedPnLPct, 2, true)} on cap)</span>
              <span className="text-gray-500 text-[8px]">PURE MAKER</span>
            </div>
          </div>

          {/* Card 2: Total Equity */}
          <div className="border border-gray-800 bg-black/40 p-2">
            <div className="text-gray-500 text-[10px]">TOTAL_EQUITY</div>
            <div className="text-lg text-gray-100 mt-0.5">{formatGbp(totalEquity, 2)}</div>
            <div className="text-[10px] text-gray-500 truncate">Cash + Live Crypto</div>
          </div>

          {/* Card 3: Net Overall PnL */}
          <div className="border border-gray-800 bg-black/40 p-2">
            <div className="text-gray-500 text-[10px]">NET_PNL (PORTFOLIO)</div>
            <div className={`text-lg mt-0.5 ${netPnLGbp >= 0 ? "text-emerald-400" : "text-red-400"}`}>
              {formatGbp(netPnLGbp, 2, true)}
            </div>
            <div className={`text-[10px] ${netPnLGbp >= 0 ? "text-emerald-500/70" : "text-red-500/70"}`}>
              {formatPct(netPnLPct, 2, true)}{" "}
              <span className="text-gray-600">
                (Unreal: {formatGbp(unrealizedPnLGbp, 2, true)})
              </span>
            </div>
          </div>

          {/* Card 4: Net Deposited / Cost Basis */}
          <div className="border border-gray-800 bg-black/40 p-2">
            <div className="flex justify-between items-center text-[10px] text-gray-500">
              <span>NET_DEPOSITED</span>
              <span className="text-[8px] text-blue-400 border border-blue-900/60 px-0.5 bg-blue-950/30">
                AUDITED
              </span>
            </div>
            <div className="text-lg text-blue-300 mt-0.5">{formatGbp(depositedCash, 2)}</div>
            <div className="text-[10px] text-gray-500">
              {audit?.deposits_count ?? 0} in • {audit?.withdrawals_count ?? 0} out
            </div>
          </div>
        </div>

        {/* Transfer Audit Ledger (Collapsible) */}
        <div className="border border-gray-800 bg-black">
          <button
            onClick={() => setShowTransfers(!showTransfers)}
            className="w-full px-2 py-1.5 flex items-center justify-between text-[10px] bg-gray-900/60 hover:bg-gray-800/80 transition-colors text-left"
          >
            <span className="text-gray-300 font-semibold flex items-center gap-1.5">
              <span>TRANSFER_AUDIT_LEDGER</span>
              <span className="text-gray-500 text-[9px]">({transfers.length} events)</span>
            </span>
            <span className="text-blue-400 font-bold text-[9px]">
              {showTransfers ? "[HIDE ▲]" : "[VIEW ▼]"}
            </span>
          </button>

          {showTransfers && (
            <div className="p-2 border-t border-gray-800/80 space-y-1.5">
              <div className="text-[9px] text-gray-500 flex justify-between">
                <span>REVOLUT X DIRECT LEDGER AUDIT</span>
                <span>EVERY 60S AUTO-CHECK</span>
              </div>
              {transfers.length === 0 ? (
                <div className="text-gray-600 italic text-[10px] py-1">
                  Click [SYNC_API] to run immediate audit against Revolut X API.
                </div>
              ) : (
                <table className="w-full text-right text-[10px]">
                  <thead>
                    <tr className="border-b border-gray-800 text-gray-500 text-[9px]">
                      <th className="text-left font-normal py-0.5">DATE</th>
                      <th className="text-left font-normal py-0.5">TYPE</th>
                      <th className="font-normal py-0.5">AMOUNT</th>
                      <th className="font-normal py-0.5 pr-1">GBP VALUE</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-gray-800/40">
                    {transfers.map((tx) => {
                      const isReceive = tx.type === "receive";
                      const dateStr = tx.created_date
                        ? new Date(tx.created_date).toLocaleDateString("en-GB", {
                            day: "2-digit",
                            month: "short",
                            hour: "2-digit",
                            minute: "2-digit",
                          })
                        : "---";
                      return (
                        <tr key={tx.id} className="text-gray-300">
                          <td className="text-left py-1 text-gray-400 font-mono text-[9px]">
                            {dateStr}
                          </td>
                          <td className="text-left py-1">
                            <span
                              className={`px-1 py-0.2 text-[8px] ${
                                isReceive
                                  ? "text-emerald-400 bg-emerald-950/60 border border-emerald-900"
                                  : "text-red-400 bg-red-950/60 border border-red-900"
                              }`}
                            >
                              {isReceive ? "DEPOSIT" : "WITHDRAW"}
                            </span>
                          </td>
                          <td className="py-1">
                            {tx.currency === "USD" ? "$" : ""}
                            {formatNum(tx.amount, 2)} {tx.currency}
                          </td>
                          <td
                            className={`py-1 pr-1 font-semibold ${
                              isReceive ? "text-emerald-400" : "text-red-400"
                            }`}
                          >
                            {isReceive ? "+" : "-"}
                            {formatGbp(tx.amount_gbp, 2)}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              )}
            </div>
          )}
        </div>

        {/* Engine Brain Partition */}
        {brain && (
          <div className="border border-gray-800 p-2 space-y-2">
            <div className="text-gray-500 text-[10px] flex justify-between">
              <span>ENGINE_BRAIN_PARTITION</span>
              <span className="text-gray-400">{brain.status}</span>
            </div>
            <div className="flex flex-col gap-1">
              <div className="flex justify-between">
                <span className="text-gray-400">SETTLED_CASH:</span>
                <span className="text-gray-200">{formatGbp(brain.total_settled_cash, 2)}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-blue-400">SNIPER_RESERVE:</span>
                <span className="text-blue-400">{formatGbp(brain.sniper_reserve_fiat, 2)}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-purple-400">GRID_POOL:</span>
                <span className="text-purple-400">{formatGbp(brain.grid_pool_fiat, 2)}</span>
              </div>
            </div>
          </div>
        )}

        {/* Raw Balances Table */}
        <div className="border border-gray-800">
          <table className="w-full text-right">
            <thead>
              <tr className="border-b border-gray-800 text-gray-500 text-[10px]">
                <th className="text-left font-normal p-1 pl-2">ASSET</th>
                <th className="font-normal p-1 pr-2">BALANCE</th>
              </tr>
            </thead>
            <tbody className="text-gray-300">
              <tr className="border-b border-gray-800/50">
                <td className="text-left p-1 pl-2">GBP</td>
                <td className="p-1 pr-2">{formatNum(gbpBalance, 2)}</td>
              </tr>
              <tr className="border-b border-gray-800/50">
                <td className="text-left p-1 pl-2">USD</td>
                <td className="p-1 pr-2">{formatNum(usdBalance, 2)}</td>
              </tr>
              <tr className="border-b border-gray-800/50">
                <td className="text-left p-1 pl-2">BTC</td>
                <td className="p-1 pr-2">{formatNum(btcBalance, 6)}</td>
              </tr>
              <tr className="border-b border-gray-800/50">
                <td className="text-left p-1 pl-2">ETH</td>
                <td className="p-1 pr-2">{formatNum(ethBalance, 6)}</td>
              </tr>
              <tr className="border-b border-gray-800/50">
                <td className="text-left p-1 pl-2">SOL</td>
                <td className="p-1 pr-2">{formatNum(solBalance, 4)}</td>
              </tr>
              <tr>
                <td className="text-left p-1 pl-2">XRP</td>
                <td className="p-1 pr-2">{formatNum(xrpBalance, 2)}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
