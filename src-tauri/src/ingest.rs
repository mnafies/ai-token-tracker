use crate::adapters::{Adapter, Source, SourceKind, SourceState};
use crate::db::Db;
use crate::model::{AgentKind, ParseOutput};
use crate::project::ensure_cwd_projects;
use anyhow::anyhow;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::UNIX_EPOCH;

#[derive(Debug, Default, Clone)]
pub struct IngestReport {
    pub sources_read: usize,
    pub events: usize,
    pub sessions: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub agent: AgentKind,
    pub done: usize,
    pub total: usize,
}

impl IngestReport {
    fn add(&mut self, other: &IngestReport) {
        self.sources_read += other.sources_read;
        self.events += other.events;
        self.sessions += other.sessions;
        self.errors += other.errors;
    }
}

fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Current (size, mtime_ms) of a source, defined exactly as the adapters report it
/// in their returned `SourceState`. `None` when the source no longer exists.
fn fingerprint(src: &Source) -> Option<(u64, i64)> {
    let meta = std::fs::metadata(&src.path).ok()?;
    Some(match src.kind {
        SourceKind::Jsonl => (meta.len(), mtime_ms(&meta)),
        SourceKind::Dir => (0, mtime_ms(&meta)),
        SourceKind::Sqlite => {
            let mut wal = src.path.as_os_str().to_owned();
            wal.push("-wal");
            let (mut size, mut mtime) = (meta.len(), mtime_ms(&meta));
            if let Ok(w) = std::fs::metadata(PathBuf::from(wal)) {
                size += w.len();
                mtime = mtime.max(mtime_ms(&w));
            }
            (size, mtime)
        }
    })
}

fn lock(db: &Mutex<Db>) -> anyhow::Result<MutexGuard<'_, Db>> {
    db.lock().map_err(|_| anyhow!("database lock poisoned"))
}

fn cwds_of(out: &ParseOutput) -> Vec<String> {
    let mut cwds: Vec<String> = out
        .events
        .iter()
        .filter_map(|e| e.cwd.clone())
        .chain(out.sessions.iter().filter_map(|s| s.cwd.clone()))
        .collect();
    cwds.sort();
    cwds.dedup();
    cwds
}

enum Outcome {
    Skipped,
    Read { events: usize, sessions: usize },
}

fn ingest_source(db: &Mutex<Db>, a: &dyn Adapter, src: &Source) -> anyhow::Result<Outcome> {
    let Some((size, mtime_ms)) = fingerprint(src) else { return Ok(Outcome::Skipped) };
    let path = src.path.to_string_lossy();
    let stored = lock(db)?.source_state(&path)?;
    let prev = match stored {
        Some(p) if p.size == size && p.mtime_ms == mtime_ms => return Ok(Outcome::Skipped),
        // File was rewritten smaller than what we already consumed: reparse from scratch.
        // Event upserts by event_key keep this idempotent.
        Some(p) if src.kind == SourceKind::Jsonl && size < p.offset => SourceState::default(),
        Some(p) => p,
        None => SourceState::default(),
    };
    let (out, state) = a.parse(src, &prev)?;
    let mut guard = lock(db)?;
    guard.apply_parse(&path, a.kind(), &out, &state)?;
    ensure_cwd_projects(&guard, &cwds_of(&out))?;
    Ok(Outcome::Read { events: out.events.len(), sessions: out.sessions.len() })
}

pub fn ingest_adapter(db: &Mutex<Db>, a: &dyn Adapter, on_progress: &dyn Fn(Progress)) -> anyhow::Result<IngestReport> {
    let sources = a.sources();
    let total = sources.len();
    let mut report = IngestReport::default();
    for (i, src) in sources.iter().enumerate() {
        match ingest_source(db, a, src) {
            Ok(Outcome::Skipped) => {}
            Ok(Outcome::Read { events, sessions }) => {
                report.sources_read += 1;
                report.events += events;
                report.sessions += sessions;
            }
            Err(e) => {
                eprintln!("ingest {}: {}: {e:#}", a.kind().as_str(), src.path.display());
                report.errors += 1;
            }
        }
        on_progress(Progress { agent: a.kind(), done: i + 1, total });
    }
    Ok(report)
}

pub fn ingest_all(db: &Mutex<Db>, adapters: &[Box<dyn Adapter>], on_progress: &dyn Fn(Progress)) -> anyhow::Result<IngestReport> {
    let mut total = IngestReport::default();
    for a in adapters {
        total.add(&ingest_adapter(db, a.as_ref(), on_progress)?);
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::adapter_for;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
    }

    fn copy_dir(from: &Path, to: &Path) {
        for entry in walkdir::WalkDir::new(from) {
            let entry = entry.unwrap();
            let dest = to.join(entry.path().strip_prefix(from).unwrap());
            if entry.file_type().is_dir() {
                fs::create_dir_all(&dest).unwrap();
            } else {
                fs::copy(entry.path(), &dest).unwrap();
            }
        }
    }

    /// Temp root holding a copy of the given fixture tree.
    fn temp_root(name: &str) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        copy_dir(&fixture(name), t.path());
        t
    }

    fn db() -> Mutex<Db> {
        Mutex::new(Db::open_in_memory().unwrap())
    }

    fn count(db: &Mutex<Db>, sql: &str) -> i64 {
        db.lock().unwrap().conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn events(db: &Mutex<Db>) -> i64 {
        count(db, "SELECT COUNT(*) FROM events")
    }

    fn run(db: &Mutex<Db>, a: &dyn Adapter) -> IngestReport {
        ingest_adapter(db, a, &|_| {}).unwrap()
    }

    fn claude_session(root: &Path) -> PathBuf {
        root.join("projects/-tmp-demo/sess-1.jsonl")
    }

    fn assistant_line(id: &str) -> String {
        format!(
            r#"{{"type":"assistant","sessionId":"sess-1","cwd":"/tmp/demo","timestamp":"2026-11-20T01:05:00.000Z","uuid":"u-{id}","message":{{"id":"{id}","model":"claude-opus-5-5","usage":{{"input_tokens":1,"output_tokens":2}}}}}}"#
        )
    }

    fn append(path: &Path, s: &str) {
        fs::OpenOptions::new().append(true).open(path).unwrap().write_all(s.as_bytes()).unwrap();
    }

    #[test]
    fn second_ingest_without_changes_reads_nothing() {
        let t = temp_root("claude");
        let a = adapter_for(AgentKind::ClaudeCode, t.path().to_path_buf());
        let db = db();
        let first = run(&db, a.as_ref());
        assert_eq!(first.sources_read, 2);
        assert!(first.events > 0);
        let second = run(&db, a.as_ref());
        assert_eq!(second.sources_read, 0);
        assert_eq!(second.events, 0);
    }

    #[test]
    fn appended_lines_are_ingested_incrementally() {
        let t = temp_root("claude");
        let a = adapter_for(AgentKind::ClaudeCode, t.path().to_path_buf());
        let db = db();
        run(&db, a.as_ref());
        let before = events(&db);
        append(&claude_session(t.path()), &format!("{}\n", assistant_line("msg_NEW")));
        let r = run(&db, a.as_ref());
        assert_eq!(r.sources_read, 1);
        assert_eq!(events(&db), before + 1);
    }

    #[test]
    fn partial_trailing_line_is_read_next_time() {
        let t = temp_root("claude");
        let a = adapter_for(AgentKind::ClaudeCode, t.path().to_path_buf());
        let db = db();
        run(&db, a.as_ref());
        let before = events(&db);
        let path = claude_session(t.path());
        append(&path, &assistant_line("msg_NEW"));
        run(&db, a.as_ref());
        assert_eq!(events(&db), before);
        append(&path, "\n");
        run(&db, a.as_ref());
        assert_eq!(events(&db), before + 1);
    }

    #[test]
    fn deleted_source_keeps_events() {
        let t = temp_root("claude");
        let a = adapter_for(AgentKind::ClaudeCode, t.path().to_path_buf());
        let db = db();
        run(&db, a.as_ref());
        let before = events(&db);
        assert!(before > 0);
        fs::remove_file(claude_session(t.path())).unwrap();
        let r = run(&db, a.as_ref());
        assert_eq!(r.errors, 0);
        assert_eq!(events(&db), before);
    }

    #[test]
    fn rewritten_smaller_file_is_reparsed_without_duplicates() {
        let t = temp_root("claude");
        let a = adapter_for(AgentKind::ClaudeCode, t.path().to_path_buf());
        let db = db();
        run(&db, a.as_ref());
        let before = events(&db);
        let path = claude_session(t.path());
        let old_len = fs::metadata(&path).unwrap().len();
        // Keep title, user and the first msg_A line, then a new event: smaller than before.
        let original = fs::read_to_string(&path).unwrap();
        let mut subset: String = original.lines().take(3).map(|l| format!("{l}\n")).collect();
        subset.push_str(&format!("{}\n", assistant_line("msg_NEW")));
        assert!((subset.len() as u64) < old_len);
        fs::write(&path, subset).unwrap();
        run(&db, a.as_ref());
        assert_eq!(events(&db), before + 1);
        assert_eq!(count(&db, "SELECT COUNT(DISTINCT event_key) FROM events"), before + 1);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM events WHERE event_key = 'claude:msg_A'"), 1);
    }

    #[test]
    fn codex_archived_copy_counted_once() {
        let t = temp_root("codex");
        let archived = t.path().join("archived_sessions");
        fs::create_dir_all(&archived).unwrap();
        fs::copy(
            t.path().join("sessions/2026/11/20/rollout-cx-1.jsonl"),
            archived.join("rollout-cx-1.jsonl"),
        )
        .unwrap();
        let a = adapter_for(AgentKind::Codex, t.path().to_path_buf());
        let db = db();
        let r = run(&db, a.as_ref());
        assert_eq!(r.sources_read, 2);
        assert_eq!(events(&db), 2);
    }

    #[test]
    fn cwds_get_project_rows() {
        let t = temp_root("claude");
        let a = adapter_for(AgentKind::ClaudeCode, t.path().to_path_buf());
        let db = db();
        run(&db, a.as_ref());
        assert_eq!(count(&db, "SELECT COUNT(*) FROM cwd_projects WHERE cwd = '/tmp/demo'"), 1);
    }
}
