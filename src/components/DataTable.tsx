import { useMemo, useState } from "react";
import type { ReactNode } from "react";
import { sortRows } from "../lib/sort";
import type { SortValue } from "../lib/sort";

export interface Column<T> {
  header: string;
  render: (row: T) => ReactNode;
  align?: "right";
  /** When set, the column is sortable by this key (click header toggles asc/desc). */
  sortValue?: (row: T) => SortValue;
}

interface Props<T> {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T) => string;
  onRowClick?: (row: T) => void;
  empty?: string;
  /** Initial sort: column header and direction. */
  defaultSort?: { header: string; dir: "asc" | "desc" };
  rowClassName?: (row: T) => string | undefined;
}

export default function DataTable<T>({ columns, rows, rowKey, onRowClick, empty = "Belum ada data", defaultSort, rowClassName }: Props<T>) {
  const [sort, setSort] = useState<{ header: string; dir: "asc" | "desc" } | null>(defaultSort ?? null);
  const sorted = useMemo(() => {
    const col = sort && columns.find((c) => c.header === sort.header);
    return col?.sortValue ? sortRows(rows, col.sortValue, sort!.dir) : rows;
  }, [rows, columns, sort]);

  if (rows.length === 0) return <div className="empty">{empty}</div>;
  const toggle = (header: string) =>
    setSort((s) => (s?.header === header && s.dir === "desc" ? { header, dir: "asc" } : { header, dir: "desc" }));

  return (
    <table className="table">
      <thead>
        <tr>
          {columns.map((c) => {
            const cls = [c.align === "right" ? "num" : "", c.sortValue ? "sortable" : ""].filter(Boolean).join(" ") || undefined;
            const active = sort?.header === c.header;
            return (
              <th key={c.header} className={cls} onClick={c.sortValue ? () => toggle(c.header) : undefined}
                aria-sort={active ? (sort!.dir === "asc" ? "ascending" : "descending") : undefined}>
                {c.header}{active ? (sort!.dir === "asc" ? " ▲" : " ▼") : ""}
              </th>
            );
          })}
        </tr>
      </thead>
      <tbody>
        {sorted.map((r) => {
          const cls = [onRowClick ? "clickable" : "", rowClassName?.(r) ?? ""].filter(Boolean).join(" ") || undefined;
          return (
            <tr key={rowKey(r)} className={cls} onClick={onRowClick && (() => onRowClick(r))}>
              {columns.map((c) => <td key={c.header} className={c.align === "right" ? "num" : undefined}>{c.render(r)}</td>)}
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
