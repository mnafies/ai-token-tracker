//! Thin `#[tauri::command]` wrappers over [`AppState`] methods; errors become strings.
use crate::period::PeriodSpec;
use crate::pricing::openrouter::fetch_models;
use crate::pricing::{ModelOverride, PricingStatus};
use crate::query::{Granularity, ModelRow, Overview, ProjectRow, SessionDetail, SessionRow, Split, TrendBucket};
use crate::services::{self, emit_updated};
use crate::settings::Settings;
use crate::state::{DiscoveryView, SessionFilterArg};
use crate::{AgentRow, AppState, AppStatus};
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

type CmdResult<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
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

#[tauri::command(async)]
pub fn get_overview(state: State<'_, AppState>, period: PeriodSpec) -> CmdResult<Overview> {
    state.overview(&period).map_err(err)
}

#[tauri::command(async)]
pub fn get_trends(state: State<'_, AppState>, granularity: Granularity, count: u32, split: Split) -> CmdResult<Vec<TrendBucket>> {
    state.trends(granularity, count, split).map_err(err)
}

#[tauri::command(async)]
pub fn list_projects(state: State<'_, AppState>, period: PeriodSpec) -> CmdResult<Vec<ProjectRow>> {
    state.projects(&period).map_err(err)
}

#[tauri::command(async)]
pub fn list_sessions(state: State<'_, AppState>, filter: SessionFilterArg) -> CmdResult<Vec<SessionRow>> {
    state.sessions(&filter).map_err(err)
}

#[tauri::command(async)]
pub fn get_session(state: State<'_, AppState>, agent: String, session_id: String) -> CmdResult<Option<SessionDetail>> {
    state.session(&agent, &session_id).map_err(err)
}

#[tauri::command(async)]
pub fn list_models(state: State<'_, AppState>, period: PeriodSpec) -> CmdResult<Vec<ModelRow>> {
    state.models(&period).map_err(err)
}

#[tauri::command(async)]
pub fn list_agents(state: State<'_, AppState>) -> CmdResult<Vec<AgentRow>> {
    state.agents().map_err(err)
}

#[tauri::command(async)]
pub fn get_app_status(state: State<'_, AppState>) -> CmdResult<AppStatus> {
    state.status().map_err(err)
}

#[tauri::command(async)]
pub fn refresh_prices(app: AppHandle, state: State<'_, AppState>) -> CmdResult<PricingStatus> {
    let status = state.refresh_prices(&fetch_models);
    emit_updated(&app);
    Ok(status)
}

#[tauri::command(async)]
pub fn set_model_override(app: AppHandle, state: State<'_, AppState>, o: ModelOverride) -> CmdResult<()> {
    state.set_model_override(&o).map_err(err)?;
    emit_updated(&app);
    Ok(())
}

#[tauri::command(async)]
pub fn list_price_catalog(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    state.price_catalog().map_err(err)
}

#[tauri::command(async)]
pub fn get_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    Ok(state.settings())
}

#[tauri::command(async)]
pub fn update_settings(app: AppHandle, state: State<'_, AppState>, s: Settings) -> CmdResult<()> {
    state.update_settings(&s).map_err(err)?;
    sync_autostart(&app, s.launch_at_login);
    emit_updated(&app);
    Ok(())
}

/// Makes the login item match `enabled`; failures are logged, not fatal.
pub fn sync_autostart(app: &AppHandle, enabled: bool) {
    let launcher = app.autolaunch();
    let result = if enabled { launcher.enable() } else { launcher.disable() };
    if let Err(e) = result {
        eprintln!("autostart (enabled={enabled}): {e}");
    }
}

#[tauri::command(async)]
pub fn start_discovery(app: AppHandle) -> CmdResult<()> {
    services::spawn_discovery(&app);
    Ok(())
}

#[tauri::command(async)]
pub fn cancel_discovery(state: State<'_, AppState>) -> CmdResult<()> {
    state.cancel_discovery();
    Ok(())
}

#[tauri::command(async)]
pub fn get_discovery(state: State<'_, AppState>) -> CmdResult<DiscoveryView> {
    state.discovery().map_err(err)
}

#[tauri::command(async)]
pub fn set_candidate_decision(app: AppHandle, state: State<'_, AppState>, path: String, decision: String) -> CmdResult<()> {
    let result = state.set_candidate_decision(&path, &decision).map_err(err);
    // The adapter list may have changed even if the follow-up ingest failed.
    services::rewatch(&app);
    emit_updated(&app);
    result.map(|_| ())
}

#[tauri::command(async)]
pub fn export_csv(state: State<'_, AppState>, kind: String, period: PeriodSpec, path: String) -> CmdResult<()> {
    state.export_csv(&kind, &period, std::path::Path::new(&path)).map_err(err)
}

#[tauri::command(async)]
pub fn wipe_all_data(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    state.wipe_all().map_err(err)?;
    emit_updated(&app);
    services::spawn_rebuild(&app);
    Ok(())
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
            Some("Aplikasi tidak berjalan 21 hari. Sesi Claude Code yang lebih tua dari 30 hari mungkin sudah terhapus sebelum sempat tercatat.")
        );
    }
}
