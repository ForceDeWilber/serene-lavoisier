import React from "react";
import { TelemetryPayload } from "../../types/telemetry";

interface Props {
  telemetry: TelemetryPayload | null;
  onSyncRevolut: () => void;
  syncingRevolut: boolean;
}

export function PortfolioPane({ telemetry, onSyncRevolut, syncingRevolut }: Props) {
  const gbpBalance = telemetry?.balances?.GBP ?? 0;
  const usdBalance = telemetry?.balances?.USD ?? 0;
  const btcBalance = telemetry?.balances?.BTC ?? 0;
  const ethBalance = telemetry?.balances?.ETH ?? 0;
  const solBalance = telemetry?.balances?.SOL ?? 0;

  const portfolio = telemetry?.portfolio;
  const totalEquity = portfolio?.total_equity_gbp ?? gbpBalance;
  const depositedCash = portfolio?.initial_budget_gbp ?? 1000.00;
  const netPnLGbp = portfolio?.total_pnl_gbp ?? (totalEquity - depositedCash);
  const netPnLPct = portfolio?.total_pnl_pct ?? (depositedCash > 0 ? (netPnLGbp / depositedCash) * 100 : 0);

  const brain = telemetry?.brain;

  return (
    <div className="flex flex-col bg-[#050505] border border-gray-800 font-mono text-xs overflow-hidden h-full">
      <div className="bg-gray-900 border-b border-gray-800 px-2 py-1 flex items-center justify-between">
        <span className="text-gray-300 font-semibold uppercase">PORTFOLIO_CAPITAL</span>
        <button onClick={onSyncRevolut} disabled={syncingRevolut} className="text-[10px] text-blue-400 hover:text-blue-300 underline">
          {syncingRevolut ? "SYNCING..." : "[SYNC_API]"}
        </button>
      </div>

      <div className="p-2 space-y-3 overflow-y-auto">
        {/* Equity Overview */}
        <div className="grid grid-cols-2 gap-2">
          <div className="border border-gray-800 p-2">
            <div className="text-gray-500 text-[10px]">TOTAL_EQUITY</div>
            <div className="text-lg text-gray-200">£{totalEquity.toFixed(2)}</div>
          </div>
          <div className="border border-gray-800 p-2">
            <div className="text-gray-500 text-[10px]">NET_PNL</div>
            <div className={`text-lg ${netPnLGbp >= 0 ? 'text-emerald-400' : 'text-red-400'}`}>
              {netPnLGbp >= 0 ? '+' : ''}£{netPnLGbp.toFixed(2)}
            </div>
            <div className={`text-[10px] ${netPnLGbp >= 0 ? 'text-emerald-500/70' : 'text-red-500/70'}`}>
              ({netPnLPct >= 0 ? '+' : ''}{netPnLPct.toFixed(2)}%)
            </div>
          </div>
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
                <span className="text-gray-200">£{brain.total_settled_cash.toFixed(2)}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-blue-400">SNIPER_RESERVE:</span>
                <span className="text-blue-400">£{brain.sniper_reserve_fiat.toFixed(2)}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-purple-400">GRID_POOL:</span>
                <span className="text-purple-400">£{brain.grid_pool_fiat.toFixed(2)}</span>
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
                <td className="p-1 pr-2">{gbpBalance.toFixed(2)}</td>
              </tr>
              <tr className="border-b border-gray-800/50">
                <td className="text-left p-1 pl-2">USD</td>
                <td className="p-1 pr-2">{usdBalance.toFixed(2)}</td>
              </tr>
              <tr className="border-b border-gray-800/50">
                <td className="text-left p-1 pl-2">BTC</td>
                <td className="p-1 pr-2">{btcBalance.toFixed(6)}</td>
              </tr>
              <tr className="border-b border-gray-800/50">
                <td className="text-left p-1 pl-2">ETH</td>
                <td className="p-1 pr-2">{ethBalance.toFixed(6)}</td>
              </tr>
              <tr>
                <td className="text-left p-1 pl-2">SOL</td>
                <td className="p-1 pr-2">{solBalance.toFixed(4)}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
