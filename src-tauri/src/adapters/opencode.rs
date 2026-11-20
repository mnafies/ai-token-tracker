use super::{Adapter, Source, SourceKind, SourceState};
use crate::model::{AgentKind, ParseOutput, SessionMeta, TokenStatus, UsageEvent};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::{Path, PathBuf};

const OVERLAP_MS: i64 = 60_000;

pub struct OpenCodeAdapter {
    pub root: PathBuf,
}

impl OpenCodeAdapter {
    fn db_path(&self) -> PathBuf {
        self.root.join("opencode.db")
    }
}

impl Adapter for OpenCodeAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::OpenCode
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn detect(&self) -> bool {
        self.root.exists()
    }

    fn sources(&self) -> Vec<Source> {
        let path = self.db_path();
        if path.is_file() { vec![Source { path, kind: SourceKind::Sqlite }] } else { Vec::new() }
    }

    fn parse(&self, src: &Source, prev: &SourceState) -> anyhow::Result<(ParseOutput, SourceState)> {
        let conn = Connection::open_with_flags(
            &src.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        // Overlap window: rows may commit later with an older time_updated than the
        // high-water mark (same-ms writes, session bumped past its messages). Re-reading
        // is safe because event_key upserts are idempotent.
        let since = (prev.offset as i64).saturating_sub(OVERLAP_MS).max(0);
        let mut max_updated = prev.offset as i64;

        let mut sessions = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id, directory, title, time_created, time_updated FROM session_v2 WHERE time_updated > ?1",
        )?;
        let rows = stmt.query_map([since], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })?;
        for row in rows {
            let (id, directory, title, created, updated) = row?;
            if let Some(u) = updated {
                max_updated = max_updated.max(u);
            }
            sessions.push(SessionMeta {
                agent: AgentKind::OpenCode,
                session_id: id,
                cwd: directory,
                title: title.filter(|t| !t.is_empty()),
                started_ms: created,
                updated_ms: updated,
                token_status: TokenStatus::Recorded,
            });
        }

        let mut events = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT m.id, m.session_id, m.time_updated, m.data, s.directory FROM session_message m \
             JOIN session_v2 s ON s.id = m.session_id WHERE m.type='assistant' AND m.time_updated > ?1",
        )?;
        let rows = stmt.query_map([since], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?;
        for row in rows {
            let (id, session_id, updated, data, directory) = row?;
            if let Some(u) = updated {
                max_updated = max_updated.max(u);
            }
            let Some(v) = data.and_then(|d| serde_json::from_str::<Value>(&d).ok()) else { continue };
            let (Some(tokens), Some(ts_ms)) =
                (v.get("tokens").filter(|t| t.is_object()), v.pointer("/time/created").and_then(Value::as_i64))
            else {
                continue;
            };
            let n = |p: &str| tokens.pointer(p).and_then(Value::as_i64).unwrap_or(0);
            let s = |p: &str| v.pointer(p).and_then(Value::as_str).unwrap_or("").to_string();
            events.push(UsageEvent {
                agent: AgentKind::OpenCode,
                event_key: format!("opencode:{id}"),
                session_id,
                ts_ms,
                cwd: directory,
                model: s("/model/id"),
                provider_hint: s("/model/providerID"),
                input: n("/input"),
                // opencode's `output` excludes reasoning; normalized output includes it.
                output: n("/output") + n("/reasoning"),
                reasoning: n("/reasoning"),
                cache_read: n("/cache/read"),
                cache_write: n("/cache/write"),
            });
        }

        // WAL mode: new rows land in the -wal file, so include it in size/mtime.
        let mut wal = src.path.as_os_str().to_owned();
        wal.push("-wal");
        let (mut size, mut mtime_ms) = (0u64, 0i64);
        for p in [src.path.clone(), PathBuf::from(wal)] {
            let Ok(meta) = std::fs::metadata(&p) else { continue };
            size += meta.len();
            let m = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            mtime_ms = mtime_ms.max(m);
        }

        Ok((
            ParseOutput { events, sessions },
            SourceState { offset: max_updated.max(0) as u64, size, mtime_ms, context: Value::Null },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSISTANT: &str = r#"{"time":{"created":1795136400000},"model":{"id":"gpt-5.6-luna-fast","providerID":"openai"},"tokens":{"input":5603,"output":173,"reasoning":32,"cache":{"read":1536,"write":0}}}"#;

    fn make_db(dir: &Path) -> Source {
        let path = dir.join("opencode.db");
        let c = Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE session_v2(id TEXT, project_id TEXT, slug TEXT, directory TEXT, title TEXT, version TEXT, time_created INTEGER, time_updated INTEGER);
             CREATE TABLE session_message(id TEXT, session_id TEXT, type TEXT, seq INTEGER, time_created INTEGER, time_updated INTEGER, data TEXT);
             INSERT INTO session_v2 VALUES('ses_1','p','s','/tmp/oc','Explore repo','1',1795136000000,1795136500000);",
        )
        .unwrap();
        c.execute(
            "INSERT INTO session_message VALUES('msg_a','ses_1','assistant',1,1795136400000,1795136500000,?1)",
            [ASSISTANT],
        )
        .unwrap();
        c.execute(
            "INSERT INTO session_message VALUES('msg_u','ses_1','user',0,1795136300000,1795136300000,'{\"text\":\"hi\"}')",
            [],
        )
        .unwrap();
        Source { path, kind: SourceKind::Sqlite }
    }

    #[test]
    fn opencode_reads_assistant_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let src = make_db(dir.path());
        let a = OpenCodeAdapter { root: dir.path().to_path_buf() };
        assert!(a.detect());
        assert_eq!(a.sources(), vec![src.clone()]);
        let (out, state) = a.parse(&src, &SourceState::default()).unwrap();
        assert_eq!(out.events.len(), 1);
        let e = &out.events[0];
        assert_eq!((e.input, e.output, e.reasoning, e.cache_read, e.cache_write), (5603, 205, 32, 1536, 0));
        assert_eq!(e.model, "gpt-5.6-luna-fast");
        assert_eq!(e.provider_hint, "openai");
        assert_eq!(e.cwd.as_deref(), Some("/tmp/oc"));
        assert_eq!(e.event_key, "opencode:msg_a");
        assert_eq!(e.session_id, "ses_1");
        assert_eq!(e.ts_ms, 1795136400000);
        assert_eq!(state.offset, 1795136500000);
        assert!(state.size > 0);
    }

    #[test]
    fn opencode_session_title() {
        let dir = tempfile::tempdir().unwrap();
        let src = make_db(dir.path());
        let a = OpenCodeAdapter { root: dir.path().to_path_buf() };
        let (out, _) = a.parse(&src, &SourceState::default()).unwrap();
        assert_eq!(out.sessions.len(), 1);
        let s = &out.sessions[0];
        assert_eq!(s.title.as_deref(), Some("Explore repo"));
        assert_eq!(s.session_id, "ses_1");
        assert_eq!(s.cwd.as_deref(), Some("/tmp/oc"));
        assert_eq!((s.started_ms, s.updated_ms), (Some(1795136000000), Some(1795136500000)));
    }

    #[test]
    fn opencode_incremental_uses_time_updated() {
        let dir = tempfile::tempdir().unwrap();
        let src = make_db(dir.path());
        let a = OpenCodeAdapter { root: dir.path().to_path_buf() };
        let (first, state) = a.parse(&src, &SourceState::default()).unwrap();
        let (out, state2) = a.parse(&src, &state).unwrap();
        // Overlap window may re-emit rows, but never new event keys.
        for e in &out.events {
            assert!(first.events.iter().any(|f| f.event_key == e.event_key));
        }
        assert_eq!(state2.offset, state.offset);
        // Rows far older than the overlap window are not re-read.
        let far = SourceState { offset: state.offset + 120_000, ..state.clone() };
        let (out, state3) = a.parse(&src, &far).unwrap();
        assert!(out.events.is_empty() && out.sessions.is_empty());
        assert_eq!(state3.offset, far.offset);
    }

    #[test]
    fn opencode_late_row_inside_overlap_is_picked_up() {
        let dir = tempfile::tempdir().unwrap();
        let src = make_db(dir.path());
        let a = OpenCodeAdapter { root: dir.path().to_path_buf() };
        let (_, state) = a.parse(&src, &SourceState::default()).unwrap();
        let c = Connection::open(&src.path).unwrap();
        let late = state.offset as i64 - 1000;
        c.execute(
            "INSERT INTO session_message VALUES('late','ses_1','assistant',9,1,?1,?2)",
            rusqlite::params![late, ASSISTANT],
        )
        .unwrap();
        drop(c);
        let (out, state2) = a.parse(&src, &state).unwrap();
        assert!(out.events.iter().any(|e| e.event_key == "opencode:late"));
        assert_eq!(state2.offset, state.offset);
    }

    #[test]
    fn opencode_skips_messages_without_tokens_or_time() {
        let dir = tempfile::tempdir().unwrap();
        let src = make_db(dir.path());
        let c = Connection::open(&src.path).unwrap();
        c.execute("INSERT INTO session_message VALUES('m2','ses_1','assistant',2,1,1795136600000,'{\"time\":{\"created\":5}}')", []).unwrap();
        c.execute("INSERT INTO session_message VALUES('m3','ses_1','assistant',3,1,1795136700000,'{\"tokens\":{\"input\":1}}')", []).unwrap();
        c.execute("INSERT INTO session_message VALUES('m4','ses_1','assistant',4,1,1795136800000,'{\"time\":{\"created\":6},\"tokens\":{\"input\":7}}')", []).unwrap();
        drop(c);
        let a = OpenCodeAdapter { root: dir.path().to_path_buf() };
        let (out, state) = a.parse(&src, &SourceState::default()).unwrap();
        assert_eq!(out.events.len(), 2);
        let e = out.events.iter().find(|e| e.event_key == "opencode:m4").unwrap();
        assert_eq!((e.input, e.output, e.cache_read, e.model.as_str()), (7, 0, 0, ""));
        assert_eq!(state.offset, 1795136800000);
    }

    #[test]
    fn opencode_never_writes() {
        let dir = tempfile::tempdir().unwrap();
        let src = make_db(dir.path());
        let before = std::fs::read(&src.path).unwrap();
        let a = OpenCodeAdapter { root: dir.path().to_path_buf() };
        a.parse(&src, &SourceState::default()).unwrap();
        assert_eq!(before, std::fs::read(&src.path).unwrap());
    }

    #[test]
    fn opencode_detect_and_sources_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let a = OpenCodeAdapter { root: dir.path().join("nope") };
        assert!(!a.detect());
        assert!(a.sources().is_empty());
        let a = OpenCodeAdapter { root: dir.path().to_path_buf() };
        assert!(a.detect());
        assert!(a.sources().is_empty());
    }
}
