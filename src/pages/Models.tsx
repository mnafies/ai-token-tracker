import { useState } from "react";
import { listModels } from "../api";
import CostCell from "../components/CostCell";
import DataTable from "../components/DataTable";
import ErrorBox from "../components/ErrorBox";
import PeriodPicker from "../components/PeriodPicker";
import { formatPer1M, formatTokens, priceStatusLabel } from "../lib/format";
import { useAsync } from "../lib/useAsync";
import type { PageProps } from "../nav";
import type { PeriodSpec } from "../types";

export default function Models({ refreshKey, navigate }: PageProps) {
  const [period, setPeriod] = useState<PeriodSpec>({ kind: "thisMonth" });
  const models = useAsync(() => listModels(period), [period, refreshKey]);

  return (
    <div className="page">
      <div className="page-head">
        <h1>Model</h1>
        <PeriodPicker value={period} onChange={setPeriod} />
      </div>
      <ErrorBox message={models.error} />
      {!models.data && models.loading && <div className="muted">Memuat…</div>}
      {models.data && (
        <section className="card">
          <DataTable rows={models.data} rowKey={(m) => `${m.providerHint}/${m.model}`}
            rowClassName={(m) => (m.status === "unknown" ? "row-warn" : undefined)}
            defaultSort={{ header: "Biaya", dir: "desc" }}
            columns={[
              { header: "Model", render: (m) => m.model, sortValue: (m) => m.model.toLowerCase() },
              { header: "Provider", render: (m) => m.providerHint || "—", sortValue: (m) => m.providerHint.toLowerCase() },
              {
                header: "Status harga", sortValue: (m) => priceStatusLabel(m.status),
                render: (m) => m.status === "unknown"
                  ? <button className="link" onClick={() => navigate("pricing")}>{priceStatusLabel(m.status)}</button>
                  : priceStatusLabel(m.status),
              },
              { header: "ID OpenRouter", render: (m) => m.openrouterId ?? "—" },
              { header: "Input / 1M", align: "right", render: (m) => formatPer1M(m.prompt), sortValue: (m) => m.prompt },
              { header: "Output / 1M", align: "right", render: (m) => formatPer1M(m.completion), sortValue: (m) => m.completion },
              { header: "Token", align: "right", render: (m) => formatTokens(m.totals.tokens), sortValue: (m) => m.totals.tokens },
              { header: "Biaya", align: "right", render: (m) => <CostCell usd={m.totals.costUsd} unpriced={m.totals.unpricedTokens} />, sortValue: (m) => m.totals.costUsd },
            ]} />
        </section>
      )}
    </div>
  );
}
