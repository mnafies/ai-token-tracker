use crate::model::AgentKind;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use std::path::PathBuf;
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_secs(2);

/// Watches each existing root recursively and calls `on_change(kind)` at most once
/// per kind per debounced batch. Roots that do not exist are skipped.
/// Dropping the returned debouncer stops watching.
pub fn spawn_watcher(
    roots: Vec<(AgentKind, PathBuf)>,
    on_change: impl Fn(AgentKind) + Send + 'static,
) -> anyhow::Result<Debouncer<RecommendedWatcher>> {
    // FSEvents reports canonical paths (e.g. /private/var/...), so match against canonical roots.
    let roots: Vec<(AgentKind, PathBuf)> = roots
        .into_iter()
        .filter_map(|(kind, root)| std::fs::canonicalize(&root).ok().map(|r| (kind, r)))
        .collect();
    let matchers = roots.clone();
    let mut debouncer = new_debouncer(DEBOUNCE, move |res: DebounceEventResult| {
        let events = match res {
            Ok(events) => events,
            Err(e) => {
                eprintln!("watcher: {e}");
                return;
            }
        };
        let mut changed: Vec<AgentKind> = Vec::new();
        for ev in &events {
            for (kind, root) in &matchers {
                if ev.path.starts_with(root) && !changed.contains(kind) {
                    changed.push(*kind);
                }
            }
        }
        for kind in changed {
            on_change(kind);
        }
    })?;
    for (_, root) in &roots {
        debouncer.watcher().watch(root, RecursiveMode::Recursive)?;
    }
    Ok(debouncer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn watcher_fires_on_append() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("s.jsonl");
        std::fs::write(&file, "{}\n").unwrap();
        let missing = t.path().join("does-not-exist");
        let (tx, rx) = mpsc::channel();
        let _w = spawn_watcher(
            vec![(AgentKind::Codex, t.path().to_path_buf()), (AgentKind::GeminiCli, missing)],
            move |k| {
                let _ = tx.send(k);
            },
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        std::fs::OpenOptions::new().append(true).open(&file).unwrap().write_all(b"{}\n").unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), AgentKind::Codex);
    }
}
