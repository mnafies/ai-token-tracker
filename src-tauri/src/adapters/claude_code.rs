use super::{Adapter, Source, SourceKind, SourceState, jsonl::read_complete_lines, truncate_title};
use crate::model::{AgentKind, ParseOutput, SessionMeta, TokenStatus, UsageEvent};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct ClaudeCodeAdapter {
    pub root: PathBuf,
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(_) => Vec::new(),
    };
    v.sort();
    v
}

fn is_jsonl(p: &Path) -> bool {
    p.is_file() && p.extension().is_some_and(|e| e == "jsonl")
}

/// If the file lives in `.../<X>/subagents/`, returns `X`.
fn subagent_session(path: &Path) -> Option<String> {
    let parent = path.parent()?;
    if parent.file_name()? != "subagents" {
        return None;
    }
    Some(parent.parent()?.file_name()?.to_string_lossy().into_owned())
}

#[derive(Default)]
struct SessionAcc {
    cwd: Option<String>,
    ai_title: Option<String>,
    first_user: Option<String>,
    started: Option<i64>,
    updated: Option<i64>,
}

fn i64_at(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

impl Adapter for ClaudeCodeAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::ClaudeCode
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn detect(&self) -> bool {
        self.root.exists()
    }

    fn sources(&self) -> Vec<Source> {
        let mut out = Vec::new();
        for proj in sorted_entries(&self.root.join("projects")) {
            if !proj.is_dir() {
                continue;
            }
            for entry in sorted_entries(&proj) {
                if is_jsonl(&entry) {
                    out.push(Source { path: entry, kind: SourceKind::Jsonl });
                } else if entry.is_dir() {
                    for f in sorted_entries(&entry.join("subagents")) {
                        if is_jsonl(&f) {
                            out.push(Source { path: f, kind: SourceKind::Jsonl });
                        }
                    }
                }
            }
        }
        out
    }

    fn parse(&self, src: &Source, prev: &SourceState) -> anyhow::Result<(ParseOutput, SourceState)> {
        let (lines, offset) = read_complete_lines(&src.path, prev.offset)?;
        let meta = std::fs::metadata(&src.path)?;
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let forced_session = subagent_session(&src.path);
        let mut events: Vec<UsageEvent> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut sessions: Vec<(String, SessionAcc)> = Vec::new();

        for line in &lines {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let Some(session_id) = forced_session
                .clone()
                .or_else(|| v.get("sessionId").and_then(Value::as_str).map(String::from))
            else {
                continue;
            };
            let ts = v
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.timestamp_millis());
            let cwd = v.get("cwd").and_then(Value::as_str).map(String::from);

            let pos = match sessions.iter().position(|(id, _)| *id == session_id) {
                Some(p) => p,
                None => {
                    sessions.push((session_id.clone(), SessionAcc::default()));
                    sessions.len() - 1
                }
            };
            let acc = &mut sessions[pos].1;
            if let Some(ts) = ts {
                acc.started = Some(acc.started.map_or(ts, |s| s.min(ts)));
                acc.updated = Some(acc.updated.map_or(ts, |s| s.max(ts)));
            }
            if acc.cwd.is_none() {
                acc.cwd = cwd.clone();
            }

            match v.get("type").and_then(Value::as_str) {
                Some("ai-title") => {
                    if let Some(t) = v.get("aiTitle").and_then(Value::as_str) {
                        acc.ai_title = Some(truncate_title(t));
                    }
                }
                Some("user") => {
                    if acc.first_user.is_none() && forced_session.is_none() {
                        if let Some(s) = v.pointer("/message/content").and_then(Value::as_str) {
                            acc.first_user = Some(truncate_title(s));
                        }
                    }
                }
                Some("assistant") => {
                    let Some(msg) = v.get("message") else { continue };
                    let Some(usage) = msg.get("usage") else { continue };
                    let Some(ts) = ts else { continue };
                    let event_key = match msg.get("id").and_then(Value::as_str) {
                        Some(id) => format!("claude:{id}"),
                        None => match v.get("uuid").and_then(Value::as_str) {
                            Some(u) => format!("claude:uuid:{u}"),
                            None => continue,
                        },
                    };
                    let ev = UsageEvent {
                        agent: AgentKind::ClaudeCode,
                        event_key: event_key.clone(),
                        session_id: session_id.clone(),
                        ts_ms: ts,
                        cwd,
                        model: msg.get("model").and_then(Value::as_str).unwrap_or("").to_string(),
                        provider_hint: "anthropic".to_string(),
                        input: i64_at(usage, "input_tokens"),
                        output: i64_at(usage, "output_tokens"),
                        reasoning: usage
                            .get("output_tokens_details")
                            .map_or(0, |d| i64_at(d, "thinking_tokens")),
                        cache_read: i64_at(usage, "cache_read_input_tokens"),
                        cache_write: i64_at(usage, "cache_creation_input_tokens"),
                    };
                    match index.get(&event_key) {
                        Some(&i) => events[i] = ev,
                        None => {
                            index.insert(event_key, events.len());
                            events.push(ev);
                        }
                    }
                }
                _ => {}
            }
        }

        let mut context = match &prev.context {
            Value::Object(_) => prev.context.clone(),
            _ => json!({}),
        };
        let fallback_done = context
            .get("title_fallback_done")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut used_fallback = false;
        let sessions = sessions
            .into_iter()
            .map(|(session_id, acc)| {
                let title = match acc.ai_title {
                    Some(t) => Some(t),
                    None if !fallback_done => {
                        let t = acc.first_user;
                        used_fallback |= t.is_some();
                        t
                    }
                    None => None,
                };
                SessionMeta {
                    agent: AgentKind::ClaudeCode,
                    session_id,
                    cwd: acc.cwd,
                    title,
                    started_ms: acc.started,
                    updated_ms: acc.updated,
                    token_status: TokenStatus::Recorded,
                }
            })
            .collect();
        if used_fallback {
            context["title_fallback_done"] = json!(true);
        }

        Ok((
            ParseOutput { events, sessions },
            SourceState { offset, size: meta.len(), mtime_ms, context },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter() -> ClaudeCodeAdapter {
        ClaudeCodeAdapter { root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude") }
    }

    fn parse_file(rel: &str) -> ParseOutput {
        let src = Source { path: adapter().root.join(rel), kind: SourceKind::Jsonl };
        adapter().parse(&src, &SourceState::default()).unwrap().0
    }

    const MAIN: &str = "projects/-tmp-demo/sess-1.jsonl";
    const SUB: &str = "projects/-tmp-demo/sess-1/subagents/agent-x.jsonl";

    #[test]
    fn claude_dedupes_streamed_duplicates() {
        let out = parse_file(MAIN);
        assert_eq!(out.events.iter().filter(|e| e.event_key == "claude:msg_A").count(), 1);
        assert_eq!(out.events.len(), 2);
    }

    #[test]
    fn claude_maps_usage_fields() {
        let out = parse_file(MAIN);
        let e = out.events.iter().find(|e| e.event_key == "claude:msg_A").unwrap();
        assert_eq!((e.input, e.cache_write, e.cache_read, e.output, e.reasoning), (10, 100, 1000, 50, 20));
        assert_eq!(e.model, "claude-opus-5-5");
        assert_eq!(e.provider_hint, "anthropic");
        assert_eq!(e.cwd.as_deref(), Some("/tmp/demo"));
        assert_eq!(e.session_id, "sess-1");
    }

    #[test]
    fn claude_subagent_events_join_parent_session() {
        let out = parse_file(SUB);
        assert_eq!(out.events.len(), 1);
        assert_eq!(out.events[0].event_key, "claude:msg_C");
        assert_eq!(out.events[0].session_id, "sess-1");
        assert_eq!(out.sessions.len(), 1);
        assert_eq!(out.sessions[0].title, None);
    }

    #[test]
    fn claude_title_prefers_ai_title() {
        let out = parse_file(MAIN);
        assert_eq!(out.sessions.len(), 1);
        assert_eq!(out.sessions[0].title.as_deref(), Some("Fix login bug"));
        assert!(out.sessions[0].started_ms.unwrap() < out.sessions[0].updated_ms.unwrap());
    }

    #[test]
    fn claude_sources_include_subagent_files() {
        assert_eq!(adapter().sources().len(), 2);
        assert!(adapter().detect());
    }

    #[test]
    fn claude_fallback_title_once_and_incremental() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("projects/p/s.jsonl");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{\"type\":\"user\",\"sessionId\":\"s\",\"timestamp\":\"2026-11-20T01:00:00Z\",\"message\":{\"content\":\"hello there\"}}\n{\"type\":\"assis").unwrap();
        let a = ClaudeCodeAdapter { root: dir.path().to_path_buf() };
        let src = Source { path: p.clone(), kind: SourceKind::Jsonl };
        let (out, st) = a.parse(&src, &SourceState::default()).unwrap();
        assert_eq!(out.sessions[0].title.as_deref(), Some("hello there"));
        assert_eq!(st.context["title_fallback_done"], true);
        let mut s = std::fs::read_to_string(&p).unwrap();
        s.push_str("tant\"}\n{\"type\":\"user\",\"sessionId\":\"s\",\"message\":{\"content\":\"second\"}}\n");
        std::fs::write(&p, s).unwrap();
        let (out2, _) = a.parse(&src, &st).unwrap();
        assert_eq!(out2.sessions[0].title, None);
    }

    #[test]
    fn claude_subagent_user_prompt_never_becomes_title() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("projects/p/sess-9/subagents/agent-y.jsonl");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{\"type\":\"user\",\"sessionId\":\"sess-9\",\"timestamp\":\"2026-11-20T01:00:00Z\",\"message\":{\"content\":\"do the subtask\"}}\n{\"type\":\"assistant\",\"sessionId\":\"sess-9\",\"timestamp\":\"2026-11-20T01:00:01Z\",\"uuid\":\"a\",\"message\":{\"id\":\"m\",\"model\":\"x\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n").unwrap();
        let a = ClaudeCodeAdapter { root: dir.path().to_path_buf() };
        let src = Source { path: p, kind: SourceKind::Jsonl };
        let (out, _) = a.parse(&src, &SourceState::default()).unwrap();
        assert_eq!(out.sessions[0].session_id, "sess-9");
        assert_eq!(out.sessions[0].title, None);
    }

    #[test]
    fn claude_event_without_timestamp_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("projects/p/s.jsonl");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{\"type\":\"assistant\",\"sessionId\":\"s\",\"uuid\":\"a\",\"message\":{\"id\":\"m1\",\"model\":\"x\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n{\"type\":\"assistant\",\"sessionId\":\"s\",\"timestamp\":\"bad\",\"uuid\":\"b\",\"message\":{\"id\":\"m2\",\"model\":\"x\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n").unwrap();
        let a = ClaudeCodeAdapter { root: dir.path().to_path_buf() };
        let src = Source { path: p, kind: SourceKind::Jsonl };
        let (out, _) = a.parse(&src, &SourceState::default()).unwrap();
        assert!(out.events.is_empty());
    }
}
