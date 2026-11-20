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

/** Last path segment (mirrors Rust project::display_name); falls back to the key itself. */
export function projectName(key: string): string {
  const seg = key.split("/").filter(Boolean).pop();
  return seg ?? key;
}

export function formatDate(ms: number): string {
  return new Date(ms).toLocaleDateString("id-ID", { day: "numeric", month: "short", year: "numeric" });
}

export function formatDateTime(ms: number): string {
  return new Date(ms).toLocaleString("id-ID", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}

/** "1j 5m" / "12m" / "45d" (jam/menit/detik); missing or negative → "—". */
export function formatDuration(startedMs: number | null, updatedMs: number | null): string {
  if (startedMs == null || updatedMs == null || updatedMs < startedMs) return "—";
  const secs = Math.floor((updatedMs - startedMs) / 1000);
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h > 0) return m > 0 ? `${h}j ${m}m` : `${h}j`;
  if (m > 0) return `${m}m`;
  return `${secs}d`;
}

const PRICE_STATUS: Record<string, string> = {
  matched: "Cocok", mapped: "Dipetakan", manual: "Manual", ignored: "Diabaikan", unknown: "Tidak diketahui",
};
export function priceStatusLabel(status: string): string {
  return PRICE_STATUS[status] ?? status;
}

/** Price per 1M tokens from a per-token price, e.g. "$3.00"; missing → "—". */
export function formatPer1M(perToken: number | null): string {
  return perToken == null ? "—" : `$${(perToken * 1e6).toFixed(2)}`;
}

/** UI price per 1M tokens → per-token price (backend unit). */
export function per1MToPerToken(per1M: number): number {
  return per1M / 1e6;
}

/** Per-token price → UI string per 1M tokens, up to 4 decimals, trailing zeros trimmed. */
export function perTokenToPer1M(perToken: number): string {
  return String(Number((perToken * 1e6).toFixed(4)));
}
