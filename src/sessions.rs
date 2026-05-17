use crate::model::{JobStatus, Project, Session, SessionGroup, SessionState};
use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Field names per `docs/notes/format-findings.md` (verified against real state.json).
#[derive(Debug, Deserialize)]
struct JobMetadata {
    /// Canonical worktree path. Prefer over `cwd` when set.
    #[serde(default, rename = "worktreePath")]
    worktree_path: Option<PathBuf>,
    /// Shell cwd at spawn. Fallback if `worktreePath` is null.
    cwd: PathBuf,
    #[serde(default)]
    state: Option<String>,
    #[serde(default, rename = "inFlight")]
    in_flight: Option<bool>,
    #[serde(default, rename = "updatedAt")]
    #[allow(dead_code)]
    updated_at: Option<String>,
    /// Original prompt / task description. Far more useful as a row label
    /// than the opaque hash id.
    #[serde(default)]
    intent: Option<String>,
}

impl JobMetadata {
    fn effective_cwd(&self) -> PathBuf {
        self.worktree_path.clone().unwrap_or_else(|| self.cwd.clone())
    }
}

/// Recursively sum regular-file sizes in `dir`. Skips symlinks and errors.
/// Returns 0 if the dir doesn't exist or can't be read.
fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return 0,
    };
    for entry in read.filter_map(|e| e.ok()) {
        let path = entry.path();
        match entry.file_type() {
            Ok(ft) if ft.is_file() => {
                if let Ok(m) = entry.metadata() {
                    total += m.len();
                }
            }
            Ok(ft) if ft.is_dir() => {
                total += dir_size(&path);
            }
            _ => {} // symlinks etc. ignored
        }
    }
    total
}

fn parse_status(state: Option<&str>, in_flight: Option<bool>) -> JobStatus {
    if in_flight == Some(true) {
        return JobStatus::Running;
    }
    match state {
        Some("working") | Some("running") | Some("in_progress") => JobStatus::Running,
        Some("done") | Some("completed") | Some("success") => JobStatus::Completed,
        Some("blocked") | Some("failed") | Some("error") | Some("cancelled") => JobStatus::Failed,
        _ => JobStatus::Unknown,
    }
}

pub fn scan_jobs(jobs_dir: &Path) -> Result<Vec<Session>> {
    let mut out = Vec::new();
    let read = match std::fs::read_dir(jobs_dir) {
        Ok(r) => r,
        Err(_) => return Ok(out),
    };
    for entry in read.filter_map(|e| e.ok()) {
        let id = match entry.file_name().to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let meta_path = dir.join("state.json");
        if !meta_path.exists() {
            continue;
        }
        let bytes = match std::fs::read(&meta_path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let meta: JobMetadata = match serde_json::from_slice(&bytes) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = std::fs::metadata(&meta_path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let size_bytes = dir_size(&dir);
        out.push(Session::BackgroundJob {
            id,
            status: parse_status(meta.state.as_deref(), meta.in_flight),
            cwd: meta.effective_cwd(),
            mtime,
            intent: meta.intent,
            size_bytes,
        });
    }
    Ok(out)
}

/// Encode an absolute path to the directory name used under `~/.claude/projects/`.
/// Both `/` and `.` collapse to `-` (verified against real data; see
/// `docs/notes/format-findings.md`). The encoding is one-way and irreversible.
/// We never decode; we only go path -> encoded -> directory lookup.
pub fn encode_cwd(p: &Path) -> String {
    p.to_string_lossy().replace('/', "-").replace('.', "-")
}

pub fn scan_interactive(
    projects_dir: &Path,
    known_worktrees: &[PathBuf],
    cutoff_seconds: Option<u64>,
) -> Result<Vec<Session>> {
    let mut out = Vec::new();
    let cutoff = match cutoff_seconds {
        Some(s) => SystemTime::now()
            .checked_sub(Duration::from_secs(s))
            .unwrap_or(SystemTime::UNIX_EPOCH),
        None => SystemTime::UNIX_EPOCH,
    };

    for known in known_worktrees {
        let sub = projects_dir.join(encode_cwd(known));
        if !sub.is_dir() {
            continue;
        }

        let files_read = match std::fs::read_dir(&sub) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let mut sessions_here: Vec<(SystemTime, Session)> = Vec::new();
        for f in files_read.filter_map(|e| e.ok()) {
            let fp = f.path();
            if fp.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let mt = match std::fs::metadata(&fp).and_then(|m| m.modified()) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if mt < cutoff {
                continue;
            }

            let stem = fp
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let summary = first_summary_line(&fp).unwrap_or_default();
            sessions_here.push((
                mt,
                Session::Interactive {
                    id: stem,
                    summary,
                    cwd: known.clone(),
                    mtime: mt,
                    state: SessionState::Active,
                },
            ));
        }
        sessions_here.sort_by(|a, b| b.0.cmp(&a.0));
        sessions_here.truncate(5);
        out.extend(sessions_here.into_iter().map(|(_, s)| s));
    }
    Ok(out)
}

/// Pull a session summary out of the jsonl. Returns the *full* text (up
/// to a generous 4096-char safety cap) so the row renderer can truncate
/// for display and the detail modal can show everything. Preference:
/// ai-title first, then first user message content.
fn first_summary_line(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};
    const HARD_CAP: usize = 4096;
    let f = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(f);
    let mut fallback: Option<String> = None;
    for line in reader.lines().take(50) {
        let line = line.ok()?;
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let line_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if line_type == "ai-title" {
            if let Some(s) = v.get("title").and_then(|t| t.as_str()) {
                return Some(cap_chars(s, HARD_CAP));
            }
        }
        if line_type == "user" && fallback.is_none() {
            let msg = v.get("message");
            let content = msg
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .or_else(|| v.get("content").and_then(|c| c.as_str()));
            if let Some(c) = content {
                fallback = Some(cap_chars(c, HARD_CAP));
            }
        }
    }
    fallback.or_else(|| Some("(no summary)".into()))
}

/// Cap-by-chars without ellipsis. Used at scan time so we never store more
/// than HARD_CAP chars per summary (defends against pasted-in megabytes).
/// The row renderer applies its own width-aware ellipsis at display time.
fn cap_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

/// Scan ALL interactive sessions across `~/.claude/projects/` regardless
/// of whether their cwd matches a known worktree. Cwd is extracted from
/// the first jsonl event that carries it (Claude stores `cwd` on most
/// events). Sessions whose cwd can't be extracted are dropped.
pub fn scan_all_interactive(
    projects_dir: &Path,
    cutoff_seconds: Option<u64>,
) -> Result<Vec<Session>> {
    let mut out = Vec::new();
    let cutoff = match cutoff_seconds {
        Some(s) => SystemTime::now()
            .checked_sub(Duration::from_secs(s))
            .unwrap_or(SystemTime::UNIX_EPOCH),
        None => SystemTime::UNIX_EPOCH,
    };

    let read = match std::fs::read_dir(projects_dir) {
        Ok(r) => r,
        Err(_) => return Ok(out),
    };

    for entry in read.filter_map(|e| e.ok()) {
        let sub = entry.path();
        if !sub.is_dir() {
            continue;
        }
        let files_read = match std::fs::read_dir(&sub) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let mut sessions_here: Vec<(SystemTime, Session)> = Vec::new();
        for f in files_read.filter_map(|e| e.ok()) {
            let fp = f.path();
            if fp.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let mt = match std::fs::metadata(&fp).and_then(|m| m.modified()) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if mt < cutoff {
                continue;
            }
            let cwd = match first_cwd_in_jsonl(&fp) {
                Some(c) => c,
                None => continue, // skip files we can't pin to a cwd
            };
            let stem = fp
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let summary = first_summary_line(&fp).unwrap_or_default();
            sessions_here.push((
                mt,
                Session::Interactive {
                    id: stem,
                    summary,
                    cwd,
                    mtime: mt,
                    state: SessionState::Active,
                },
            ));
        }
        sessions_here.sort_by(|a, b| b.0.cmp(&a.0));
        sessions_here.truncate(5);
        out.extend(sessions_here.into_iter().map(|(_, s)| s));
    }
    Ok(out)
}

fn first_cwd_in_jsonl(path: &Path) -> Option<PathBuf> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(f);
    for line in reader.lines().take(20) {
        let line = line.ok()?;
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(c) = v.get("cwd").and_then(|c| c.as_str()) {
            return Some(PathBuf::from(c));
        }
    }
    None
}

/// Helper: every Session has a cwd (BackgroundJob: effective_cwd field;
/// Interactive: cwd extracted from jsonl).
pub fn session_cwd(s: &Session) -> &Path {
    match s {
        Session::BackgroundJob { cwd, .. } => cwd,
        Session::Interactive { cwd, .. } => cwd,
    }
}

/// Helper: mtime of a session.
pub fn session_mtime(s: &Session) -> SystemTime {
    match s {
        Session::BackgroundJob { mtime, .. } => *mtime,
        Session::Interactive { mtime, .. } => *mtime,
    }
}

/// Build the Sessions-view groups: every cwd that has Claude sessions
/// (bg or interactive), sorted by most-recent activity within each
/// group, and groups sorted by their most-recent session.
pub fn build_session_groups(
    jobs_dir: &Path,
    projects_dir: &Path,
    known_worktrees: &[PathBuf],
    cutoff_seconds: Option<u64>,
) -> Vec<SessionGroup> {
    use std::collections::HashMap;
    let known: std::collections::HashSet<PathBuf> = known_worktrees.iter().cloned().collect();

    let jobs = scan_jobs(jobs_dir).unwrap_or_default();
    let interactive = scan_all_interactive(projects_dir, cutoff_seconds).unwrap_or_default();

    let mut groups: HashMap<PathBuf, SessionGroup> = HashMap::new();
    for s in jobs.into_iter().chain(interactive) {
        let cwd = session_cwd(&s).to_path_buf();
        let entry = groups.entry(cwd.clone()).or_insert_with(|| SessionGroup {
            cwd: cwd.clone(),
            has_worktree: known.contains(&cwd),
            sessions: vec![],
        });
        entry.sessions.push(s);
    }

    let mut out: Vec<SessionGroup> = groups.into_values().collect();
    for g in &mut out {
        g.sessions.sort_by_key(|s| std::cmp::Reverse(session_mtime(s)));
    }
    out.sort_by_key(|g| {
        std::cmp::Reverse(
            g.sessions
                .first()
                .map(session_mtime)
                .unwrap_or(SystemTime::UNIX_EPOCH),
        )
    });
    out
}

/// Attach sessions to the worktree whose `path` matches the session's `cwd`.
/// Sessions with no matching worktree are dropped.
pub fn attach_to_worktrees(projects: &mut [Project], sessions: Vec<Session>) {
    for sess in sessions {
        let cwd = match &sess {
            Session::BackgroundJob { cwd, .. } => cwd.clone(),
            Session::Interactive { cwd, .. } => cwd.clone(),
        };
        for p in projects.iter_mut() {
            for wt in p.worktrees.iter_mut() {
                if wt.path == cwd {
                    wt.sessions.push(sess.clone());
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_job(parent: &Path, id: &str, json: &str) {
        let dir = parent.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("state.json"), json).unwrap();
    }

    #[test]
    fn scan_jobs_reads_state_with_worktree_path() {
        let tmp = tempfile::tempdir().unwrap();
        write_job(
            tmp.path(),
            "abc1",
            r#"{"cwd":"/home/jane","worktreePath":"/home/jane/projects/thelma","state":"working","inFlight":true}"#,
        );
        write_job(
            tmp.path(),
            "abc2",
            r#"{"cwd":"/home/jane/projects/zele","worktreePath":null,"state":"done","inFlight":false}"#,
        );

        let out = scan_jobs(tmp.path()).unwrap();
        assert_eq!(out.len(), 2);
        let working: Vec<_> = out
            .iter()
            .filter(|s| matches!(s, Session::BackgroundJob { status: JobStatus::Running, .. }))
            .collect();
        assert_eq!(working.len(), 1);
        if let Session::BackgroundJob { cwd, .. } = working[0] {
            assert_eq!(cwd, &PathBuf::from("/home/jane/projects/thelma"));
        }
    }

    #[test]
    fn scan_jobs_skips_malformed() {
        let tmp = tempfile::tempdir().unwrap();
        write_job(tmp.path(), "good",
            r#"{"cwd":"/x","state":"working","inFlight":true}"#);
        write_job(tmp.path(), "bad", "this is not json");

        let out = scan_jobs(tmp.path()).unwrap();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn scan_jobs_blocked_maps_to_failed() {
        let tmp = tempfile::tempdir().unwrap();
        write_job(tmp.path(), "b",
            r#"{"cwd":"/x","state":"blocked","inFlight":false}"#);
        let out = scan_jobs(tmp.path()).unwrap();
        assert!(matches!(&out[0],
            Session::BackgroundJob { status: JobStatus::Failed, .. }));
    }

    #[test]
    fn scan_jobs_missing_dir_returns_empty() {
        let out = scan_jobs(Path::new("/nope/nada")).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn encode_cwd_replaces_slashes() {
        let cwd = Path::new("/home/jane/projects/thelma");
        assert_eq!(encode_cwd(cwd), "-home-jane-projects-thelma");
    }

    #[test]
    fn encode_cwd_replaces_dots() {
        let cwd = Path::new("/home/jane/projects/thelma/.claude/worktrees/spec-dashboard-design");
        assert_eq!(
            encode_cwd(cwd),
            "-home-jane-projects-thelma--claude-worktrees-spec-dashboard-design"
        );
    }

    #[test]
    fn encode_cwd_handles_hyphens_in_dir_names() {
        let cwd = Path::new("/home/jane/projects/foo-bar");
        assert_eq!(encode_cwd(cwd), "-home-jane-projects-foo-bar");
    }

    #[test]
    fn scan_interactive_caps_at_five_per_known_path() {
        let tmp = tempfile::tempdir().unwrap();
        let known = PathBuf::from("/home/jane/projects/thelma");
        let dir = tmp.path().join(encode_cwd(&known));
        fs::create_dir_all(&dir).unwrap();
        for i in 0..10 {
            let f = dir.join(format!("session-{i:02}.jsonl"));
            fs::write(&f, "{}\n").unwrap();
        }
        let out = scan_interactive(tmp.path(), &[known.clone()], Some(60 * 60 * 24 * 30)).unwrap();
        let mine: Vec<_> = out
            .iter()
            .filter(|s| matches!(s, Session::Interactive { cwd, .. } if cwd == &known))
            .collect();
        assert_eq!(mine.len(), 5);
    }

    #[test]
    fn scan_interactive_ignores_unknown_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("-some-unknown-project");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.jsonl"), "{}\n").unwrap();
        let out = scan_interactive(
            tmp.path(),
            &[PathBuf::from("/home/jane/projects/thelma")],
            Some(60 * 60 * 24 * 30),
        )
        .unwrap();
        assert!(out.is_empty());
    }
}
