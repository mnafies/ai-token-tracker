import { useState } from "react";
import type { PeriodSpec } from "../types";

const OPTIONS: { kind: Exclude<PeriodSpec["kind"], "custom">; label: string }[] = [
  { kind: "today", label: "Hari ini" },
  { kind: "thisWeek", label: "Minggu ini" },
  { kind: "lastWeek", label: "Minggu lalu" },
  { kind: "thisMonth", label: "Bulan ini" },
  { kind: "lastMonth", label: "Bulan lalu" },
];

/** "YYYY-MM-DD" -> epoch ms at local midnight. */
function localMidnight(date: string): number {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, m - 1, d).getTime();
}

export default function PeriodPicker({ value, onChange }: { value: PeriodSpec; onChange: (p: PeriodSpec) => void }) {
  const [start, setStart] = useState("");
  const [end, setEnd] = useState("");

  // Both dates are inclusive; the range ends at local midnight after the end date.
  const apply = (s: string, e: string) => {
    if (!s || !e) return;
    const startMs = localMidnight(s);
    const endMs = new Date(localMidnight(e) + 36 * 3600 * 1000).setHours(0, 0, 0, 0);
    if (endMs > startMs) onChange({ kind: "custom", startMs, endMs });
  };

  return (
    <div className="period-picker">
      <div className="segmented">
        {OPTIONS.map((o) => (
          <button key={o.kind} className={value.kind === o.kind ? "active" : ""} onClick={() => onChange({ kind: o.kind })}>
            {o.label}
          </button>
        ))}
        <button className={value.kind === "custom" ? "active" : ""} onClick={() => apply(start, end)}>Custom</button>
      </div>
      <div className="custom-range">
        <input type="date" aria-label="Tanggal awal" value={start}
          onChange={(e) => { setStart(e.target.value); apply(e.target.value, end); }} />
        <span className="muted">–</span>
        <input type="date" aria-label="Tanggal akhir" value={end}
          onChange={(e) => { setEnd(e.target.value); apply(start, e.target.value); }} />
      </div>
    </div>
  );
}
