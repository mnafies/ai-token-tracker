import { useMemo } from "react";
import { CartesianGrid, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { getSession, listAgents } from "../api";
import CostCell from "../components/CostCell";
import DataTable from "../components/DataTable";
import ErrorBox from "../components/ErrorBox";
import { buildCumulative } from "../lib/cumulative";
import { formatDateTime, formatMoney, formatTokens, priceStatusLabel, projectName } from "../lib/format";
import { useAsync } from "../lib/useAsync";
import { useSettings } from "../nav";
import type { PageProps } from "../nav";
import type { EventRow } from "../types";

export default function SessionDetail({ refreshKey, params, goBack }: PageProps) {
  const s = useSettings();
  const agent = params.agent ?? "";
  const sessionId = params.sessionId ?? "";
  const detail = useAsync(() => getSession(agent, sessionId), [agent, sessionId, refreshKey]);
  const agents = useAsync(listAgents, [refreshKey]);
  const d = detail.data;
  const label = (agents.data ?? []).find((a) => a.agent === agent)?.label ?? agent;

  const cum = useMemo(() => buildCumulative(d?.events ?? []), [d]);
  const folders = useMemo(() => [...new Set((d?.events ?? []).map((e) => e.cwd).filter((c): c is string => !!c))], [d]);
  const rows = useMemo(() => (d?.events ?? []).map((e, i) => ({ ...e, _i: i })), [d]);

  return (
    <div className="page">
      <button className="back" onClick={() => goBack?.()}>← Kembali</button>
      <ErrorBox message={detail.error ?? agents.error} />
      {!d && detail.loading && <div className="muted">Memuat…</div>}
      {!d && !detail.loading && !detail.error && <div className="empty">Sesi tidak ditemukan</div>}
      {d && (
        <>
          <section className="card">
            <h1>{d.row.title ?? d.row.sessionId.slice(0, 8)}</h1>
            <div className="muted">
              {label} · Project: {d.row.projectKey ? <span title={d.row.projectKey}>{projectName(d.row.projectKey)}</span> : "—"} · Folder kerja: {folders.length ? folders.join(", ") : "—"}
            </div>
            <div style={{ marginTop: 8 }}>
              {d.row.tokenStatus === "not-recorded" ? "—" : <CostCell usd={d.row.costUsd} unpriced={d.row.unpricedTokens} />}
              <span className="muted"> · {formatTokens(d.row.tokens)} token · {d.events.length} respons</span>
            </div>
            {d.row.tokenStatus === "not-recorded" && <div className="muted small">token tidak tercatat lokal</div>}
          </section>

          <section className="card">
            <h2>Biaya kumulatif per respons</h2>
            {cum.points.length === 0 ? (
              <div className="empty">Belum ada respons</div>
            ) : (
              <div style={{ width: "100%", height: 260 }}>
                <ResponsiveContainer>
                  <LineChart data={cum.points} margin={{ top: 8, right: 8, bottom: 0, left: 0 }}>
                    <CartesianGrid strokeDasharray="3 3" stroke="var(--grid)" vertical={false} />
                    <XAxis dataKey="index" tick={{ fill: "var(--muted)", fontSize: 11 }} stroke="var(--border)" />
                    <YAxis tick={{ fill: "var(--muted)", fontSize: 11 }} stroke="var(--border)" width={56}
                      tickFormatter={(v: number) => formatMoney(v, s)} />
                    <Tooltip
                      formatter={(v) => [formatMoney(Number(v), s), "Kumulatif"]}
                      labelFormatter={(i) => `Respons ${i}`}
                      contentStyle={{ background: "var(--card)", border: "1px solid var(--border)", borderRadius: 8, color: "var(--text)" }} />
                    <Line type="monotone" dataKey="cumulative" stroke="var(--accent)" dot={false} strokeWidth={2} />
                  </LineChart>
                </ResponsiveContainer>
              </div>
            )}
            {cum.unpricedTokens > 0 && (
              <p className="muted small">+ {formatTokens(cum.unpricedTokens)} token tanpa harga</p>
            )}
          </section>

          <section className="card">
            <h2>Per respons</h2>
            <DataTable rows={rows} rowKey={(e) => `${e.tsMs}/${e._i}`}
              defaultSort={{ header: "Waktu", dir: "asc" }}
              columns={[
                { header: "Waktu", render: (e) => formatDateTime(e.tsMs), sortValue: (e) => e.tsMs },
                { header: "Model", render: (e) => e.model, sortValue: (e) => e.model.toLowerCase() },
                { header: "Input", align: "right", render: (e) => formatTokens(e.input), sortValue: (e) => e.input },
                { header: "Output", align: "right", render: (e) => formatTokens(e.output), sortValue: (e) => e.output },
                { header: "Reasoning", align: "right", render: (e) => formatTokens(e.reasoning), sortValue: (e) => e.reasoning },
                { header: "Cache read", align: "right", render: (e) => formatTokens(e.cacheRead), sortValue: (e) => e.cacheRead },
                { header: "Cache write", align: "right", render: (e) => formatTokens(e.cacheWrite), sortValue: (e) => e.cacheWrite },
                { header: "Biaya", align: "right", render: (e) => <EventCost e={e} />, sortValue: (e) => e.costUsd },
              ]} />
          </section>
        </>
      )}
    </div>
  );
}

function EventCost({ e }: { e: EventRow }) {
  const s = useSettings();
  if (e.costUsd == null) {
    return <span>— <span className="muted small">{priceStatusLabel(e.priceStatus)}</span></span>;
  }
  return <span>{formatMoney(e.costUsd, s)}</span>;
}
