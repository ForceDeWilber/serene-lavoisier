import React from "react";
import { TelemetryPayload } from "../../types/telemetry";

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
            <div className="text-gray-200">{(sniper.impulse_threshold_pct * 100).toFixed(3)}%</div>
          </div>
          <div className="border border-gray-800 p-1.5">
            <div className="text-gray-500 text-[10px]">SUCCESS/TOTAL</div>
            <div className="text-gray-200">{sniper.successful_snipes}/{sniper.total_snipes} ({sniper.win_rate_pct.toFixed(1)}%)</div>
          </div>
          <div className="border border-gray-800 p-1.5">
            <div className="text-gray-500 text-[10px]">TOTAL_PNL</div>
            <div className={sniper.total_sniper_profit_gbp >= 0 ? 'text-emerald-400' : 'text-red-400'}>
              £{sniper.total_sniper_profit_gbp.toFixed(2)}
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
                    const disloc = r.current_dislocation_pct || 0;
                    const isHot = r.in_snipe_zone;
                    const dislocColor = isHot ? 'text-yellow-400 font-bold' : (Math.abs(disloc) > 0.05 ? 'text-gray-200' : 'text-gray-500');
                    return (
                      <tr key={i} className={`border-b border-gray-800/50 ${isHot ? 'bg-yellow-900/20' : ''}`}>
                        <td className="text-left p-1 pl-2 font-bold">{r.symbol.replace('/GBP', '')}</td>
                        <td className="p-1">{r.kraken_price?.toFixed(2) || '-'}</td>
                        <td className="p-1">{r.revolut_best_ask?.toFixed(2) || '-'}</td>
                        <td className={`p-1 pr-2 ${dislocColor}`}>{disloc > 0 ? '+' : ''}{disloc.toFixed(3)}%</td>
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
