use super::{Adapter, Source, SourceState};
use crate::model::{AgentKind, ParseOutput};
use std::path::{Path, PathBuf};

/// Detection-only adapter (no usable token data): reports whether the app exists.
pub struct PresenceAdapter {
    pub kind: AgentKind,
    pub root: PathBuf,
}

impl Adapter for PresenceAdapter {
    fn kind(&self) -> AgentKind {
        self.kind
    }
    fn root(&self) -> &Path {
        &self.root
    }
    fn detect(&self) -> bool {
        self.root.exists()
    }
    fn sources(&self) -> Vec<Source> {
        Vec::new()
    }
    fn parse(
        &self,
        _src: &Source,
        _prev: &SourceState,
    ) -> anyhow::Result<(ParseOutput, SourceState)> {
        Ok((ParseOutput::default(), SourceState::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_detects_only() {
        let dir = tempfile::tempdir().unwrap();
        let a = PresenceAdapter {
            kind: AgentKind::VsCode,
            root: dir.path().to_path_buf(),
        };
        assert!(a.detect());
        assert!(a.sources().is_empty());
        assert_eq!(a.kind(), AgentKind::VsCode);
        let b = PresenceAdapter {
            kind: AgentKind::ClaudeDesktop,
            root: dir.path().join("nope"),
        };
        assert!(!b.detect());
    }
}
