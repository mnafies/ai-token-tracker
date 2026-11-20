import { useState } from "react";
import { listAgents, listProjects } from "../api";
import CostCell from "../components/CostCell";
import DataTable from "../components/DataTable";
import ErrorBox from "../components/ErrorBox";
import PeriodPicker from "../components/PeriodPicker";
import { formatTokens } from "../lib/format";
import { useAsync } from "../lib/useAsync";
import type { PageProps } from "../nav";
import type { PeriodSpec } from "../types";

export default function Projects({ refreshKey, navigate }: PageProps) {
  const [period, setPeriod] = useState<PeriodSpec>({ kind: "thisMonth" });
  const projects = useAsync(() => listProjects(period), [period, refreshKey]);
  const agents = useAsync(listAgents, [refreshKey]);
  const labels = new Map((agents.data ?? []).map((a) => [a.agent, a.label]));

  return (
    <div className="page">
      <div className="page-head">
        <h1>Project</h1>
        <PeriodPicker value={period} onChange={setPeriod} />
      </div>
      <ErrorBox message={projects.error ?? agents.error} />
      {!projects.data && projects.loading && <div className="muted">Memuat…</div>}
      {projects.data && (
        <section className="card">
          <DataTable rows={projects.data} rowKey={(p) => p.projectKey}
            onRowClick={(p) => navigate("sessions", { projectKey: p.projectKey })}
            defaultSort={{ header: "Biaya", dir: "desc" }}
            columns={[
              { header: "Nama", render: (p) => p.name, sortValue: (p) => p.name.toLowerCase() },
              { header: "Biaya", align: "right", render: (p) => <CostCell usd={p.costUsd} unpriced={p.unpricedTokens} />, sortValue: (p) => p.costUsd },
              { header: "Token", align: "right", render: (p) => formatTokens(p.tokens), sortValue: (p) => p.tokens },
              { header: "Sesi", align: "right", render: (p) => p.sessions, sortValue: (p) => p.sessions },
              { header: "Agent dipakai", render: (p) => p.agents.map((a) => labels.get(a) ?? a).join(", ") || "—", sortValue: (p) => p.agents.length },
              { header: "Footprint", render: (p) => p.footprints.length === 0 ? "—" : p.footprints.map((f) => <span key={f} className="badge">{f}</span>) },
            ]} />
        </section>
      )}
    </div>
  );
}
