import { useEffect, useState } from "react";
import { cancelDiscovery, getAppStatus, getDiscovery, listAgents, setCandidateDecision, startDiscovery } from "../api";
import DataTable from "../components/DataTable";
import ErrorBox from "../components/ErrorBox";
import { formatDate, formatDateTime } from "../lib/format";
import { useAsync } from "../lib/useAsync";
import type { PageProps } from "../nav";
import type { AgentRow, CandidateRow } from "../types";

const LEVEL: Record<AgentRow["level"], string> = { full: "Token penuh", sessions: "Sesi saja", presence: "Deteksi saja" };

export function agentStatusLabel(a: AgentRow): string {
  if (a.running.length > 0) return "Sedang jalan";
  if (a.detected && a.sessions > 0) return "Pernah jalan";
  if (a.detected) return "Terdeteksi, belum ada sesi";
  return "Tidak terdeteksi";
}

const DECISION: Record<string, string> = { pending: "Belum diputuskan", track: "Dilacak", ignore: "Diabaikan" };

export default function Agents({ refreshKey }: PageProps) {
  const [tick, setTick] = useState(0);
  const agents = useAsync(listAgents, [refreshKey, tick]);
  const status = useAsync(getAppStatus, [refreshKey, tick]);
  const discovery = useAsync(getDiscovery, [refreshKey, tick]);
  const [actionError, setActionError] = useState<string | null>(null);
  const running = status.data?.discoveryRunning ?? false;

  // Poll only while a scan is running.
  useEffect(() => {
    if (!running) return;
    const id = setInterval(() => setTick((t) => t + 1), 2000);
    return () => clearInterval(id);
  }, [running]);

  const act = async (fn: () => Promise<void>) => {
    setActionError(null);
    try { await fn(); } catch (e) { setActionError(String(e)); }
    setTick((t) => t + 1);
  };

  const progress = status.data?.progress;
  const d = discovery.data;

  return (
    <div className="page">
      <div className="page-head"><h1>Agent</h1></div>
      {status.data?.retentionWarning && <div className="banner warn">{status.data.retentionWarning}</div>}
      <ErrorBox message={agents.error ?? status.error ?? discovery.error ?? actionError} />
      {!agents.data && agents.loading && <div className="muted">Memuat…</div>}
      <div className="grid-2">
        {agents.data?.map((a) => (
          <section className="card" key={a.agent}>
            <h2>{a.label}</h2>
            <div>
              <span className="badge">{agentStatusLabel(a)}</span>
              <span className="badge">{LEVEL[a.level]}</span>
            </div>
            <div className="small muted" style={{ marginTop: 8 }}>
              <div>Lokasi data: {a.dataRoot}</div>
              <div>Sesi: {a.sessions}</div>
              <div>Data sejak: {a.firstMs != null ? formatDate(a.firstMs) : "—"}</div>
            </div>
            {a.running.length > 0 && (
              <ul className="running" style={{ marginTop: 8 }}>
                {a.running.map((r) => (
                  <li key={r.pid} className="small">
                    <span className="dot" style={{ background: "#16a34a" }} />
                    PID {r.pid} · {r.cwd ?? "folder kerja tidak diketahui"} · mulai {formatDateTime(r.startedMs)}
                    {r.sessionHint ? ` · sesi ${r.sessionHint}` : ""}
                  </li>
                ))}
              </ul>
            )}
          </section>
        ))}
      </div>

      <section className="card">
        <div className="page-head">
          <h2>Scan direktori</h2>
          {running
            ? <button onClick={() => act(cancelDiscovery)}>Batalkan</button>
            : <button onClick={() => act(startDiscovery)}>Scan ulang</button>}
        </div>
        {running && (
          <div className="small muted">
            Memindai…{progress ? ` ${progress.agent}: ${progress.done}/${progress.total}` : ""}
            {progress && progress.total > 0 && <progress value={progress.done} max={progress.total} style={{ width: "100%" }} />}
          </div>
        )}
        {d && (
          <>
            <p className="small muted">Footprint project ditemukan: {d.footprintsCount}</p>
            <DataTable<CandidateRow> rows={d.candidates} rowKey={(c) => c.path} empty="Belum ada kandidat"
              columns={[
                { header: "Lokasi", render: (c) => c.path },
                { header: "Perkiraan agent", render: (c) => c.guessedAgent ?? "—" },
                { header: "Format", render: (c) => c.knownFormat ?? "—" },
                { header: "File", align: "right", render: (c) => c.fileCount },
                { header: "Keputusan", render: (c) => DECISION[c.decision] ?? c.decision },
                {
                  header: "Aksi",
                  render: (c) => (
                    <span className="controls">
                      <button disabled={c.decision === "track" || c.knownFormat == null}
                        title={c.knownFormat == null ? "format belum didukung" : undefined}
                        onClick={() => act(() => setCandidateDecision(c.path, "track"))}>Lacak</button>
                      <button disabled={c.decision === "ignore"} onClick={() => act(() => setCandidateDecision(c.path, "ignore"))}>Abaikan</button>
                      {c.knownFormat == null && <span className="small muted">format belum didukung</span>}
                    </span>
                  ),
                },
              ]} />
            {d.denied.length > 0 && (
              <div style={{ marginTop: 12 }}>
                <div className="small">macOS menolak akses — beri izin di System Settings › Privacy &amp; Security › Files and Folders</div>
                <ul className="small muted">{d.denied.map((p) => <li key={p}>{p}</li>)}</ul>
              </div>
            )}
          </>
        )}
      </section>
    </div>
  );
}
