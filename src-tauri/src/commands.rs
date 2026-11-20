//! Thin `#[tauri::command]` wrappers over [`AppState`] methods; errors become strings.
use crate::period::PeriodSpec;
use crate::pricing::openrouter::fetch_models;
use crate::pricing::{ModelOverride, PricingStatus};
use crate::query::{
    Granularity, ModelRow, Overview, ProjectRow, SessionDetail, SessionRow, Split, TrendBucket,
};
use crate::services::{self, emit_updated};
use crate::settings::Settings;
use crate::state::{DiscoveryView, SessionFilterArg};
use crate::{AgentRow, AppState, AppStatus};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

type CmdResult<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

/// Runs blocking command logic on Tauri's blocking thread pool, off the async workers
/// (where e.g. `reqwest::blocking` panics). Panics and errors both become an error string.
pub async fn run_blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("command task failed: {e}"))?
        .map_err(err)
}

/// [`run_blocking`] with the managed [`AppState`] and the app handle.
async fn with_state<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&AppState, &AppHandle) -> anyhow::Result<T> + Send + 'static,
) -> CmdResult<T> {
    run_blocking(move || f(&app.state::<AppState>(), &app)).await
}

/// Warning shown when the app was not running for more than 20 days: Claude Code deletes
/// session files older than ~30 days, so part of the history may be gone.
pub fn retention_warning(gap_days: f64) -> Option<String> {
    (gap_days > 20.0).then(|| {
        format!(
            "Aplikasi tidak berjalan {} hari. Sesi Claude Code yang lebih tua dari 30 hari mungkin sudah terhapus sebelum sempat tercatat.",
            gap_days.floor() as i64
        )
    })
}

#[tauri::command]
pub async fn get_overview(app: AppHandle, period: PeriodSpec) -> CmdResult<Overview> {
    with_state(app, move |s, _| s.overview(&period)).await
}

#[tauri::command]
pub async fn get_trends(
    app: AppHandle,
    granularity: Granularity,
    count: u32,
    split: Split,
) -> CmdResult<Vec<TrendBucket>> {
    with_state(app, move |s, _| s.trends(granularity, count, split)).await
}

#[tauri::command]
pub async fn list_projects(app: AppHandle, period: PeriodSpec) -> CmdResult<Vec<ProjectRow>> {
    with_state(app, move |s, _| s.projects(&period)).await
}

#[tauri::command]
pub async fn list_sessions(app: AppHandle, filter: SessionFilterArg) -> CmdResult<Vec<SessionRow>> {
    with_state(app, move |s, _| s.sessions(&filter)).await
}

#[tauri::command]
pub async fn get_session(
    app: AppHandle,
    agent: String,
    session_id: String,
) -> CmdResult<Option<SessionDetail>> {
    with_state(app, move |s, _| s.session(&agent, &session_id)).await
}

#[tauri::command]
pub async fn list_models(app: AppHandle, period: PeriodSpec) -> CmdResult<Vec<ModelRow>> {
    with_state(app, move |s, _| s.models(&period)).await
}

#[tauri::command]
pub async fn list_agents(app: AppHandle) -> CmdResult<Vec<AgentRow>> {
    with_state(app, |s, _| s.agents()).await
}

#[tauri::command]
pub async fn get_app_status(app: AppHandle) -> CmdResult<AppStatus> {
    with_state(app, |s, _| s.status()).await
}

#[tauri::command]
pub async fn refresh_prices(app: AppHandle) -> CmdResult<PricingStatus> {
    with_state(app, |s, app| {
        let status = s.refresh_prices(&fetch_models);
        emit_updated(app);
        Ok(status)
    })
    .await
}

#[tauri::command]
pub async fn set_model_override(app: AppHandle, o: ModelOverride) -> CmdResult<()> {
    with_state(app, move |s, app| {
        s.set_model_override(&o)?;
        emit_updated(app);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn list_price_catalog(app: AppHandle) -> CmdResult<Vec<String>> {
    with_state(app, |s, _| s.price_catalog()).await
}

#[tauri::command]
pub async fn get_settings(app: AppHandle) -> CmdResult<Settings> {
    with_state(app, |s, _| Ok(s.settings())).await
}

#[tauri::command]
pub async fn update_settings(app: AppHandle, s: Settings) -> CmdResult<()> {
    with_state(app, move |state, app| {
        state.update_settings(&s)?;
        sync_autostart(app, s.launch_at_login);
        emit_updated(app);
        Ok(())
    })
    .await
}

/// Makes the login item match `enabled`; failures are logged, not fatal.
pub fn sync_autostart(app: &AppHandle, enabled: bool) {
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    if let Err(e) = result {
        eprintln!("autostart (enabled={enabled}): {e}");
    }
}

/// Marks discovery as running before returning (so an immediate `get_app_status` sees it),
/// then scans in the background. No-op while a run is in progress.
#[tauri::command]
pub async fn start_discovery(app: AppHandle) -> CmdResult<()> {
    with_state(app, |s, app| {
        if s.begin_discovery() {
            services::spawn_begun_discovery(app);
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn cancel_discovery(app: AppHandle) -> CmdResult<()> {
    with_state(app, |s, _| {
        s.cancel_discovery();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn get_discovery(app: AppHandle) -> CmdResult<DiscoveryView> {
    with_state(app, |s, _| s.discovery()).await
}

#[tauri::command]
pub async fn set_candidate_decision(
    app: AppHandle,
    path: String,
    decision: String,
) -> CmdResult<()> {
    with_state(app, move |s, app| {
        let result = s.set_candidate_decision(&path, &decision);
        // The adapter list may have changed even if the follow-up ingest failed.
        services::rewatch(app);
        emit_updated(app);
        result.map(|_| ())
    })
    .await
}

#[tauri::command]
pub async fn export_csv(
    app: AppHandle,
    kind: String,
    period: PeriodSpec,
    path: String,
) -> CmdResult<()> {
    with_state(app, move |s, _| {
        s.export_csv(&kind, &period, std::path::Path::new(&path))
    })
    .await
}

#[tauri::command]
pub async fn wipe_all_data(app: AppHandle) -> CmdResult<()> {
    with_state(app, |s, app| {
        s.wipe_all()?;
        services::rewatch(app);
        emit_updated(app);
        services::spawn_rebuild(app);
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_warning_after_20_days_gap() {
        assert_eq!(retention_warning(3.0), None);
        assert_eq!(retention_warning(20.0), None);
        assert_eq!(
            retention_warning(21.7).as_deref(),
            Some(
                "Aplikasi tidak berjalan 21 hari. Sesi Claude Code yang lebih tua dari 30 hari mungkin sudah terhapus sebelum sempat tercatat."
            )
        );
    }
    #[test]
    fn run_blocking_allows_blocking_http_client() {
        let r = tauri::async_runtime::block_on(run_blocking(|| {
            drop(reqwest::blocking::Client::new());
            Ok(42)
        }));
        assert_eq!(r, Ok(42));
    }

    #[test]
    fn run_blocking_maps_errors_and_panics() {
        let e = tauri::async_runtime::block_on(run_blocking(|| -> anyhow::Result<()> {
            anyhow::bail!("nope")
        }));
        assert_eq!(e, Err("nope".to_string()));
        let p = tauri::async_runtime::block_on(run_blocking(|| -> anyhow::Result<()> {
            panic!("boom")
        }));
        assert!(p.is_err());
    }
}
