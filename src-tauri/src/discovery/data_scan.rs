use super::walk::walk_pruned;
use crate::model::AgentKind;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[derive(Debug, Clone, PartialEq)]
pub struct Sniff {
    pub guessed_agent: Option<String>,
    pub known_format: Option<AgentKind>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    pub guessed_agent: Option<String>,
    pub known_format: Option<AgentKind>,
    pub file_count: i64,
}

const USAGE_KEYS: &[&str] = &[
    "\"input_tokens\"",
    "\"output_tokens\"",
    "\"prompt_tokens\"",
    "\"completion_tokens\"",
    "\"cache_read_input_tokens\"",
    "\"usage\"",
];

/// Reads at most 8 KiB of the file to decide whether it looks like agent token data.
pub fn sniff_file(path: &Path) -> Option<Sniff> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(ext.as_str(), "jsonl" | "json" | "db" | "sqlite") {
        return None;
    }
    let mut buf = Vec::with_capacity(8192);
    std::fs::File::open(path)
        .ok()?
        .take(8192)
        .read_to_end(&mut buf)
        .ok()?;
    let buf = &buf[..];
    if matches!(ext.as_str(), "db" | "sqlite") {
        return if buf.starts_with(b"SQLite format 3\0") {
            sniff_sqlite(path)
        } else {
            None
        };
    }
    let text = String::from_utf8_lossy(buf);
    if !text.contains("\"model\"") || !USAGE_KEYS.iter().any(|k| text.contains(k)) {
        return None;
    }
    let known_format = if text.contains("\"sessionId\"")
        && text.contains("\"message\"")
        && text.contains("\"usage\"")
    {
        Some(AgentKind::ClaudeCode)
    } else if text.contains("\"type\":\"session_meta\"") || text.contains("token_count") {
        Some(AgentKind::Codex)
    } else {
        None
    };
    Some(Sniff {
        guessed_agent: known_format.map(|k| k.as_str().to_string()),
        known_format,
    })
}

fn sniff_sqlite(path: &Path) -> Option<Sniff> {
    use rusqlite::{Connection, OpenFlags};
    // immutable=1: never create -shm/-wal side files in the scanned directory.
    let abs = std::fs::canonicalize(path).ok()?;
    let mut uri = String::from("file:");
    for b in abs.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri.push_str("?immutable=1");
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")
        .ok()?
        .query_map([], |r| r.get::<_, String>(0))
        .ok()?
        .filter_map(Result::ok)
        .collect();
    if tables.iter().any(|t| t == "session_message") && tables.iter().any(|t| t == "session_v2") {
        return Some(Sniff {
            guessed_agent: Some(AgentKind::OpenCode.as_str().to_string()),
            known_format: Some(AgentKind::OpenCode),
        });
    }
    for t in &tables {
        let sql = format!("PRAGMA table_info(\"{}\")", t.replace('"', "\"\""));
        let Ok(mut st) = conn.prepare(&sql) else {
            continue;
        };
        let Ok(rows) = st.query_map([], |r| r.get::<_, String>(1)) else {
            continue;
        };
        if rows
            .filter_map(Result::ok)
            .any(|c| c.to_lowercase().contains("token"))
        {
            return Some(Sniff {
                guessed_agent: None,
                known_format: None,
            });
        }
    }
    None
}

/// Folder that represents the agent's data root for a sniffed file.
fn group_dir(file: &Path, kind: Option<AgentKind>) -> PathBuf {
    let marker = match kind {
        Some(AgentKind::ClaudeCode) => Some("projects"),
        Some(AgentKind::Codex) => Some("sessions"),
        _ => None,
    };
    if let Some(m) = marker {
        for anc in file.ancestors().skip(1) {
            if anc.file_name().is_some_and(|n| n == m) {
                if let Some(p) = anc.parent() {
                    return p.to_path_buf();
                }
            }
        }
    }
    file.parent().unwrap_or(file).to_path_buf()
}

pub fn scan_data(
    roots: &[PathBuf],
    home: &Path,
    known_roots: &[PathBuf],
    cancel: &AtomicBool,
) -> (Vec<Candidate>, Vec<PathBuf>) {
    let mut denied = Vec::new();
    let own_dir = home.join("Library/Application Support/AI Token Tracker");
    let mut groups: BTreeMap<PathBuf, Candidate> = BTreeMap::new();
    walk_pruned(
        roots,
        home,
        cancel,
        &mut |e| {
            let p = e.path();
            if p.starts_with(&own_dir) || known_roots.iter().any(|k| p.starts_with(k)) {
                return false;
            }
            if !e.file_type().is_file() {
                return true;
            }
            if let Some(s) = sniff_file(p) {
                let dir = group_dir(p, s.known_format);
                let c = groups.entry(dir.clone()).or_insert_with(|| Candidate {
                    path: dir.to_string_lossy().into_owned(),
                    guessed_agent: None,
                    known_format: None,
                    file_count: 0,
                });
                c.file_count += 1;
                if c.known_format.is_none() {
                    c.known_format = s.known_format;
                }
                if c.guessed_agent.is_none() {
                    c.guessed_agent = s.guessed_agent;
                }
            }
            true
        },
        &mut denied,
    );
    (groups.into_values().collect(), denied)
}
