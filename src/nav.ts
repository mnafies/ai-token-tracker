import { createContext, useContext } from "react";
import type { Settings } from "./types";

export type Page =
  | "dashboard" | "trends" | "projects" | "sessions" | "models" | "agents" | "pricing" | "settings"
  | "sessionDetail";

/** Optional context for the target page, e.g. a session filter or a session to open. */
export interface NavParams {
  projectKey?: string;
  agent?: string;
  sessionId?: string;
}

export interface PageProps {
  /** Bumps on every "data-updated" event; refetch when it changes. */
  refreshKey: number;
  params: NavParams;
  navigate: (page: Page, params?: NavParams) => void;
  /** Return to the page that opened the session detail. */
  goBack?: () => void;
}

export const DEFAULT_SETTINGS: Settings = {
  currency: "usd", idrRate: null, launchAtLogin: true, processPollSecs: 5, scanRoots: [],
};

export const SettingsContext = createContext<Settings>(DEFAULT_SETTINGS);
export const useSettings = () => useContext(SettingsContext);
