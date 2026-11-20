//! Menu bar item: today's cost as the title, a summary menu, and the way back into the window.
use crate::AppState;
use crate::period::PeriodSpec;
use crate::project;
use crate::query;
use crate::services;
use crate::settings::{Currency, Settings};
use crate::state::{lock, read};
use chrono::{DateTime, FixedOffset};
use rusqlite::OptionalExtension;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

const TRAY_ID: &str = "main-tray";
const ID_OPEN: &str = "open";
const ID_PRICES: &str = "prices";
const ID_QUIT: &str = "quit";
const ICON: &[u8] = include_bytes!("../icons/tray-template.png");

pub struct TraySummary {
    pub today: f64,
    pub week: f64,
    pub week_change: Option<f64>,
    pub month: f64,
    pub month_change: Option<f64>,
    /// (agent label, project display name)
    pub running: Vec<(String, String)>,
}

pub fn format_money(usd: f64, s: &Settings) -> String {
    match (s.currency, s.idr_rate) {
        (Currency::Idr, Some(rate)) if rate > 0.0 => format!("Rp {}", group_thousands((usd * rate).round() as i64)),
        _ => format!("${usd:.2}"),
    }
}

/// `65920` -> `"65.920"`.
fn group_thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

pub fn format_title(sum: &TraySummary, s: &Settings) -> String {
    format_money(sum.today, s)
}

fn format_change(pct: Option<f64>) -> String {
    match pct {
        None => String::new(),
        Some(p) => {
            let r = p.round();
            format!("  {}{}%", if r < 0.0 { '▼' } else { '▲' }, r.abs())
        }
    }
}

pub fn menu_lines(sum: &TraySummary, s: &Settings) -> Vec<String> {
    let mut lines = vec![
        format!("Hari ini  {}", format_money(sum.today, s)),
        format!("Minggu ini  {}{}", format_money(sum.week, s), format_change(sum.week_change)),
        format!("Bulan ini  {}{}", format_money(sum.month, s), format_change(sum.month_change)),
    ];
    if sum.running.is_empty() {
        lines.push("Tidak ada agent berjalan".into());
    } else {
        lines.push("Sedang jalan:".into());
        lines.extend(sum.running.iter().map(|(agent, project)| format!("• {agent} — {project}")));
    }
    lines
}

pub fn build_summary(state: &AppState, now: DateTime<FixedOffset>) -> anyhow::Result<TraySummary> {
    let (today, week, month, names) = {
        let db = lock(&state.db);
        let today = query::overview(&db, &PeriodSpec::Today, now)?;
        let week = query::overview(&db, &PeriodSpec::ThisWeek, now)?;
        let month = query::overview(&db, &PeriodSpec::ThisMonth, now)?;
        let mut names = Vec::new();
        for r in read(&state.running).iter() {
            let name = match r.cwd.as_deref() {
                None => "—".to_string(),
                Some(cwd) => {
                    let key: Option<String> = db
                        .conn
                        .query_row("SELECT project_key FROM cwd_projects WHERE cwd = ?1", [cwd], |row| row.get(0))
                        .optional()?;
                    project::display_name(key.as_deref().unwrap_or(cwd))
                }
            };
            names.push((r.agent.label().to_string(), name));
        }
        (today, week, month, names)
    };
    Ok(TraySummary {
        today: today.totals.cost_usd,
        week: week.totals.cost_usd,
        week_change: week.change_pct,
        month: month.totals.cost_usd,
        month_change: month.change_pct,
        running: names,
    })
}

fn build_menu(app: &AppHandle, sum: &TraySummary, s: &Settings) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    for line in menu_lines(sum, s) {
        menu.append(&MenuItem::new(app, line, false, None::<&str>)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, ID_OPEN, "Buka dashboard", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, ID_PRICES, "Refresh harga", true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, ID_QUIT, "Keluar", true, None::<&str>)?)?;
    Ok(menu)
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        ID_OPEN => {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }
        ID_PRICES => services::spawn_price_refresh(app),
        ID_QUIT => app.exit(0),
        _ => {}
    }
}

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let sum = build_summary(&state, chrono::Local::now().fixed_offset()).unwrap_or_else(|e| {
        eprintln!("tray summary: {e:#}");
        TraySummary { today: 0.0, week: 0.0, week_change: None, month: 0.0, month_change: None, running: vec![] }
    });
    let menu = build_menu(app, &sum, &settings)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON)?)
        .icon_as_template(true)
        .title(format_title(&sum, &settings))
        .menu(&menu)
        .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()))
        .build(app)?;
    Ok(())
}

/// Rebuilds the menu and title from current data. Errors are logged, never raised.
pub fn refresh_tray(app: &AppHandle) -> tauri::Result<()> {
    if app.tray_by_id(TRAY_ID).is_none() {
        return Ok(());
    }
    let state = app.state::<AppState>();
    let settings = state.settings();
    let sum = match build_summary(&state, chrono::Local::now().fixed_offset()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("tray summary: {e:#}");
            return Ok(());
        }
    };
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let Some(tray) = handle.tray_by_id(TRAY_ID) else { return };
        let result = build_menu(&handle, &sum, &settings).and_then(|menu| {
            tray.set_menu(Some(menu))?;
            tray.set_title(Some(format_title(&sum, &settings)))
        });
        if let Err(e) = result {
            eprintln!("tray refresh: {e}");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Currency;

    fn usd() -> Settings {
        Settings::defaults(std::path::Path::new("/h"))
    }
    fn idr(rate: Option<f64>) -> Settings {
        Settings { currency: Currency::Idr, idr_rate: rate, ..Settings::defaults(std::path::Path::new("/h")) }
    }
    fn sum(week_change: Option<f64>, running: Vec<(String, String)>) -> TraySummary {
        TraySummary { today: 4.12, week: 31.8, week_change, month: 142.3, month_change: Some(18.0), running }
    }

    #[test]
    fn money_usd_two_decimals() {
        assert_eq!(format_money(4.1234, &usd()), "$4.12");
    }
    #[test]
    fn money_idr_uses_rate() {
        assert_eq!(format_money(4.12, &idr(Some(16000.0))), "Rp 65.920");
        assert_eq!(format_money(1000.0, &idr(Some(16000.0))), "Rp 16.000.000");
    }
    #[test]
    fn idr_without_rate_falls_back_to_usd() {
        assert_eq!(format_money(4.12, &idr(None)), "$4.12");
        assert_eq!(format_money(4.12, &idr(Some(0.0))), "$4.12");
    }
    #[test]
    fn title_is_today() {
        assert_eq!(format_title(&sum(None, vec![]), &usd()), "$4.12");
    }
    #[test]
    fn menu_lines_show_change_arrows() {
        let l = menu_lines(&sum(Some(12.0), vec![("Claude Code".into(), "backoffice-be".into())]), &usd());
        assert_eq!(
            l,
            vec![
                "Hari ini  $4.12",
                "Minggu ini  $31.80  ▲12%",
                "Bulan ini  $142.30  ▲18%",
                "Sedang jalan:",
                "• Claude Code — backoffice-be"
            ]
        );
        assert_eq!(menu_lines(&sum(Some(-5.0), vec![]), &usd())[1], "Minggu ini  $31.80  ▼5%");
        assert_eq!(menu_lines(&sum(Some(0.0), vec![]), &usd())[1], "Minggu ini  $31.80  ▲0%");
        assert_eq!(menu_lines(&sum(None, vec![]), &usd())[1], "Minggu ini  $31.80");
    }
    #[test]
    fn menu_lines_when_nothing_running() {
        let l = menu_lines(&sum(None, vec![]), &usd());
        assert_eq!(l.last().unwrap(), "Tidak ada agent berjalan");
        assert_eq!(l.len(), 4);
    }

    #[test]
    fn build_summary_names_running_projects() {
        use crate::model::AgentKind;
        use crate::process::RunningAgent;
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::open(dir.path().to_path_buf(), &AppState::db_path(dir.path())).unwrap();
        let ra = |cwd: Option<&str>| RunningAgent {
            agent: AgentKind::ClaudeCode,
            pid: 1,
            cwd: cwd.map(String::from),
            started_ms: 0,
            session_hint: None,
        };
        *state.running.write().unwrap() = vec![ra(Some("/w/backoffice-be")), ra(None)];
        let sum = build_summary(&state, chrono::Local::now().fixed_offset()).unwrap();
        assert_eq!(sum.today, 0.0);
        assert_eq!(
            sum.running,
            vec![("Claude Code".to_string(), "backoffice-be".to_string()), ("Claude Code".to_string(), "—".to_string())]
        );
    }
}
