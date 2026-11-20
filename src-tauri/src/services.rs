//! Background services: startup ingest, file watcher, process poll, price refresh, heartbeat.
use crate::AppState;
use crate::model::AgentKind;
use crate::pricing::openrouter::fetch_models;
use crate::watcher::spawn_watcher;
use notify::RecommendedWatcher;
use notify_debouncer_mini::Debouncer;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager};

pub const DATA_UPDATED: &str = "data-updated";
const HEARTBEAT: Duration = Duration::from_secs(5 * 60);
const PRICE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// How often the price loop wakes to check whether 24 h (wall clock) have passed.
const PRICE_CHECK: Duration = Duration::from_secs(60 * 60);

/// Live file watcher plus the channel its callback feeds; replaced when the adapters change.
pub struct Watch {
    debouncer: Mutex<Option<Debouncer<RecommendedWatcher>>>,
    tx: Sender<AgentKind>,
}

pub fn emit_updated(app: &AppHandle) {
    if let Err(e) = app.emit(DATA_UPDATED, ()) {
        eprintln!("emit {DATA_UPDATED}: {e}");
    }
}

fn spawn(app: &AppHandle, name: &str, f: impl FnOnce(AppHandle) + Send + 'static) {
    let app = app.clone();
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(move || f(app)) {
        eprintln!("spawn {name}: {e}");
    }
}

pub fn start(app: &AppHandle) {
    let (tx, rx) = channel();
    app.manage(Watch { debouncer: Mutex::new(None), tx });
    spawn(app, "ingest-worker", move |app| ingest_worker(&app, rx));
    spawn(app, "startup", |app| startup(&app));
    spawn(app, "process-poll", |app| process_poll(&app));
    spawn(app, "prices", |app| price_loop(&app));
    spawn(app, "heartbeat", |app| heartbeat(&app));
}

fn startup(app: &AppHandle) {
    rewatch(app);
    let state = app.state::<AppState>();
    match state.ingest_all() {
        Ok(r) if r.sources_read > 0 => emit_updated(app),
        Ok(_) => {}
        Err(e) => eprintln!("startup ingest: {e:#}"),
    }
    if state.first_run {
        run_discovery(app);
    }
}

/// (Re)starts the file watcher on the roots of every adapter that has token or session data.
pub fn rewatch(app: &AppHandle) {
    let state = app.state::<AppState>();
    let watch = app.state::<Watch>();
    let roots = state
        .adapters
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|a| !matches!(a.kind(), AgentKind::ClaudeDesktop | AgentKind::VsCode))
        .map(|a| (a.kind(), a.root().to_path_buf()))
        .collect();
    let tx = watch.tx.clone();
    let mut slot = watch.debouncer.lock().unwrap_or_else(|e| e.into_inner());
    // Drop the old watcher before starting the new one so no root is watched twice.
    *slot = None;
    match spawn_watcher(roots, move |kind| {
        let _ = tx.send(kind);
    }) {
        Ok(d) => *slot = Some(d),
        Err(e) => eprintln!("watcher: {e:#}"),
    }
}

/// Serializes watcher-triggered ingests off the watcher's own thread.
fn ingest_worker(app: &AppHandle, rx: Receiver<AgentKind>) {
    let state = app.state::<AppState>();
    for kind in rx {
        match state.ingest_kind(kind) {
            Ok(r) if r.sources_read > 0 => emit_updated(app),
            Ok(_) => {}
            Err(e) => eprintln!("ingest {}: {e:#}", kind.as_str()),
        }
    }
}

pub fn run_discovery(app: &AppHandle) {
    match app.state::<AppState>().run_discovery() {
        Ok(true) => emit_updated(app),
        Ok(false) => {}
        Err(e) => eprintln!("discovery: {e:#}"),
    }
}

/// Starts a discovery run in the background (no-op when one is already running).
pub fn spawn_discovery(app: &AppHandle) {
    spawn(app, "discovery", |app| run_discovery(&app));
}

/// Re-ingests everything and refreshes prices after a wipe, so the database is again a
/// consistent record of what is still on disk.
pub fn spawn_rebuild(app: &AppHandle) {
    spawn(app, "rebuild", |app| {
        let state = app.state::<AppState>();
        if let Err(e) = state.ingest_all() {
            eprintln!("rebuild ingest: {e:#}");
        }
        state.refresh_prices(&fetch_models);
        emit_updated(&app);
    });
}

fn process_poll(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut sys = sysinfo::System::new();
    loop {
        match state.poll_processes(&mut sys) {
            Ok(true) => emit_updated(app),
            Ok(false) => {}
            Err(e) => eprintln!("process poll: {e:#}"),
        }
        std::thread::sleep(Duration::from_secs(state.settings().process_poll_secs.max(1)));
    }
}

fn price_loop(app: &AppHandle) {
    let state = app.state::<AppState>();
    loop {
        let st = state.refresh_prices(&fetch_models);
        if let Some(e) = &st.last_error {
            eprintln!("price refresh: {e}");
        }
        emit_updated(app);
        // Wall-clock check so time spent asleep (laptop lid closed) counts toward the 24 h.
        let attempted = SystemTime::now();
        while attempted.elapsed().map_or(true, |d| d < PRICE_INTERVAL) {
            std::thread::sleep(PRICE_CHECK);
        }
    }
}

fn heartbeat(app: &AppHandle) {
    let state = app.state::<AppState>();
    loop {
        std::thread::sleep(HEARTBEAT);
        if let Err(e) = state.heartbeat() {
            eprintln!("heartbeat: {e:#}");
        }
    }
}
