//! Command logic as plain methods on [`AppState`], testable without a Tauri runtime.
use crate::adapters::{Adapter, default_adapters};
use crate::commands::retention_warning;
use crate::db::Db;
use crate::discovery;
use crate::export;
use crate::ingest::{self, IngestReport};
use crate::model::AgentKind;
use crate::period::{self, PeriodSpec};
use crate::pricing::{self, ModelOverride, PricingStatus, openrouter::OrModel};
use crate::process;
use crate::query::{
    self, Granularity, ModelRow, Overview, ProjectRow, SessionDetail, SessionFilter, SessionRow, Split, TrendBucket,
};
use crate::settings::Settings;
use crate::{AgentRow, AppState, AppStatus};
use anyhow::{Result, bail};
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

const DAY_MS: i64 = 86_400_000;
const MAX_CUSTOM_DAYS: i64 = 1098;
/// 9999-12-31T23:59:59.999Z; keeps custom ranges inside what chrono can represent.
const MAX_MS: i64 = 253_402_300_799_999;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFilterArg {
    pub period: Option<PeriodSpec>,
    pub agent: Option<String>,
    pub project_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateRow {
    pub path: String,
    pub guessed_agent: Option<String>,
    pub known_format: Option<String>,
    pub file_count: i64,
    /// "pending" | "track" | "ignore"
    pub decision: String,
    pub found_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryView {
    pub candidates: Vec<CandidateRow>,
    pub footprints_count: i64,
    pub denied: Vec<String>,
}

fn now() -> DateTime<FixedOffset> {
    chrono::Local::now().fixed_offset()
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// Poisoned locks are recovered: a panic elsewhere must not take every command down with it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn read<T>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(|e| e.into_inner())
}

fn write<T>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(|e| e.into_inner())
}

/// Rejects custom periods the queries cannot handle (empty/inverted, too long, out of range).
fn check_period(spec: &PeriodSpec) -> Result<()> {
    if let PeriodSpec::Custom { start_ms, end_ms } = *spec {
        if end_ms <= start_ms {
            bail!("Periode tidak valid: akhir harus setelah awal");
        }
        if start_ms < 0 || end_ms > MAX_MS {
            bail!("Periode tidak valid: tanggal di luar jangkauan");
        }
        if end_ms - start_ms > MAX_CUSTOM_DAYS * DAY_MS {
            bail!("Periode custom maksimal {MAX_CUSTOM_DAYS} hari");
        }
    }
    Ok(())
}

fn all_adapters(home: &Path, db: &Db) -> Result<Vec<Box<dyn Adapter>>> {
    let mut v = default_adapters(home);
    v.extend(discovery::tracked_adapters(db)?);
    Ok(v)
}

fn level(kind: AgentKind, has_recorded: bool) -> &'static str {
    match kind {
        AgentKind::ClaudeCode | AgentKind::Codex | AgentKind::OpenCode => "full",
        AgentKind::GeminiCli if has_recorded => "full",
        AgentKind::GeminiCli | AgentKind::CopilotCli => "sessions",
        AgentKind::ClaudeDesktop | AgentKind::VsCode => "presence",
    }
}

impl AppState {
    /// `$HOME/Library/Application Support/AI Token Tracker/tracker.db`
    pub fn db_path(home: &Path) -> PathBuf {
        home.join("Library/Application Support/AI Token Tracker/tracker.db")
    }

    /// Opens the database, saves default settings on first run and records the heartbeat
    /// gap since the previous launch.
    pub fn open(home: PathBuf, db_path: &Path) -> Result<AppState> {
        let db = Db::open(db_path)?;
        let first_run = Settings::stored(&db)?.is_none();
        if first_run {
            Settings::defaults(&home).save(&db)?;
        }
        let gap_days = db
            .get_meta("last_seen_ms")?
            .and_then(|v| v.parse::<i64>().ok())
            .map(|last| (now_ms() - last).max(0) as f64 / DAY_MS as f64);
        let adapters = all_adapters(&home, &db)?;
        let state = AppState {
            db: Arc::new(Mutex::new(db)),
            home,
            adapters: RwLock::new(adapters),
            running: RwLock::new(Vec::new()),
            progress: RwLock::new(None),
            discovery_cancel: Default::default(),
            discovery_running: Default::default(),
            ingest_lock: Mutex::new(()),
            first_run,
            gap_days,
        };
        state.heartbeat()?;
        Ok(state)
    }

    fn db(&self) -> MutexGuard<'_, Db> {
        lock(&self.db)
    }

    /// Writes `last_seen_ms = now`.
    pub fn heartbeat(&self) -> Result<()> {
        self.db().set_meta("last_seen_ms", &now_ms().to_string())
    }

    /// Ingests the adapters accepted by `pick` under the ingest lock, publishing progress,
    /// and resolves prices for any newly seen model.
    fn ingest(&self, pick: impl Fn(usize, &dyn Adapter) -> bool) -> Result<IngestReport> {
        let _serial = lock(&self.ingest_lock);
        let adapters = read(&self.adapters);
        let on_progress = |p| *write(&self.progress) = Some(p);
        let mut total = IngestReport::default();
        let mut result = Ok(());
        for (_, a) in adapters.iter().enumerate().filter(|(i, a)| pick(*i, a.as_ref())) {
            match ingest::ingest_adapter(&self.db, a.as_ref(), &on_progress) {
                Ok(r) => {
                    total.sources_read += r.sources_read;
                    total.events += r.events;
                    total.sessions += r.sessions;
                    total.errors += r.errors;
                }
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        *write(&self.progress) = None;
        result?;
        if total.events > 0 {
            pricing::rebuild_resolved(&self.db())?;
        }
        Ok(total)
    }

    /// Ingests every adapter (startup / after wipe), publishing progress while it runs.
    pub fn ingest_all(&self) -> Result<IngestReport> {
        self.ingest(|_, _| true)
    }

    /// Ingests the adapters of one agent kind (watcher-triggered).
    pub fn ingest_kind(&self, kind: AgentKind) -> Result<IngestReport> {
        self.ingest(|_, a| a.kind() == kind)
    }

    pub fn overview(&self, spec: &PeriodSpec) -> Result<Overview> {
        check_period(spec)?;
        query::overview(&self.db(), spec, now())
    }

    pub fn trends(&self, g: Granularity, count: u32, split: Split) -> Result<Vec<TrendBucket>> {
        if !(1..=60).contains(&count) {
            bail!("Jumlah periode harus 1–60");
        }
        query::trends(&self.db(), g, count, split, now())
    }

    pub fn projects(&self, spec: &PeriodSpec) -> Result<Vec<ProjectRow>> {
        check_period(spec)?;
        query::projects(&self.db(), &period::resolve(spec, now()))
    }

    pub fn sessions(&self, f: &SessionFilterArg) -> Result<Vec<SessionRow>> {
        if let Some(p) = &f.period {
            check_period(p)?;
        }
        let filter = SessionFilter {
            range: f.period.as_ref().map(|p| period::resolve(p, now())),
            agent: f.agent.clone(),
            project_key: f.project_key.clone(),
        };
        query::sessions(&self.db(), &filter)
    }

    pub fn session(&self, agent: &str, session_id: &str) -> Result<Option<SessionDetail>> {
        query::session_detail(&self.db(), agent, session_id)
    }

    pub fn models(&self, spec: &PeriodSpec) -> Result<Vec<ModelRow>> {
        check_period(spec)?;
        query::models(&self.db(), &period::resolve(spec, now()))
    }

    pub fn agents(&self) -> Result<Vec<AgentRow>> {
        let db = self.db();
        let running = read(&self.running);
        let mut stats = db.conn.prepare(
            "SELECT COUNT(*), MIN(started_ms), MAX(updated_ms), COALESCE(SUM(token_status = 'recorded'), 0) > 0
             FROM sessions WHERE agent = ?1",
        )?;
        let mut out = Vec::new();
        for a in default_adapters(&self.home) {
            let kind = a.kind();
            let (sessions, first_ms, last_ms, has_recorded): (i64, Option<i64>, Option<i64>, bool) =
                stats.query_row([kind.as_str()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            out.push(AgentRow {
                agent: kind.as_str().to_string(),
                label: kind.label().to_string(),
                detected: a.detect(),
                data_root: a.root().to_string_lossy().into_owned(),
                level: level(kind, has_recorded).to_string(),
                sessions,
                first_ms,
                last_ms,
                running: running.iter().filter(|r| r.agent == kind).cloned().collect(),
            });
        }
        Ok(out)
    }

    pub fn status(&self) -> Result<AppStatus> {
        let db = self.db();
        Ok(AppStatus {
            first_run: self.first_run,
            gap_days: self.gap_days,
            retention_warning: self.gap_days.and_then(retention_warning),
            progress: read(&self.progress).clone(),
            pricing: pricing::status(&db)?,
            discovery_running: self.discovery_running.load(Ordering::SeqCst),
            data_since_ms: db.conn.query_row("SELECT MIN(ts_ms) FROM events", [], |r| r.get(0))?,
        })
    }

    pub fn refresh_prices(&self, fetch: &dyn Fn() -> Result<Vec<OrModel>>) -> PricingStatus {
        pricing::refresh_prices(&self.db, fetch)
    }

    pub fn set_model_override(&self, o: &ModelOverride) -> Result<()> {
        if !matches!(o.action.as_str(), "map" | "ignore" | "manual" | "clear") {
            bail!("Aksi override tidak dikenal: {}", o.action);
        }
        pricing::set_override(&self.db(), o)
    }

    pub fn price_catalog(&self) -> Result<Vec<String>> {
        let db = self.db();
        let mut st = db.conn.prepare("SELECT openrouter_id FROM prices ORDER BY openrouter_id")?;
        let ids = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    pub fn settings(&self) -> Settings {
        Settings::load(&self.db(), &self.home)
    }

    pub fn update_settings(&self, s: &Settings) -> Result<()> {
        if s.process_poll_secs == 0 {
            bail!("Interval poll proses minimal 1 detik");
        }
        if s.idr_rate.is_some_and(|r| !r.is_finite() || r <= 0.0) {
            bail!("Kurs IDR harus angka positif");
        }
        s.save(&self.db())
    }

    /// Runs discovery over the configured scan roots. Returns `false` without scanning
    /// when a run is already in progress.
    pub fn run_discovery(&self) -> Result<bool> {
        if self.discovery_running.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Ok(false);
        }
        self.discovery_cancel.store(false, Ordering::SeqCst);
        let roots: Vec<PathBuf> = self.settings().scan_roots.iter().map(PathBuf::from).collect();
        let known: Vec<PathBuf> = read(&self.adapters).iter().map(|a| a.root().to_path_buf()).collect();
        let result = discovery::run_discovery(&self.db, &roots, &self.home, &known, &self.discovery_cancel);
        self.discovery_running.store(false, Ordering::SeqCst);
        result.map(|_| true)
    }

    pub fn cancel_discovery(&self) {
        self.discovery_cancel.store(true, Ordering::SeqCst);
    }

    pub fn discovery(&self) -> Result<DiscoveryView> {
        let db = self.db();
        let mut st = db.conn.prepare(
            "SELECT path, guessed_agent, known_format, file_count, decision, found_ms FROM discovery_candidates ORDER BY path",
        )?;
        let candidates = st
            .query_map([], |r| {
                Ok(CandidateRow {
                    path: r.get(0)?,
                    guessed_agent: r.get(1)?,
                    known_format: r.get(2)?,
                    file_count: r.get(3)?,
                    decision: r.get(4)?,
                    found_ms: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let footprints_count = db.conn.query_row("SELECT COUNT(*) FROM project_footprints", [], |r| r.get(0))?;
        let mut st = db.conn.prepare("SELECT path FROM discovery_denied ORDER BY path")?;
        let denied = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(DiscoveryView { candidates, footprints_count, denied })
    }

    /// Stores the decision, rebuilds the adapter list and, for "track", ingests the new source.
    pub fn set_candidate_decision(&self, path: &str, decision: &str) -> Result<IngestReport> {
        if !matches!(decision, "track" | "ignore") {
            bail!("Keputusan tidak dikenal: {decision}");
        }
        let adapters = {
            let db = self.db();
            let n = db.conn.execute(
                "UPDATE discovery_candidates SET decision = ?2 WHERE path = ?1",
                rusqlite::params![path, decision],
            )?;
            if n == 0 {
                bail!("Kandidat tidak ditemukan: {path}");
            }
            all_adapters(&self.home, &db)?
        };
        *write(&self.adapters) = adapters;
        if decision != "track" {
            return Ok(IngestReport::default());
        }
        // Only tracked entries (after the defaults) can be this candidate.
        let n_default = default_adapters(&self.home).len();
        self.ingest(|i, a| i >= n_default && a.root() == Path::new(path))
    }

    /// Writes a CSV ("sessions" | "daily") for the period to `path`, which must not lie
    /// inside any agent data root.
    pub fn export_csv(&self, kind: &str, spec: &PeriodSpec, path: &Path) -> Result<()> {
        check_period(spec)?;
        self.check_export_path(path)?;
        let range = period::resolve(spec, now());
        let csv = match kind {
            "sessions" => {
                let f = SessionFilter { range: Some(range), ..Default::default() };
                export::sessions_csv(&query::sessions(&self.db(), &f)?)
            }
            "daily" => export::daily_csv(&query::daily(&self.db(), &range, now())?),
            _ => bail!("Jenis ekspor tidak dikenal: {kind}"),
        };
        std::fs::write(path, csv)?;
        Ok(())
    }

    /// Agent data is read-only: refuse any export target inside an agent data root.
    fn check_export_path(&self, path: &Path) -> Result<()> {
        if !path.is_absolute() {
            bail!("Lokasi ekspor harus path absolut");
        }
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            bail!("Lokasi ekspor tidak valid");
        };
        let target = std::fs::canonicalize(parent)?.join(name);
        // An existing target may be a symlink into an agent root: check where it really points.
        let target = std::fs::canonicalize(&target).unwrap_or(target);
        for a in read(&self.adapters).iter() {
            let root = std::fs::canonicalize(a.root()).unwrap_or_else(|_| a.root().to_path_buf());
            if target.starts_with(&root) {
                bail!("Tidak boleh menulis ke folder data agent: {}", root.display());
            }
        }
        Ok(())
    }

    /// Deletes all recorded data (settings and model overrides are kept).
    pub fn wipe_all(&self) -> Result<()> {
        let _serial = lock(&self.ingest_lock);
        let mut db = self.db();
        db.wipe_all()?;
        db.set_meta("last_seen_ms", &now_ms().to_string())
    }

    /// Refreshes the running-agents snapshot; returns true when the set changed.
    pub fn poll_processes(&self, sys: &mut sysinfo::System) -> Result<bool> {
        let mut now_running = process::snapshot(sys);
        process::attach_session_hints(&self.db(), &mut now_running)?;
        let mut running = write(&self.running);
        if *running == now_running {
            return Ok(false);
        }
        *running = now_running;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::RunningAgent;
    use std::fs;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
    }

    fn copy_dir(from: &Path, to: &Path) {
        for entry in walkdir::WalkDir::new(from) {
            let entry = entry.unwrap();
            let dest = to.join(entry.path().strip_prefix(from).unwrap());
            if entry.file_type().is_dir() {
                fs::create_dir_all(&dest).unwrap();
            } else {
                fs::copy(entry.path(), &dest).unwrap();
            }
        }
    }

    /// Temp home holding the Claude fixture at `~/.claude`, plus a state opened on it.
    fn setup() -> (tempfile::TempDir, AppState) {
        let t = tempfile::tempdir().unwrap();
        copy_dir(&fixture("claude"), &t.path().join(".claude"));
        let s = open(t.path());
        (t, s)
    }

    fn open(home: &Path) -> AppState {
        AppState::open(home.to_path_buf(), &AppState::db_path(home)).unwrap()
    }

    fn ms(s: &str) -> i64 {
        DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
    }

    /// Custom period covering the fixture date (2026-11-20).
    fn november() -> PeriodSpec {
        PeriodSpec::Custom { start_ms: ms("2026-11-01T00:00:00Z"), end_ms: ms("2026-12-01T00:00:00Z") }
    }

    fn count(s: &AppState, sql: &str) -> i64 {
        s.db.lock().unwrap().conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn db_path_is_under_application_support() {
        assert_eq!(
            AppState::db_path(Path::new("/Users/x")),
            PathBuf::from("/Users/x/Library/Application Support/AI Token Tracker/tracker.db")
        );
    }

    #[test]
    fn startup_ingest_shows_claude_sessions_and_overview() {
        let (_t, s) = setup();
        let r = s.ingest_all().unwrap();
        assert!(r.events > 0);
        assert!(s.progress.read().unwrap().is_none(), "progress is cleared after ingest");
        let agents = s.agents().unwrap();
        let claude = agents.iter().find(|a| a.agent == "claude-code").unwrap();
        assert!(claude.detected);
        assert!(claude.sessions > 0);
        assert!(claude.first_ms.is_some() && claude.last_ms.is_some());
        let o = s.overview(&november()).unwrap();
        assert!(o.totals.events > 0);
        assert!(s.status().unwrap().data_since_ms.is_some());
    }

    #[test]
    fn ingest_resolves_prices_for_new_models() {
        let (_t, s) = setup();
        s.ingest_all().unwrap();
        // Every event model has a resolved_prices row right after ingest (status unknown
        // without a catalog), so it shows as unpriced instead of disappearing.
        assert_eq!(
            count(&s, "SELECT COUNT(*) FROM (SELECT DISTINCT provider_hint, model FROM events)"),
            count(&s, "SELECT COUNT(*) FROM resolved_prices")
        );
    }

    #[test]
    fn first_run_saves_defaults_and_reopen_reports_gap() {
        let t = tempfile::tempdir().unwrap();
        let s = open(t.path());
        assert!(s.first_run);
        assert_eq!(s.gap_days, None);
        assert_eq!(s.settings(), Settings::defaults(t.path()));
        let st = s.status().unwrap();
        assert!(st.first_run);
        assert_eq!(st.retention_warning, None);
        assert_eq!(st.data_since_ms, None);
        let last_seen = chrono::Utc::now().timestamp_millis() - 25 * DAY_MS;
        s.db.lock().unwrap().set_meta("last_seen_ms", &last_seen.to_string()).unwrap();
        drop(s);

        let s = open(t.path());
        assert!(!s.first_run);
        let gap = s.gap_days.unwrap();
        assert!((24.9..25.1).contains(&gap), "{gap}");
        assert!(s.status().unwrap().retention_warning.unwrap().contains("25 hari"));
        let seen: i64 = s.db.lock().unwrap().get_meta("last_seen_ms").unwrap().unwrap().parse().unwrap();
        assert!(seen > last_seen + 24 * DAY_MS, "open refreshes the heartbeat");
    }

    #[test]
    fn agent_rows_follow_default_order_and_levels() {
        let (_t, s) = setup();
        s.ingest_all().unwrap();
        let level = |s: &AppState, agent: &str| s.agents().unwrap().into_iter().find(|a| a.agent == agent).unwrap().level;
        let rows = s.agents().unwrap();
        let order: Vec<&str> = rows.iter().map(|r| r.agent.as_str()).collect();
        assert_eq!(order, ["claude-code", "codex", "opencode", "gemini-cli", "copilot-cli", "claude-desktop", "vs-code"]);
        assert_eq!(rows[0].label, "Claude Code");
        assert!(!rows[1].detected);
        assert_eq!(level(&s, "claude-code"), "full");
        assert_eq!(level(&s, "codex"), "full");
        assert_eq!(level(&s, "gemini-cli"), "sessions");
        assert_eq!(level(&s, "copilot-cli"), "sessions");
        assert_eq!(level(&s, "vs-code"), "presence");
        assert_eq!(level(&s, "claude-desktop"), "presence");
        s.db.lock().unwrap().conn
            .execute("INSERT INTO sessions(agent, session_id, token_status) VALUES ('gemini-cli', 'g', 'recorded')", [])
            .unwrap();
        assert_eq!(level(&s, "gemini-cli"), "full");
        *s.running.write().unwrap() = vec![RunningAgent {
            agent: AgentKind::Codex, pid: 7, cwd: None, started_ms: 0, session_hint: None,
        }];
        let rows = s.agents().unwrap();
        assert_eq!(rows[1].running.len(), 1);
        assert!(rows[0].running.is_empty());
    }

    #[test]
    fn invalid_periods_and_counts_are_rejected() {
        let (t, s) = setup();
        assert!(s.trends(Granularity::Week, 0, Split::Agent).is_err());
        assert!(s.trends(Granularity::Week, 61, Split::Agent).is_err());
        assert_eq!(s.trends(Granularity::Month, 60, Split::Agent).unwrap().len(), 60);
        let bad = [
            PeriodSpec::Custom { start_ms: 10, end_ms: 10 },
            PeriodSpec::Custom { start_ms: 10, end_ms: 5 },
            PeriodSpec::Custom { start_ms: 0, end_ms: 1099 * DAY_MS },
            PeriodSpec::Custom { start_ms: i64::MIN, end_ms: i64::MIN + DAY_MS },
            PeriodSpec::Custom { start_ms: i64::MAX - DAY_MS, end_ms: i64::MAX },
        ];
        let out = t.path().join("out.csv");
        for p in &bad {
            assert!(s.overview(p).is_err(), "{p:?}");
            assert!(s.projects(p).is_err(), "{p:?}");
            assert!(s.models(p).is_err(), "{p:?}");
            assert!(s.sessions(&SessionFilterArg { period: Some(p.clone()), ..Default::default() }).is_err(), "{p:?}");
            assert!(s.export_csv("daily", p, &out).is_err(), "{p:?}");
        }
        assert!(s.overview(&PeriodSpec::Custom { start_ms: 0, end_ms: 1098 * DAY_MS }).is_ok());
        assert!(s.sessions(&SessionFilterArg::default()).is_ok());
    }

    #[test]
    fn sessions_filter_and_detail() {
        let (_t, s) = setup();
        s.ingest_all().unwrap();
        let all = s.sessions(&SessionFilterArg { period: Some(november()), agent: Some("claude-code".into()), project_key: None }).unwrap();
        assert!(!all.is_empty());
        assert!(s.sessions(&SessionFilterArg { agent: Some("codex".into()), ..Default::default() }).unwrap().is_empty());
        let d = s.session("claude-code", &all[0].session_id).unwrap().unwrap();
        assert!(!d.events.is_empty());
        assert!(s.session("claude-code", "nope").unwrap().is_none());
    }

    #[test]
    fn export_writes_csv_and_refuses_agent_roots() {
        let (t, s) = setup();
        s.ingest_all().unwrap();
        let out_dir = tempfile::tempdir().unwrap();
        let out = out_dir.path().join("sessions.csv");
        s.export_csv("sessions", &november(), &out).unwrap();
        let csv = fs::read_to_string(&out).unwrap();
        assert!(csv.starts_with("agent,session_id,title,project,started,updated,models,tokens,cost_usd,unpriced_tokens\n"));
        assert!(csv.lines().count() > 1);
        let daily = out_dir.path().join("daily.csv");
        s.export_csv("daily", &november(), &daily).unwrap();
        assert!(fs::read_to_string(&daily).unwrap().lines().nth(1).unwrap().contains(",claude-code,"));

        let inside = t.path().join(".claude/projects/export.csv");
        assert!(s.export_csv("sessions", &november(), &inside).is_err());
        assert!(!inside.exists());
        fs::create_dir_all(t.path().join("Library")).unwrap();
        let sneaky = t.path().join("Library/../.claude/x.csv");
        assert!(s.export_csv("sessions", &november(), &sneaky).is_err());
        assert!(s.export_csv("weekly", &november(), &out).is_err());
        assert!(s.export_csv("sessions", &november(), Path::new("relative.csv")).is_err());
    }

    #[test]
    fn export_refuses_symlink_into_agent_root() {
        let (t, s) = setup();
        let out_dir = tempfile::tempdir().unwrap();
        let link = out_dir.path().join("link.csv");
        std::os::unix::fs::symlink(t.path().join(".claude/projects/-tmp-demo/sess-1.jsonl"), &link).unwrap();
        let before = fs::read(t.path().join(".claude/projects/-tmp-demo/sess-1.jsonl")).unwrap();
        assert!(s.export_csv("sessions", &november(), &link).is_err());
        assert_eq!(fs::read(t.path().join(".claude/projects/-tmp-demo/sess-1.jsonl")).unwrap(), before);
    }

    #[test]
    fn command_json_shapes_are_camel_case() {
        let f: SessionFilterArg =
            serde_json::from_str(r#"{"period":{"kind":"today"},"agent":null,"projectKey":"/p"}"#).unwrap();
        assert_eq!(f.period, Some(PeriodSpec::Today));
        assert_eq!(f.project_key.as_deref(), Some("/p"));
        let (_t, s) = setup();
        let v = serde_json::to_value(s.discovery().unwrap()).unwrap();
        assert!(v.get("footprintsCount").is_some() && v.get("denied").is_some());
        let st = serde_json::to_value(s.status().unwrap()).unwrap();
        for k in ["firstRun", "gapDays", "retentionWarning", "progress", "pricing", "discoveryRunning", "dataSinceMs"] {
            assert!(st.get(k).is_some(), "{k}");
        }
        let a = serde_json::to_value(&s.agents().unwrap()[0]).unwrap();
        for k in ["agent", "label", "detected", "dataRoot", "level", "sessions", "firstMs", "lastMs", "running"] {
            assert!(a.get(k).is_some(), "{k}");
        }
    }

    #[test]
    fn tracking_a_candidate_adds_and_ingests_its_adapter() {
        let (t, s) = setup();
        let alt = t.path().join("alt-claude");
        copy_dir(&fixture("claude"), &alt);
        let path = alt.to_string_lossy().into_owned();
        s.db.lock().unwrap().conn
            .execute(
                "INSERT INTO discovery_candidates(path, guessed_agent, known_format, file_count, found_ms) VALUES (?1, 'claude-code', 'claude-code', 2, 1)",
                [&path],
            )
            .unwrap();
        assert!(s.set_candidate_decision(&path, "maybe").is_err());
        assert!(s.set_candidate_decision("/not/a/candidate", "track").is_err());

        let r = s.set_candidate_decision(&path, "track").unwrap();
        assert!(r.sources_read > 0);
        assert_eq!(s.adapters.read().unwrap().len(), 8);
        assert_eq!(s.discovery().unwrap().candidates[0].decision, "track");
        let again = open(t.path());
        assert_eq!(again.adapters.read().unwrap().len(), 8, "tracked adapters are restored on open");
        drop(again);

        s.set_candidate_decision(&path, "ignore").unwrap();
        assert_eq!(s.adapters.read().unwrap().len(), 7);
    }

    #[test]
    fn discovery_runs_once_and_lists_candidates() {
        let (t, s) = setup();
        copy_dir(&fixture("claude"), &t.path().join("alt-claude"));
        s.update_settings(&Settings { scan_roots: vec![t.path().to_string_lossy().into_owned()], ..s.settings() }).unwrap();
        assert!(s.run_discovery().unwrap());
        assert!(!s.status().unwrap().discovery_running);
        let v = s.discovery().unwrap();
        assert_eq!(v.candidates.len(), 1, "{:?}", v.candidates);
        assert!(v.candidates[0].path.ends_with("alt-claude"));
        assert_eq!(v.candidates[0].known_format.as_deref(), Some("claude-code"));
        assert_eq!(v.candidates[0].decision, "pending");
        assert!(v.denied.is_empty());

        s.discovery_running.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(!s.run_discovery().unwrap(), "a second concurrent run is refused");
        s.discovery_running.store(false, std::sync::atomic::Ordering::SeqCst);
        s.cancel_discovery();
        assert!(s.discovery_cancel.load(std::sync::atomic::Ordering::SeqCst));
        assert!(s.run_discovery().unwrap(), "a new run resets the cancel flag");
    }

    #[test]
    fn settings_update_is_validated_and_persisted() {
        let t = tempfile::tempdir().unwrap();
        let s = open(t.path());
        let base = s.settings();
        assert!(s.update_settings(&Settings { process_poll_secs: 0, ..base.clone() }).is_err());
        assert!(s.update_settings(&Settings { idr_rate: Some(-1.0), ..base.clone() }).is_err());
        assert!(s.update_settings(&Settings { idr_rate: Some(f64::NAN), ..base.clone() }).is_err());
        let new = Settings { process_poll_secs: 15, idr_rate: Some(16000.0), ..base };
        s.update_settings(&new).unwrap();
        assert_eq!(s.settings(), new);
        drop(s);
        assert_eq!(open(t.path()).settings(), new);
    }

    #[test]
    fn prices_overrides_and_catalog() {
        let (_t, s) = setup();
        s.ingest_all().unwrap();
        let st = s.refresh_prices(&|| {
            Ok(vec![OrModel { id: "anthropic/claude-opus-5.5".into(), prompt: 5e-6, completion: 25e-6, cache_read: None, cache_write: None }])
        });
        assert_eq!(st.catalog_size, 1);
        assert_eq!(s.price_catalog().unwrap(), vec!["anthropic/claude-opus-5.5".to_string()]);
        let o = ModelOverride {
            provider_hint: "anthropic".into(), raw_model: "x".into(), action: "bogus".into(),
            openrouter_id: None, prompt: None, completion: None, cache_read: None, cache_write: None,
        };
        assert!(s.set_model_override(&o).is_err());
        s.set_model_override(&ModelOverride { action: "ignore".into(), ..o }).unwrap();
        assert_eq!(count(&s, "SELECT COUNT(*) FROM model_overrides WHERE raw_model = 'x'"), 1);
    }

    #[test]
    fn wipe_clears_data_but_keeps_settings_and_heartbeat() {
        let (_t, s) = setup();
        s.ingest_all().unwrap();
        s.update_settings(&Settings { process_poll_secs: 9, ..s.settings() }).unwrap();
        s.wipe_all().unwrap();
        assert_eq!(count(&s, "SELECT COUNT(*) FROM events"), 0);
        assert_eq!(count(&s, "SELECT COUNT(*) FROM source_state"), 0);
        assert_eq!(s.settings().process_poll_secs, 9);
        assert!(s.db.lock().unwrap().get_meta("last_seen_ms").unwrap().is_some());
    }

    #[test]
    fn process_poll_does_not_fail() {
        let (_t, s) = setup();
        let mut sys = sysinfo::System::new();
        s.poll_processes(&mut sys).unwrap();
        s.poll_processes(&mut sys).unwrap();
    }
}
