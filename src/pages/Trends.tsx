import { useMemo, useState } from "react";
import { getAppStatus, getTrends, listAgents } from "../api";
import CostCell from "../components/CostCell";
import DailyStackedBar from "../components/DailyStackedBar";
import DataTable from "../components/DataTable";
import ErrorBox from "../components/ErrorBox";
import { buildChart } from "../lib/chart";
import { formatDate, formatPct, formatTokens } from "../lib/format";
import { useAsync } from "../lib/useAsync";
import type { PageProps } from "../nav";
import type { Granularity, Split } from "../types";

export default function Trends({ refreshKey }: PageProps) {
  const [granularity, setGranularity] = useState<Granularity>("week");
  const [split, setSplit] = useState<Split>("agent");
  const trends = useAsync(() => getTrends(granularity, 12, split), [granularity, split, refreshKey]);
  const status = useAsync(getAppStatus, [refreshKey]);
  const agents = useAsync(listAgents, [refreshKey]);

  const buckets = trends.data ?? [];
  const chart = useMemo(() => {
    const labels = new Map((agents.data ?? []).map((a) => [a.agent, a.label]));
    return buildChart(buckets, split, (k) => labels.get(k) ?? k);
  }, [trends.data, split, agents.data]);
  const since = status.data?.dataSinceMs;

  const unpricedTotal = buckets.reduce((n, b) => n + b.totals.unpricedTokens, 0);
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

      <ErrorBox message={trends.error ?? status.error ?? agents.error} />
      {!trends.data && trends.loading && <div className="muted">Memuat…</div>}

      {trends.data && (
        <>
          <section className="card">
            <h2>Biaya per {granularity === "week" ? "minggu" : "bulan"} (12 terakhir)</h2>
            <DailyStackedBar rows={chart.rows} series={chart.series} />
            {unpricedTotal > 0 && (
              <p className="muted small">+ {formatTokens(unpricedTotal)} token tanpa harga (lihat halaman Harga)</p>
            )}
            {since != null && (
              <p className="muted small">Data tersedia sejak {formatDate(since)}. Minggu/bulan sebelum data tersedia ditampilkan kosong.</p>
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
