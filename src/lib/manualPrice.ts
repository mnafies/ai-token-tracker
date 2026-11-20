import { per1MToPerToken } from "./format";

export interface ManualPriceInput { prompt?: string; completion?: string; cacheRead?: string; cacheWrite?: string }
export interface ManualPrices { prompt: number; completion: number; cacheRead: number | null; cacheWrite: number | null }

/** Empty → null; a number ≥ 0 (comma or dot decimal) → that number; anything else → NaN. */
function parsePrice(s: string | undefined): number | null {
  if (s == null || s.trim() === "") return null;
  const n = Number(s.replace(",", "."));
  return Number.isFinite(n) && n >= 0 ? n : NaN;
}

/** Manual price form (USD per 1M tokens) → per-token prices, or an error message. */
export function manualPrices(i: ManualPriceInput): ManualPrices | string {
  const [p, c, cr, cw] = [i.prompt, i.completion, i.cacheRead, i.cacheWrite].map(parsePrice);
  if (p == null || c == null || Number.isNaN(p) || Number.isNaN(c)) {
    return "Isi harga input dan output (angka ≥ 0, per 1M token).";
  }
  if (Number.isNaN(cr) || Number.isNaN(cw)) return "Harga cache harus angka ≥ 0 atau dikosongkan.";
  const perToken = (v: number | null) => (v == null ? null : per1MToPerToken(v));
  return { prompt: per1MToPerToken(p), completion: per1MToPerToken(c), cacheRead: perToken(cr), cacheWrite: perToken(cw) };
}
