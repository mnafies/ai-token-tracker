import { useSettings } from "../nav";
import { formatMoney, formatTokens } from "../lib/format";

/** Cost with unpriced tokens made explicit; never shows $0 for unpriced usage. */
export default function CostCell({ usd, unpriced }: { usd: number; unpriced: number }) {
  const s = useSettings();
  if (unpriced <= 0) return <span>{formatMoney(usd, s)}</span>;
  return (
    <span>
      {usd > 0 ? formatMoney(usd, s) : "—"}
      <span className="muted small"> + {formatTokens(unpriced)} token tanpa harga</span>
    </span>
  );
}
