//! Background services: startup ingest, file watcher, process poll, price refresh, heartbeat.
use crate::AppState;
use crate::state::{lock, read};
use crate::model::AgentKind;
use crate::pricing::openrouter::fetch_models;
use crate::watcher::spawn_watcher;
use notify::RecommendedWatcher;
use notify_debouncer_mini::Debouncer;
use std::panic::AssertUnwindSafe;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager};

pub const DATA_UPDATED: &str = "data-updated";
const TRAY_INTERVAL: Duration = Duration::from_secs(30);
const HEARTBEAT: Duration = Duration::from_secs(5 * 60);
const PRICE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// How often the price loop wakes to check whether 24 h (wall clock) have passed.
const PRICE_CHECK: Duration = Duration::from_secs(60 * 60);

/// Live file watcher plus the channel its callback feeds; replaced when the adapters change.
pub struct Watch {
    debouncer: Mutex<Option<Debouncer<RecommendedWatcher>>>,
    tx: Sender<AgentKind>,
}

/// Runs `f`, logging (instead of propagating) a panic so a background loop keeps going.
/// Returns false when `f` panicked.
pub fn run_guarded(name: &str, f: impl FnOnce()) -> bool {
    match std::panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(()) => true,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic".into());
            eprintln!("{name}: panicked: {msg}");
            false
        }
    }
}

pub fn emit_updated(app: &AppHandle) {
    if let Err(e) = app.emit(DATA_UPDATED, ()) {
        eprintln!("emit {DATA_UPDATED}: {e}");
    }
    if let Err(e) = crate::tray::refresh_tray(app) {
        eprintln!("tray refresh: {e}");
    }
}

/// Refreshes model prices off the calling thread (tray menu action).
pub fn spawn_price_refresh(app: &AppHandle) {
    spawn(app, "price-refresh", |app| {
        let st = app.state::<AppState>().refresh_prices(&fetch_models);
        if let Some(e) = &st.last_error {
            eprintln!("price refresh: {e}");
        }
        emit_updated(&app);
    });
}

/// Spawns a named thread; a panic in `f` is logged rather than lost with the thread.
fn spawn(app: &AppHandle, name: &str, f: impl FnOnce(AppHandle) + Send + 'static) {
    let app = app.clone();
    let thread_name = name.to_string();
    let spawned = std::thread::Builder::new().name(name.into()).spawn(move || {
        run_guarded(&thread_name, || f(app));
    });
    if let Err(e) = spawned {
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
    spawn(app, "tray", |app| tray_timer(&app));
}

fn tray_timer(app: &AppHandle) {
    loop {
        std::thread::sleep(TRAY_INTERVAL);
        run_guarded("tray", || {
            if let Err(e) = crate::tray::refresh_tray(app) {
                eprintln!("tray refresh: {e}");
            }
        });
    }
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
    let roots = read(&state.adapters)
        .iter()
        .filter(|a| !matches!(a.kind(), AgentKind::ClaudeDesktop | AgentKind::VsCode))
        .map(|a| (a.kind(), a.root().to_path_buf()))
        .collect();
    let tx = watch.tx.clone();
    let mut slot = lock(&watch.debouncer);
    // Drop the old watcher before starting the new one so no root is watched twice.
    *slot = None;
    match spawn_watcher(roots, move |kind| {
        if let Err(e) = tx.send(kind) {
            eprintln!("watcher: ingest worker gone, dropping change for {}: {e}", e.0.as_str());
        }
    }) {
        Ok(d) => *slot = Some(d),
        Err(e) => eprintln!("watcher: {e:#}"),
    }
}

/// Serializes watcher-triggered ingests off the watcher's own thread.
fn ingest_worker(app: &AppHandle, rx: Receiver<AgentKind>) {
    let state = app.state::<AppState>();
    for kind in rx {
        run_guarded("ingest-worker", || match state.ingest_kind(kind) {
            Ok(r) if r.sources_read > 0 => emit_updated(app),
            Ok(_) => {}
            Err(e) => eprintln!("ingest {}: {e:#}", kind.as_str()),
        });
    }
}

pub fn run_discovery(app: &AppHandle) {
    match app.state::<AppState>().run_discovery() {
        Ok(true) => emit_updated(app),
        Ok(false) => {}
        Err(e) => eprintln!("discovery: {e:#}"),
    }
}

/// Scans in the background for a run already started with `AppState::begin_discovery`.
pub fn spawn_begun_discovery(app: &AppHandle) {
    spawn(app, "discovery", |app| {
        if let Err(e) = app.state::<AppState>().run_begun_discovery() {
            eprintln!("discovery: {e:#}");
        }
        emit_updated(&app);
    });
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
        run_guarded("process-poll", || match state.poll_processes(&mut sys) {
            Ok(true) => emit_updated(app),
            Ok(false) => {}
            Err(e) => eprintln!("process poll: {e:#}"),
        });
        std::thread::sleep(Duration::from_secs(state.settings().process_poll_secs.max(1)));
    }
}

fn price_loop(app: &AppHandle) {
    let state = app.state::<AppState>();
    loop {
        run_guarded("prices", || {
            let st = state.refresh_prices(&fetch_models);
            if let Some(e) = &st.last_error {
                eprintln!("price refresh: {e}");
            }
            emit_updated(app);
        });
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
        run_guarded("heartbeat", || {
            if let Err(e) = state.heartbeat() {
                eprintln!("heartbeat: {e:#}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_guarded_survives_a_panic() {
        let mut ran = 0;
        assert!(!run_guarded("test", || panic!("boom")));
        assert!(run_guarded("test", || ran += 1));
        assert_eq!(ran, 1);
    }
}
