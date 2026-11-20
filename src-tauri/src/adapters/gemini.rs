use super::{Adapter, Source, SourceKind, SourceState};
use crate::model::{AgentKind, ParseOutput, SessionMeta, TokenStatus, UsageEvent};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct GeminiAdapter {
    pub root: PathBuf,
}

impl Adapter for GeminiAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::GeminiCli
    }
    fn root(&self) -> &Path {
        &self.root
    }
    fn detect(&self) -> bool {
        self.root.exists()
    }
    fn sources(&self) -> Vec<Source> {
        let Ok(rd) = std::fs::read_dir(self.root.join("tmp")) else { return Vec::new() };
        let mut v = Vec::new();
        for proj in rd.flatten() {
            let Ok(files) = std::fs::read_dir(proj.path().join("chats")) else { continue };
            for f in files.flatten() {
                let p = f.path();
                let name = f.file_name().to_string_lossy().into_owned();
                if p.is_file() && name.starts_with("session-") && name.ends_with(".jsonl") {
                    v.push(Source { path: p, kind: SourceKind::Jsonl });
                }
            }
        }
        v.sort_by(|a, b| a.path.cmp(&b.path));
        v
    }
    fn parse(&self, src: &Source, _prev: &SourceState) -> anyhow::Result<(ParseOutput, SourceState)> {
        // Always a full re-parse: files are small and `$set` records overwrite earlier state.
        let text = String::from_utf8_lossy(&std::fs::read(&src.path)?).into_owned();
        let meta = std::fs::metadata(&src.path)?;
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let cwd = src
            .path
            .parent()
            .and_then(Path::parent)
            .and_then(|d| std::fs::read_to_string(d.join(".project_root")).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let mut session: Option<(String, Option<i64>)> = None;
        let mut messages: Vec<Value> = Vec::new();
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            if session.is_none() {
                if let Some(id) = v.get("sessionId").and_then(Value::as_str) {
                    session = Some((id.to_string(), v.get("startTime").and_then(Value::as_str).and_then(parse_ts)));
                }
            }
            for arr in [v.get("messages"), v.pointer("/$set/messages")].into_iter().flatten() {
                if let Some(a) = arr.as_array() {
                    messages.extend(a.iter().filter(|m| m.is_object()).cloned());
                }
            }
        }

        let Some((session_id, started_ms)) = session else {
            return Ok((ParseOutput::default(), SourceState { offset: 0, size: meta.len(), mtime_ms, context: Value::Null }));
        };

        // Dedupe by event key within this parse: last occurrence wins, first-seen order kept.
        let mut events: Vec<UsageEvent> = Vec::new();
        for m in &messages {
            if m.get("type").and_then(Value::as_str) != Some("gemini") {
                continue;
            }
            let (Some(id), Some(tokens), Some(ts_ms)) = (
                m.get("id").and_then(Value::as_str),
                m.get("tokens").filter(|t| t.is_object()),
                m.get("timestamp").and_then(Value::as_str).and_then(parse_ts),
            ) else {
                continue;
            };
            let n = |k: &str| tokens.get(k).and_then(Value::as_i64).unwrap_or(0);
            let (cached, thoughts) = (n("cached"), n("thoughts"));
            let ev = UsageEvent {
                agent: AgentKind::GeminiCli,
                event_key: format!("gemini:{session_id}:{id}"),
                session_id: session_id.clone(),
                ts_ms,
                cwd: cwd.clone(),
                model: m.get("model").and_then(Value::as_str).unwrap_or("").to_string(),
                provider_hint: "google".to_string(),
                input: (n("input") - cached).max(0),
                output: n("output") + thoughts,
                reasoning: thoughts,
                cache_read: cached,
                cache_write: 0,
            };
            match events.iter_mut().find(|e| e.event_key == ev.event_key) {
                Some(slot) => *slot = ev,
                None => events.push(ev),
            }
        }

        let token_status = if events.is_empty() { TokenStatus::NotRecorded } else { TokenStatus::Recorded };
        let session_meta = SessionMeta {
            agent: AgentKind::GeminiCli,
            session_id,
            cwd,
            title: None,
            started_ms,
            updated_ms: Some(mtime_ms),
            token_status,
        };
        Ok((
            ParseOutput { events, sessions: vec![session_meta] },
            SourceState { offset: 0, size: meta.len(), mtime_ms, context: Value::Null },
        ))
    }
}

fn parse_ts(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TokenStatus;

    fn fixture() -> (GeminiAdapter, Source) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gemini");
        let a = GeminiAdapter { root };
        let srcs = a.sources();
        assert_eq!(srcs.len(), 1);
        let s = srcs.into_iter().next().unwrap();
        (a, s)
    }

    #[test]
    fn gemini_session_meta_and_project_root() {
        let (a, s) = fixture();
        let (out, st) = a.parse(&s, &SourceState::default()).unwrap();
        assert_eq!(out.sessions.len(), 1);
        assert_eq!(out.sessions[0].session_id, "g1");
        assert_eq!(out.sessions[0].cwd.as_deref(), Some("/tmp/gem"));
        assert_eq!(out.sessions[0].started_ms, Some(1_795_143_600_000));
        assert_eq!(st.offset, 0);
        assert!(st.size > 0 && st.mtime_ms > 0);
    }

    #[test]
    fn gemini_tokens_when_present() {
        let (a, s) = fixture();
        let (out, _) = a.parse(&s, &SourceState::default()).unwrap();
        assert_eq!(out.events.len(), 1);
        let e = &out.events[0];
        assert_eq!(e.event_key, "gemini:g1:m2");
        assert_eq!((e.input, e.cache_read, e.output, e.reasoning, e.cache_write), (600, 300, 140, 40, 0));
        assert_eq!(e.provider_hint, "google");
        assert_eq!(e.model, "gemini-3-pro");
        assert_eq!(e.cwd.as_deref(), Some("/tmp/gem"));
        assert_eq!(out.sessions[0].token_status, TokenStatus::Recorded);
    }

    #[test]
    fn gemini_without_tokens_is_not_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("tmp/abc/chats");
        std::fs::create_dir_all(&chats).unwrap();
        let p = chats.join("session-x.jsonl");
        std::fs::write(&p, "{\"sessionId\":\"g2\",\"startTime\":\"2026-11-20T03:00:00.000Z\"}\n").unwrap();
        let a = GeminiAdapter { root: dir.path().to_path_buf() };
        let s = a.sources().into_iter().next().unwrap();
        let (out, _) = a.parse(&s, &SourceState::default()).unwrap();
        assert!(out.events.is_empty());
        assert_eq!(out.sessions[0].token_status, TokenStatus::NotRecorded);
        assert_eq!(out.sessions[0].cwd, None);
    }
}
