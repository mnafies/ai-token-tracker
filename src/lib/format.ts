import type { Settings } from "../types";

/** Mirrors Rust `tray::format_money`. */
export function formatMoney(usd: number, s: Settings): string {
  if (s.currency === "idr" && s.idrRate != null && s.idrRate > 0) {
    const n = Math.round(usd * s.idrRate);
    const grouped = Math.abs(n).toString().replace(/\B(?=(\d{3})+(?!\d))/g, ".");
    return `Rp ${n < 0 ? "-" : ""}${grouped}`;
  }
  return `$${usd.toFixed(2)}`;
}

const UNITS = ["", "K", "M", "B"];

/** 3 significant digits, trailing zeros trimmed, suffix K/M/B. */
export function formatTokens(n: number): string {
  const sign = n < 0 ? "-" : "";
  let v = Math.abs(n);
  let u = 0;
  while (u < UNITS.length - 1 && Number(v.toPrecision(3)) >= 1000) {
    v /= 1000;
    u++;
  }
  return `${sign}${Number(v.toPrecision(3))}${UNITS[u]}`;
}

export function formatPct(p: number | null): string {
  if (p == null) return "—";
  const r = Math.round(p);
  return `${r < 0 ? "▼" : "▲"}${Math.abs(r)}%`;
}

export function costLabel(usd: number, unpriced: number, s: Settings): string {
  const money = formatMoney(usd, s);
  return unpriced > 0 ? `${money} + ${formatTokens(unpriced)} token tanpa harga` : money;
}

export function formatDate(ms: number): string {
  return new Date(ms).toLocaleDateString("id-ID", { day: "numeric", month: "short", year: "numeric" });
}

export function formatDateTime(ms: number): string {
  return new Date(ms).toLocaleString("id-ID", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}
