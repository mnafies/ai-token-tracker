/** One stable color per agent (readable on light and dark backgrounds). */
const AGENT_COLORS: Record<string, string> = {
  "claude-code": "#d97757",
  codex: "#10a37f",
  opencode: "#3b82f6",
  "gemini-cli": "#8b5cf6",
  "copilot-cli": "#ec4899",
  "claude-desktop": "#eab308",
  "vs-code": "#06b6d4",
};

const SERIES_PALETTE = ["#3b82f6", "#f97316", "#10b981", "#a855f7", "#ef4444", "#14b8a6", "#eab308", "#ec4899"];
export const OTHER_COLOR = "#94a3b8";

export function agentColor(agent: string): string {
  return AGENT_COLORS[agent] ?? OTHER_COLOR;
}

/** Color for a non-agent series (model / project) by its position. */
export function seriesColor(index: number): string {
  return SERIES_PALETTE[index % SERIES_PALETTE.length];
}
