pub mod adapters;
pub mod commands;
pub mod db;
pub mod discovery;
pub mod export;
pub mod ingest;
pub mod model;
pub mod period;
pub mod pricing;
pub mod process;
pub mod project;
pub mod query;
pub mod services;
pub mod settings;
pub mod state;
pub mod tray;
pub mod watcher;

use adapters::Adapter;
use db::Db;
use ingest::Progress;
use pricing::PricingStatus;
use process::RunningAgent;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use tauri::{Manager, WindowEvent};

/// Shared state behind every command and background service. Methods live in [`state`].
pub struct AppState {
    pub db: Arc<Mutex<Db>>,
    pub home: PathBuf,
    /// Default adapters followed by the ones tracked from discovery.
    pub adapters: RwLock<Vec<Box<dyn Adapter>>>,
    pub running: RwLock<Vec<RunningAgent>>,
    pub progress: RwLock<Option<Progress>>,
    pub discovery_cancel: Arc<AtomicBool>,
    pub discovery_running: AtomicBool,
    /// Held for the whole duration of any ingest so a source is never parsed concurrently.
    pub ingest_lock: Mutex<()>,
    /// True when this launch created the settings (no settings were saved before).
    pub first_run: bool,
    /// Days since the previous launch's last heartbeat; `None` without a previous heartbeat.
    pub gap_days: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub first_run: bool,
    pub gap_days: Option<f64>,
    pub retention_warning: Option<String>,
    pub progress: Option<Progress>,
    pub pricing: PricingStatus,
    pub discovery_running: bool,
    pub data_since_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRow {
    pub agent: String,
    pub label: String,
    pub detected: bool,
    pub data_root: String,
    /// "full" | "sessions" | "presence"
    pub level: String,
    pub sessions: i64,
    pub first_ms: Option<i64>,
    pub last_ms: Option<i64>,
    pub running: Vec<RunningAgent>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME is not set"));
    let state = AppState::open(home.clone(), &AppState::db_path(&home)).expect("cannot open the tracker database");
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::get_overview,
            commands::get_trends,
            commands::list_projects,
            commands::list_sessions,
            commands::get_session,
            commands::list_models,
            commands::list_agents,
            commands::get_app_status,
            commands::refresh_prices,
            commands::set_model_override,
            commands::list_price_catalog,
            commands::get_settings,
            commands::update_settings,
            commands::start_discovery,
            commands::cancel_discovery,
            commands::get_discovery,
            commands::set_candidate_decision,
            commands::export_csv,
            commands::wipe_all_data,
        ])
        .on_window_event(|window, event| {
            // Closing the window keeps the app (and its background services) running.
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            if app.state::<AppState>().first_run {
                let enabled = app.state::<AppState>().settings().launch_at_login;
                commands::sync_autostart(app.handle(), enabled);
            }
            // The window and background services still work without a menu bar item.
            if let Err(e) = tray::setup_tray(app.handle()) {
                eprintln!("tray setup: {e}");
            }
            services::start(app.handle());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
