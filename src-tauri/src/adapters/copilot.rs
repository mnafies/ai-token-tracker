use super::{Adapter, Source, SourceKind, SourceState};
use crate::model::{AgentKind, ParseOutput, SessionMeta, TokenStatus};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct CopilotAdapter {
    pub root: PathBuf,
}

impl Adapter for CopilotAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::CopilotCli
    }
    fn root(&self) -> &Path {
        &self.root
    }
    fn detect(&self) -> bool {
        self.root.exists()
    }
    fn sources(&self) -> Vec<Source> {
        let Ok(rd) = std::fs::read_dir(self.root.join("session-state")) else { return Vec::new() };
        let mut v: Vec<Source> = rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| Source { path: e.path(), kind: SourceKind::Dir })
            .collect();
        v.sort_by(|a, b| a.path.cmp(&b.path));
        v
    }
    fn parse(&self, src: &Source, _prev: &SourceState) -> anyhow::Result<(ParseOutput, SourceState)> {
        let mtime_ms = std::fs::metadata(&src.path)?
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let session_id = src.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let meta = SessionMeta {
            agent: AgentKind::CopilotCli,
            session_id,
            cwd: None,
            title: None,
            started_ms: Some(mtime_ms),
            updated_ms: Some(mtime_ms),
            token_status: TokenStatus::NotRecorded,
        };
        Ok((
            ParseOutput { events: Vec::new(), sessions: vec![meta] },
            SourceState { offset: 0, size: 0, mtime_ms, context: Value::Null },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TokenStatus;

    #[test]
    fn copilot_sessions_from_state_dirs() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["s-one", "s-two"] {
            std::fs::create_dir_all(dir.path().join("session-state").join(n)).unwrap();
        }
        std::fs::write(dir.path().join("session-state/stray.txt"), "x").unwrap();
        let a = CopilotAdapter { root: dir.path().to_path_buf() };
        assert!(a.detect());
        let srcs = a.sources();
        assert_eq!(srcs.len(), 2);
        let mut ids = Vec::new();
        for s in &srcs {
            assert_eq!(s.kind, super::super::SourceKind::Dir);
            let (out, st) = a.parse(s, &SourceState::default()).unwrap();
            assert!(out.events.is_empty());
            assert_eq!(out.sessions.len(), 1);
            let m = &out.sessions[0];
            assert_eq!(m.token_status, TokenStatus::NotRecorded);
            assert_eq!(m.cwd, None);
            assert!(m.started_ms.is_some() && m.updated_ms == m.started_ms);
            assert_eq!((st.offset, st.size), (0, 0));
            ids.push(m.session_id.clone());
        }
        ids.sort();
        assert_eq!(ids, vec!["s-one", "s-two"]);
    }
}
