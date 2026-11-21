import { useEffect, useState } from "react";
import { listAgents, listSessions } from "../api";
import CostCell from "../components/CostCell";
import DataTable from "../components/DataTable";
import ErrorBox from "../components/ErrorBox";
import PeriodPicker from "../components/PeriodPicker";
import { formatDateTime, formatDuration, formatTokens, projectName } from "../lib/format";
import { useAsync } from "../lib/useAsync";
import type { PageProps } from "../nav";
import type { PeriodSpec } from "../types";

export default function Sessions({ refreshKey, params, navigate, onParamsChange }: PageProps) {
  const [period, setPeriod] = useState<PeriodSpec>(params.period ?? { kind: "thisMonth" });
  const [agent, setAgent] = useState<string>(params.agent ?? "");
  const [projectKey, setProjectKey] = useState<string | null>(params.projectKey ?? null);
  useEffect(() => {
    onParamsChange?.({ ...params, period, agent: agent || undefined, projectKey: projectKey ?? undefined });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [period, agent, projectKey]);
  const agents = useAsync(listAgents, [refreshKey]);
  const sessions = useAsync(
    () => listSessions({ period, agent: agent || null, projectKey }),
    [period, agent, projectKey, refreshKey],
  );
  const labels = new Map((agents.data ?? []).map((a) => [a.agent, a.label]));

  return (
    <div className="page">
      <div className="page-head">
        <h1>Sesi</h1>
        <PeriodPicker value={period} onChange={setPeriod} />
      </div>
      <div className="filters">
        <label>Agent{" "}
          <select value={agent} onChange={(e) => setAgent(e.target.value)}>
            <option value="">Semua agent</option>
            {(agents.data ?? []).map((a) => <option key={a.agent} value={a.agent}>{a.label}</option>)}
          </select>
        </label>
        {projectKey && (
          <span className="chip">Project: <span title={projectKey}>{projectName(projectKey)}</span>
            <button aria-label="Hapus filter project" onClick={() => setProjectKey(null)}>✕</button>
          </span>
        )}
      </div>
      <ErrorBox message={sessions.error ?? agents.error} />
      {!sessions.data && sessions.loading && <div className="muted">Memuat…</div>}
      {sessions.data && (
        <section className="card">
          <DataTable rows={sessions.data} rowKey={(r) => `${r.agent}/${r.sessionId}`}
            onRowClick={(r) => navigate("sessionDetail", { agent: r.agent, sessionId: r.sessionId })}
            defaultSort={{ header: "Mulai", dir: "desc" }}
            columns={[
              { header: "Agent", render: (r) => labels.get(r.agent) ?? r.agent, sortValue: (r) => labels.get(r.agent) ?? r.agent },
              {
                header: "Judul", sortValue: (r) => r.title?.toLowerCase() ?? null,
                render: (r) => (
                  <>
                    {r.title ?? r.sessionId.slice(0, 8)}
                    {r.tokenStatus === "not-recorded" && <div className="muted small">token tidak tercatat lokal</div>}
                  </>
                ),
              },
              { header: "Project", render: (r) => r.projectKey ? <span title={r.projectKey}>{projectName(r.projectKey)}</span> : "—", sortValue: (r) => r.projectKey ? projectName(r.projectKey).toLowerCase() : null },
              { header: "Mulai", render: (r) => (r.startedMs != null ? formatDateTime(r.startedMs) : "—"), sortValue: (r) => r.startedMs },
              {
                header: "Durasi", align: "right", render: (r) => formatDuration(r.startedMs, r.updatedMs),
                sortValue: (r) => (r.startedMs != null && r.updatedMs != null ? r.updatedMs - r.startedMs : null),
              },
              { header: "Model", render: (r) => r.models.join(", ") || "—" },
              { header: "Token", align: "right", render: (r) => formatTokens(r.tokens), sortValue: (r) => r.tokens },
              { header: "Biaya", align: "right", render: (r) => r.tokenStatus === "not-recorded" ? "—" : <CostCell usd={r.costUsd} unpriced={r.unpricedTokens} />, sortValue: (r) => (r.tokenStatus === "not-recorded" ? null : r.costUsd) },
            ]} />
        </section>
      )}
    </div>
  );
}
