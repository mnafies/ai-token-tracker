import type { ReactNode } from "react";

export interface Column<T> {
  header: string;
  render: (row: T) => ReactNode;
  align?: "right";
}

interface Props<T> {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T) => string;
  onRowClick?: (row: T) => void;
  empty?: string;
}

export default function DataTable<T>({ columns, rows, rowKey, onRowClick, empty = "Belum ada data" }: Props<T>) {
  if (rows.length === 0) return <div className="empty">{empty}</div>;
  return (
    <table className="table">
      <thead>
        <tr>{columns.map((c) => <th key={c.header} className={c.align === "right" ? "num" : undefined}>{c.header}</th>)}</tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr key={rowKey(r)} className={onRowClick ? "clickable" : undefined} onClick={onRowClick && (() => onRowClick(r))}>
            {columns.map((c) => <td key={c.header} className={c.align === "right" ? "num" : undefined}>{c.render(r)}</td>)}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
