use crate::git::{self, WorktreeEntry};
use crate::model::{CommitSummary, Project, Worktree};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
        last_commit: None,
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
        last_commit: None,
        sessions: vec![],
        has_upstream: false,
    }
}

/// Populate dirty/ahead/behind/has_upstream/last_commit on each worktree.
/// Runs per-repo git calls in parallel via `thread::scope`.
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
                let log = git::run_log_recent(&path, 1).ok();
                let _ = tx.send((i, j, status, log));
            });
        }
        drop(tx);
    });

    let now_secs: i64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

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
            if let Some(e) = entries.first() {
                let age_secs = (now_secs - e.committed_at).max(0) as u64;
                projects[i].worktrees[j].last_commit = Some(CommitSummary {
                    short_sha: e.short_sha.clone(),
                    subject: e.subject.clone(),
                    age: Duration::from_secs(age_secs),
                });
            }
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
    fn enrich_populates_last_commit_from_init() {
        let tmp = tempfile::tempdir().unwrap();
        make_tmp_repo(tmp.path(), "alpha");
        let mut projects = scan(tmp.path()).unwrap();
        enrich_with_status(&mut projects);
        let wt = &projects[0].worktrees[0];
        assert!(wt.last_commit.is_some());
        assert_eq!(wt.last_commit.as_ref().unwrap().subject, "init");
    }
}
