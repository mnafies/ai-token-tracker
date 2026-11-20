use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use walkdir::{DirEntry, WalkDir};

pub const SKIP_DIRS: &[&str] = &["node_modules", ".git", "vendor", "target", "dist", "build", ".venv", ".Trash"];
pub const SKIP_PATHS: &[&str] = &["Library/Caches", "Library/Containers"]; // relative to home

pub fn walk(roots: &[PathBuf], home: &Path, cancel: &AtomicBool, visit: &mut dyn FnMut(&DirEntry), denied: &mut Vec<PathBuf>) {
    walk_pruned(roots, home, cancel, &mut |e| {
        visit(e);
        true
    }, denied);
}

/// Like `walk`, but `visit` returns false to not descend into a directory.
pub fn walk_pruned(
    roots: &[PathBuf],
    home: &Path,
    cancel: &AtomicBool,
    visit: &mut dyn FnMut(&DirEntry) -> bool,
    denied: &mut Vec<PathBuf>,
) {
    let library = home.join("Library");
    let app_support = library.join("Application Support");
    let skip_paths: Vec<PathBuf> = SKIP_PATHS.iter().map(|p| home.join(p)).collect();
    for root in roots {
        let mut it = WalkDir::new(root).follow_links(false).into_iter();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let Some(next) = it.next() else { break };
            match next {
                Ok(e) => {
                    let is_dir = e.file_type().is_dir();
                    if e.depth() > 0 && should_skip(&e, &library, &app_support, &skip_paths) {
                        if is_dir {
                            it.skip_current_dir();
                        }
                        continue;
                    }
                    if !visit(&e) && is_dir {
                        it.skip_current_dir();
                    }
                }
                Err(err) => {
                    let perm = err.io_error().is_some_and(|io| {
                        io.kind() == std::io::ErrorKind::PermissionDenied || matches!(io.raw_os_error(), Some(1) | Some(13))
                    });
                    if perm {
                        if let Some(p) = err.path() {
                            denied.push(p.to_path_buf());
                        }
                    }
                }
            }
        }
    }
}

fn should_skip(e: &DirEntry, library: &Path, app_support: &Path, skip_paths: &[PathBuf]) -> bool {
    let name = e.file_name().to_string_lossy();
    if SKIP_DIRS.contains(&name.as_ref()) {
        return true;
    }
    let p = e.path();
    if skip_paths.iter().any(|s| p.starts_with(s)) {
        return true;
    }
    // Under ~/Library only Application Support is explored.
    p.parent() == Some(library) && p != app_support
}
