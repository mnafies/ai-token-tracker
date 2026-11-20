import { useMemo, useState } from "react";
import { getAppStatus, getTrends } from "../api";
import CostCell from "../components/CostCell";
import DailyStackedBar from "../components/DailyStackedBar";
import type { StackRow } from "../components/DailyStackedBar";
import DataTable from "../components/DataTable";
import ErrorBox from "../components/ErrorBox";
import { OTHER_COLOR, agentColor, seriesColor } from "../lib/colors";
import { formatDate, formatPct, formatTokens } from "../lib/format";
import { useAsync } from "../lib/useAsync";
import type { PageProps } from "../nav";
import type { Granularity, Split, TrendBucket } from "../types";

const MAX_SERIES = 8;
const OTHER = "Lainnya";

/** Keeps the top series by total cost; the rest are merged into "Lainnya". */
function buildChart(buckets: TrendBucket[], split: Split) {
  const totals = new Map<string, number>();
  for (const b of buckets) for (const [k, v] of b.by) totals.set(k, (totals.get(k) ?? 0) + v);
  const ranked = [...totals.entries()].sort((a, b) => b[1] - a[1]).map(([k]) => k);
  const top = ranked.slice(0, MAX_SERIES);
  const hasOther = ranked.length > MAX_SERIES;
  const rows: StackRow[] = buckets.map((b) => {
    const row: StackRow = { label: b.label };
    for (const [k, v] of b.by) {
      const key = top.includes(k) ? k : OTHER;
      row[key] = ((row[key] as number | undefined) ?? 0) + v;
    }
    return row;
  });
  const series = top.map((k, i) => ({ key: k, name: k, color: split === "agent" ? agentColor(k) : seriesColor(i) }));
  if (hasOther) series.push({ key: OTHER, name: OTHER, color: OTHER_COLOR });
  return { rows, series };
}

export default function Trends({ refreshKey }: PageProps) {
  const [granularity, setGranularity] = useState<Granularity>("week");
  const [split, setSplit] = useState<Split>("agent");
  const trends = useAsync(() => getTrends(granularity, 12, split), [granularity, split, refreshKey]);
  const status = useAsync(getAppStatus, [refreshKey]);

  const buckets = trends.data ?? [];
  const chart = useMemo(() => buildChart(buckets, split), [trends.data, split]); // eslint-disable-line react-hooks/exhaustive-deps
  const since = status.data?.dataSinceMs;

  const rows = buckets.map((b, i) => {
    const prev = i > 0 ? buckets[i - 1].totals.costUsd : 0;
    return { b, change: prev > 0 ? ((b.totals.costUsd - prev) / prev) * 100 : null };
  });

  return (
    <div className="page">
      <div className="page-head">
        <h1>Mingguan &amp; Bulanan</h1>
        <div className="controls">
          <div className="segmented">
            <button className={granularity === "week" ? "active" : ""} onClick={() => setGranularity("week")}>Minggu</button>
            <button className={granularity === "month" ? "active" : ""} onClick={() => setGranularity("month")}>Bulan</button>
          </div>
          <div className="segmented">
            {([["agent", "Agent"], ["model", "Model"], ["project", "Project"]] as const).map(([k, l]) => (
              <button key={k} className={split === k ? "active" : ""} onClick={() => setSplit(k)}>{l}</button>
            ))}
          </div>
        </div>
      </div>

      <ErrorBox message={trends.error ?? status.error} />
      {!trends.data && trends.loading && <div className="muted">Memuat…</div>}

      {trends.data && (
        <>
          <section className="card">
            <h2>Biaya per {granularity === "week" ? "minggu" : "bulan"} (12 terakhir)</h2>
            <DailyStackedBar rows={chart.rows} series={chart.series} />
            {since != null && (
              <p className="muted small">Data tersedia sejak {formatDate(since)}. Periode sebelumnya kosong, bukan nol pemakaian.</p>
            )}
          </section>

          <section className="card">
            <h2>Ringkasan</h2>
            <DataTable rows={rows} rowKey={(r) => String(r.b.startMs)}
              columns={[
                { header: "Periode", render: (r) => r.b.label },
                { header: "Biaya", align: "right", render: (r) => <CostCell usd={r.b.totals.costUsd} unpriced={r.b.totals.unpricedTokens} /> },
                { header: "Token", align: "right", render: (r) => formatTokens(r.b.totals.tokens) },
                { header: "Perubahan", align: "right", render: (r) => formatPct(r.change) },
              ]} />
          </section>
        </>
      )}
    </div>
  );
}
