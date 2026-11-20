use crate::adapters::SourceState;
use crate::model::{AgentKind, ParseOutput};
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub struct Db {
    pub conn: Connection,
}

const SCHEMA_V1: &str = include_str!("schema.sql");

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::migrate(conn)
    }

    pub fn open_in_memory() -> Result<Db> {
        Self::migrate(Connection::open_in_memory()?)
    }

    fn migrate(conn: Connection) -> Result<Db> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version == 0 {
            conn.execute_batch(&format!("BEGIN;\n{SCHEMA_V1}\nPRAGMA user_version = 1;\nCOMMIT;"))?;
        }
        Ok(Db { conn })
    }

    pub fn apply_parse(&mut self, source_path: &str, agent: AgentKind, out: &ParseOutput, state: &SourceState) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut ev = tx.prepare_cached(
                "INSERT INTO events(event_key, agent, session_id, ts_ms, cwd, model, provider_hint, input, output, reasoning, cache_read, cache_write)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                 ON CONFLICT(event_key) DO UPDATE SET agent=excluded.agent, session_id=excluded.session_id, ts_ms=excluded.ts_ms,
                   cwd=excluded.cwd, model=excluded.model, provider_hint=excluded.provider_hint, input=excluded.input,
                   output=excluded.output, reasoning=excluded.reasoning, cache_read=excluded.cache_read, cache_write=excluded.cache_write",
            )?;
            for e in &out.events {
                ev.execute(params![
                    e.event_key, e.agent.as_str(), e.session_id, e.ts_ms, e.cwd, e.model, e.provider_hint,
                    e.input, e.output, e.reasoning, e.cache_read, e.cache_write
                ])?;
            }
            let mut se = tx.prepare_cached(
                "INSERT INTO sessions(agent, session_id, cwd, title, started_ms, updated_ms, token_status)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(agent, session_id) DO UPDATE SET
                   cwd = COALESCE(sessions.cwd, excluded.cwd),
                   title = COALESCE(excluded.title, sessions.title),
                   started_ms = CASE WHEN sessions.started_ms IS NULL THEN excluded.started_ms
                                     WHEN excluded.started_ms IS NULL THEN sessions.started_ms
                                     ELSE MIN(sessions.started_ms, excluded.started_ms) END,
                   updated_ms = CASE WHEN sessions.updated_ms IS NULL THEN excluded.updated_ms
                                     WHEN excluded.updated_ms IS NULL THEN sessions.updated_ms
                                     ELSE MAX(sessions.updated_ms, excluded.updated_ms) END,
                   token_status = CASE WHEN sessions.token_status = 'recorded' OR excluded.token_status = 'recorded'
                                       THEN 'recorded' ELSE 'not-recorded' END",
            )?;
            for s in &out.sessions {
                se.execute(params![
                    s.agent.as_str(), s.session_id, s.cwd, s.title, s.started_ms, s.updated_ms, s.token_status.as_str()
                ])?;
            }
            tx.execute(
                "INSERT INTO source_state(path, agent, offset, size, mtime_ms, context) VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(path) DO UPDATE SET agent=excluded.agent, offset=excluded.offset, size=excluded.size,
                   mtime_ms=excluded.mtime_ms, context=excluded.context",
                params![source_path, agent.as_str(), state.offset as i64, state.size as i64, state.mtime_ms, serde_json::to_string(&state.context)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn source_state(&self, source_path: &str) -> Result<Option<SourceState>> {
        let row = self.conn
            .query_row(
                "SELECT offset, size, mtime_ms, context FROM source_state WHERE path = ?1",
                params![source_path],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?)),
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some((offset, size, mtime_ms, ctx)) => Ok(Some(SourceState {
                offset: offset as u64,
                size: size as u64,
                mtime_ms,
                context: serde_json::from_str(&ctx)?,
            })),
        }
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn
            .query_row("SELECT value FROM app_meta WHERE key = ?1", params![key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO app_meta(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Delete all rows except user configuration (settings, model_overrides).
    pub fn wipe_all(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        for t in [
            "events", "sessions", "cwd_projects", "source_state", "prices", "resolved_prices",
            "app_meta", "discovery_candidates", "discovery_denied", "project_footprints",
        ] {
            tx.execute(&format!("DELETE FROM {t}"), [])?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SessionMeta, TokenStatus, UsageEvent};

    fn ev(key: &str, output: i64) -> UsageEvent {
        UsageEvent {
            agent: AgentKind::ClaudeCode, event_key: key.into(), session_id: "s1".into(), ts_ms: 1000,
            cwd: Some("/p".into()), model: "m".into(), provider_hint: "anthropic".into(),
            input: 1, output, reasoning: 0, cache_read: 0, cache_write: 0,
        }
    }
    fn sess(started: i64, updated: i64, title: Option<&str>) -> SessionMeta {
        SessionMeta {
            agent: AgentKind::ClaudeCode, session_id: "s1".into(), cwd: None, title: title.map(String::from),
            started_ms: Some(started), updated_ms: Some(updated), token_status: TokenStatus::Recorded,
        }
    }

    #[test]
    fn apply_parse_is_idempotent_and_last_write_wins() {
        let mut db = Db::open_in_memory().unwrap();
        let st = SourceState::default();
        let o1 = ParseOutput { events: vec![ev("k1", 10)], sessions: vec![] };
        db.apply_parse("/f", AgentKind::ClaudeCode, &o1, &st).unwrap();
        let o2 = ParseOutput { events: vec![ev("k1", 50)], sessions: vec![] };
        db.apply_parse("/f", AgentKind::ClaudeCode, &o2, &st).unwrap();
        let n: i64 = db.conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap();
        let out: i64 = db.conn.query_row("SELECT output FROM events WHERE event_key='k1'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        assert_eq!(out, 50);
    }

    #[test]
    fn session_merge_keeps_min_start_max_update_and_latest_title() {
        let mut db = Db::open_in_memory().unwrap();
        let st = SourceState::default();
        let o1 = ParseOutput { events: vec![], sessions: vec![sess(200, 300, None)] };
        db.apply_parse("/f", AgentKind::ClaudeCode, &o1, &st).unwrap();
        let o2 = ParseOutput { events: vec![], sessions: vec![sess(100, 250, Some("T"))] };
        db.apply_parse("/f", AgentKind::ClaudeCode, &o2, &st).unwrap();
        let (s, u, t): (i64, i64, String) = db.conn
            .query_row("SELECT started_ms, updated_ms, title FROM sessions", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!((s, u, t.as_str()), (100, 300, "T"));
    }

    #[test]
    fn source_state_roundtrip() {
        let mut db = Db::open_in_memory().unwrap();
        let st = SourceState { offset: 42, size: 100, mtime_ms: 7, context: serde_json::json!({"m":"x"}) };
        db.apply_parse("/f", AgentKind::Codex, &ParseOutput::default(), &st).unwrap();
        assert_eq!(db.source_state("/f").unwrap(), Some(st));
        assert_eq!(db.source_state("/none").unwrap(), None);
    }

    #[test]
    fn wipe_all_keeps_settings() {
        let mut db = Db::open_in_memory().unwrap();
        db.conn.execute("INSERT INTO settings(key,value) VALUES('a','b')", []).unwrap();
        let o = ParseOutput { events: vec![ev("k1", 1)], sessions: vec![] };
        db.apply_parse("/f", AgentKind::ClaudeCode, &o, &SourceState::default()).unwrap();
        db.wipe_all().unwrap();
        let e: i64 = db.conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap();
        let s: i64 = db.conn.query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0)).unwrap();
        assert_eq!((e, s), (0, 1));
        let m: i64 = db.conn.query_row("SELECT COUNT(*) FROM model_overrides", [], |r| r.get(0)).unwrap();
        assert_eq!(m, 2);
    }
}
