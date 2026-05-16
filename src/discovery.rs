use crate::git::{self, WorktreeEntry};
use crate::model::{CommitSummary, Project, Worktree};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Walk `root` for immediate non-hidden subdirectories that contain `.git`.
/// One Project per directory, worktrees populated by `git worktree list`.
pub fn scan(root: &Path) -> Result<Vec<Project>> {
    let mut out = Vec::new();
    let read = match std::fs::read_dir(root) {
        Ok(r) => r,
        Err(_) => return Ok(out),
    };
    let mut entries: Vec<_> = read.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) if !n.starts_with('.') => n.to_string(),
            _ => continue,
        };
        if !path.is_dir() || !is_git_dir(&path) {
            continue;
        }
        let worktrees = match git::run_worktree_list(&path) {
            Ok(entries) => entries.into_iter().map(entry_to_worktree).collect(),
            Err(_) => vec![bare_worktree(&path)],
        };
        out.push(Project { name, root: path, worktrees });
    }
    Ok(out)
}

/// Scan multiple roots and merge results.
///
/// Roots are scanned in order; the first occurrence of any worktree path
/// wins, so later roots can shadow earlier ones (rare). The combined
/// project list is then sorted by name. Duplicate worktree paths across
/// roots are deduped so the same repo doesn't show up twice if it happens
/// to be reachable from two configured roots.
pub fn scan_many(roots: &[PathBuf]) -> Result<Vec<Project>> {
    let mut seen_worktrees: std::collections::HashSet<PathBuf> = Default::default();
    let mut projects: Vec<Project> = Vec::new();

    for root in roots {
        let chunk = scan(root)?;
        for mut p in chunk {
            // Drop worktrees we've already seen via an earlier root.
            p.worktrees.retain(|w| seen_worktrees.insert(w.path.clone()));
            if p.worktrees.is_empty() {
                continue;
            }
            projects.push(p);
        }
    }

    // Sort projects by name (case-insensitive) for stable display.
    projects.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(projects)
}

/// Resolve the project roots to scan. Reads `WT_ROOTS` env var
/// (colon-separated paths, like `PATH`); falls back to a sensible default
/// covering common locations: `~/projects`, `~/Projects`,
/// `~/Projects/clients`.
pub fn default_roots() -> Vec<PathBuf> {
    if let Ok(env_value) = std::env::var("WT_ROOTS") {
        if !env_value.trim().is_empty() {
            return env_value
                .split(':')
                .filter(|s| !s.is_empty())
                .map(|s| expand_tilde(s))
                .collect();
        }
    }
    let home = dirs::home_dir().unwrap_or_default();
    let mut roots = vec![
        home.join("projects"),
        home.join("Projects"),
        home.join("Projects/clients"),
    ];
    // Drop roots that don't exist so we don't waste readdir cycles.
    roots.retain(|p| p.is_dir());
    roots
}

fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(s)
}

fn is_git_dir(path: &Path) -> bool {
    path.join(".git").exists()
}

fn entry_to_worktree(e: WorktreeEntry) -> Worktree {
    Worktree {
        path: e.path,
        branch: e.branch,
        dirty: false,
        ahead: 0,
        behind: 0,
        recent_commits: vec![],
        sessions: vec![],
        has_upstream: false,
    }
}

fn bare_worktree(path: &Path) -> Worktree {
    Worktree {
        path: path.to_path_buf(),
        branch: None,
        dirty: false,
        ahead: 0,
        behind: 0,
        recent_commits: vec![],
        sessions: vec![],
        has_upstream: false,
    }
}

/// Populate dirty/ahead/behind/has_upstream/recent_commits on each worktree.
/// Runs per-repo git calls in parallel via `thread::scope`. Fetches the
/// last 5 commits per worktree so the sessions pane can show a mini log
/// when there's empty space.
pub fn enrich_with_status(projects: &mut [Project]) {
    use std::sync::mpsc;

    let work: Vec<(usize, usize, PathBuf)> = projects
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            p.worktrees
                .iter()
                .enumerate()
                .map(move |(j, w)| (i, j, w.path.clone()))
        })
        .collect();

    let (tx, rx) = mpsc::channel();
    std::thread::scope(|s| {
        for (i, j, path) in work {
            let tx = tx.clone();
            s.spawn(move || {
                let status = git::run_status_v2(&path).ok();
                let log = git::run_log_recent(&path, 5).ok();
                let _ = tx.send((i, j, status, log));
            });
        }
        drop(tx);
    });

    while let Ok((i, j, status, log)) = rx.recv() {
        if let Some(s) = status {
            let wt = &mut projects[i].worktrees[j];
            wt.branch = s.branch.or(wt.branch.take());
            wt.dirty = s.dirty;
            wt.ahead = s.ahead;
            wt.behind = s.behind;
            wt.has_upstream = s.upstream.is_some();
        }
        if let Some(entries) = log {
            projects[i].worktrees[j].recent_commits = entries
                .into_iter()
                .map(|e| CommitSummary {
                    short_sha: e.short_sha,
                    subject: e.subject,
                })
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_tmp_repo(parent: &Path, name: &str) -> PathBuf {
        let p = parent.join(name);
        fs::create_dir_all(&p).unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&p)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-q", "-m", "init"])
            .current_dir(&p)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap();
        p
    }

    #[test]
    fn scan_finds_git_dirs_and_skips_others() {
        let tmp = tempfile::tempdir().unwrap();
        make_tmp_repo(tmp.path(), "alpha");
        make_tmp_repo(tmp.path(), "beta");
        fs::create_dir_all(tmp.path().join("not-a-repo")).unwrap();
        fs::create_dir_all(tmp.path().join(".hidden")).unwrap();

        let projects = scan(tmp.path()).unwrap();
        let names: Vec<_> = projects.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        assert!(!names.contains(&"not-a-repo"));
        assert!(!names.contains(&".hidden"));
        assert_eq!(projects.len(), 2);
    }

    #[test]
    fn scan_missing_root_returns_empty() {
        let out = scan(Path::new("/definitely/not/here/abc123")).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn enrich_populates_recent_commits_from_init() {
        let tmp = tempfile::tempdir().unwrap();
        make_tmp_repo(tmp.path(), "alpha");
        let mut projects = scan(tmp.path()).unwrap();
        enrich_with_status(&mut projects);
        let wt = &projects[0].worktrees[0];
        assert!(!wt.recent_commits.is_empty());
        assert_eq!(wt.recent_commits[0].subject, "init");
    }
}
