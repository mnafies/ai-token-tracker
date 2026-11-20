import { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { exportCsv, getSettings, updateSettings, wipeAllData } from "../api";
import ErrorBox from "../components/ErrorBox";
import PeriodPicker from "../components/PeriodPicker";
import type { PageProps } from "../nav";
import type { PeriodSpec, Settings } from "../types";

function todayStr(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export default function SettingsPage({ refreshKey }: PageProps) {
  const [form, setForm] = useState<Settings | null>(null);
  const [rate, setRate] = useState("");
  const [newRoot, setNewRoot] = useState("");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [showValidation, setShowValidation] = useState(false);

  // Loaded once when the page opens; later refreshes must not clobber unsaved edits.
  useEffect(() => {
    getSettings().then(
      (s) => { setForm(s); setRate(s.idrRate != null ? String(s.idrRate) : ""); },
      (e) => setLoadError(`Gagal memuat pengaturan: ${e}`),
    );
  }, []);
  // refreshKey bumps after saving (update_settings emits data-updated); App reloads the context itself.
  void refreshKey;

  const [kind, setKind] = useState<"sessions" | "daily">("sessions");
  const [period, setPeriod] = useState<PeriodSpec>({ kind: "thisMonth" });
  const [exportMsg, setExportMsg] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);

  const [confirmWipe, setConfirmWipe] = useState(false);
  const [wipeMsg, setWipeMsg] = useState<string | null>(null);
  const [wipeError, setWipeError] = useState<string | null>(null);

  if (!form) return <div className="page"><h1>Pengaturan</h1>{loadError ? <ErrorBox message={loadError} /> : <div className="muted">Memuat…</div>}</div>;

  const patch = (p: Partial<Settings>) => { setForm({ ...form, ...p }); setSaved(false); };
  const rateNum = Number(rate.replace(",", "."));
  const rateInvalid = form.currency === "idr" && !(rate.trim() !== "" && Number.isFinite(rateNum) && rateNum > 0);
  const pollInvalid = !Number.isInteger(form.processPollSecs) || form.processPollSecs < 1 || form.processPollSecs > 3600;

  const addRoot = () => {
    const r = newRoot.trim();
    if (!r || form.scanRoots.includes(r)) return;
    patch({ scanRoots: [...form.scanRoots, r] });
    setNewRoot("");
  };
  const rootError = newRoot.trim() && !newRoot.trim().startsWith("/") ? "Gunakan path absolut (diawali /)." : null;

  const onSave = async () => {
    setShowValidation(true);
    setSaveError(null);
    if (rateInvalid || pollInvalid) return;
    try {
      await updateSettings({ ...form, idrRate: form.currency === "idr" ? rateNum : form.idrRate });
      setSaved(true);
      setShowValidation(false);
      // Fetch the clamped/normalized values the backend stored.
      const s = await getSettings();
      setForm(s);
      setRate(s.idrRate != null ? String(s.idrRate) : "");
    } catch (e) {
      setSaveError(String(e));
    }
  };

  const onExport = async () => {
    setExportMsg(null);
    setExportError(null);
    try {
      const path = await save({
        defaultPath: `ai-token-tracker-${kind === "sessions" ? "sesi" : "harian"}-${todayStr()}.csv`,
        filters: [{ name: "CSV", extensions: ["csv"] }],
      });
      if (!path) return;
      await exportCsv(kind === "sessions" ? "sessions" : "daily", period, path);
      setExportMsg(`Berhasil diekspor ke ${path}`);
    } catch (e) {
      setExportError(String(e));
    }
  };

  const onWipe = async () => {
    setWipeError(null);
    try {
      await wipeAllData();
      setConfirmWipe(false);
      setWipeMsg("Data dihapus. Riwayat yang masih ada di disk dibaca ulang di background.");
    } catch (e) {
      setWipeError(String(e));
    }
  };

  const lastRoot = form.scanRoots.length <= 1;

  return (
    <div className="page">
      <div className="page-head"><h1>Pengaturan</h1></div>

      <section className="card">
        <h2>Umum</h2>
        <div className="filters">
          <span>Mata uang:</span>
          <label><input type="radio" name="currency" checked={form.currency === "usd"} onChange={() => patch({ currency: "usd" })} /> USD</label>
          <label><input type="radio" name="currency" checked={form.currency === "idr"} onChange={() => patch({ currency: "idr" })} /> IDR</label>
          {form.currency === "idr" && (
            <label>Kurs (Rp per USD){" "}
              <input type="number" min="0" step="any" value={rate} aria-label="Kurs IDR" style={{ width: 110 }}
                onChange={(e) => { setRate(e.target.value); setSaved(false); }} />
            </label>
          )}
        </div>
        {showValidation && rateInvalid && <div className="small" style={{ color: "var(--err-text)" }}>Kurs IDR wajib diisi dengan angka lebih dari 0.</div>}

        <div className="filters" style={{ marginTop: 12 }}>
          <label><input type="checkbox" checked={form.launchAtLogin} onChange={(e) => patch({ launchAtLogin: e.target.checked })} /> Jalankan saat login</label>
        </div>

        <div className="filters" style={{ marginTop: 12 }}>
          <label>Interval cek proses (detik, 1–3600){" "}
            <input type="number" min={1} max={3600} step={1} value={Number.isNaN(form.processPollSecs) ? "" : form.processPollSecs}
              onChange={(e) => patch({ processPollSecs: e.target.value === "" ? NaN : Number(e.target.value) })} style={{ width: 80 }} />
          </label>
        </div>
        {showValidation && pollInvalid && <div className="small" style={{ color: "var(--err-text)" }}>Interval harus bilangan bulat 1–3600.</div>}

        <h2 style={{ marginTop: 16 }}>Folder scan</h2>
        <ul className="running">
          {form.scanRoots.map((r) => (
            <li key={r} className="filters">
              <code>{r}</code>
              <button disabled={lastRoot} onClick={() => patch({ scanRoots: form.scanRoots.filter((x) => x !== r) })}>Hapus</button>
            </li>
          ))}
        </ul>
        {lastRoot && <div className="small muted">Minimal satu folder scan harus tersisa.</div>}
        <div className="controls" style={{ marginTop: 8 }}>
          <input value={newRoot} placeholder="/Users/nama/proyek" aria-label="Folder scan baru" style={{ width: 280 }}
            onChange={(e) => setNewRoot(e.target.value)} onKeyDown={(e) => e.key === "Enter" && !rootError && addRoot()} />
          <button disabled={!newRoot.trim() || !!rootError} onClick={addRoot}>Tambah</button>
        </div>
        {rootError && <div className="small" style={{ color: "var(--err-text)" }}>{rootError}</div>}

        <div className="controls" style={{ marginTop: 16 }}>
          <button onClick={onSave}>Simpan</button>
          {saved && <span className="small muted">Pengaturan disimpan.</span>}
        </div>
        <ErrorBox message={saveError} />
      </section>

      <section className="card">
        <h2>Ekspor CSV</h2>
        <div className="filters">
          <label>Jenis{" "}
            <select value={kind} onChange={(e) => setKind(e.target.value as "sessions" | "daily")}>
              <option value="sessions">Sesi</option>
              <option value="daily">Harian</option>
            </select>
          </label>
          <PeriodPicker value={period} onChange={setPeriod} />
          <button onClick={onExport}>Ekspor…</button>
        </div>
        {exportMsg && <div className="small" style={{ marginTop: 8 }}>{exportMsg}</div>}
        <ErrorBox message={exportError} />
      </section>

      <section className="card">
        <h2>Hapus semua data</h2>
        {!confirmWipe ? (
          <button onClick={() => { setConfirmWipe(true); setWipeMsg(null); }}>Hapus semua data</button>
        ) : (
          <div className="banner warn">
            <div>Semua riwayat token yang tersimpan di aplikasi akan dihapus dan tidak bisa dikembalikan. Lanjutkan?</div>
            <div className="controls">
              <button onClick={onWipe} style={{ background: "#dc2626", color: "#fff", border: 0, borderRadius: 6, padding: "6px 12px" }}>Ya, hapus semua</button>
              <button onClick={() => setConfirmWipe(false)}>Batal</button>
            </div>
          </div>
        )}
        {wipeMsg && <div className="small" style={{ marginTop: 8 }}>{wipeMsg}</div>}
        <ErrorBox message={wipeError} />
      </section>
    </div>
  );
}
