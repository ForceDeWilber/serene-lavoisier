"use client";

import React, { useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { Lock, ShieldCheck, ArrowRight, Delete, KeyRound } from "lucide-react";

export default function LoginPage() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const returnUrl = searchParams.get("returnUrl") || "/dashboard";

  const [pin, setPin] = useState("");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const handleDigit = (digit: string) => {
    if (pin.length < 8) {
      setPin((prev) => prev + digit);
      setError("");
    }
  };

  const handleDelete = () => {
    setPin((prev) => prev.slice(0, -1));
  };

  const handleClear = () => {
    setPin("");
    setError("");
  };

  const handleSubmit = async (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!pin) return;

    setSubmitting(true);
    setError("");

    try {
      const res = await fetch("/api/auth/login", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ pin }),
      });

      const data = await res.json();
      if (!res.ok) {
        setError(data.error || "Authentication failed. Incorrect PIN.");
        setPin("");
      } else {
        router.push(returnUrl);
        router.refresh();
      }
    } catch {
      setError("Network error contacting auth gateway");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <main className="min-h-screen bg-[#070a10] flex items-center justify-center p-4 font-sans text-slate-100">
      <div className="max-w-sm w-full bg-[#0e131f] border border-slate-800 rounded-2xl p-6 sm:p-8 space-y-6 shadow-2xl relative overflow-hidden">
        {/* Ambient Top Glow */}
        <div className="absolute -top-24 left-1/2 -translate-x-1/2 w-48 h-48 bg-cyan-500/10 rounded-full blur-3xl pointer-events-none" />

        {/* Header */}
        <div className="text-center space-y-2 relative">
          <div className="inline-flex items-center justify-center w-12 h-12 rounded-xl bg-cyan-500/10 border border-cyan-500/30 text-cyan-400 mb-1">
            <Lock className="w-6 h-6" />
          </div>
          <h1 className="text-lg font-bold text-white font-mono uppercase tracking-wider">
            Trading Desk Gate
          </h1>
          <p className="text-xs text-slate-400 font-sans">
            Enter your Master Access PIN to unlock the production trading terminal.
          </p>
        </div>

        {/* PIN Dots Indicator */}
        <form onSubmit={handleSubmit} className="space-y-5">
          <div className="flex justify-center items-center gap-3 py-2">
            {[0, 1, 2, 3, 4, 5].map((idx) => (
              <div
                key={idx}
                className={`w-3.5 h-3.5 rounded-full border transition-all duration-200 ${
                  pin.length > idx
                    ? "bg-cyan-400 border-cyan-300 shadow-md shadow-cyan-500/50 scale-110"
                    : "bg-[#070a10] border-slate-700"
                }`}
              />
            ))}
          </div>

          {/* Keyboard input fallback / Hidden input for auto-focus */}
          <div className="relative">
            <input
              type="password"
              inputMode="numeric"
              autoFocus
              value={pin}
              onChange={(e) => {
                setPin(e.target.value.replace(/[^0-9]/g, "").slice(0, 8));
                setError("");
              }}
              placeholder="Enter PIN on keyboard..."
              className="w-full text-center bg-[#070a10] border border-slate-800 focus:border-cyan-500/60 rounded-lg py-2 px-3 text-xs font-mono tracking-widest text-slate-300 placeholder:text-slate-600 outline-none transition"
            />
          </div>

          {error && (
            <div className="bg-rose-500/10 border border-rose-500/40 text-rose-400 text-xs text-center py-2 px-3 rounded-lg font-mono">
              {error}
            </div>
          )}

          {/* Numeric Keypad for Mobile / Quick Click */}
          <div className="grid grid-cols-3 gap-2 font-mono text-sm pt-1">
            {["1", "2", "3", "4", "5", "6", "7", "8", "9"].map((d) => (
              <button
                key={d}
                type="button"
                onClick={() => handleDigit(d)}
                className="py-3 rounded-xl bg-[#070a10] hover:bg-slate-800 border border-slate-800 hover:border-slate-700 text-white font-bold transition active:scale-95"
              >
                {d}
              </button>
            ))}
            <button
              type="button"
              onClick={handleClear}
              className="py-3 rounded-xl bg-[#070a10] hover:bg-slate-800 border border-slate-800 text-slate-400 text-xs font-bold transition"
            >
              C
            </button>
            <button
              type="button"
              onClick={() => handleDigit("0")}
              className="py-3 rounded-xl bg-[#070a10] hover:bg-slate-800 border border-slate-800 hover:border-slate-700 text-white font-bold transition active:scale-95"
            >
              0
            </button>
            <button
              type="button"
              onClick={handleDelete}
              className="py-3 rounded-xl bg-[#070a10] hover:bg-slate-800 border border-slate-800 text-slate-400 flex items-center justify-center transition active:scale-95"
            >
              <Delete className="w-4 h-4" />
            </button>
          </div>

          <button
            type="submit"
            disabled={submitting || pin.length === 0}
            className="w-full bg-cyan-500 hover:bg-cyan-400 text-black font-black py-2.5 rounded-xl text-xs font-mono tracking-wider flex items-center justify-center gap-2 transition disabled:opacity-40 shadow-lg shadow-cyan-950/50"
          >
            {submitting ? "VERIFYING..." : "UNLOCK TERMINAL"}
            <ArrowRight className="w-4 h-4" />
          </button>
        </form>

        {/* Security Footer Note */}
        <div className="pt-2 border-t border-slate-800/80 flex items-center justify-center gap-2 text-[11px] text-slate-500 font-mono">
          <ShieldCheck className="w-3.5 h-3.5 text-emerald-400" />
          <span>AES-256 / Ed25519 Isolated Vault</span>
        </div>
      </div>
    </main>
  );
}
