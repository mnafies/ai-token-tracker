use crate::db::Db;
use crate::period::{self, PeriodRange, PeriodSpec};
use crate::project;
use chrono::{DateTime, FixedOffset, Months, NaiveDate};
use rusqlite::{params, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub cost_usd: f64,
    pub unpriced_tokens: i64,
    pub tokens: i64,
    pub input: i64,
    pub output: i64,
    pub reasoning: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub events: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCost {
    pub agent: String,
    pub cost_usd: f64,
    pub tokens: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyPoint {
    pub date: String,
    pub by_agent: Vec<AgentCost>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub project_key: String,
    pub name: String,
    pub cost_usd: f64,
    pub unpriced_tokens: i64,
    pub tokens: i64,
    pub sessions: i64,
    pub agents: Vec<String>,
    pub footprints: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub agent: String,
    pub session_id: String,
    pub title: Option<String>,
    pub project_key: Option<String>,
    pub started_ms: Option<i64>,
    pub updated_ms: Option<i64>,
    pub models: Vec<String>,
    pub tokens: i64,
    pub cost_usd: f64,
    pub unpriced_tokens: i64,
    pub token_status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub range: PeriodRange,
    pub totals: Totals,
    pub previous: Totals,
    pub change_pct: Option<f64>,
    pub daily: Vec<DailyPoint>,
    pub top_projects: Vec<ProjectRow>,
    pub top_sessions: Vec<SessionRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Granularity {
    Week,
    Month,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Split {
    Agent,
    Model,
    Project,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendBucket {
    pub label: String,
    pub start_ms: i64,
    pub totals: Totals,
    pub by: Vec<(String, f64)>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionFilter {
    pub range: Option<PeriodRange>,
    pub agent: Option<String>,
    pub project_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventRow {
    pub ts_ms: i64,
    pub model: String,
    pub input: i64,
    pub output: i64,
    pub reasoning: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub cost_usd: Option<f64>,
    pub price_status: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub row: SessionRow,
    pub events: Vec<EventRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRow {
    pub provider_hint: String,
    pub model: String,
    pub status: String,
    pub openrouter_id: Option<String>,
    pub prompt: Option<f64>,
    pub completion: Option<f64>,
    pub totals: Totals,
}

/// Per-event cost; NULL means unpriced (never counted as $0). Only `ignored` models cost 0.
const COST_SQL: &str = "CASE WHEN rp.status IN ('matched','mapped','manual') THEN e.input*rp.prompt + e.output*rp.completion
       + e.cache_read*COALESCE(rp.cache_read, rp.prompt) + e.cache_write*COALESCE(rp.cache_write, rp.prompt)
     WHEN rp.status = 'ignored' THEN 0 ELSE NULL END";

/// Aggregate columns over `ev` read back by [`read_totals`] (9 columns).
const TOTALS_COLS: &str = "COALESCE(SUM(cost), 0), COALESCE(SUM(CASE WHEN cost IS NULL THEN tokens END), 0),
  COALESCE(SUM(tokens), 0), COALESCE(SUM(input), 0), COALESCE(SUM(output), 0), COALESCE(SUM(reasoning), 0),
  COALESCE(SUM(cache_read), 0), COALESCE(SUM(cache_write), 0), COUNT(*)";

const IN_RANGE: &str = "ts_ms >= ?1 AND ts_ms < ?2";

/// `ev`: every event with its billable token count, project (via its own cwd, else the
/// session's cwd; raw cwd when unmapped), resolved price status and cost.
fn ev_cte() -> String {
    format!(
        "WITH ev AS (SELECT e.event_key, e.agent, e.session_id, e.ts_ms, e.cwd, e.model, e.provider_hint,
           e.input, e.output, e.reasoning, e.cache_read, e.cache_write,
           e.input + e.output + e.cache_read + e.cache_write AS tokens,
           COALESCE(cp.project_key, e.cwd, s.cwd) AS project_key,
           rp.status AS price_status, {COST_SQL} AS cost
         FROM events e
         LEFT JOIN sessions s ON s.agent = e.agent AND s.session_id = e.session_id
         LEFT JOIN cwd_projects cp ON cp.cwd = COALESCE(e.cwd, s.cwd)
         LEFT JOIN resolved_prices rp ON rp.provider_hint = e.provider_hint AND rp.raw_model = e.model)"
    )
}

fn read_totals(r: &Row, i: usize) -> rusqlite::Result<Totals> {
    Ok(Totals {
        cost_usd: r.get(i)?,
        unpriced_tokens: r.get(i + 1)?,
        tokens: r.get(i + 2)?,
        input: r.get(i + 3)?,
        output: r.get(i + 4)?,
        reasoning: r.get(i + 5)?,
        cache_read: r.get(i + 6)?,
        cache_write: r.get(i + 7)?,
        events: r.get(i + 8)?,
    })
}

fn local_date(ms: i64, tz: FixedOffset) -> NaiveDate {
    DateTime::from_timestamp_millis(ms).expect("valid timestamp").with_timezone(&tz).date_naive()
}

pub fn totals(db: &Db, r: &PeriodRange) -> anyhow::Result<Totals> {
    let sql = format!("{} SELECT {TOTALS_COLS} FROM ev WHERE {IN_RANGE}", ev_cte());
    Ok(db.conn.query_row(&sql, params![r.start_ms, r.end_ms], |row| read_totals(row, 0))?)
}

pub fn overview(db: &Db, spec: &PeriodSpec, now: DateTime<FixedOffset>) -> anyhow::Result<Overview> {
    let range = period::resolve(spec, now);
    let cur = totals(db, &range)?;
    let prev = totals(db, &period::previous(spec, now))?;
    let change_pct = (prev.cost_usd != 0.0).then(|| (cur.cost_usd - prev.cost_usd) / prev.cost_usd * 100.0);
    let mut top_projects = projects(db, &range)?;
    top_projects.truncate(5);
    let mut top_sessions = sessions(db, &SessionFilter { range: Some(range), ..Default::default() })?;
    top_sessions.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd));
    top_sessions.truncate(5);
    Ok(Overview {
        range,
        totals: cur,
        previous: prev,
        change_pct,
        daily: daily(db, &range, now)?,
        top_projects,
        top_sessions,
    })
}

/// One point per local day in the range, including empty days.
pub fn daily(db: &Db, r: &PeriodRange, now: DateTime<FixedOffset>) -> anyhow::Result<Vec<DailyPoint>> {
    let sql = format!(
        "{} SELECT date(ts_ms/1000 + ?3, 'unixepoch') AS d, agent, COALESCE(SUM(cost), 0), SUM(tokens)
         FROM ev WHERE {IN_RANGE} GROUP BY d, agent ORDER BY d, agent",
        ev_cte()
    );
    let mut by_day: HashMap<String, Vec<AgentCost>> = HashMap::new();
    let mut stmt = db.conn.prepare(&sql)?;
    let mut rows = stmt.query(params![r.start_ms, r.end_ms, now.offset().local_minus_utc()])?;
    while let Some(row) = rows.next()? {
        by_day.entry(row.get(0)?).or_default().push(AgentCost { agent: row.get(1)?, cost_usd: row.get(2)?, tokens: row.get(3)? });
    }
    let mut out = Vec::new();
    if r.end_ms <= r.start_ms {
        return Ok(out);
    }
    let tz = *now.offset();
    let last = local_date(r.end_ms - 1, tz);
    let mut d = local_date(r.start_ms, tz);
    while d <= last {
        let date = d.format("%Y-%m-%d").to_string();
        let by_agent = by_day.remove(&date).unwrap_or_default();
        out.push(DailyPoint { date, by_agent });
        d = d.succ_opt().expect("date in range");
    }
    Ok(out)
}

pub fn trends(db: &Db, g: Granularity, count: u32, split: Split, now: DateTime<FixedOffset>) -> anyhow::Result<Vec<TrendBucket>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let today = now.date_naive();
    let (current, bucket_sql, label_fmt) = match g {
        Granularity::Week => (period::week_start(today), "date(ts_ms/1000 + ?3, 'unixepoch', 'weekday 0', '-6 days')", "%Y-%m-%d"),
        Granularity::Month => (period::month_start(today), "strftime('%Y-%m', ts_ms/1000 + ?3, 'unixepoch')", "%Y-%m"),
    };
    let shift = |d: NaiveDate, n: i64| match g {
        Granularity::Week => d + chrono::Duration::days(7 * n),
        Granularity::Month if n >= 0 => d + Months::new(n as u32),
        Granularity::Month => d - Months::new(n.unsigned_abs() as u32),
    };
    let starts: Vec<NaiveDate> = (0..count as i64).rev().map(|i| shift(current, -i)).collect();
    let p = params![
        period::local_midnight_ms(now, starts[0]),
        period::local_midnight_ms(now, shift(current, 1)),
        now.offset().local_minus_utc()
    ];

    let cte = ev_cte();
    let mut totals_by: HashMap<String, Totals> = HashMap::new();
    let sql = format!("{cte} SELECT {bucket_sql} AS b, {TOTALS_COLS} FROM ev WHERE {IN_RANGE} GROUP BY b");
    let mut stmt = db.conn.prepare(&sql)?;
    let mut rows = stmt.query(p)?;
    while let Some(row) = rows.next()? {
        totals_by.insert(row.get(0)?, read_totals(row, 1)?);
    }

    let key = match split {
        Split::Agent => "agent",
        Split::Model => "model",
        Split::Project => "project_key",
    };
    let mut split_by: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    let sql = format!(
        "{cte} SELECT {bucket_sql} AS b, {key}, COALESCE(SUM(cost), 0) AS c FROM ev
         WHERE {IN_RANGE} AND {key} IS NOT NULL GROUP BY b, {key} ORDER BY c DESC, {key}"
    );
    let mut stmt = db.conn.prepare(&sql)?;
    let mut rows = stmt.query(p)?;
    while let Some(row) = rows.next()? {
        split_by.entry(row.get(0)?).or_default().push((row.get(1)?, row.get(2)?));
    }

    Ok(starts
        .into_iter()
        .map(|d| {
            let label = d.format(label_fmt).to_string();
            TrendBucket {
                start_ms: period::local_midnight_ms(now, d),
                totals: totals_by.remove(&label).unwrap_or_default(),
                by: split_by.remove(&label).unwrap_or_default(),
                label,
            }
        })
        .collect())
}

pub fn projects(db: &Db, r: &PeriodRange) -> anyhow::Result<Vec<ProjectRow>> {
    let sql = format!(
        "{} SELECT project_key, COALESCE(SUM(cost), 0) AS c, COALESCE(SUM(CASE WHEN cost IS NULL THEN tokens END), 0),
           SUM(tokens), COUNT(DISTINCT agent || char(31) || session_id), group_concat(DISTINCT agent)
         FROM ev WHERE {IN_RANGE} AND project_key IS NOT NULL GROUP BY project_key ORDER BY c DESC, project_key",
        ev_cte()
    );
    let mut fp = db.conn.prepare("SELECT DISTINCT agent FROM project_footprints WHERE project_key = ?1 ORDER BY agent")?;
    let mut stmt = db.conn.prepare(&sql)?;
    let mut rows = stmt.query(params![r.start_ms, r.end_ms])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let project_key: String = row.get(0)?;
        let mut agents: Vec<String> = row.get::<_, String>(5)?.split(',').map(String::from).collect();
        agents.sort();
        let footprints = fp.query_map([&project_key], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        out.push(ProjectRow {
            name: project::display_name(&project_key),
            project_key,
            cost_usd: row.get(1)?,
            unpriced_tokens: row.get(2)?,
            tokens: row.get(3)?,
            sessions: row.get(4)?,
            agents,
            footprints,
        });
    }
    Ok(out)
}

pub fn sessions(db: &Db, f: &SessionFilter) -> anyhow::Result<Vec<SessionRow>> {
    session_rows(db, f, None)
}

/// Sessions in scope: those with events in the range plus not-recorded sessions updated in
/// it (all sessions when there is no range). Token/cost columns cover in-range events only;
/// the session's project is the one holding most of its tokens across all its events.
fn session_rows(db: &Db, f: &SessionFilter, session_id: Option<&str>) -> anyhow::Result<Vec<SessionRow>> {
    let sql = format!(
        "{},
         scoped AS (SELECT * FROM ev WHERE ?1 IS NULL OR (ts_ms >= ?1 AND ts_ms < ?2)),
         agg AS (SELECT agent, session_id, SUM(tokens) AS tokens, COALESCE(SUM(cost), 0) AS cost,
                   COALESCE(SUM(CASE WHEN cost IS NULL THEN tokens END), 0) AS unpriced
                 FROM scoped GROUP BY agent, session_id),
         mdl AS (SELECT agent, session_id, group_concat(model, char(31)) AS models
                 FROM (SELECT DISTINCT agent, session_id, model FROM scoped) GROUP BY agent, session_id),
         proj AS (SELECT agent, session_id, project_key,
                    ROW_NUMBER() OVER (PARTITION BY agent, session_id ORDER BY SUM(tokens) DESC, project_key) AS rn
                  FROM ev WHERE project_key IS NOT NULL GROUP BY agent, session_id, project_key),
         keys AS (SELECT agent, session_id FROM agg
                  UNION SELECT agent, session_id FROM sessions
                  WHERE ?1 IS NULL OR (token_status = 'not-recorded' AND updated_ms >= ?1 AND updated_ms < ?2)),
         sr AS (SELECT k.agent, k.session_id, s.title, COALESCE(p.project_key, cps.project_key, s.cwd) AS project_key,
                  s.started_ms, s.updated_ms, m.models, COALESCE(a.tokens, 0) AS tokens, COALESCE(a.cost, 0) AS cost,
                  COALESCE(a.unpriced, 0) AS unpriced, COALESCE(s.token_status, 'recorded') AS token_status
                FROM keys k
                LEFT JOIN sessions s ON s.agent = k.agent AND s.session_id = k.session_id
                LEFT JOIN agg a ON a.agent = k.agent AND a.session_id = k.session_id
                LEFT JOIN mdl m ON m.agent = k.agent AND m.session_id = k.session_id
                LEFT JOIN proj p ON p.agent = k.agent AND p.session_id = k.session_id AND p.rn = 1
                LEFT JOIN cwd_projects cps ON cps.cwd = s.cwd)
         SELECT * FROM sr
         WHERE (?3 IS NULL OR agent = ?3) AND (?4 IS NULL OR project_key = ?4) AND (?5 IS NULL OR session_id = ?5)
         ORDER BY updated_ms IS NULL, updated_ms DESC, agent, session_id",
        ev_cte()
    );
    let mut stmt = db.conn.prepare(&sql)?;
    let rows = stmt.query_map(
        params![f.range.map(|r| r.start_ms), f.range.map(|r| r.end_ms), f.agent, f.project_key, session_id],
        |row| {
            let mut models: Vec<String> = row
                .get::<_, Option<String>>(6)?
                .map(|m| m.split('\u{1f}').map(String::from).collect())
                .unwrap_or_default();
            models.sort();
            Ok(SessionRow {
                agent: row.get(0)?,
                session_id: row.get(1)?,
                title: row.get(2)?,
                project_key: row.get(3)?,
                started_ms: row.get(4)?,
                updated_ms: row.get(5)?,
                models,
                tokens: row.get(7)?,
                cost_usd: row.get(8)?,
                unpriced_tokens: row.get(9)?,
                token_status: row.get(10)?,
            })
        },
    )?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn session_detail(db: &Db, agent: &str, session_id: &str) -> anyhow::Result<Option<SessionDetail>> {
    let filter = SessionFilter { agent: Some(agent.to_string()), ..Default::default() };
    let Some(row) = session_rows(db, &filter, Some(session_id))?.into_iter().next() else {
        return Ok(None);
    };
    let sql = format!(
        "{} SELECT ts_ms, model, input, output, reasoning, cache_read, cache_write, cost,
           COALESCE(price_status, 'unknown'), cwd
         FROM ev WHERE agent = ?1 AND session_id = ?2 ORDER BY ts_ms, event_key",
        ev_cte()
    );
    let mut stmt = db.conn.prepare(&sql)?;
    let events = stmt
        .query_map(params![agent, session_id], |r| {
            Ok(EventRow {
                ts_ms: r.get(0)?,
                model: r.get(1)?,
                input: r.get(2)?,
                output: r.get(3)?,
                reasoning: r.get(4)?,
                cache_read: r.get(5)?,
                cache_write: r.get(6)?,
                cost_usd: r.get(7)?,
                price_status: r.get(8)?,
                cwd: r.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(SessionDetail { row, events }))
}

pub fn models(db: &Db, r: &PeriodRange) -> anyhow::Result<Vec<ModelRow>> {
    let sql = format!(
        "{} SELECT t.*, COALESCE(rp.status, 'unknown'), rp.openrouter_id, rp.prompt, rp.completion
         FROM (SELECT provider_hint, model, {TOTALS_COLS} FROM ev WHERE {IN_RANGE} GROUP BY provider_hint, model) t
         LEFT JOIN resolved_prices rp ON rp.provider_hint = t.provider_hint AND rp.raw_model = t.model
         ORDER BY 3 DESC, 5 DESC, t.provider_hint, t.model",
        ev_cte()
    );
    let mut stmt = db.conn.prepare(&sql)?;
    let rows = stmt.query_map(params![r.start_ms, r.end_ms], |row| {
        Ok(ModelRow {
            provider_hint: row.get(0)?,
            model: row.get(1)?,
            totals: read_totals(row, 2)?,
            status: row.get(11)?,
            openrouter_id: row.get(12)?,
            prompt: row.get(13)?,
            completion: row.get(14)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::period::resolve;

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-21T10:00:00+07:00").unwrap()
    }
    fn ms(s: &str) -> i64 {
        DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
    }
    fn this_week() -> PeriodRange {
        resolve(&PeriodSpec::ThisWeek, now())
    }
    const WED: &str = "2026-11-18T12:00:00+07:00";
    const LAST_WED: &str = "2026-11-11T12:00:00+07:00";

    struct Ev<'a> {
        key: &'a str,
        agent: &'a str,
        session: &'a str,
        ts: &'a str,
        cwd: Option<&'a str>,
        model: &'a str,
        input: i64,
        output: i64,
        reasoning: i64,
        cache_read: i64,
        cache_write: i64,
    }
    impl Default for Ev<'_> {
        fn default() -> Self {
            Ev {
                key: "k", agent: "claude-code", session: "s1", ts: WED, cwd: Some("/r"), model: "m",
                input: 0, output: 0, reasoning: 0, cache_read: 0, cache_write: 0,
            }
        }
    }

    fn insert(db: &Db, e: Ev<'_>) {
        db.conn
            .execute(
                "INSERT INTO events(event_key, agent, session_id, ts_ms, cwd, model, provider_hint, input, output, reasoning, cache_read, cache_write)
                 VALUES (?1,?2,?3,?4,?5,?6,'p',?7,?8,?9,?10,?11)",
                params![e.key, e.agent, e.session, ms(e.ts), e.cwd, e.model, e.input, e.output, e.reasoning, e.cache_read, e.cache_write],
            )
            .unwrap();
    }

    fn price(db: &Db, model: &str, status: &str, prompt: Option<f64>, completion: Option<f64>, cr: Option<f64>, cw: Option<f64>) {
        db.conn
            .execute(
                "INSERT INTO resolved_prices(provider_hint, raw_model, status, openrouter_id, prompt, completion, cache_read, cache_write)
                 VALUES ('p',?1,?2,'or/m',?3,?4,?5,?6)",
                params![model, status, prompt, completion, cr, cw],
            )
            .unwrap();
    }

    /// `m` priced at $1e-6 per token for every component.
    fn flat_price(db: &Db, model: &str) {
        price(db, model, "matched", Some(1e-6), Some(1e-6), None, None);
    }

    fn session(db: &Db, agent: &str, id: &str, cwd: Option<&str>, updated: Option<&str>, status: &str) {
        db.conn
            .execute(
                "INSERT INTO sessions(agent, session_id, cwd, title, started_ms, updated_ms, token_status) VALUES (?1,?2,?3,?4,?5,?5,?6)",
                params![agent, id, cwd, format!("title {id}"), updated.map(ms), status],
            )
            .unwrap();
    }

    fn cwd_project(db: &Db, cwd: &str, key: &str) {
        db.conn
            .execute("INSERT INTO cwd_projects(cwd, project_key, is_git) VALUES (?1,?2,1)", params![cwd, key])
            .unwrap();
    }

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn cost_uses_all_four_components() {
        let db = Db::open_in_memory().unwrap();
        price(&db, "m", "matched", Some(1e-6), Some(2e-6), Some(1e-7), Some(1.25e-6));
        insert(&db, Ev { input: 1000, output: 100, cache_read: 10000, cache_write: 100, ..Default::default() });
        let t = totals(&db, &this_week()).unwrap();
        approx(t.cost_usd, 0.002325);
        assert_eq!((t.tokens, t.unpriced_tokens, t.events), (11200, 0, 1));
    }

    #[test]
    fn cache_price_missing_falls_back_to_prompt() {
        let db = Db::open_in_memory().unwrap();
        price(&db, "m", "matched", Some(1e-6), Some(2e-6), None, None);
        insert(&db, Ev { cache_read: 1000, cache_write: 500, ..Default::default() });
        approx(totals(&db, &this_week()).unwrap().cost_usd, 0.0015);
    }

    #[test]
    fn unpriced_model_excluded_from_cost_and_counted() {
        let db = Db::open_in_memory().unwrap();
        flat_price(&db, "m");
        price(&db, "u", "unknown", None, None, None, None);
        insert(&db, Ev { key: "a", input: 10_000, ..Default::default() });
        insert(&db, Ev { key: "b", model: "u", input: 400, output: 100, ..Default::default() });
        // A model with no resolved_prices row at all is unpriced too.
        insert(&db, Ev { key: "c", model: "never-seen", input: 7, ..Default::default() });
        let t = totals(&db, &this_week()).unwrap();
        approx(t.cost_usd, 0.01);
        assert_eq!(t.unpriced_tokens, 507);
        assert_eq!(t.tokens, 10_507);
    }

    #[test]
    fn ignored_model_costs_zero_not_unpriced() {
        let db = Db::open_in_memory().unwrap();
        price(&db, "m", "ignored", None, None, None, None);
        insert(&db, Ev { input: 500, ..Default::default() });
        let t = totals(&db, &this_week()).unwrap();
        assert_eq!((t.cost_usd, t.unpriced_tokens, t.tokens), (0.0, 0, 500));
    }

    #[test]
    fn event_after_utc_evening_lands_on_next_local_day() {
        let db = Db::open_in_memory().unwrap();
        flat_price(&db, "m");
        insert(&db, Ev { ts: "2026-11-20T18:30:00Z", input: 100, ..Default::default() });
        let o = overview(&db, &PeriodSpec::ThisWeek, now()).unwrap();
        assert_eq!(o.daily.len(), 7);
        assert_eq!(o.daily[0].date, "2026-11-16");
        let sat = o.daily.iter().find(|d| d.date == "2026-11-21").unwrap();
        assert_eq!(sat.by_agent.len(), 1);
        assert_eq!((sat.by_agent[0].agent.as_str(), sat.by_agent[0].tokens), ("claude-code", 100));
        assert!(o.daily.iter().filter(|d| d.date != "2026-11-21").all(|d| d.by_agent.is_empty()));
        let last_week = resolve(&PeriodSpec::LastWeek, now());
        assert_eq!(totals(&db, &last_week).unwrap().events, 0);
    }

    #[test]
    fn change_pct_against_previous_period() {
        let db = Db::open_in_memory().unwrap();
        flat_price(&db, "m");
        insert(&db, Ev { key: "cur", input: 15_000_000, ..Default::default() });
        let o = overview(&db, &PeriodSpec::ThisWeek, now()).unwrap();
        assert_eq!(o.previous.cost_usd, 0.0);
        assert_eq!(o.change_pct, None);

        insert(&db, Ev { key: "prev", ts: LAST_WED, input: 10_000_000, ..Default::default() });
        let o = overview(&db, &PeriodSpec::ThisWeek, now()).unwrap();
        approx(o.totals.cost_usd, 15.0);
        approx(o.previous.cost_usd, 10.0);
        approx(o.change_pct.unwrap(), 50.0);
    }

    #[test]
    fn project_groups_subfolders() {
        let db = Db::open_in_memory().unwrap();
        flat_price(&db, "m");
        cwd_project(&db, "/r/a", "/r");
        cwd_project(&db, "/r/b", "/r");
        insert(&db, Ev { key: "1", session: "s1", cwd: Some("/r/a"), input: 100, ..Default::default() });
        insert(&db, Ev { key: "2", session: "s2", agent: "codex", cwd: Some("/r/b"), input: 300, ..Default::default() });
        db.conn
            .execute("INSERT INTO project_footprints(project_key, agent, marker) VALUES ('/r','codex','AGENTS.md')", [])
            .unwrap();
        let rows = projects(&db, &this_week()).unwrap();
        assert_eq!(rows.len(), 1);
        let p = &rows[0];
        assert_eq!((p.project_key.as_str(), p.name.as_str(), p.sessions, p.tokens), ("/r", "r", 2, 400));
        approx(p.cost_usd, 0.0004);
        assert_eq!(p.agents, vec!["claude-code", "codex"]);
        assert_eq!(p.footprints, vec!["codex"]);
    }

    #[test]
    fn event_without_cwd_uses_session_cwd_for_project() {
        let db = Db::open_in_memory().unwrap();
        cwd_project(&db, "/r/a", "/r");
        session(&db, "claude-code", "s1", Some("/r/a"), Some(WED), "recorded");
        insert(&db, Ev { cwd: None, input: 5, ..Default::default() });
        insert(&db, Ev { key: "orphan", session: "s9", cwd: None, input: 1, ..Default::default() });
        let rows = projects(&db, &this_week()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].project_key.as_str(), rows[0].tokens), ("/r", 5));
        assert_eq!(totals(&db, &this_week()).unwrap().tokens, 6);
    }

    #[test]
    fn session_project_is_majority_tokens() {
        let db = Db::open_in_memory().unwrap();
        cwd_project(&db, "/x", "/x");
        cwd_project(&db, "/y", "/y");
        session(&db, "claude-code", "s1", Some("/x"), Some(WED), "recorded");
        insert(&db, Ev { key: "1", cwd: Some("/x"), input: 100, ..Default::default() });
        insert(&db, Ev { key: "2", cwd: Some("/y"), input: 150, ..Default::default() });
        insert(&db, Ev { key: "3", cwd: Some("/y"), input: 100, ..Default::default() });
        let rows = sessions(&db, &SessionFilter::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project_key.as_deref(), Some("/y"));
        assert_eq!(rows[0].tokens, 350);
        let only_y = SessionFilter { project_key: Some("/y".into()), ..Default::default() };
        assert_eq!(sessions(&db, &only_y).unwrap().len(), 1);
        let only_x = SessionFilter { project_key: Some("/x".into()), ..Default::default() };
        assert!(sessions(&db, &only_x).unwrap().is_empty());
    }

    #[test]
    fn sessions_filter_range_and_not_recorded() {
        let db = Db::open_in_memory().unwrap();
        flat_price(&db, "m");
        price(&db, "u", "unknown", None, None, None, None);
        session(&db, "claude-code", "s1", Some("/r"), Some("2026-11-19T00:00:00+07:00"), "recorded");
        session(&db, "copilot-cli", "nr", Some("/r"), Some("2026-11-20T00:00:00+07:00"), "not-recorded");
        session(&db, "copilot-cli", "old", None, Some(LAST_WED), "not-recorded");
        session(&db, "codex", "s2", None, Some(LAST_WED), "recorded");
        insert(&db, Ev { key: "1", model: "u", input: 40, ..Default::default() });
        insert(&db, Ev { key: "2", model: "m", input: 1000, ..Default::default() });
        insert(&db, Ev { key: "3", ts: LAST_WED, input: 9, ..Default::default() });
        insert(&db, Ev { key: "4", agent: "codex", session: "s2", ts: LAST_WED, input: 9, ..Default::default() });
        let f = SessionFilter { range: Some(this_week()), ..Default::default() };
        let rows = sessions(&db, &f).unwrap();
        let ids: Vec<_> = rows.iter().map(|r| r.session_id.as_str()).collect();
        assert_eq!(ids, vec!["nr", "s1"]); // updated_ms desc
        let nr = &rows[0];
        assert_eq!((nr.tokens, nr.token_status.as_str(), nr.models.len()), (0, "not-recorded", 0));
        assert_eq!(nr.project_key.as_deref(), Some("/r")); // no cwd_projects row → raw session cwd
        let s1 = &rows[1];
        assert_eq!((s1.tokens, s1.unpriced_tokens), (1040, 40)); // event 3 outside the range
        approx(s1.cost_usd, 0.001);
        assert_eq!(s1.models, vec!["m", "u"]);
        assert_eq!(s1.title.as_deref(), Some("title s1"));

        let codex = SessionFilter { agent: Some("codex".into()), ..Default::default() };
        let rows = sessions(&db, &codex).unwrap();
        assert_eq!(rows.iter().map(|r| r.session_id.as_str()).collect::<Vec<_>>(), vec!["s2"]);
    }

    #[test]
    fn weekly_trend_buckets_are_mondays() {
        let db = Db::open_in_memory().unwrap();
        flat_price(&db, "m");
        insert(&db, Ev { key: "1", agent: "codex", input: 1_000_000, ..Default::default() });
        insert(&db, Ev { key: "2", input: 3_000_000, ..Default::default() });
        insert(&db, Ev { key: "3", ts: "2026-11-02T00:30:00+07:00", input: 7, ..Default::default() });
        insert(&db, Ev { key: "4", ts: "2026-11-01T23:30:00+07:00", input: 7, ..Default::default() }); // Sunday, too old
        let b = trends(&db, Granularity::Week, 3, Split::Agent, now()).unwrap();
        let labels: Vec<_> = b.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, vec!["2026-11-02", "2026-11-09", "2026-11-16"]);
        assert_eq!(b[0].start_ms, ms("2026-11-02T00:00:00+07:00"));
        assert_eq!(b[0].totals.tokens, 7);
        assert_eq!(b[1].totals.events, 0);
        assert!(b[1].by.is_empty());
        let by: Vec<_> = b[2].by.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(by, vec!["claude-code", "codex"]);
        approx(b[2].by[0].1, 3.0);
    }

    #[test]
    fn monthly_trend_split_by_project() {
        let db = Db::open_in_memory().unwrap();
        flat_price(&db, "m");
        cwd_project(&db, "/r/a", "/r");
        insert(&db, Ev { key: "1", cwd: Some("/r/a"), ts: "2026-10-31T23:00:00+07:00", input: 2_000_000, ..Default::default() });
        let b = trends(&db, Granularity::Month, 2, Split::Project, now()).unwrap();
        assert_eq!(b.iter().map(|b| b.label.as_str()).collect::<Vec<_>>(), vec!["2026-10", "2026-11"]);
        assert_eq!(b[1].start_ms, ms("2026-11-01T00:00:00+07:00"));
        assert_eq!(b[0].by.len(), 1);
        assert_eq!(b[0].by[0].0, "/r");
        approx(b[0].by[0].1, 2.0);
        assert_eq!(b[1].totals.events, 0);
    }

    #[test]
    fn session_detail_lists_events_in_time_order() {
        let db = Db::open_in_memory().unwrap();
        flat_price(&db, "m");
        session(&db, "claude-code", "s1", Some("/r"), Some(WED), "recorded");
        insert(&db, Ev { key: "late", ts: "2026-11-18T13:00:00+07:00", input: 2, ..Default::default() });
        insert(&db, Ev { key: "early", ts: "2026-11-18T11:00:00+07:00", model: "u", input: 1, ..Default::default() });
        insert(&db, Ev { key: "other", session: "s2", input: 5, ..Default::default() });
        let d = session_detail(&db, "claude-code", "s1").unwrap().unwrap();
        assert_eq!(d.row.session_id, "s1");
        assert_eq!(d.row.tokens, 3);
        assert_eq!(d.events.len(), 2);
        assert_eq!(d.events[0].ts_ms, ms("2026-11-18T11:00:00+07:00"));
        assert_eq!((d.events[0].cost_usd, d.events[0].price_status.as_str()), (None, "unknown"));
        assert_eq!(d.events[1].price_status, "matched");
        approx(d.events[1].cost_usd.unwrap(), 2e-6);
        assert_eq!(d.events[1].cwd.as_deref(), Some("/r"));
        assert!(session_detail(&db, "claude-code", "nope").unwrap().is_none());
    }

    #[test]
    fn models_report_status_and_totals() {
        let db = Db::open_in_memory().unwrap();
        price(&db, "m", "matched", Some(1e-6), Some(2e-6), None, None);
        insert(&db, Ev { key: "1", input: 1000, output: 500, reasoning: 200, ..Default::default() });
        insert(&db, Ev { key: "2", model: "u", input: 10, ..Default::default() });
        let rows = models(&db, &this_week()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].model.as_str(), rows[0].status.as_str()), ("m", "matched"));
        assert_eq!(rows[0].openrouter_id.as_deref(), Some("or/m"));
        assert_eq!((rows[0].prompt, rows[0].completion), (Some(1e-6), Some(2e-6)));
        approx(rows[0].totals.cost_usd, 0.002);
        assert_eq!((rows[0].totals.reasoning, rows[0].totals.output), (200, 500));
        assert_eq!((rows[1].model.as_str(), rows[1].status.as_str()), ("u", "unknown"));
        assert_eq!(rows[1].totals.unpriced_tokens, 10);
    }

    #[test]
    fn overview_top_lists() {
        let db = Db::open_in_memory().unwrap();
        flat_price(&db, "m");
        for (i, key) in ["a", "b", "c", "d", "e", "f"].into_iter().enumerate() {
            let (cwd, sid) = (format!("/p{i}"), format!("s{i}"));
            insert(&db, Ev { key, session: &sid, cwd: Some(&cwd), input: (i as i64 + 1) * 1000, ..Default::default() });
        }
        let o = overview(&db, &PeriodSpec::ThisWeek, now()).unwrap();
        assert_eq!(o.top_projects.len(), 5);
        assert_eq!(o.top_projects[0].project_key, "/p5");
        assert_eq!(o.top_sessions.len(), 5);
        assert_eq!(o.top_sessions[0].session_id, "s5");
        assert_eq!(o.top_sessions[4].session_id, "s1");
    }
}
