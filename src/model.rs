use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
    #[allow(dead_code)]
    pub root: PathBuf,
    pub worktrees: Vec<Worktree>,
}

#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub dirty: bool,
    pub ahead: u32,
    pub behind: u32,
    /// Most recent commits, newest first. Populated by discovery::enrich_with_status.
    pub recent_commits: Vec<CommitSummary>,
    pub sessions: Vec<Session>,
    pub has_upstream: bool,
}

#[derive(Debug, Clone)]
pub struct CommitSummary {
    pub short_sha: String,
    pub subject: String,
    /// Unix epoch seconds (committer date). For display formatting; never
    /// compared across timezones since git already normalises this.
    pub committed_at: i64,
}

#[derive(Debug, Clone)]
pub enum Session {
    BackgroundJob {
        id: String,
        status: JobStatus,
        cwd: PathBuf,
        mtime: SystemTime,
        /// Original prompt / task description from state.json `intent`.
        /// Surfaces a meaningful description instead of just the opaque id.
        intent: Option<String>,
        /// Total bytes on disk under ~/.claude/jobs/<id>/ (sum of regular
        /// files; symlinks not followed). For "what would I free?" display.
        size_bytes: u64,
    },
    Interactive {
        id: String,
        summary: String,
        cwd: PathBuf,
        mtime: SystemTime,
        state: SessionState,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus { Running, Completed, Failed, Unknown }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState { Active, Compact, Archived }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane { Worktrees, Sessions }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveFilter { ActiveOnly, All }

/// Time window for which interactive sessions to surface. Affects
/// sessions::scan_interactive's mtime cutoff. Bg jobs are not bounded
/// (they age out via Claude's own cleanup).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionWindow {
    Days7,
    Days30,
    All,
}

impl SessionWindow {
    pub fn cutoff_seconds(self) -> Option<u64> {
        match self {
            SessionWindow::Days7 => Some(60 * 60 * 24 * 7),
            SessionWindow::Days30 => Some(60 * 60 * 24 * 30),
            SessionWindow::All => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SessionWindow::Days7 => "7d",
            SessionWindow::Days30 => "30d",
            SessionWindow::All => "all",
        }
    }
    pub fn cycle(self) -> Self {
        match self {
            SessionWindow::Days7 => SessionWindow::Days30,
            SessionWindow::Days30 => SessionWindow::All,
            SessionWindow::All => SessionWindow::Days7,
        }
    }
}

/// Identifies a row in the worktree pane. Survives refresh by content,
/// not by index. We resolve back to indices each render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreePath {
    pub project: String,
    pub worktree: Option<PathBuf>,
}

impl TreePath {
    pub fn project_header(p: &Project) -> Self {
        Self { project: p.name.clone(), worktree: None }
    }
    pub fn worktree_row(p: &Project, wt: &Worktree) -> Self {
        Self { project: p.name.clone(), worktree: Some(wt.path.clone()) }
    }
}

#[derive(Debug)]
pub struct AppState {
    pub projects: Vec<Project>,
    pub selected: Option<TreePath>,
    pub focus: Pane,
    pub filter: ActiveFilter,
    pub search: Option<String>,
    pub last_refresh: std::time::Instant,
    pub status: StatusLine,
    pub expanded: std::collections::HashSet<String>,
    pub generation: u64,
    /// Index into the currently-selected worktree's sessions list. Reset to
    /// 0 whenever the worktree selection changes. None means no session
    /// selected (e.g., current worktree has no sessions).
    pub selected_session: Option<usize>,
    /// Interactive-session time window; cycled with the `t` key.
    pub session_window: SessionWindow,
}

#[derive(Debug, Clone, Default)]
pub struct StatusLine {
    pub message: Option<String>,
    pub set_at: Option<std::time::Instant>,
}

impl StatusLine {
    pub fn say(&mut self, msg: impl Into<String>) {
        self.message = Some(msg.into());
        self.set_at = Some(std::time::Instant::now());
    }
    pub fn current(&self) -> Option<&str> {
        let set_at = self.set_at?;
        if set_at.elapsed() < std::time::Duration::from_secs(3) {
            self.message.as_deref()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn treepath_equality_survives_clone() {
        let p = Project { name: "x".into(), root: PathBuf::from("/x"), worktrees: vec![] };
        let a = TreePath::project_header(&p);
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn treepath_distinguishes_project_from_worktree() {
        let p = Project { name: "x".into(), root: PathBuf::from("/x"), worktrees: vec![] };
        let wt = Worktree {
            path: "/x/wt".into(), branch: None, dirty: false,
            ahead: 0, behind: 0, recent_commits: vec![], sessions: vec![], has_upstream: false,
        };
        let header = TreePath::project_header(&p);
        let row = TreePath::worktree_row(&p, &wt);
        assert_ne!(header, row);
    }

    #[test]
    fn statusline_expires() {
        let mut s = StatusLine::default();
        s.say("copied");
        assert_eq!(s.current(), Some("copied"));
    }
}
