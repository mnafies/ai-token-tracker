//! Menu bar item: today's cost as the title, a summary menu, and the way back into the window.
use crate::AppState;
use crate::db::Db;
use crate::period::{self, PeriodSpec};
use crate::project;
use crate::query;
use crate::services;
use crate::settings::{Currency, Settings};
use crate::state::{lock, read};
use chrono::{DateTime, FixedOffset};
use rusqlite::OptionalExtension;
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

const TRAY_ID: &str = "main-tray";
const ID_OPEN: &str = "open";
const ID_PRICES: &str = "prices";
const ID_QUIT: &str = "quit";
const ICON: &[u8] = include_bytes!("../icons/tray-template.png");

/// Cost of one period; `unpriced_tokens` are tokens with no known price (never shown as $0).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PeriodCost {
    pub cost_usd: f64,
    pub unpriced_tokens: i64,
    pub change_pct: Option<f64>,
}

pub struct TraySummary {
    pub today: PeriodCost,
    pub week: PeriodCost,
    pub month: PeriodCost,
    /// (agent label, project display name)
    pub running: Vec<(String, String)>,
}

/// Title and menu lines as last pushed to the tray.
pub type Rendered = (String, Vec<String>);

/// Mirrors `formatTokens` in src/lib/format.ts: 3 significant digits, K/M/B suffix.
pub fn format_tokens(n: i64) -> String {
    const UNITS: [&str; 4] = ["", "K", "M", "B"];
    let sign = if n < 0 { "-" } else { "" };
    let mut v = n.unsigned_abs() as f64;
    let mut u = 0;
    while u < UNITS.len() - 1 && sig3(v) >= 1000.0 {
        v /= 1000.0;
        u += 1;
    }
    format!("{sign}{}{}", sig3(v), UNITS[u])
}

/// `v` rounded to 3 significant digits (like JS `Number(v.toPrecision(3))`).
fn sig3(v: f64) -> f64 {
    if v == 0.0 {
        return 0.0;
    }
    let factor = 10f64.powi(2 - v.abs().log10().floor() as i32);
    (v * factor).round() / factor
}

pub fn format_money(usd: f64, s: &Settings) -> String {
    match (s.currency, s.idr_rate) {
        (Currency::Idr, Some(rate)) if rate > 0.0 => {
            format!("Rp {}", group_thousands((usd * rate).round() as i64))
        }
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

/// Today's cost; "—" when all of it is unpriced, "+?" appended when part of it is.
pub fn format_title(sum: &TraySummary, s: &Settings) -> String {
    let t = sum.today;
    match (t.cost_usd != 0.0, t.unpriced_tokens > 0) {
        (false, true) => "—".into(),
        (true, true) => format!("{}+?", format_money(t.cost_usd, s)),
        _ => format_money(t.cost_usd, s),
    }
}

/// A period's cost for the menu, never showing unpriced usage as $0.
fn cost_label(p: &PeriodCost, s: &Settings) -> String {
    match (p.cost_usd != 0.0, p.unpriced_tokens > 0) {
        (false, true) => format!("— ({} token tanpa harga)", format_tokens(p.unpriced_tokens)),
        (true, true) => format!(
            "{} + {} token tanpa harga",
            format_money(p.cost_usd, s),
            format_tokens(p.unpriced_tokens)
        ),
        _ => format_money(p.cost_usd, s),
    }
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
        format!("Hari ini  {}", cost_label(&sum.today, s)),
        format!(
            "Minggu ini  {}{}",
            cost_label(&sum.week, s),
            format_change(sum.week.change_pct)
        ),
        format!(
            "Bulan ini  {}{}",
            cost_label(&sum.month, s),
            format_change(sum.month.change_pct)
        ),
    ];
    if sum.running.is_empty() {
        lines.push("Tidak ada agent berjalan".into());
    } else {
        lines.push("Sedang jalan:".into());
        lines.extend(
            sum.running
                .iter()
                .map(|(agent, project)| format!("• {agent} — {project}")),
        );
    }
    lines
}

/// Cost, unpriced tokens and change vs. the previous period, from two `totals` queries.
fn period_cost(
    db: &Db,
    spec: &PeriodSpec,
    now: DateTime<FixedOffset>,
) -> anyhow::Result<PeriodCost> {
    let cur = query::totals(db, &period::resolve(spec, now))?;
    let prev = query::totals(db, &period::previous(spec, now))?;
    Ok(PeriodCost {
        cost_usd: cur.cost_usd,
        unpriced_tokens: cur.unpriced_tokens,
        change_pct: query::change_pct(cur.cost_usd, prev.cost_usd),
    })
}

pub fn build_summary(state: &AppState, now: DateTime<FixedOffset>) -> anyhow::Result<TraySummary> {
    let db = lock(&state.db);
    let today = period_cost(&db, &PeriodSpec::Today, now)?;
    let week = period_cost(&db, &PeriodSpec::ThisWeek, now)?;
    let month = period_cost(&db, &PeriodSpec::ThisMonth, now)?;
    let mut running = Vec::new();
    for r in read(&state.running).iter() {
        let name = match r.cwd.as_deref() {
            None => "—".to_string(),
            Some(cwd) => {
                let key: Option<String> = db
                    .conn
                    .query_row(
                        "SELECT project_key FROM cwd_projects WHERE cwd = ?1",
                        [cwd],
                        |row| row.get(0),
                    )
                    .optional()?;
                project::display_name(key.as_deref().unwrap_or(cwd))
            }
        };
        running.push((r.agent.label().to_string(), name));
    }
    Ok(TraySummary {
        today,
        week,
        month,
        running,
    })
}

/// Stores `r` as the last rendered state; false when it equals what is already shown.
fn note_rendered(cache: &Mutex<Option<Rendered>>, r: &Rendered) -> bool {
    let mut last = lock(cache);
    if last.as_ref() == Some(r) {
        return false;
    }
    *last = Some(r.clone());
    true
}

fn build_menu(app: &AppHandle, lines: &[String]) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    for line in lines {
        menu.append(&MenuItem::new(app, line, false, None::<&str>)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        ID_OPEN,
        "Buka dashboard",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        ID_PRICES,
        "Refresh harga",
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        ID_QUIT,
        "Keluar",
        true,
        None::<&str>,
    )?)?;
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

/// What the tray currently shows, so an unchanged refresh skips rebuilding the menu.
struct TrayCache(Mutex<Option<Rendered>>);

fn render(state: &AppState) -> anyhow::Result<Rendered> {
    let settings = state.settings();
    let sum = build_summary(state, chrono::Local::now().fixed_offset())?;
    Ok((format_title(&sum, &settings), menu_lines(&sum, &settings)))
}

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let (title, lines) = render(&state).unwrap_or_else(|e| {
        eprintln!("tray summary: {e:#}");
        let empty = TraySummary {
            today: PeriodCost::default(),
            week: PeriodCost::default(),
            month: PeriodCost::default(),
            running: vec![],
        };
        let settings = state.settings();
        (
            format_title(&empty, &settings),
            menu_lines(&empty, &settings),
        )
    });
    let menu = build_menu(app, &lines)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON)?)
        .icon_as_template(true)
        .title(&title)
        .menu(&menu)
        .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()))
        .build(app)?;
    app.manage(TrayCache(Mutex::new(Some((title, lines)))));
    Ok(())
}

/// Rebuilds the menu and title from current data when they changed. Errors are logged, never raised.
pub fn refresh_tray(app: &AppHandle) -> tauri::Result<()> {
    let (Some(_), Some(cache)) = (app.tray_by_id(TRAY_ID), app.try_state::<TrayCache>()) else {
        return Ok(());
    };
    let rendered = match render(&app.state::<AppState>()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("tray summary: {e:#}");
            return Ok(());
        }
    };
    if !note_rendered(&cache.0, &rendered) {
        return Ok(());
    }
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let Some(tray) = handle.tray_by_id(TRAY_ID) else {
            return;
        };
        let (title, lines) = rendered;
        let result = build_menu(&handle, &lines).and_then(|menu| {
            tray.set_menu(Some(menu))?;
            tray.set_title(Some(title))
        });
        if let Err(e) = result {
            eprintln!("tray refresh: {e}");
            // Forget what was "shown" so the next refresh tries again.
            *lock(&handle.state::<TrayCache>().0) = None;
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
        Settings {
            currency: Currency::Idr,
            idr_rate: rate,
            ..Settings::defaults(std::path::Path::new("/h"))
        }
    }
    fn pc(cost_usd: f64, unpriced_tokens: i64, change_pct: Option<f64>) -> PeriodCost {
        PeriodCost {
            cost_usd,
            unpriced_tokens,
            change_pct,
        }
    }
    fn sum(week_change: Option<f64>, running: Vec<(String, String)>) -> TraySummary {
        TraySummary {
            today: pc(4.12, 0, None),
            week: pc(31.8, 0, week_change),
            month: pc(142.3, 0, Some(18.0)),
            running,
        }
    }
    fn unpriced(today: PeriodCost) -> TraySummary {
        TraySummary {
            today,
            ..sum(None, vec![])
        }
    }

    #[test]
    fn tokens_format_like_the_ui() {
        let cases = [
            (950, "950"),
            (12_300, "12.3K"),
            (4_560_000, "4.56M"),
            (1_200_000_000, "1.2B"),
            (1000, "1K"),
            (0, "0"),
            (999_999, "1M"),
            (123_456, "123K"),
            (1_234, "1.23K"),
            (-12_300, "-12.3K"),
        ];
        for (n, want) in cases {
            assert_eq!(format_tokens(n), want, "{n}");
        }
    }
    #[test]
    fn title_is_dash_when_today_is_only_unpriced() {
        assert_eq!(format_title(&unpriced(pc(0.0, 12_000, None)), &usd()), "—");
    }
    #[test]
    fn title_marks_partly_unpriced_today() {
        assert_eq!(
            format_title(&unpriced(pc(4.12, 1_200, None)), &usd()),
            "$4.12+?"
        );
    }
    #[test]
    fn title_is_zero_when_nothing_happened() {
        assert_eq!(format_title(&unpriced(pc(0.0, 0, None)), &usd()), "$0.00");
    }
    #[test]
    fn menu_lines_show_unpriced_tokens() {
        let l = menu_lines(&unpriced(pc(4.12, 1_200, None)), &usd());
        assert_eq!(l[0], "Hari ini  $4.12 + 1.2K token tanpa harga");
        let l = menu_lines(&unpriced(pc(0.0, 12_000, None)), &usd());
        assert_eq!(l[0], "Hari ini  — (12K token tanpa harga)");
        let s = TraySummary {
            week: pc(31.8, 500, Some(12.0)),
            month: pc(0.0, 2_000_000, None),
            ..sum(None, vec![])
        };
        let l = menu_lines(&s, &usd());
        assert_eq!(l[1], "Minggu ini  $31.80 + 500 token tanpa harga  ▲12%");
        assert_eq!(l[2], "Bulan ini  — (2M token tanpa harga)");
    }
    #[test]
    fn unchanged_render_is_skipped() {
        let cache = Mutex::new(None);
        let a: Rendered = ("$1.00".into(), vec!["Hari ini  $1.00".into()]);
        let b: Rendered = ("$2.00".into(), vec!["Hari ini  $2.00".into()]);
        assert!(note_rendered(&cache, &a));
        assert!(!note_rendered(&cache, &a));
        assert!(note_rendered(&cache, &b));
        assert!(note_rendered(&cache, &a));
    }

    #[test]
    fn build_summary_matches_overview_without_running_it() {
        use rusqlite::params;
        let dir = tempfile::tempdir().unwrap();
        let state =
            AppState::open(dir.path().to_path_buf(), &AppState::db_path(dir.path())).unwrap();
        let now = DateTime::parse_from_rfc3339("2026-11-21T10:00:00+07:00").unwrap();
        let at = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis();
        {
            let db = lock(&state.db);
            db.conn
                .execute(
                    "INSERT INTO resolved_prices(provider_hint, raw_model, status, openrouter_id, prompt, completion)
                     VALUES ('p','m','matched','or/m',1e-6,1e-6)",
                    [],
                )
                .unwrap();
            let events = [
                ("today", "2026-11-21T08:00:00+07:00", "m", 2_000_000),
                ("today-unpriced", "2026-11-21T09:00:00+07:00", "u", 7_000),
                ("yesterday", "2026-11-20T08:00:00+07:00", "m", 1_000_000),
                ("last-week", "2026-11-12T08:00:00+07:00", "m", 4_000_000),
                ("last-month", "2026-10-15T08:00:00+07:00", "m", 3_000_000),
            ];
            for (key, ts, model, input) in events {
                db.conn
                    .execute(
                        "INSERT INTO events(event_key, agent, session_id, ts_ms, model, provider_hint, input, output, reasoning, cache_read, cache_write)
                         VALUES (?1,'claude-code','s',?2,?3,'p',?4,0,0,0,0)",
                        params![key, at(ts), model, input],
                    )
                    .unwrap();
            }
        }
        let sum = build_summary(&state, now).unwrap();
        let db = lock(&state.db);
        for (got, spec) in [
            (sum.today, PeriodSpec::Today),
            (sum.week, PeriodSpec::ThisWeek),
            (sum.month, PeriodSpec::ThisMonth),
        ] {
            let o = query::overview(&db, &spec, now).unwrap();
            assert_eq!(
                got,
                pc(o.totals.cost_usd, o.totals.unpriced_tokens, o.change_pct),
                "{spec:?}"
            );
        }
        assert_eq!(sum.today.unpriced_tokens, 7_000);
        assert!((sum.today.change_pct.unwrap() - 100.0).abs() < 1e-9);
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
        let l = menu_lines(
            &sum(
                Some(12.0),
                vec![("Claude Code".into(), "backoffice-be".into())],
            ),
            &usd(),
        );
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
        assert_eq!(
            menu_lines(&sum(Some(-5.0), vec![]), &usd())[1],
            "Minggu ini  $31.80  ▼5%"
        );
        assert_eq!(
            menu_lines(&sum(Some(0.0), vec![]), &usd())[1],
            "Minggu ini  $31.80  ▲0%"
        );
        assert_eq!(
            menu_lines(&sum(None, vec![]), &usd())[1],
            "Minggu ini  $31.80"
        );
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
        let state =
            AppState::open(dir.path().to_path_buf(), &AppState::db_path(dir.path())).unwrap();
        let ra = |cwd: Option<&str>| RunningAgent {
            agent: AgentKind::ClaudeCode,
            pid: 1,
            cwd: cwd.map(String::from),
            started_ms: 0,
            session_hint: None,
        };
        *state.running.write().unwrap() = vec![ra(Some("/w/backoffice-be")), ra(None)];
        let sum = build_summary(&state, chrono::Local::now().fixed_offset()).unwrap();
        assert_eq!(sum.today, PeriodCost::default());
        assert_eq!(
            sum.running,
            vec![
                ("Claude Code".to_string(), "backoffice-be".to_string()),
                ("Claude Code".to_string(), "—".to_string())
            ]
        );
    }
}
