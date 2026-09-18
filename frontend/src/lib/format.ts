export function toNum(val: unknown, fallback: number = 0): number {
  if (typeof val === "number") {
    return isNaN(val) ? fallback : val;
  }
  if (typeof val === "string") {
    const parsed = parseFloat(val);
    return isNaN(parsed) ? fallback : parsed;
  }
  return fallback;
}

export function formatNum(val: unknown, decimals: number = 2, fallback: string = "---"): string {
  if (val === null || val === undefined || val === "") return fallback;
  const num = toNum(val, NaN);
  if (isNaN(num)) return fallback;
  return num.toFixed(decimals);
}

export function formatGbp(val: unknown, decimals: number = 2, showSign: boolean = false): string {
  if (val === null || val === undefined || val === "") return "£0.00";
  const num = toNum(val, 0);
  const sign = num > 0 && showSign ? "+" : num < 0 ? "-" : "";
  return `${sign}£${Math.abs(num).toFixed(decimals)}`;
}

export function formatPct(val: unknown, decimals: number = 2, showSign: boolean = false): string {
  if (val === null || val === undefined || val === "") return "0.00%";
  const num = toNum(val, 0);
  const sign = num > 0 && showSign ? "+" : num < 0 ? "-" : "";
  return `${sign}${Math.abs(num).toFixed(decimals)}%`;
}
