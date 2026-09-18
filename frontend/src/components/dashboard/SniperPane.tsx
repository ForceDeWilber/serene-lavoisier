import React from "react";
import { TelemetryPayload } from "../../types/telemetry";
import { formatNum, formatGbp, formatPct, toNum } from "../../lib/format";

interface Props {
  telemetry: TelemetryPayload | null;
  onToggleSniper: () => void;
}

export function SniperPane({ telemetry, onToggleSniper }: Props) {
  const sniper = telemetry?.sniper;
  if (!sniper) {
    return (
      <div className="flex flex-col bg-[#050505] border border-gray-800 font-mono text-xs overflow-hidden h-full">
        <div className="bg-gray-900 border-b border-gray-800 px-2 py-1 flex items-center justify-between">
          <span className="text-gray-300 font-semibold uppercase">LEAD_LAG_SNIPER</span>
        </div>
        <div className="p-2 text-gray-600 italic">WAITING_FOR_SNIPER_TELEMETRY...</div>
      </div>
    );
  }

  const radarItems = Object.values(sniper.radar || {});
  const profitVal = toNum(sniper.total_sniper_profit_gbp, 0);

  return (
    <div className="flex flex-col bg-[#050505] border border-gray-800 font-mono text-xs overflow-hidden h-full">
      <div className="bg-gray-900 border-b border-gray-800 px-2 py-1 flex items-center justify-between">
        <span className="text-gray-300 font-semibold uppercase">LEAD_LAG_SNIPER</span>
        <button 
          onClick={onToggleSniper}
          className={`px-2 py-0.5 text-[10px] ${sniper.enabled ? 'bg-emerald-950 text-emerald-400 border border-emerald-900' : 'bg-red-950 text-red-400 border border-red-900'}`}
        >
          {sniper.enabled ? "[ARMED]" : "[DISARMED]"}
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-2 flex flex-col gap-2">
        {/* Global Sniper Stats */}
        <div className="grid grid-cols-3 gap-1">
          <div className="border border-gray-800 p-1.5">
            <div className="text-gray-500 text-[10px]">HURDLE_PCT</div>
            <div className="text-gray-200">{formatPct(sniper.impulse_threshold_pct, 3)}</div>
          </div>
          <div className="border border-gray-800 p-1.5">
            <div className="text-gray-500 text-[10px]">SUCCESS/TOTAL</div>
            <div className="text-gray-200">{sniper.successful_snipes ?? 0}/{sniper.total_snipes ?? 0} ({formatNum(sniper.win_rate_pct, 1)}%)</div>
          </div>
          <div className="border border-gray-800 p-1.5">
            <div className="text-gray-500 text-[10px]">TOTAL_PNL</div>
            <div className={profitVal >= 0 ? 'text-emerald-400' : 'text-red-400'}>
              {formatGbp(sniper.total_sniper_profit_gbp, 2, true)}
            </div>
          </div>
        </div>

        {/* Radar Table */}
        <div className="border border-gray-800 flex-1 flex flex-col min-h-0">
          <table className="w-full text-right sticky top-0">
            <thead className="bg-gray-900">
              <tr className="border-b border-gray-800 text-gray-500 text-[10px]">
                <th className="text-left font-normal p-1 pl-2">SYM</th>
                <th className="font-normal p-1">ORACLE(K)</th>
                <th className="font-normal p-1">EXEC(R)</th>
                <th className="font-normal p-1 pr-2">DISLOC(%)</th>
              </tr>
            </thead>
          </table>
          <div className="overflow-y-auto">
            <table className="w-full text-right">
              <tbody className="text-gray-300">
                {radarItems.length === 0 ? (
                  <tr><td colSpan={4} className="text-center p-2 text-gray-600">NO_RADAR_DATA</td></tr>
                ) : (
                  radarItems.map((r, i) => {
                    const disloc = toNum(r.current_dislocation_pct, 0);
                    const isHot = Boolean(r.in_snipe_zone);
                    const dislocColor = isHot ? 'text-yellow-400 font-bold' : (Math.abs(disloc) > 0.05 ? 'text-gray-200' : 'text-gray-500');
                    return (
                      <tr key={i} className={`border-b border-gray-800/50 ${isHot ? 'bg-yellow-900/20' : ''}`}>
                        <td className="text-left p-1 pl-2 font-bold">{String(r.symbol).replace('/GBP', '')}</td>
                        <td className="p-1">{formatNum(r.kraken_price, 2)}</td>
                        <td className="p-1">{formatNum(r.revolut_best_ask, 2)}</td>
                        <td className={`p-1 pr-2 ${dislocColor}`}>{formatPct(r.current_dislocation_pct, 3, true)}</td>
                      </tr>
                    );
                  })
                )}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  );
}
