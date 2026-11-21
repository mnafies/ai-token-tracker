// Mirrors the Rust backend types (all JSON is camelCase).

export type PeriodSpec =
  | { kind: "today" | "thisWeek" | "lastWeek" | "thisMonth" | "lastMonth" }
  | { kind: "custom"; startMs: number; endMs: number };

export interface PeriodRange { startMs: number; endMs: number }

export interface Settings {
  currency: "usd" | "idr";
  idrRate: number | null;
  launchAtLogin: boolean;
  processPollSecs: number;
  scanRoots: string[];
}

export interface Totals {
  costUsd: number;
  unpricedTokens: number;
  tokens: number;
  input: number;
  output: number;
  reasoning: number;
  cacheRead: number;
  cacheWrite: number;
  events: number;
}

export interface AgentCost { agent: string; costUsd: number; tokens: number; unpricedTokens: number }
export interface DailyPoint { date: string; byAgent: AgentCost[] }

export interface ProjectRow {
  projectKey: string;
  name: string;
  costUsd: number;
  unpricedTokens: number;
  tokens: number;
  sessions: number;
  agents: string[];
  footprints: string[];
}

export interface SessionRow {
  agent: string;
  sessionId: string;
  title: string | null;
  projectKey: string | null;
  startedMs: number | null;
  updatedMs: number | null;
  models: string[];
  tokens: number;
  costUsd: number;
  unpricedTokens: number;
  tokenStatus: string;
}

export interface Overview {
  range: PeriodRange;
  totals: Totals;
  previous: Totals;
  changePct: number | null;
  daily: DailyPoint[];
  topProjects: ProjectRow[];
  topSessions: SessionRow[];
}

export type Granularity = "week" | "month";
export type Split = "agent" | "model" | "project";

export interface TrendBucket {
  label: string;
  startMs: number;
  totals: Totals;
  by: [string, number][];
}

export interface EventRow {
  tsMs: number;
  model: string;
  input: number;
  output: number;
  reasoning: number;
  cacheRead: number;
  cacheWrite: number;
  costUsd: number | null;
  priceStatus: string;
  cwd: string | null;
}

export interface SessionDetail { row: SessionRow; events: EventRow[] }

export interface ModelRow {
  providerHint: string;
  model: string;
  status: string;
  openrouterId: string | null;
  prompt: number | null;
  completion: number | null;
  totals: Totals;
}

export interface SessionFilter {
  period?: PeriodSpec | null;
  agent?: string | null;
  projectKey?: string | null;
}

export interface RunningAgent {
  agent: string;
  pid: number;
  cwd: string | null;
  startedMs: number;
  sessionHint: string | null;
}

export interface AgentRow {
  agent: string;
  label: string;
  detected: boolean;
  dataRoot: string;
  level: "full" | "sessions" | "presence";
  sessions: number;
  firstMs: number | null;
  lastMs: number | null;
  running: RunningAgent[];
}

export interface Progress { agent: string; done: number; total: number }

export interface PricingStatus {
  fetchedMs: number | null;
  lastError: string | null;
  catalogSize: number;
  unknownModels: [string, string][];
}

export interface ModelOverride {
  providerHint: string;
  rawModel: string;
  action: string;
  openrouterId: string | null;
  prompt: number | null;
  completion: number | null;
  cacheRead: number | null;
  cacheWrite: number | null;
}

export interface AppStatus {
  firstRun: boolean;
  gapDays: number | null;
  retentionWarning: string | null;
  progress: Progress | null;
  pricing: PricingStatus;
  discoveryRunning: boolean;
  dataSinceMs: number | null;
}

export interface CandidateRow {
  path: string;
  guessedAgent: string | null;
  knownFormat: string | null;
  fileCount: number;
  /** "pending" | "track" | "ignore" */
  decision: string;
  foundMs: number;
}

export interface DiscoveryView {
  candidates: CandidateRow[];
  footprintsCount: number;
  denied: string[];
}
