import { invoke } from "@tauri-apps/api/core";
import type {
  AgentRow, AppStatus, DiscoveryView, Granularity, ModelOverride, ModelRow, Overview, PeriodSpec,
  PricingStatus, ProjectRow, SessionDetail, SessionFilter, SessionRow, Settings, Split, TrendBucket,
} from "./types";

export const getOverview = (period: PeriodSpec) => invoke<Overview>("get_overview", { period });
export const getTrends = (granularity: Granularity, count: number, split: Split) =>
  invoke<TrendBucket[]>("get_trends", { granularity, count, split });
export const listProjects = (period: PeriodSpec) => invoke<ProjectRow[]>("list_projects", { period });
export const listSessions = (filter: SessionFilter) => invoke<SessionRow[]>("list_sessions", { filter });
export const getSession = (agent: string, sessionId: string) =>
  invoke<SessionDetail | null>("get_session", { agent, sessionId });
export const listModels = (period: PeriodSpec) => invoke<ModelRow[]>("list_models", { period });
export const listAgents = () => invoke<AgentRow[]>("list_agents");
export const getAppStatus = () => invoke<AppStatus>("get_app_status");
export const refreshPrices = () => invoke<PricingStatus>("refresh_prices");
export const setModelOverride = (o: ModelOverride) => invoke<void>("set_model_override", { o });
export const listPriceCatalog = () => invoke<string[]>("list_price_catalog");
export const getSettings = () => invoke<Settings>("get_settings");
export const updateSettings = (s: Settings) => invoke<void>("update_settings", { s });
export const startDiscovery = () => invoke<void>("start_discovery");
export const cancelDiscovery = () => invoke<void>("cancel_discovery");
export const getDiscovery = () => invoke<DiscoveryView>("get_discovery");
export const setCandidateDecision = (path: string, decision: string) =>
  invoke<void>("set_candidate_decision", { path, decision });
export const exportCsv = (kind: string, period: PeriodSpec, path: string) =>
  invoke<void>("export_csv", { kind, period, path });
export const wipeAllData = () => invoke<void>("wipe_all_data");
