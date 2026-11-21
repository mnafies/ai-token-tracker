import { useState } from "react";
import { customRange } from "../lib/range";
import type { PeriodSpec } from "../types";

const OPTIONS: { kind: Exclude<PeriodSpec["kind"], "custom">; label: string }[] = [
  { kind: "today", label: "Hari ini" },
  { kind: "thisWeek", label: "Minggu ini" },
  { kind: "lastWeek", label: "Minggu lalu" },
  { kind: "thisMonth", label: "Bulan ini" },
  { kind: "lastMonth", label: "Bulan lalu" },
];

export default function PeriodPicker({ value, onChange }: { value: PeriodSpec; onChange: (p: PeriodSpec) => void }) {
  const [start, setStart] = useState("");
  const [end, setEnd] = useState("");
  const [touched, setTouched] = useState(false);

  const apply = (s: string, e: string) => {
    const r = customRange(s, e);
    if (r) onChange(r);
  };
  const hint = !touched ? null : !start || !end ? "Isi tanggal awal dan akhir untuk periode custom." : start > end ? "Tanggal awal harus sebelum tanggal akhir." : null;

  return (
    <div className="period-picker">
      <div className="segmented">
        {OPTIONS.map((o) => (
          <button key={o.kind} className={value.kind === o.kind ? "active" : ""} onClick={() => onChange({ kind: o.kind })}>
            {o.label}
          </button>
        ))}
        <button className={value.kind === "custom" ? "active" : ""} onClick={() => { setTouched(true); apply(start, end); }}>Custom</button>
      </div>
      <div className="custom-range">
        <input type="date" aria-label="Tanggal awal" value={start}
          onChange={(e) => { setTouched(true); setStart(e.target.value); apply(e.target.value, end); }} />
        <span className="muted">–</span>
        <input type="date" aria-label="Tanggal akhir" value={end}
          onChange={(e) => { setTouched(true); setEnd(e.target.value); apply(start, e.target.value); }} />
      </div>
    {hint && <div className="muted small hint">{hint}</div>}
    </div>
  );
}
