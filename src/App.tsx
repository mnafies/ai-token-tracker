import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getSettings } from "./api";
import ErrorBox from "./components/ErrorBox";
import { DEFAULT_SETTINGS, SettingsContext } from "./nav";
import type { NavParams, Page } from "./nav";
import Agents from "./pages/Agents";
import Dashboard from "./pages/Dashboard";
import Models from "./pages/Models";
import Pricing from "./pages/Pricing";
import Projects from "./pages/Projects";
import SessionDetail from "./pages/SessionDetail";
import Sessions from "./pages/Sessions";
import SettingsPage from "./pages/Settings";
import Trends from "./pages/Trends";
import type { Settings } from "./types";

const NAV: { page: Page; label: string }[] = [
  { page: "dashboard", label: "Dashboard" },
  { page: "trends", label: "Mingguan & Bulanan" },
  { page: "projects", label: "Project" },
  { page: "sessions", label: "Sesi" },
  { page: "models", label: "Model" },
  { page: "agents", label: "Agent" },
  { page: "pricing", label: "Harga" },
  { page: "settings", label: "Pengaturan" },
];

const PAGES = {
  dashboard: Dashboard, trends: Trends, projects: Projects, sessions: Sessions,
  models: Models, agents: Agents, pricing: Pricing, settings: SettingsPage, sessionDetail: SessionDetail,
} as const;

export default function App() {
  const [nav, setNav] = useState<{ page: Page; params: NavParams }>({ page: "dashboard", params: {} });
  const [prev, setPrev] = useState<{ page: Page; params: NavParams } | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [settingsError, setSettingsError] = useState<string | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen("data-updated", () => setRefreshKey((k) => k + 1))
      .then((fn) => { if (disposed) fn(); else unlisten = fn; })
      .catch((e) => console.error("listen data-updated:", e));
    return () => { disposed = true; unlisten?.(); };
  }, []);

  // Settings load once and again after every data-updated (update_settings emits it).
  useEffect(() => {
    let cancelled = false;
    getSettings().then(
      (s) => { if (!cancelled) { setSettings(s); setSettingsError(null); } },
      (e) => { if (!cancelled) setSettingsError(`Gagal memuat pengaturan: ${e}`); },
    );
    return () => { cancelled = true; };
  }, [refreshKey]);

  const navigate = useCallback((page: Page, params: NavParams = {}) => {
    // Remember where we came from so the session detail can go back.
    if (page === "sessionDetail" && nav.page !== "sessionDetail") setPrev(nav);
    setNav({ page, params });
  }, [nav]);
  const goBack = useCallback(() => {
    const target = prev ?? { page: "sessions" as Page, params: {} };
    setNav(target);
  }, [prev]);
  const Current = PAGES[nav.page];

  return (
    <SettingsContext.Provider value={settings}>
      <div className="app">
        <nav className="sidebar">
          <div className="brand">AI Token Tracker</div>
          {NAV.map((n) => (
            <button key={n.page} className={nav.page === n.page || (nav.page === "sessionDetail" && prev?.page === n.page) ? "active" : ""} onClick={() => navigate(n.page)}>
              {n.label}
            </button>
          ))}
        </nav>
        <main className="content">
          <ErrorBox message={settingsError} />
          <Current refreshKey={refreshKey} params={nav.params} navigate={navigate} goBack={goBack} />
        </main>
      </div>
    </SettingsContext.Provider>
  );
}
