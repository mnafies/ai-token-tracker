use crate::db::Db;
use std::path::{Path, PathBuf};

pub struct ResolvedProject {
    pub project_key: String,
    pub is_git: bool,
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Resolves a working directory to its project root: the git repo root
/// (main repo for worktrees), or the cwd itself when not in git / missing.
pub fn resolve_project(cwd: &Path) -> ResolvedProject {
    let fallback = || ResolvedProject {
        project_key: cwd.to_string_lossy().into_owned(),
        is_git: false,
    };
    let start = match std::fs::canonicalize(cwd) {
        Ok(p) => p,
        Err(_) => return fallback(),
    };
    let mut cur: &Path = &start;
    loop {
        let dot_git = cur.join(".git");
        if dot_git.is_dir() {
            return git_result(cur);
        }
        if dot_git.is_file() {
            let root = std::fs::read_to_string(&dot_git)
                .ok()
                .and_then(|s| {
                    let gitdir = s.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
                    // A relative gitdir is relative to the directory holding the `.git` file.
                    let gitdir = cur.join(gitdir).to_string_lossy().into_owned();
                    let idx = gitdir.find("/.git/worktrees/")?;
                    Some(PathBuf::from(&gitdir[..idx]))
                })
                .unwrap_or_else(|| cur.to_path_buf());
            return git_result(&root);
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => break,
        }
    }
    ResolvedProject {
        project_key: start.to_string_lossy().into_owned(),
        is_git: false,
    }
}

fn git_result(root: &Path) -> ResolvedProject {
    ResolvedProject {
        project_key: canon(root).to_string_lossy().into_owned(),
        is_git: true,
    }
}

/// Resolves only cwds not yet present in `cwd_projects`; existing rows are never overwritten.
pub fn ensure_cwd_projects(db: &Db, cwds: &[String]) -> anyhow::Result<()> {
    let mut exists = db.conn.prepare("SELECT 1 FROM cwd_projects WHERE cwd = ?1")?;
    let mut insert = db.conn.prepare(
        "INSERT OR IGNORE INTO cwd_projects (cwd, project_key, is_git) VALUES (?1, ?2, ?3)",
    )?;
    for cwd in cwds {
        if exists.exists([cwd])? {
            continue;
        }
        let r = resolve_project(Path::new(cwd));
        insert.execute(rusqlite::params![cwd, r.project_key, r.is_git as i64])?;
    }
    Ok(())
}

/// Last folder name of the project key.
pub fn display_name(project_key: &str) -> String {
    Path::new(project_key)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| project_key.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn canon_s(p: &Path) -> String {
        fs::canonicalize(p).unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn subfolder_resolves_to_repo_root() {
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(repo.join("src/api")).unwrap();
        let r = resolve_project(&repo.join("src/api"));
        assert_eq!(r.project_key, canon_s(&repo));
        assert!(r.is_git);
    }

    #[test]
    fn worktree_resolves_to_main_repo() {
        let t = tempfile::tempdir().unwrap();
        let main = t.path().join("main");
        fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
        let wt = t.path().join("wt");
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}/.git/worktrees/wt\n", main.display())).unwrap();
        let r = resolve_project(&wt);
        assert_eq!(r.project_key, canon_s(&main));
        assert!(r.is_git);
    }

    #[test]
    fn relative_gitdir_resolves_against_the_dot_git_file() {
        let t = tempfile::tempdir().unwrap();
        let main = t.path().join("main");
        fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
        let wt = t.path().join("wt");
        fs::create_dir_all(wt.join("sub")).unwrap();
        fs::write(wt.join(".git"), "gitdir: ../main/.git/worktrees/wt\n").unwrap();
        let r = resolve_project(&wt.join("sub"));
        assert_eq!(r.project_key, canon_s(&main));
        assert!(r.is_git);
    }

    #[test]
    fn claude_worktree_inside_repo() {
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().join("repo");
        fs::create_dir_all(repo.join(".git/worktrees/fitur-x")).unwrap();
        let wt = repo.join(".claude/worktrees/fitur-x");
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}/.git/worktrees/fitur-x\n", repo.display())).unwrap();
        let r = resolve_project(&wt);
        assert_eq!(r.project_key, canon_s(&repo));
        assert!(r.is_git);
    }

    #[test]
    fn non_git_folder_is_its_own_project() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("plain");
        fs::create_dir_all(&d).unwrap();
        let r = resolve_project(&d);
        assert_eq!(r.project_key, canon_s(&d));
        assert!(!r.is_git);
    }

    #[test]
    fn missing_folder_falls_back_to_cwd() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("gone");
        let r = resolve_project(&d);
        assert_eq!(r.project_key, d.to_string_lossy());
        assert!(!r.is_git);
    }

    #[test]
    fn ensure_cwd_projects_does_not_overwrite_existing() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("gone");
        let cwd = d.to_string_lossy().into_owned();
        let db = Db::open_in_memory().unwrap();
        db.conn
            .execute("INSERT INTO cwd_projects (cwd, project_key, is_git) VALUES (?1, 'old', 1)", [&cwd])
            .unwrap();
        ensure_cwd_projects(&db, &[cwd.clone(), "/nonexistent/other".into()]).unwrap();
        let (k, g): (String, i64) = db
            .conn
            .query_row("SELECT project_key, is_git FROM cwd_projects WHERE cwd=?1", [&cwd], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!((k.as_str(), g), ("old", 1));
        let n: i64 = db.conn.query_row("SELECT COUNT(*) FROM cwd_projects", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn display_name_is_last_folder() {
        assert_eq!(display_name("/a/b/proj"), "proj");
    }
}
