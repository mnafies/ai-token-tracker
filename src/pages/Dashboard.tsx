import { useMemo, useState } from "react";
import { getAppStatus, getOverview, listAgents } from "../api";
import CostCell from "../components/CostCell";
import DailyStackedBar from "../components/DailyStackedBar";
import type { StackRow } from "../lib/chart";
import DataTable from "../components/DataTable";
import ErrorBox from "../components/ErrorBox";
import PeriodPicker from "../components/PeriodPicker";
import StatCard from "../components/StatCard";
import { agentColor } from "../lib/colors";
import { formatDateTime, formatPct, formatTokens } from "../lib/format";
import { useAsync } from "../lib/useAsync";
import type { PageProps } from "../nav";
import type { PeriodSpec } from "../types";

export default function Dashboard({ refreshKey, navigate }: PageProps) {
  const [period, setPeriod] = useState<PeriodSpec>({ kind: "thisMonth" });
  const overview = useAsync(() => getOverview(period), [period, refreshKey]);
  const status = useAsync(getAppStatus, [refreshKey]);
  const agents = useAsync(listAgents, [refreshKey]);

  const o = overview.data;
  const chart = useMemo(() => {
    if (!o) return { rows: [] as StackRow[], series: [] };
    const keys = new Set<string>();
    const rows = o.daily.map((d) => {
      const row: StackRow = { label: d.date.slice(5) };
      for (const a of d.byAgent) { if (a.costUsd > 0) { row[a.agent] = a.costUsd; keys.add(a.agent); } }
      return row;
    });
    const series = [...keys].sort().map((k) => ({ key: k, name: k, color: agentColor(k) }));
    return { rows, series };
  }, [o]);

  const labels = useMemo(() => new Map((agents.data ?? []).map((a) => [a.agent, a.label])), [agents.data]);
  const chartSeries = chart.series.map((s) => ({ ...s, name: labels.get(s.key) ?? s.name }));
  const running = (agents.data ?? []).flatMap((a) => a.running.map((r) => ({ ...r, label: a.label })));
  const st = status.data;
  const scanning = st?.progress && st.progress.done < st.progress.total ? st.progress : null;

  return (
    <div className="page">
      <div className="page-head">
        <h1>Dashboard</h1>
        <PeriodPicker value={period} onChange={setPeriod} />
      </div>

      {st?.retentionWarning && <div className="banner warn">{st.retentionWarning}</div>}
      {scanning && (
        <div className="banner info">
          Memindai {labels.get(scanning.agent) ?? scanning.agent}: {scanning.done}/{scanning.total}
          <progress value={scanning.done} max={scanning.total} />
        </div>
      )}
      <ErrorBox message={overview.error ?? status.error ?? agents.error} />
      {!o && overview.loading && <div className="muted">Memuat…</div>}

      {o && (
        <>
          <div className="grid-3">
            <StatCard label="Biaya"
              value={<CostCell usd={o.totals.costUsd} unpriced={o.totals.unpricedTokens} />}
              sub={<>{formatPct(o.changePct)} <span className="muted">vs periode sebelumnya</span></>} />
            <StatCard label="Token" value={formatTokens(o.totals.tokens)}
              sub={`${o.totals.events} permintaan`} />
            <StatCard label="Token tanpa harga" value={formatTokens(o.totals.unpricedTokens)}
              sub={o.totals.unpricedTokens > 0 ? "Atur harga di halaman Harga" : "Semua token punya harga"} />
          </div>

          <section className="card">
            <h2>Biaya harian per agent</h2>
            <DailyStackedBar rows={chart.rows} series={chartSeries} />
            {o.totals.unpricedTokens > 0 && (
              <p className="muted small">+ {formatTokens(o.totals.unpricedTokens)} token tanpa harga (lihat halaman Harga)</p>
            )}
          </section>

          <div className="grid-2">
            <section className="card">
              <h2>Top 5 project</h2>
              <DataTable rows={o.topProjects} rowKey={(p) => p.projectKey}
                onRowClick={(p) => navigate("sessions", { projectKey: p.projectKey })}
                columns={[
                  { header: "Project", render: (p) => p.name },
                  { header: "Biaya", align: "right", render: (p) => <CostCell usd={p.costUsd} unpriced={p.unpricedTokens} /> },
                  { header: "Token", align: "right", render: (p) => formatTokens(p.tokens) },
                ]} />
            </section>
            <section className="card">
              <h2>Top 5 sesi</h2>
              <DataTable rows={o.topSessions} rowKey={(r) => `${r.agent}/${r.sessionId}`}
                onRowClick={(r) => navigate("sessionDetail", { agent: r.agent, sessionId: r.sessionId })}
                columns={[
                  { header: "Sesi", render: (r) => r.title ?? r.sessionId.slice(0, 8) },
                  { header: "Agent", render: (r) => labels.get(r.agent) ?? r.agent },
                  { header: "Biaya", align: "right", render: (r) => <CostCell usd={r.costUsd} unpriced={r.unpricedTokens} /> },
                ]} />
            </section>
          </div>
        </>
      )}

      <section className="card">
        <h2>Agent sedang berjalan</h2>
        {running.length === 0 ? (
          <div className="empty">Tidak ada agent berjalan</div>
        ) : (
          <ul className="running">
            {running.map((r) => (
              <li key={`${r.agent}-${r.pid}`}>
                <span className="dot" style={{ background: agentColor(r.agent) }} />
                <strong>{r.label}</strong>
                <span className="muted"> {r.cwd ?? "—"} · sejak {formatDateTime(r.startedMs)}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
