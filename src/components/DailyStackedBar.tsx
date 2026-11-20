import { Bar, BarChart, CartesianGrid, Legend, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { useSettings } from "../nav";
import { formatMoney } from "../lib/format";

export interface StackRow { label: string; [series: string]: number | string }
export interface StackSeries { key: string; name: string; color: string }

/** Stacked bar chart of cost (USD) per label, one segment per series. */
export default function DailyStackedBar({ rows, series, height = 280 }: { rows: StackRow[]; series: StackSeries[]; height?: number }) {
  const s = useSettings();
  if (rows.length === 0 || series.length === 0) return <div className="empty">Belum ada data</div>;
  return (
    <div style={{ width: "100%", height }}>
      <ResponsiveContainer>
        <BarChart data={rows} margin={{ top: 8, right: 8, bottom: 0, left: 0 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="var(--grid)" vertical={false} />
          <XAxis dataKey="label" tick={{ fill: "var(--muted)", fontSize: 11 }} stroke="var(--border)" />
          <YAxis tick={{ fill: "var(--muted)", fontSize: 11 }} stroke="var(--border)" width={56}
            tickFormatter={(v: number) => formatMoney(v, s)} />
          <Tooltip
            formatter={(v, name) => [formatMoney(Number(v), s), String(name)]}
            contentStyle={{ background: "var(--card)", border: "1px solid var(--border)", borderRadius: 8, color: "var(--text)" }}
            cursor={{ fill: "var(--hover)" }} />
          <Legend wrapperStyle={{ fontSize: 12 }} />
          {series.map((sr) => (
            <Bar key={sr.key} dataKey={sr.key} name={sr.name} stackId="a" fill={sr.color} />
          ))}
        </BarChart>
      </ResponsiveContainer>
    </div>
  );
}
