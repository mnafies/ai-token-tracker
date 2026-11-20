import { useEffect, useState } from "react";
import { getAppStatus, listModels, listPriceCatalog, refreshPrices, setModelOverride } from "../api";
import DataTable from "../components/DataTable";
import ErrorBox from "../components/ErrorBox";
import { formatDateTime, formatPer1M, priceStatusLabel } from "../lib/format";
import { manualPrices, type ManualPriceInput } from "../lib/manualPrice";
import { useAsync } from "../lib/useAsync";
import type { PageProps } from "../nav";
import type { ModelOverride, ModelRow, PeriodSpec } from "../types";

const DAY_MS = 86_400_000;
/** Widest custom range the backend accepts (1098 days), ending tomorrow. */
function widestPeriod(): PeriodSpec {
  const endMs = Date.now() + DAY_MS;
  return { kind: "custom", startMs: endMs - 1095 * DAY_MS, endMs };
}
const key = (hint: string, model: string) => `${hint}/${model}`;

export default function Pricing({ refreshKey }: PageProps) {
  const [tick, setTick] = useState(0);
  const status = useAsync(getAppStatus, [refreshKey, tick]);
  const models = useAsync(() => listModels(widestPeriod()), [refreshKey, tick]);
  const [catalog, setCatalog] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [mapInput, setMapInput] = useState<Record<string, string>>({});
  const [manualIn, setManualIn] = useState<Record<string, ManualPriceInput>>({});
  const [rowErr, setRowErr] = useState<Record<string, string>>({});

  // Catalog is only needed here, so load it lazily on open.
  useEffect(() => {
    let cancelled = false;
    listPriceCatalog().then((c) => { if (!cancelled) setCatalog(c); }, (e) => { if (!cancelled) setError(String(e)); });
    return () => { cancelled = true; };
  }, [refreshKey]);

  const run = async (fn: () => Promise<unknown>) => {
    setError(null);
    setBusy(true);
    try { await fn(); } catch (e) { setError(String(e)); }
    setBusy(false);
    setTick((t) => t + 1);
  };

  const override = (m: ModelRow, o: Partial<ModelOverride>, rk: string) => {
    setRowErr((r) => ({ ...r, [rk]: "" }));
    return run(() => setModelOverride({
      providerHint: m.providerHint, rawModel: m.model, action: "clear", openrouterId: null,
      prompt: null, completion: null, cacheRead: null, cacheWrite: null, ...o,
    }));
  };

  const pricing = status.data?.pricing;
  const unknown = models.data?.filter((m) => m.status === "unknown") ?? [];
  const active = models.data?.filter((m) => ["mapped", "manual", "ignored"].includes(m.status)) ?? [];

  const mapModel = (m: ModelRow) => {
    const rk = key(m.providerHint, m.model);
    const id = (mapInput[rk] ?? "").trim();
    if (!id) return setRowErr((r) => ({ ...r, [rk]: "Isi ID OpenRouter." }));
    if (catalog.length > 0 && !catalog.includes(id)) return setRowErr((r) => ({ ...r, [rk]: "ID tidak ada di katalog OpenRouter." }));
    return override(m, { action: "map", openrouterId: id }, rk);
  };
  const manual = (m: ModelRow) => {
    const rk = key(m.providerHint, m.model);
    const prices = manualPrices(manualIn[rk] ?? {});
    if (typeof prices === "string") return setRowErr((r) => ({ ...r, [rk]: prices }));
    return override(m, { action: "manual", ...prices }, rk);
  };

  return (
    <div className="page">
      <div className="page-head">
        <h1>Harga</h1>
        <button disabled={busy} onClick={() => run(refreshPrices)}>{busy ? "Memperbarui…" : "Perbarui harga"}</button>
      </div>
      <ErrorBox message={error ?? status.error ?? models.error} />
      <section className="card">
        <h2>Sumber harga</h2>
        <div className="small">OpenRouter · {pricing?.catalogSize ?? 0} model di katalog</div>
        <div className="small muted">
          Terakhir diperbarui: {pricing?.fetchedMs != null ? formatDateTime(pricing.fetchedMs) : "belum pernah"}
        </div>
        {pricing?.lastError && <div className="error-box" style={{ marginTop: 8 }}>Error terakhir: {pricing.lastError}</div>}
      </section>

      <section className="card">
        <h2>Model tanpa harga</h2>
        <datalist id="price-catalog">{catalog.map((id) => <option key={id} value={id} />)}</datalist>
        <DataTable<ModelRow> rows={unknown} rowKey={(m) => key(m.providerHint, m.model)} empty="Semua model sudah punya harga"
          columns={[
            { header: "Model", render: (m) => <>{m.model}<div className="small muted">{m.providerHint || "—"}</div></> },
            {
              header: "Petakan ke OpenRouter",
              render: (m) => {
                const rk = key(m.providerHint, m.model);
                return (
                  <span className="controls">
                    <input list="price-catalog" placeholder="provider/model" aria-label={`ID OpenRouter untuk ${m.model}`}
                      value={mapInput[rk] ?? ""} onChange={(e) => setMapInput((s) => ({ ...s, [rk]: e.target.value }))} />
                    <button disabled={busy} onClick={() => mapModel(m)}>Petakan</button>
                  </span>
                );
              },
            },
            {
              header: "Harga manual (USD / 1M token)",
              render: (m) => {
                const rk = key(m.providerHint, m.model);
                const field = (f: keyof ManualPriceInput, placeholder: string, label: string) => (
                  <input type="number" min="0" step="any" placeholder={placeholder} aria-label={`${label} ${m.model}`}
                    style={{ width: f.startsWith("cache") ? 120 : 80 }}
                    value={manualIn[rk]?.[f] ?? ""}
                    onChange={(e) => setManualIn((s) => ({ ...s, [rk]: { ...s[rk], [f]: e.target.value } }))} />
                );
                return (
                  <>
                    <span className="controls">
                      {field("prompt", "Input", "Harga input")}
                      {field("completion", "Output", "Harga output")}
                      {field("cacheRead", "Cache read / 1M", "Harga cache read")}
                      {field("cacheWrite", "Cache write / 1M", "Harga cache write")}
                      <button disabled={busy} onClick={() => manual(m)}>Simpan</button>
                    </span>
                    <div className="small muted">Jika harga cache dikosongkan, token cache dihitung dengan harga input.</div>
                  </>
                );
              },
            },
            {
              header: "Aksi",
              render: (m) => {
                const rk = key(m.providerHint, m.model);
                return (
                  <>
                    <button disabled={busy} onClick={() => override(m, { action: "ignore" }, rk)}>Abaikan</button>
                    {rowErr[rk] && <div className="small" style={{ color: "var(--err-text)" }}>{rowErr[rk]}</div>}
                  </>
                );
              },
            },
          ]} />
      </section>

      <section className="card">
        <h2>Override aktif</h2>
        <DataTable<ModelRow> rows={active} rowKey={(m) => key(m.providerHint, m.model)} empty="Belum ada override"
          columns={[
            { header: "Model", render: (m) => <>{m.model}<div className="small muted">{m.providerHint || "—"}</div></> },
            { header: "Status", render: (m) => priceStatusLabel(m.status) },
            { header: "ID OpenRouter", render: (m) => m.openrouterId ?? "—" },
            { header: "Input / 1M", align: "right", render: (m) => formatPer1M(m.prompt) },
            { header: "Output / 1M", align: "right", render: (m) => formatPer1M(m.completion) },
            { header: "", render: (m) => <button disabled={busy} onClick={() => override(m, { action: "clear" }, key(m.providerHint, m.model))}>Hapus</button> },
          ]} />
      </section>
    </div>
  );
}
