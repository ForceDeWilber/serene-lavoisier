import React from "react";
import { Activity, Shield, Power, Lock } from "lucide-react";
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
    <header className="bg-black border-b md:border border-gray-800 px-2.5 md:px-3 py-1.5 flex items-center justify-between text-xs font-mono select-none flex-shrink-0">
      <div className="flex items-center gap-2 md:gap-3">
        <div className="flex items-center gap-1.5 md:gap-2">
          <Activity className="w-3.5 h-3.5 text-blue-500 flex-shrink-0" />
          <span className="font-bold text-gray-200 tracking-tight">
            SERENE<span className="hidden sm:inline">_LAVOISIER</span>
          </span>
        </div>
        
        <div className="flex items-center gap-1 bg-gray-900 border border-gray-800 px-1.5 md:px-2 py-0.5 text-[9px] md:text-[10px]">
          <span className={`w-1.5 h-1.5 rounded-full ${isLive ? "bg-emerald-500" : "bg-blue-500"}`} />
          <span className={isLive ? "text-emerald-400 font-semibold" : "text-blue-400"}>
            {isLive ? "LIVE" : "PAPER"}
          </span>
          <span className="hidden md:inline text-gray-500 border-l border-gray-700 pl-1.5">
            RUST CORE
          </span>
        </div>
      </div>

      <div className="flex items-center gap-1.5 md:gap-3">
        {/* Connection Status */}
        <div className="flex items-center gap-1 text-[10px] md:text-xs text-gray-500">
          <span className={`w-1.5 h-1.5 rounded-full ${connected && !isFeedStale ? "bg-emerald-500" : "bg-yellow-500"}`} />
          <span className="hidden xs:inline">
            {connected && !isFeedStale ? `${telemetry?.latency_ms ?? '--'}ms` : "SYNC"}
          </span>
        </div>

        {/* Risk State */}
        <div className={`flex items-center gap-1 px-1.5 md:px-2 py-0.5 border text-[10px] md:text-xs ${
          cbTripped
            ? "bg-red-950 text-red-400 border-red-900"
            : "hidden sm:flex bg-gray-900 text-gray-400 border-gray-800"
        }`}>
          <Shield className="w-3 h-3 flex-shrink-0" />
          <span>{cbTripped ? "TRIPPED" : "RISK: OK"}</span>
          {cbTripped && (
            <button onClick={onResetCircuitBreaker} className="ml-1 underline hover:text-white font-bold">
              [RESET]
            </button>
          )}
        </div>

        {/* Global Kill Switch */}
        <button
          onClick={onKillSwitch}
          className="flex items-center gap-1 px-2 py-0.5 bg-red-950 text-red-400 border border-red-900 hover:bg-red-900 hover:text-white transition-colors text-[10px] md:text-xs font-bold"
          title="Emergency Halt Execution"
        >
          <Power className="w-3 h-3 flex-shrink-0" />
          <span className="hidden sm:inline">HALT_EXEC</span>
          <span className="sm:hidden">HALT</span>
        </button>

        {/* Lock */}
        <button onClick={onLogout} className="text-gray-500 hover:text-gray-300 p-0.5 ml-0.5" title="Lock session">
          <Lock className="w-3.5 h-3.5" />
        </button>
      </div>
    </header>
  );
}
