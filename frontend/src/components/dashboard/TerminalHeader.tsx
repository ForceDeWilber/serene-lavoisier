import React from "react";
import { Activity, Shield, Power, Radio, RefreshCw, Lock } from "lucide-react";
import { TelemetryPayload } from "../../types/telemetry";

interface Props {
  telemetry: TelemetryPayload | null;
  connected: boolean;
  isFeedStale: boolean;
  onKillSwitch: () => void;
  onResetCircuitBreaker: () => void;
  onLogout: () => void;
}

export function TerminalHeader({
  telemetry,
  connected,
  isFeedStale,
  onKillSwitch,
  onResetCircuitBreaker,
  onLogout,
}: Props) {
  const cbTripped = telemetry?.circuit_breaker_tripped ?? false;
  const isLive = telemetry?.mode === "live" || telemetry?.is_live;

  return (
    <header className="bg-black border border-gray-800 px-3 py-1.5 flex items-center justify-between text-xs font-mono select-none">
      <div className="flex items-center gap-3">
        <div className="flex items-center gap-2">
          <Activity className="w-3.5 h-3.5 text-blue-500" />
          <span className="font-bold text-gray-200">SERENE_LAVOISIER</span>
        </div>
        
        <div className="flex items-center gap-1.5 bg-gray-900 border border-gray-800 px-2 py-0.5 text-[10px]">
          <span className={`w-1.5 h-1.5 rounded-full ${isLive ? "bg-emerald-500" : "bg-blue-500"}`} />
          <span className={isLive ? "text-emerald-400" : "text-blue-400"}>
            {isLive ? "LIVE PROD" : "PAPER SIM"}
          </span>
          <span className="text-gray-500 border-l border-gray-700 pl-1.5">
            RUST CORE
          </span>
        </div>
      </div>

      <div className="flex items-center gap-3">
        {/* Connection Status */}
        <div className="flex items-center gap-1.5 text-gray-500">
          <span className={`w-1.5 h-1.5 rounded-full ${connected && !isFeedStale ? "bg-emerald-500" : "bg-yellow-500"}`} />
          <span>{connected && !isFeedStale ? `WS CONN · ${telemetry?.latency_ms || '--'}ms` : "SYNCING..."}</span>
        </div>

        {/* Risk State */}
        <div className={`flex items-center gap-1.5 px-2 py-0.5 border ${
          cbTripped
            ? "bg-red-950 text-red-400 border-red-900"
            : "bg-gray-900 text-gray-400 border-gray-800"
        }`}>
          <Shield className="w-3 h-3" />
          <span>{cbTripped ? "CIRCUIT_TRIPPED" : "RISK: NORMAL"}</span>
          {cbTripped && (
            <button onClick={onResetCircuitBreaker} className="ml-2 underline hover:text-white">
              [RESET]
            </button>
          )}
        </div>

        {/* Global Kill Switch */}
        <button
          onClick={onKillSwitch}
          className="flex items-center gap-1.5 px-2 py-0.5 bg-red-950 text-red-500 border border-red-900 hover:bg-red-900 hover:text-white transition-colors"
        >
          <Power className="w-3 h-3" />
          <span>HALT_EXEC</span>
        </button>

        {/* Lock */}
        <button onClick={onLogout} className="text-gray-500 hover:text-gray-300 ml-1">
          <Lock className="w-3.5 h-3.5" />
        </button>
      </div>
    </header>
  );
}
