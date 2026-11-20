use super::{Adapter, Source, SourceKind, SourceState, jsonl::read_complete_lines, truncate_title};
use crate::model::{AgentKind, ParseOutput, SessionMeta, TokenStatus, UsageEvent};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct CodexAdapter {
    pub root: PathBuf,
}

fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.filter_map(Result::ok) {
        let path = entry.path();
        // file_type() does not follow symlinks, so a linked directory (or a loop) is not walked.
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            collect_jsonl(&path, out);
        } else if path.is_file() && path.extension().is_some_and(|e| e == "jsonl") {
            out.push(path);
        }
    }
}

/// Trailing UUID (last 36 chars) of `rollout-<timestamp>-<uuid>.jsonl`.
fn filename_session_id(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let n = stem.chars().count();
    (n >= 36).then(|| stem.chars().skip(n - 36).collect())
}

fn i64_at(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn str_ctx(ctx: &Value, key: &str) -> Option<String> {
    ctx.get(key).and_then(Value::as_str).map(String::from)
}

impl Adapter for CodexAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn detect(&self) -> bool {
        self.root.exists()
    }

    fn sources(&self) -> Vec<Source> {
        let mut paths = Vec::new();
        collect_jsonl(&self.root.join("sessions"), &mut paths);
        collect_jsonl(&self.root.join("archived_sessions"), &mut paths);
        paths.sort();
        paths
            .into_iter()
            .map(|path| Source {
                path,
                kind: SourceKind::Jsonl,
            })
            .collect()
    }

    fn parse(
        &self,
        src: &Source,
        prev: &SourceState,
    ) -> anyhow::Result<(ParseOutput, SourceState)> {
        let (lines, offset) = read_complete_lines(&src.path, prev.offset)?;
        let meta = std::fs::metadata(&src.path)?;
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let ctx = &prev.context;
        let mut session_id = str_ctx(ctx, "session_id");
        let mut cwd = str_ctx(ctx, "cwd");
        let mut model = str_ctx(ctx, "model").unwrap_or_default();
        let mut provider = str_ctx(ctx, "provider_hint").unwrap_or_else(|| "openai".to_string());
        let mut title_done = ctx
            .get("title_done")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut events: Vec<UsageEvent> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut session_cwd: Option<String> = None;
        let mut title: Option<String> = None;
        let mut started: Option<i64> = None;
        let mut updated: Option<i64> = None;
        let mut seen_line = false;

        for line in &lines {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            seen_line = true;
            let ts = v
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.timestamp_millis());
            if let Some(ts) = ts {
                started = Some(started.map_or(ts, |s| s.min(ts)));
                updated = Some(updated.map_or(ts, |s| s.max(ts)));
            }
            let Some(payload) = v.get("payload") else {
                continue;
            };
            match v.get("type").and_then(Value::as_str) {
                Some("session_meta") => {
                    if let Some(id) = payload.get("id").and_then(Value::as_str) {
                        session_id = Some(id.to_string());
                    }
                    if let Some(c) = payload.get("cwd").and_then(Value::as_str) {
                        cwd = Some(c.to_string());
                        session_cwd = Some(c.to_string());
                    }
                    provider = payload
                        .get("model_provider")
                        .and_then(Value::as_str)
                        .unwrap_or("openai")
                        .to_string();
                }
                Some("turn_context") => {
                    if let Some(c) = payload.get("cwd").and_then(Value::as_str) {
                        cwd = Some(c.to_string());
                        if session_cwd.is_none() {
                            session_cwd = Some(c.to_string());
                        }
                    }
                    if let Some(m) = payload.get("model").and_then(Value::as_str) {
                        model = m.to_string();
                    }
                }
                Some("event_msg") => match payload.get("type").and_then(Value::as_str) {
                    Some("user_message") if !title_done => {
                        if let Some(m) = payload.get("message").and_then(Value::as_str) {
                            title = Some(truncate_title(m));
                            title_done = true;
                        }
                    }
                    Some("token_count") => {
                        let Some(info) = payload.get("info").filter(|i| !i.is_null()) else {
                            continue;
                        };
                        let (Some(last), Some(total)) = (
                            info.get("last_token_usage"),
                            info.pointer("/total_token_usage/total_tokens")
                                .and_then(Value::as_i64),
                        ) else {
                            continue;
                        };
                        let Some(ts) = ts else { continue };
                        if session_id.is_none() {
                            session_id = filename_session_id(&src.path);
                        }
                        let Some(sid) = session_id.clone() else {
                            continue;
                        };
                        let cached = i64_at(last, "cached_input_tokens");
                        let event_key = format!("codex:{sid}:{total}");
                        let ev = UsageEvent {
                            agent: AgentKind::Codex,
                            event_key: event_key.clone(),
                            session_id: sid,
                            ts_ms: ts,
                            cwd: cwd.clone(),
                            model: model.clone(),
                            provider_hint: provider.clone(),
                            input: i64_at(last, "input_tokens") - cached,
                            output: i64_at(last, "output_tokens"),
                            reasoning: i64_at(last, "reasoning_output_tokens"),
                            cache_read: cached,
                            cache_write: i64_at(last, "cache_write_input_tokens"),
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
                },
                _ => {}
            }
        }

        if session_id.is_none() && seen_line {
            session_id = filename_session_id(&src.path);
        }
        let sessions = match (&session_id, seen_line) {
            (Some(id), true) => vec![SessionMeta {
                agent: AgentKind::Codex,
                session_id: id.clone(),
                cwd: session_cwd,
                title,
                started_ms: started,
                updated_ms: updated,
                token_status: TokenStatus::Recorded,
            }],
            _ => Vec::new(),
        };

        let mut context = match &prev.context {
            Value::Object(_) => prev.context.clone(),
            _ => json!({}),
        };
        context["session_id"] = json!(session_id);
        context["cwd"] = json!(cwd);
        context["model"] = json!(model);
        context["provider_hint"] = json!(provider);
        context["title_done"] = json!(title_done);

        Ok((
            ParseOutput { events, sessions },
            SourceState {
                offset,
                size: meta.len(),
                mtime_ms,
                context,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{SourceKind, jsonl::read_complete_lines};

    fn adapter() -> CodexAdapter {
        CodexAdapter {
            root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex"),
        }
    }

    fn fixture() -> Source {
        Source {
            path: adapter()
                .root
                .join("sessions/2026/11/20/rollout-cx-1.jsonl"),
            kind: SourceKind::Jsonl,
        }
    }

    fn parse_all() -> ParseOutput {
        adapter()
            .parse(&fixture(), &SourceState::default())
            .unwrap()
            .0
    }

    #[test]
    fn sources_do_not_follow_symlinked_directories() {
        let t = tempfile::tempdir().unwrap();
        let day = t.path().join("sessions/2026/11/20");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(day.join("rollout-a.jsonl"), "").unwrap();
        let outside = t.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("rollout-b.jsonl"), "").unwrap();
        std::os::unix::fs::symlink(&outside, t.path().join("sessions/link")).unwrap();
        std::os::unix::fs::symlink(t.path().join("sessions"), day.join("loop")).unwrap();
        let a = CodexAdapter {
            root: t.path().to_path_buf(),
        };
        let paths: Vec<PathBuf> = a.sources().into_iter().map(|s| s.path).collect();
        assert_eq!(paths, vec![day.join("rollout-a.jsonl")]);
    }

    #[test]
    fn codex_emits_one_event_per_distinct_cumulative_total() {
        let out = parse_all();
        let keys: Vec<&str> = out.events.iter().map(|e| e.event_key.as_str()).collect();
        assert_eq!(keys, vec!["codex:cx-1:1200", "codex:cx-1:2800"]);
    }

    #[test]
    fn codex_splits_cached_from_input() {
        let out = parse_all();
        let e = &out.events[0];
        assert_eq!(
            (e.input, e.cache_read, e.cache_write, e.output, e.reasoning),
            (600, 400, 0, 200, 50)
        );
        let e = &out.events[1];
        assert_eq!(
            (e.input, e.cache_read, e.cache_write, e.output, e.reasoning),
            (500, 1000, 0, 100, 10)
        );
    }

    #[test]
    fn codex_uses_turn_context_model_and_cwd() {
        let out = parse_all();
        let e = &out.events[0];
        assert_eq!(e.model, "gpt-6-sol");
        assert_eq!(e.cwd.as_deref(), Some("/tmp/demo/sub"));
        assert_eq!(e.provider_hint, "openai");
        assert_eq!(e.session_id, "cx-1");
        assert_eq!(out.sessions[0].cwd.as_deref(), Some("/tmp/demo"));
    }

    #[test]
    fn codex_title_from_first_user_message() {
        let out = parse_all();
        assert_eq!(out.sessions.len(), 1);
        assert_eq!(
            out.sessions[0].title.as_deref(),
            Some("Refactor the parser module please")
        );
        assert!(out.sessions[0].started_ms.unwrap() < out.sessions[0].updated_ms.unwrap());
    }

    #[test]
    fn codex_resumes_mid_file_with_context() {
        let a = adapter();
        let src = fixture();
        // Split after the first 5 lines (through the first token_count event).
        let text = std::fs::read_to_string(&src.path).unwrap();
        let head: String = text.split_inclusive('\n').take(5).collect();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("rollout-cx-1.jsonl");
        std::fs::write(&p, &head).unwrap();
        let s = Source {
            path: p.clone(),
            kind: SourceKind::Jsonl,
        };
        let (_, off) = read_complete_lines(&p, 0).unwrap();
        assert_eq!(off as usize, head.len());
        let (out1, st1) = a.parse(&s, &SourceState::default()).unwrap();
        assert_eq!(out1.events.len(), 1);
        assert_eq!(st1.offset, off);
        std::fs::write(&p, &text).unwrap();
        let (out2, st2) = a.parse(&s, &st1).unwrap();
        assert_eq!(out2.events.len(), 2); // dup of 1200 + new 2800
        let e2 = out2
            .events
            .iter()
            .find(|e| e.event_key == "codex:cx-1:2800")
            .unwrap();
        assert_eq!(e2.model, "gpt-6-sol");
        assert_eq!(e2.session_id, "cx-1");
        assert_eq!(out2.sessions[0].title, None);
        assert_eq!(st2.context["title_done"], true);
    }

    #[test]
    fn codex_falls_back_to_filename_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let id = "0198a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
        let p = dir
            .path()
            .join(format!("sessions/rollout-2026-11-20T02-00-00-{id}.jsonl"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{\"timestamp\":\"2026-11-20T02:00:03.000Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"total_tokens\":10},\"last_token_usage\":{\"input_tokens\":10,\"cached_input_tokens\":0,\"output_tokens\":1,\"reasoning_output_tokens\":0}}}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"total_tokens\":20},\"last_token_usage\":{\"input_tokens\":10}}}}\n").unwrap();
        let a = CodexAdapter {
            root: dir.path().to_path_buf(),
        };
        let (out, _) = a
            .parse(
                &Source {
                    path: p,
                    kind: SourceKind::Jsonl,
                },
                &SourceState::default(),
            )
            .unwrap();
        assert_eq!(out.events.len(), 1); // second line has no timestamp
        assert_eq!(out.events[0].session_id, id);
        assert_eq!(out.events[0].event_key, format!("codex:{id}:10"));
    }

    #[test]
    fn codex_sources_walk_sessions_and_archived() {
        assert_eq!(adapter().sources().len(), 1);
        let dir = tempfile::tempdir().unwrap();
        for rel in [
            "sessions/a/b/x.jsonl",
            "archived_sessions/y.jsonl",
            "sessions/skip.txt",
        ] {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "").unwrap();
        }
        let a = CodexAdapter {
            root: dir.path().to_path_buf(),
        };
        assert_eq!(a.sources().len(), 2);
    }
}
