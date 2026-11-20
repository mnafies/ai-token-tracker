use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceState {
    pub offset: u64,
    pub size: u64,
    pub mtime_ms: i64,
    pub context: Value,
}

impl Default for SourceState {
    fn default() -> Self {
        SourceState { offset: 0, size: 0, mtime_ms: 0, context: Value::Null }
    }
}

pub mod claude_code;
pub mod jsonl;

use crate::model::{AgentKind, ParseOutput};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Jsonl,
    Sqlite,
    Dir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub path: PathBuf,
    pub kind: SourceKind,
}

pub trait Adapter: Send + Sync {
    fn kind(&self) -> AgentKind;
    fn root(&self) -> &Path;
    /// True when the agent's root directory exists.
    fn detect(&self) -> bool;
    fn sources(&self) -> Vec<Source>;
    fn parse(&self, src: &Source, prev: &SourceState) -> anyhow::Result<(ParseOutput, SourceState)>;
}

pub fn adapter_for(kind: AgentKind, root: PathBuf) -> Box<dyn Adapter> {
    match kind {
        AgentKind::ClaudeCode => Box::new(claude_code::ClaudeCodeAdapter { root }),
        _ => unimplemented!("adapter for {kind:?} is added in a later task"),
    }
}

pub fn default_adapters(home: &Path) -> Vec<Box<dyn Adapter>> {
    vec![adapter_for(AgentKind::ClaudeCode, home.join(".claude"))]
}

/// Trim, replace newlines with spaces, keep at most 80 chars.
pub fn truncate_title(s: &str) -> String {
    s.trim()
        .replace("\r\n", " ")
        .replace(['\n', '\r'], " ")
        .chars()
        .take(80)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_title_limits_to_80_chars() {
        let s = "é".repeat(100);
        assert_eq!(truncate_title(&s).chars().count(), 80);
        assert_eq!(truncate_title("  a\nb "), "a b");
    }
}
