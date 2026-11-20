use super::walk::walk_pruned;
use crate::project::resolve_project;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// Agent for a marker file/dir name. `copilot-instructions.md` only counts inside `.github`
/// (checked by the scanner).
pub fn marker_agent(file_name: &str, is_dir: bool) -> Option<&'static str> {
    match (file_name, is_dir) {
        (".claude", true) | ("CLAUDE.md", false) => Some("claude-code"),
        ("AGENTS.md", false) | (".codex", true) => Some("codex"),
        ("GEMINI.md", false) | (".gemini", true) => Some("gemini-cli"),
        (".cursor", true) | (".cursorrules", false) => Some("cursor"),
        ("copilot-instructions.md", false) => Some("copilot"),
        (".opencode", true) | ("opencode.json", false) => Some("opencode"),
        (n, _) if n.starts_with(".aider") => Some("aider"),
        (".windsurf", true) | (".windsurfrules", false) => Some("windsurf"),
        (".continue", true) => Some("continue"),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Footprint {
    pub project_key: String,
    pub agent: String,
    pub marker: String,
}

pub fn scan_footprints(roots: &[PathBuf], home: &Path, cancel: &AtomicBool) -> (Vec<Footprint>, Vec<PathBuf>) {
    let mut denied = Vec::new();
    let mut found: BTreeSet<Footprint> = BTreeSet::new();
    walk_pruned(
        roots,
        home,
        cancel,
        &mut |e| {
            let is_dir = e.file_type().is_dir();
            let name = e.file_name().to_string_lossy();
            let parent = e.path().parent();
            if is_dir && e.depth() > 0 && name.starts_with('.') && parent == Some(home) {
                return false; // home config dirs are not projects
            }
            let Some(agent) = marker_agent(&name, is_dir) else { return true };
            let Some(mut folder) = parent else { return true };
            if name == "copilot-instructions.md" {
                if folder.file_name().is_none_or(|n| n != ".github") {
                    return true;
                }
                let Some(up) = folder.parent() else { return true };
                folder = up;
            }
            if folder == home && matches!(name.as_ref(), ".claude" | ".codex" | ".gemini") {
                return false;
            }
            found.insert(Footprint {
                project_key: resolve_project(folder).project_key,
                agent: agent.to_string(),
                marker: name.into_owned(),
            });
            !is_dir // do not descend into marker directories
        },
        &mut denied,
    );
    (found.into_iter().collect(), denied)
}
