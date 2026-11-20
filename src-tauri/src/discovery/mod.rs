pub mod data_scan;
pub mod footprint;
pub mod walk;

use crate::adapters::{Adapter, adapter_for};
use crate::db::Db;
use crate::model::AgentKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

pub fn run_discovery(db: &Mutex<Db>, roots: &[PathBuf], home: &Path, known_roots: &[PathBuf], cancel: &AtomicBool) -> anyhow::Result<()> {
    let (candidates, mut denied) = data_scan::scan_data(roots, home, known_roots, cancel);
    let (footprints, denied2) = footprint::scan_footprints(roots, home, cancel);
    denied.extend(denied2);
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(()); // partial scan: keep previous results untouched
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let mut guard = db.lock().map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    let tx = guard.conn.transaction()?;
    {
        let mut up = tx.prepare(
            "INSERT INTO discovery_candidates (path, guessed_agent, known_format, file_count, found_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(path) DO UPDATE SET guessed_agent=excluded.guessed_agent, \
             known_format=excluded.known_format, file_count=excluded.file_count",
        )?;
        for c in &candidates {
            up.execute(rusqlite::params![c.path, c.guessed_agent, c.known_format.map(|k| k.as_str()), c.file_count, now_ms])?;
        }
    }
    tx.execute("DELETE FROM project_footprints", [])?;
    {
        let mut ins = tx.prepare("INSERT OR IGNORE INTO project_footprints (project_key, agent, marker) VALUES (?1, ?2, ?3)")?;
        for f in &footprints {
            ins.execute(rusqlite::params![f.project_key, f.agent, f.marker])?;
        }
    }
    tx.execute("DELETE FROM discovery_denied", [])?;
    {
        let mut ins = tx.prepare("INSERT OR IGNORE INTO discovery_denied (path, found_ms) VALUES (?1, ?2)")?;
        for p in &denied {
            ins.execute(rusqlite::params![p.to_string_lossy(), now_ms])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn tracked_adapters(db: &Db) -> anyhow::Result<Vec<Box<dyn Adapter>>> {
    let mut st = db
        .conn
        .prepare("SELECT path, known_format FROM discovery_candidates WHERE decision='track' AND known_format IS NOT NULL")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (path, fmt) = row?;
        if let Some(kind) = AgentKind::from_str(&fmt) {
            out.push(adapter_for(kind, PathBuf::from(path)));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::data_scan::{scan_data, sniff_file};
    use super::footprint::scan_footprints;
    use super::walk::walk;
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;

    const CLAUDE_LINE: &str = r#"{"type":"assistant","sessionId":"sess-1","cwd":"/tmp/demo","timestamp":"2026-11-20T01:00:05.000Z","uuid":"u1","requestId":"r1","message":{"id":"msg_A","model":"claude-opus-5-5","usage":{"input_tokens":10,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":50}}}"#;
    const CODEX_LINES: &str = concat!(
        r#"{"timestamp":"2026-11-20T02:00:00.000Z","type":"session_meta","payload":{"id":"cx-1","cwd":"/tmp/demo"}}"#, "\n",
        r#"{"timestamp":"2026-11-20T02:00:01.000Z","type":"turn_context","payload":{"cwd":"/tmp/demo/sub","model":"gpt-6-sol"}}"#, "\n",
        r#"{"timestamp":"2026-11-20T02:00:03.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"output_tokens":5}}}}"#, "\n",
    );

    fn write(p: &std::path::Path, s: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }
    fn canon(p: &std::path::Path) -> String {
        fs::canonicalize(p).unwrap().to_string_lossy().into_owned()
    }
    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn walk_skips_node_modules_and_caches() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        for f in ["proj/a.txt", "proj/node_modules/x.txt", "Library/Caches/c.txt", "Library/Application Support/s.txt", "Library/Other/o.txt", "Library/Containers/k.txt"] {
            write(&home.join(f), "x");
        }
        let mut seen = Vec::new();
        let mut denied = Vec::new();
        walk(&[home.to_path_buf()], home, &no_cancel(), &mut |e| {
            if e.file_type().is_file() {
                seen.push(e.path().strip_prefix(home).unwrap().to_string_lossy().into_owned());
            }
        }, &mut denied);
        seen.sort();
        assert_eq!(seen, vec!["Library/Application Support/s.txt", "proj/a.txt"]);
        assert!(denied.is_empty());
    }

    #[test]
    fn walk_does_not_follow_symlinks() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        write(&home.join("real.txt"), "x");
        write(&t.path().join("outside/secret.txt"), "x");
        std::os::unix::fs::symlink(t.path().join("outside"), home.join("link")).unwrap();
        let mut seen = Vec::new();
        let mut denied = Vec::new();
        walk(&[home.clone()], &home, &no_cancel(), &mut |e| {
            seen.push(e.file_name().to_string_lossy().into_owned());
        }, &mut denied);
        assert!(seen.contains(&"real.txt".to_string()));
        assert!(!seen.contains(&"secret.txt".to_string()));
    }

    #[test]
    fn sniff_recognizes_claude_and_codex_shapes() {
        let t = tempfile::tempdir().unwrap();
        write(&t.path().join("c.jsonl"), CLAUDE_LINE);
        write(&t.path().join("x.jsonl"), CODEX_LINES);
        let c = sniff_file(&t.path().join("c.jsonl")).unwrap();
        assert_eq!(c.known_format, Some(crate::model::AgentKind::ClaudeCode));
        assert_eq!(c.guessed_agent.as_deref(), Some("claude-code"));
        let x = sniff_file(&t.path().join("x.jsonl")).unwrap();
        assert_eq!(x.known_format, Some(crate::model::AgentKind::Codex));
    }

    #[test]
    fn sniff_rejects_json_without_model_key() {
        let t = tempfile::tempdir().unwrap();
        write(&t.path().join("a.json"), r#"{"usage":1}"#);
        assert!(sniff_file(&t.path().join("a.json")).is_none());
    }

    #[test]
    fn scan_data_finds_relocated_claude_dir() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        write(&home.join("alt-claude/projects/p/s.jsonl"), CLAUDE_LINE);
        let (cands, _) = scan_data(&[home.to_path_buf()], home, &[], &no_cancel());
        assert_eq!(cands.len(), 1);
        assert_eq!(canon(std::path::Path::new(&cands[0].path)), canon(&home.join("alt-claude")));
        assert_eq!(cands[0].known_format, Some(crate::model::AgentKind::ClaudeCode));
        assert_eq!(cands[0].file_count, 1);
    }

    #[test]
    fn scan_data_excludes_known_roots() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        write(&home.join(".claude/projects/p/s.jsonl"), CLAUDE_LINE);
        let (cands, _) = scan_data(&[home.to_path_buf()], home, &[home.join(".claude")], &no_cancel());
        assert!(cands.is_empty());
    }

    #[test]
    fn footprints_map_markers_to_git_project() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        let repo = home.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        write(&repo.join("sub/CLAUDE.md"), "x");
        write(&repo.join("AGENTS.md"), "x");
        let (fps, _) = scan_footprints(&[home.to_path_buf()], home, &no_cancel());
        let got: BTreeSet<(String, String)> = fps.iter().map(|f| (f.project_key.clone(), f.agent.clone())).collect();
        let want: BTreeSet<(String, String)> =
            [(canon(&repo), "claude-code".to_string()), (canon(&repo), "codex".to_string())].into_iter().collect();
        assert_eq!(got, want);
    }

    #[test]
    fn home_level_data_dirs_are_not_footprints() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        write(&home.join(".claude/settings.json"), "{}");
        write(&home.join(".codex/config.toml"), "");
        let (fps, _) = scan_footprints(&[home.to_path_buf()], home, &no_cancel());
        assert!(fps.is_empty());
    }

    fn setup_db_with_claude_dir() -> (tempfile::TempDir, Mutex<Db>) {
        let t = tempfile::tempdir().unwrap();
        write(&t.path().join("alt-claude/projects/p/s.jsonl"), CLAUDE_LINE);
        (t, Mutex::new(Db::open_in_memory().unwrap()))
    }

    #[test]
    fn rerun_preserves_user_decision() {
        let (t, db) = setup_db_with_claude_dir();
        let home = t.path();
        run_discovery(&db, &[home.to_path_buf()], home, &[], &no_cancel()).unwrap();
        {
            let g = db.lock().unwrap();
            assert_eq!(g.conn.execute("UPDATE discovery_candidates SET decision='ignore'", []).unwrap(), 1);
        }
        run_discovery(&db, &[home.to_path_buf()], home, &[], &no_cancel()).unwrap();
        let g = db.lock().unwrap();
        let d: String = g.conn.query_row("SELECT decision FROM discovery_candidates", [], |r| r.get(0)).unwrap();
        assert_eq!(d, "ignore");
    }

    #[test]
    fn tracked_candidate_becomes_adapter() {
        let (t, db) = setup_db_with_claude_dir();
        let home = t.path();
        run_discovery(&db, &[home.to_path_buf()], home, &[], &no_cancel()).unwrap();
        let g = db.lock().unwrap();
        assert!(tracked_adapters(&g).unwrap().is_empty());
        g.conn.execute("UPDATE discovery_candidates SET decision='track'", []).unwrap();
        let ads = tracked_adapters(&g).unwrap();
        assert_eq!(ads.len(), 1);
        assert_eq!(ads[0].kind(), crate::model::AgentKind::ClaudeCode);
        assert_eq!(canon(ads[0].root()), canon(&home.join("alt-claude")));
    }

    fn names(dir: &std::path::Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn cancelled_run_keeps_previous_results() {
        let (t, db) = setup_db_with_claude_dir();
        let home = t.path();
        fs::create_dir_all(home.join("repo/.git")).unwrap();
        write(&home.join("repo/CLAUDE.md"), "x");
        run_discovery(&db, &[home.to_path_buf()], home, &[], &no_cancel()).unwrap();
        let count = |sql: &str| -> i64 { db.lock().unwrap().conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        let (f, c) = (count("SELECT COUNT(*) FROM project_footprints"), count("SELECT COUNT(*) FROM discovery_candidates"));
        assert!(f >= 1 && c >= 1);
        db.lock().unwrap().conn.execute("INSERT INTO discovery_denied (path, found_ms) VALUES ('/x', 1)", []).unwrap();
        run_discovery(&db, &[home.to_path_buf()], home, &[], &AtomicBool::new(true)).unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM project_footprints"), f);
        assert_eq!(count("SELECT COUNT(*) FROM discovery_candidates"), c);
        assert_eq!(count("SELECT COUNT(*) FROM discovery_denied"), 1);
    }

    #[test]
    fn sqlite_sniff_creates_no_side_files() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("w.db");
        {
            let c = rusqlite::Connection::open(&p).unwrap();
            c.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0)).unwrap();
            c.execute_batch("CREATE TABLE u(input_tokens INTEGER); INSERT INTO u VALUES (1);").unwrap();
        }
        let _ = fs::remove_file(t.path().join("w.db-shm"));
        let _ = fs::remove_file(t.path().join("w.db-wal"));
        let before = names(t.path());
        assert!(sniff_file(&p).is_some());
        assert_eq!(names(t.path()), before);
    }

    #[test]
    fn sniff_sqlite_opencode_tokens_and_bad_header() {
        let t = tempfile::tempdir().unwrap();
        let oc = t.path().join("opencode.db");
        rusqlite::Connection::open(&oc)
            .unwrap()
            .execute_batch("CREATE TABLE session_message(id TEXT); CREATE TABLE session_v2(id TEXT);")
            .unwrap();
        assert_eq!(sniff_file(&oc).unwrap().known_format, Some(crate::model::AgentKind::OpenCode));
        let tk = t.path().join("tok.sqlite");
        rusqlite::Connection::open(&tk).unwrap().execute_batch("CREATE TABLE m(id TEXT, total_tokens INTEGER);").unwrap();
        let s = sniff_file(&tk).unwrap();
        assert_eq!(s.known_format, None);
        let plain = t.path().join("fake.db");
        write(&plain, "this is not sqlite at all");
        assert!(sniff_file(&plain).is_none());
    }

    #[test]
    fn scan_data_groups_codex_and_unknown_formats() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        write(&home.join("alt-codex/sessions/2026/11/20/rollout-x.jsonl"), CODEX_LINES);
        write(&home.join("misc/logs/u.jsonl"), r#"{"model":"m","usage":{"output_tokens":1}}"#);
        let (cands, _) = scan_data(&[home.to_path_buf()], home, &[], &no_cancel());
        assert_eq!(cands.len(), 2);
        let cx = cands.iter().find(|c| c.known_format == Some(crate::model::AgentKind::Codex)).unwrap();
        assert_eq!(canon(std::path::Path::new(&cx.path)), canon(&home.join("alt-codex")));
        let un = cands.iter().find(|c| c.known_format.is_none()).unwrap();
        assert_eq!(canon(std::path::Path::new(&un.path)), canon(&home.join("misc/logs")));
    }

    #[test]
    fn scan_data_excludes_tracker_own_data_dir() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        write(&home.join("Library/Application Support/AI Token Tracker/x.jsonl"), CLAUDE_LINE);
        let (cands, _) = scan_data(&[home.to_path_buf()], home, &[], &no_cancel());
        assert!(cands.is_empty());
    }
}
