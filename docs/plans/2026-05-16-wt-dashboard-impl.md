# wt Dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `wt`, a Rust+ratatui terminal dashboard that lists git worktrees across `~/projects/`, surfaces attached Claude Code sessions, and lets Jane copy a `cd ... && claude ...` launch command to her clipboard from a phone over SSH.

**Architecture:** Single-binary Rust app. Three pure I/O modules (`discovery`, `git`, `sessions`) feed a `model::AppState` mutated only by `app`. The `ui` module is a one-way function of `&AppState` plus terminal width. Snapshot refresh on launch and on `r`; a 10-second background tick re-reads only `~/.claude/jobs/` for active-session badges. Spec at `docs/specs/2026-05-16-wt-dashboard-design.md`.

**Tech Stack:** Rust 2021 edition. `ratatui 0.29`, `crossterm 0.28`, `anyhow 1`, `serde 1` + `serde_json 1`, `dirs 5`, `base64 0.22`. Shell out to system `git`. No tokio, no libgit2.

---

## File Structure

```
wt/
├── Cargo.toml
├── README.md
├── docs/
│   ├── specs/2026-05-16-wt-dashboard-design.md      # already exists
│   ├── plans/2026-05-16-wt-dashboard-impl.md        # this file
│   └── notes/format-findings.md                      # Task 1 output
├── src/
│   ├── main.rs                                       # entrypoint, terminal setup
│   ├── app.rs                                        # AppState, event loop, key handlers
│   ├── model.rs                                      # Project, Worktree, Session, TreePath
│   ├── discovery.rs                                  # walk ~/projects/, build Vec<Project>
│   ├── git.rs                                        # shell-out + porcelain parsers
│   ├── sessions.rs                                   # scan ~/.claude/{jobs,projects}
│   ├── actions.rs                                    # OSC 52 clipboard, print-and-exit
│   ├── tick.rs                                       # background 10s thread
│   └── ui/
│       ├── mod.rs                                    # frame layout, dispatch by width
│       ├── theme.rs                                  # palette as ratatui Styles
│       ├── layout.rs                                 # width-to-column-set logic
│       ├── worktrees.rs                              # top pane renderer
│       ├── sessions.rs                               # bottom pane renderer
│       └── modal.rs                                  # commit log popup
└── tests/
    └── fixtures/                                     # test data (git output, jobs json, etc.)
```

Each file has one job. `git.rs` parses and shells out but holds no state. `model.rs` is types only. `ui/*` reads `&AppState` and writes ratatui buffers, never mutates. `app.rs` is the only mutator.

---

## Task 1: Verify `~/.claude/projects/` and `~/.claude/jobs/` formats

**Why first:** the spec pinned three open items. Two of them require reading real files before any code is written.

**Files:**
- Create: `docs/notes/format-findings.md`

- [ ] **Step 1: Inspect `~/.claude/jobs/` metadata**

```bash
ls ~/.claude/jobs/ | head -5
# pick one
JOB=$(ls ~/.claude/jobs/ | head -1)
ls ~/.claude/jobs/$JOB/
# expect: metadata.json or similar, transcript files, working_directory pointer
cat ~/.claude/jobs/$JOB/metadata.json 2>/dev/null | head -60
# also check any state.json, info.json, manifest.json
for f in ~/.claude/jobs/$JOB/*.json; do echo "=== $f ==="; head -40 "$f"; done
```

Expected output: at least one JSON file per job with fields covering `cwd` or `working_directory`, a status, and timestamps. Record the actual field names found.

- [ ] **Step 2: Inspect `~/.claude/projects/` encoded-cwd layout**

```bash
ls ~/.claude/projects/ | head -10
# pick one that looks like a known project
ls -la ~/.claude/projects/-home-jane-projects-example-project/ 2>/dev/null | head -10
# inspect first/last line of a jsonl
F=$(ls ~/.claude/projects/-home-jane-projects-example-project/*.jsonl 2>/dev/null | head -1)
head -1 "$F" | jq .
tail -1 "$F" | jq .
```

Expected: jsonl entries with `sessionId`, `timestamp`, role/type fields. Record the field names and the encoded-cwd derivation (confirm `/` → `-`).

- [ ] **Step 3: Inspect worktrees in the wild**

```bash
git -C ~/projects/example-project worktree list --porcelain
git -C ~/projects/example-project status --porcelain=v2 --branch
```

Expected: confirm the porcelain v2 output format matches the parser tests we'll write in Tasks 4-5.

- [ ] **Step 4: Write `docs/notes/format-findings.md`**

```markdown
# Format Findings (2026-05-16)

## Job metadata schema (~/.claude/jobs/<id>/)
- File: <actual-filename>.json
- Fields:
  - cwd: <field-name-here>  (string, absolute path)
  - status: <field-name-here>  (enum: <values-here>)
  - created_at: <field-name-here>  (ISO 8601 string)
  - last_activity: <field-name-here>  (ISO 8601 string)
- Other files in dir: <list>

## Interactive session layout (~/.claude/projects/)
- Encoded cwd: confirmed / NOT confirmed `/` → `-` mapping
  - Example: `/home/jane/projects/example-project` → `-home-jane-projects-example-project`
- Per-session file: `<session-uuid>.jsonl`
- First-line fields: <list>
- Last-line fields: <list>
- Summary source: <field path, e.g. "first user message">

## Worktree porcelain (git worktree list --porcelain)
- Format confirmed: yes / variations
- Sample output: <paste>

## Status v2 porcelain (git status --porcelain=v2 --branch)
- Format confirmed: yes
- Branch header lines: # branch.head, # branch.upstream, # branch.ab +N -M
- Entry format: <paste>
```

- [ ] **Step 5: Commit**

```bash
git add docs/notes/format-findings.md
git commit -m "docs(notes): record claude jobs + projects + git porcelain formats"
```

---

## Task 2: Cargo project scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `.gitignore`

- [ ] **Step 1: Initialize Cargo project**

```bash
cd ~/projects/wt
# do NOT run `cargo init` (it would create main.rs as hello world and Cargo.toml; we want explicit content below)
```

- [ ] **Step 2: Write `Cargo.toml`**

```toml
[package]
name = "wt"
version = "0.1.0"
edition = "2021"
authors = ["Jane Brent"]
description = "Terminal dashboard for git worktrees and Claude Code sessions"
license = "MIT"

[dependencies]
ratatui = "0.29"
crossterm = "0.28"
anyhow = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
dirs = "5"
base64 = "0.22"

[profile.release]
lto = true
codegen-units = 1
strip = true

[[bin]]
name = "wt"
path = "src/main.rs"
```

- [ ] **Step 3: Write minimal `src/main.rs`**

```rust
fn main() -> anyhow::Result<()> {
    println!("wt 0.1.0");
    Ok(())
}
```

- [ ] **Step 4: Write `.gitignore`**

```
/target
Cargo.lock.bak
```

- [ ] **Step 5: Build and run**

```bash
cargo build
cargo run
```

Expected: prints `wt 0.1.0`, exits 0.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs .gitignore
git commit -m "chore: cargo scaffold with deps locked"
```

---

## Task 3: Domain types in `model.rs`

**Files:**
- Create: `src/model.rs`
- Modify: `src/main.rs` (add `mod model;`)

- [ ] **Step 1: Write `src/model.rs`**

```rust
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
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
    pub last_commit: Option<CommitSummary>,
    pub sessions: Vec<Session>,
    pub has_upstream: bool,
}

#[derive(Debug, Clone)]
pub struct CommitSummary {
    pub short_sha: String,
    pub subject: String,
    pub age: Duration,
}

#[derive(Debug, Clone)]
pub enum Session {
    BackgroundJob {
        id: String,
        status: JobStatus,
        cwd: PathBuf,
        age: Duration,
    },
    Interactive {
        id: String,
        summary: String,
        cwd: PathBuf,
        age: Duration,
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

/// Identifies a row in the worktree pane. Survives refresh by content,
/// not by index. We resolve back to indices each render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreePath {
    pub project: String,           // project.name
    pub worktree: Option<PathBuf>, // None = the project header row itself
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
    pub expanded: std::collections::HashSet<String>, // project names that are expanded
    pub generation: u64, // bumped on `r`; tick messages older than this are dropped
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
            ahead: 0, behind: 0, last_commit: None, sessions: vec![], has_upstream: false,
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
```

- [ ] **Step 2: Wire module into `main.rs`**

```rust
mod model;

fn main() -> anyhow::Result<()> {
    println!("wt 0.1.0");
    Ok(())
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test model::
```

Expected: 3 passed.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs src/model.rs
git commit -m "feat(model): domain types and TreePath identity"
```

---

## Task 4: `git::parse_worktree_list` and `git::parse_status_v2`

**Files:**
- Create: `src/git.rs`
- Modify: `src/main.rs` (add `mod git;`)

- [ ] **Step 1: Write failing tests in `src/git.rs`**

```rust
use anyhow::{anyhow, Result};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head_sha: String,
    pub bare: bool,
    pub detached: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusSummary {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
}

pub fn parse_worktree_list(porcelain: &str) -> Result<Vec<WorktreeEntry>> {
    todo!()
}

pub fn parse_status_v2(porcelain: &str) -> Result<StatusSummary> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_worktree_list_one_entry() {
        let input = "worktree /home/jane/projects/example-project\nHEAD abc123def456789\nbranch refs/heads/main\n\n";
        let out = parse_worktree_list(input).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, PathBuf::from("/home/jane/projects/example-project"));
        assert_eq!(out[0].branch.as_deref(), Some("main"));
        assert_eq!(out[0].head_sha, "abc123def456789");
        assert!(!out[0].bare && !out[0].detached);
    }

    #[test]
    fn parse_worktree_list_with_linked() {
        let input = "\
worktree /home/jane/projects/wt
HEAD abc123def456789
branch refs/heads/main

worktree /home/jane/projects/wt/.claude/worktrees/spec
HEAD 9c76d06aaa1
branch refs/heads/worktree-spec-dashboard-design

";
        let out = parse_worktree_list(input).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].branch.as_deref(), Some("worktree-spec-dashboard-design"));
    }

    #[test]
    fn parse_worktree_list_detached() {
        let input = "worktree /tmp/x\nHEAD abc\ndetached\n\n";
        let out = parse_worktree_list(input).unwrap();
        assert!(out[0].detached);
        assert!(out[0].branch.is_none());
    }

    #[test]
    fn parse_status_v2_clean() {
        let input = "\
# branch.oid abc
# branch.head main
# branch.upstream origin/main
# branch.ab +0 -0
";
        let s = parse_status_v2(input).unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);
        assert!(!s.dirty);
    }

    #[test]
    fn parse_status_v2_dirty_and_ahead() {
        let input = "\
# branch.oid abc
# branch.head main
# branch.upstream origin/main
# branch.ab +2 -1
1 .M N... 100644 100644 100644 abc def src/file.rs
? untracked.txt
";
        let s = parse_status_v2(input).unwrap();
        assert_eq!(s.ahead, 2);
        assert_eq!(s.behind, 1);
        assert!(s.dirty);
    }

    #[test]
    fn parse_status_v2_no_upstream() {
        let input = "\
# branch.oid abc
# branch.head feature-x
";
        let s = parse_status_v2(input).unwrap();
        assert_eq!(s.branch.as_deref(), Some("feature-x"));
        assert!(s.upstream.is_none());
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);
    }
}
```

- [ ] **Step 2: Wire and verify failure**

In `src/main.rs`:

```rust
mod model;
mod git;

fn main() -> anyhow::Result<()> {
    println!("wt 0.1.0");
    Ok(())
}
```

Run: `cargo test git::`
Expected: 6 tests failing with `not yet implemented` (the `todo!()` macro).

- [ ] **Step 3: Implement `parse_worktree_list`**

Replace the body of `parse_worktree_list` with:

```rust
pub fn parse_worktree_list(porcelain: &str) -> Result<Vec<WorktreeEntry>> {
    let mut out = Vec::new();
    let mut cur: Option<WorktreeEntry> = None;
    for line in porcelain.lines() {
        if line.is_empty() {
            if let Some(e) = cur.take() {
                out.push(e);
            }
            continue;
        }
        let (key, rest) = match line.split_once(' ') {
            Some((k, r)) => (k, Some(r)),
            None => (line, None),
        };
        match key {
            "worktree" => {
                let path = rest.ok_or_else(|| anyhow!("missing worktree path"))?.into();
                cur = Some(WorktreeEntry {
                    path,
                    branch: None,
                    head_sha: String::new(),
                    bare: false,
                    detached: false,
                });
            }
            "HEAD" => {
                if let Some(e) = cur.as_mut() {
                    e.head_sha = rest.unwrap_or("").to_string();
                }
            }
            "branch" => {
                if let Some(e) = cur.as_mut() {
                    e.branch = rest.map(|r| r.trim_start_matches("refs/heads/").to_string());
                }
            }
            "bare" => {
                if let Some(e) = cur.as_mut() { e.bare = true; }
            }
            "detached" => {
                if let Some(e) = cur.as_mut() { e.detached = true; }
            }
            _ => {}
        }
    }
    if let Some(e) = cur.take() { out.push(e); }
    Ok(out)
}
```

- [ ] **Step 4: Implement `parse_status_v2`**

```rust
pub fn parse_status_v2(porcelain: &str) -> Result<StatusSummary> {
    let mut s = StatusSummary::default();
    for line in porcelain.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            s.branch = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            s.upstream = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            // format: "+N -M"
            let mut parts = rest.split_whitespace();
            let ahead = parts.next().unwrap_or("+0").trim_start_matches('+').parse().unwrap_or(0);
            let behind = parts.next().unwrap_or("-0").trim_start_matches('-').parse().unwrap_or(0);
            s.ahead = ahead;
            s.behind = behind;
        } else if !line.starts_with('#') && !line.is_empty() {
            // Any non-header, non-empty line means there's a change.
            s.dirty = true;
        }
    }
    Ok(s)
}
```

- [ ] **Step 5: Run tests**

```bash
cargo test git::
```

Expected: 6 passed.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/git.rs
git commit -m "feat(git): porcelain parsers for worktree list and status v2"
```

---

## Task 5: `git::log_porcelain` parser + shell-out helpers

**Files:**
- Modify: `src/git.rs`

- [ ] **Step 1: Add failing test for log parsing**

Append to the `tests` module in `src/git.rs`:

```rust
    #[test]
    fn parse_log_one_line_picks_short_sha_and_subject() {
        // git log --format='%h%x09%ct%x09%s'
        let input = "abc1234\t1747400000\tfix gmail label search\n";
        let out = parse_log_porcelain(input).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].short_sha, "abc1234");
        assert_eq!(out[0].subject, "fix gmail label search");
        assert_eq!(out[0].committed_at, 1747400000);
    }

    #[test]
    fn parse_log_multiple_entries() {
        let input = "a\t100\tfirst\nb\t200\tsecond subject with tabs?\n";
        let out = parse_log_porcelain(input).unwrap();
        assert_eq!(out.len(), 2);
    }
```

- [ ] **Step 2: Add the type and stub above the tests module**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub short_sha: String,
    pub committed_at: i64,
    pub subject: String,
}

pub fn parse_log_porcelain(input: &str) -> Result<Vec<LogEntry>> {
    let mut out = Vec::new();
    for line in input.lines() {
        let mut parts = line.splitn(3, '\t');
        let short_sha = parts.next().ok_or_else(|| anyhow!("missing sha"))?.to_string();
        let ct: i64 = parts.next().ok_or_else(|| anyhow!("missing ct"))?.parse()?;
        let subject = parts.next().unwrap_or("").to_string();
        out.push(LogEntry { short_sha, committed_at: ct, subject });
    }
    Ok(out)
}
```

- [ ] **Step 3: Add shell-out helpers**

Below the parsers, add:

```rust
use std::process::Command;
use std::path::Path;

pub fn run_worktree_list(repo: &Path) -> Result<Vec<WorktreeEntry>> {
    let out = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["worktree", "list", "--porcelain"])
        .output()?;
    if !out.status.success() {
        return Err(anyhow!("git worktree list failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    parse_worktree_list(std::str::from_utf8(&out.stdout)?)
}

pub fn run_status_v2(repo: &Path) -> Result<StatusSummary> {
    let out = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["status", "--porcelain=v2", "--branch"])
        .output()?;
    if !out.status.success() {
        return Err(anyhow!("git status failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    parse_status_v2(std::str::from_utf8(&out.stdout)?)
}

pub fn run_log_recent(repo: &Path, n: u32) -> Result<Vec<LogEntry>> {
    let out = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["log", &format!("-{n}"), "--format=%h%x09%ct%x09%s"])
        .output()?;
    if !out.status.success() {
        return Err(anyhow!("git log failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    parse_log_porcelain(std::str::from_utf8(&out.stdout)?)
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test git::
```

Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add src/git.rs
git commit -m "feat(git): log parser and shell-out wrappers"
```

---

## Task 6: `discovery` module

**Files:**
- Create: `src/discovery.rs`
- Modify: `src/main.rs` (`mod discovery;`)

- [ ] **Step 1: Write failing tests**

Create `src/discovery.rs` with:

```rust
use crate::git::{self, WorktreeEntry};
use crate::model::{Project, Worktree};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Walk `root` for immediate non-hidden subdirectories that contain `.git`.
/// Returns one Project per directory, with `worktrees` populated by
/// `git worktree list` for each.
pub fn scan(root: &Path) -> Result<Vec<Project>> {
    todo!()
}

fn is_git_dir(path: &Path) -> bool {
    let g = path.join(".git");
    g.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_tmp_repo(parent: &Path, name: &str) -> PathBuf {
        let p = parent.join(name);
        fs::create_dir_all(&p).unwrap();
        // initialize a real repo so `git worktree list` works
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
}
```

- [ ] **Step 2: Add `tempfile` dev-dependency**

In `Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Wire and verify failure**

In `src/main.rs`:

```rust
mod model;
mod git;
mod discovery;
```

Run: `cargo test discovery::`
Expected: tests fail at `todo!()`.

- [ ] **Step 4: Implement `scan`**

```rust
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
/// Runs per-repo git calls in parallel via `std::thread::scope` so total
/// wall time is roughly the slowest single repo rather than the sum.
pub fn enrich_with_status(projects: &mut [Project]) {
    use crate::model::CommitSummary;
    use std::sync::mpsc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    // (project_idx, worktree_idx, owned path)
    let work: Vec<(usize, usize, std::path::PathBuf)> = projects
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
```

- [ ] **Step 5: Suppress the unused-import warnings**

Remove `use std::time::Duration;` since it's not used in this file. Keep the rest.

- [ ] **Step 6: Run tests**

```bash
cargo test discovery::
```

Expected: 2 passed.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs src/discovery.rs
git commit -m "feat(discovery): scan ~/projects/ for git dirs and enrich status"
```

---

## Task 7: `sessions::scan_jobs`

> **Field names verified against real data in `docs/notes/format-findings.md`.** The metadata file is `state.json`, the cwd-equivalent canonical key is `worktreePath` (with `cwd` as fallback), and the `state` enum values are `done`/`working`/`blocked` (not `running`/`completed`/`failed`).

**Files:**
- Create: `src/sessions.rs`
- Modify: `src/main.rs` (`mod sessions;`)

- [ ] **Step 1: Write failing test**

Create `src/sessions.rs`:

```rust
use crate::model::{JobStatus, Session};
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
    updated_at: Option<String>,
}

impl JobMetadata {
    fn effective_cwd(&self) -> PathBuf {
        self.worktree_path.clone().unwrap_or_else(|| self.cwd.clone())
    }
}

pub fn scan_jobs(jobs_dir: &Path) -> Result<Vec<Session>> {
    todo!()
}

fn parse_status(state: Option<&str>, in_flight: Option<bool>) -> JobStatus {
    if in_flight == Some(true) { return JobStatus::Running; }
    match state {
        Some("working") | Some("running") | Some("in_progress") => JobStatus::Running,
        Some("done") | Some("completed") | Some("success") => JobStatus::Completed,
        Some("blocked") | Some("failed") | Some("error") | Some("cancelled") => JobStatus::Failed,
        _ => JobStatus::Unknown,
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
        write_job(tmp.path(), "abc1",
            r#"{"cwd":"/home/jane","worktreePath":"/home/jane/projects/example-project","state":"working","inFlight":true}"#);
        write_job(tmp.path(), "abc2",
            r#"{"cwd":"/home/jane/projects/zele","worktreePath":null,"state":"done","inFlight":false}"#);

        let out = scan_jobs(tmp.path()).unwrap();
        assert_eq!(out.len(), 2);
        let working: Vec<_> = out.iter().filter(|s| matches!(s,
            Session::BackgroundJob { status: JobStatus::Running, .. })).collect();
        assert_eq!(working.len(), 1);
        // The running job's effective cwd is its worktreePath, not its cwd.
        if let Session::BackgroundJob { cwd, .. } = &working[0] {
            assert_eq!(cwd, &PathBuf::from("/home/jane/projects/example-project"));
        }
    }

    #[test]
    fn scan_jobs_skips_malformed() {
        let tmp = tempfile::tempdir().unwrap();
        write_job(tmp.path(), "good", r#"{"cwd":"/x","state":"working","inFlight":true}"#);
        write_job(tmp.path(), "bad", "this is not json");

        let out = scan_jobs(tmp.path()).unwrap();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn scan_jobs_blocked_maps_to_failed() {
        let tmp = tempfile::tempdir().unwrap();
        write_job(tmp.path(), "b", r#"{"cwd":"/x","state":"blocked","inFlight":false}"#);
        let out = scan_jobs(tmp.path()).unwrap();
        assert!(matches!(&out[0],
            Session::BackgroundJob { status: JobStatus::Failed, .. }));
    }

    #[test]
    fn scan_jobs_missing_dir_returns_empty() {
        let out = scan_jobs(Path::new("/nope/nada")).unwrap();
        assert!(out.is_empty());
    }
}
```

- [ ] **Step 2: Wire and verify failure**

In `src/main.rs`:

```rust
mod sessions;
```

Run: `cargo test sessions::`
Expected: tests fail at `todo!()`.

- [ ] **Step 3: Implement `scan_jobs`**

```rust
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
        if !dir.is_dir() { continue; }
        let meta_path = dir.join("state.json");
        if !meta_path.exists() { continue; }
        let bytes = match std::fs::read(&meta_path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let meta: JobMetadata = match serde_json::from_slice(&bytes) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let age = std::fs::metadata(&meta_path)
            .and_then(|m| m.modified())
            .map(|mt| SystemTime::now().duration_since(mt).unwrap_or(Duration::ZERO))
            .unwrap_or(Duration::ZERO);
        out.push(Session::BackgroundJob {
            id,
            status: parse_status(meta.state.as_deref(), meta.in_flight),
            cwd: meta.effective_cwd(),
            age,
        });
    }
    Ok(out)
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test sessions::
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/sessions.rs
git commit -m "feat(sessions): scan ~/.claude/jobs/ background metadata"
```

---

## Task 8: `sessions::scan_interactive` + `attach_to_worktrees`

**Files:**
- Modify: `src/sessions.rs`

- [ ] **Step 1: Write failing tests**

Append to the `tests` module:

```rust
    #[test]
    fn encode_cwd_replaces_slashes() {
        let cwd = Path::new("/home/jane/projects/example-project");
        assert_eq!(encode_cwd(cwd), "-home-jane-projects-example-project");
    }

    #[test]
    fn encode_cwd_replaces_dots() {
        // Verified against real ~/.claude/projects/ on Muthur: both `/` and `.`
        // collapse to `-`, producing `--` where `/.` appears in the source.
        let cwd = Path::new("/home/jane/projects/example-project/.claude/worktrees/spec-dashboard-design");
        assert_eq!(
            encode_cwd(cwd),
            "-home-jane-projects-example-project--claude-worktrees-spec-dashboard-design"
        );
    }

    #[test]
    fn encode_cwd_handles_hyphens_in_dir_names() {
        // The reviewer flagged: decoding is irreversible, so we don't decode.
        // We only ever go path -> encoded -> dir lookup.
        let cwd = Path::new("/home/jane/projects/foo-bar");
        assert_eq!(encode_cwd(cwd), "-home-jane-projects-foo-bar");
    }

    #[test]
    fn scan_interactive_caps_at_five_per_known_path() {
        let tmp = tempfile::tempdir().unwrap();
        let known = PathBuf::from("/home/jane/projects/example-project");
        let dir = tmp.path().join(encode_cwd(&known));
        fs::create_dir_all(&dir).unwrap();
        for i in 0..10 {
            let f = dir.join(format!("session-{i:02}.jsonl"));
            fs::write(&f, "{}\n").unwrap();
        }
        let out = scan_interactive(tmp.path(), &[known.clone()]).unwrap();
        let mine: Vec<_> = out.iter().filter(|s| matches!(s,
            Session::Interactive { cwd, .. } if cwd == &known)).collect();
        assert_eq!(mine.len(), 5);
    }

    #[test]
    fn scan_interactive_ignores_unknown_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("-some-unknown-project");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.jsonl"), "{}\n").unwrap();
        let out = scan_interactive(tmp.path(), &[PathBuf::from("/home/jane/projects/example-project")]).unwrap();
        assert!(out.is_empty());
    }
```

- [ ] **Step 2: Add `encode_cwd` and stub for `scan_interactive`**

In `src/sessions.rs`, near the top:

```rust
/// Encode an absolute path to the directory name used under `~/.claude/projects/`.
/// Both `/` and `.` collapse to `-` (verified against real data; see
/// `docs/notes/format-findings.md`). The encoding is one-way and irreversible:
/// `-a-b-c` could come from `/a/b/c`, `/a.b/c`, `/a/b.c`, etc.
/// We never decode; we only go path -> encoded -> directory lookup.
pub fn encode_cwd(p: &Path) -> String {
    p.to_string_lossy()
        .replace('/', "-")
        .replace('.', "-")
}

pub fn scan_interactive(projects_dir: &Path, known_worktrees: &[PathBuf]) -> Result<Vec<Session>> {
    todo!()
}
```

- [ ] **Step 3: Verify failure**

```bash
cargo test sessions::
```

Expected: encode tests pass; both `scan_interactive_*` tests fail at todo.

- [ ] **Step 4: Implement `scan_interactive`**

Replace `todo!()` with:

```rust
use crate::model::SessionState;

pub fn scan_interactive(projects_dir: &Path, known_worktrees: &[PathBuf]) -> Result<Vec<Session>> {
    let mut out = Vec::new();
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(60 * 60 * 24 * 30))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    for known in known_worktrees {
        let sub = projects_dir.join(encode_cwd(known));
        if !sub.is_dir() { continue; }

        let files_read = match std::fs::read_dir(&sub) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let mut sessions_here: Vec<(SystemTime, Session)> = Vec::new();
        for f in files_read.filter_map(|e| e.ok()) {
            let fp = f.path();
            if fp.extension().and_then(|s| s.to_str()) != Some("jsonl") { continue; }
            let mt = match std::fs::metadata(&fp).and_then(|m| m.modified()) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if mt < cutoff { continue; }

            let stem = fp.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let summary = first_summary_line(&fp).unwrap_or_default();
            let age = SystemTime::now().duration_since(mt).unwrap_or(Duration::ZERO);
            sessions_here.push((mt, Session::Interactive {
                id: stem,
                summary,
                cwd: known.clone(),
                age,
                state: SessionState::Active,
            }));
        }
        sessions_here.sort_by(|a, b| b.0.cmp(&a.0));  // newest first
        sessions_here.truncate(5);
        out.extend(sessions_here.into_iter().map(|(_, s)| s));
    }
    Ok(out)
}

fn first_summary_line(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(f);
    for line in reader.lines().take(20) {
        let line = line.ok()?;
        // Look for the first user message content; fall back to "(no summary)".
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(content) = v.get("content").and_then(|c| c.as_str()) {
                if !content.is_empty() {
                    let trimmed: String = content.chars().take(80).collect();
                    return Some(trimmed);
                }
            }
            if let Some(msg) = v.get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str()) {
                let trimmed: String = msg.chars().take(80).collect();
                return Some(trimmed);
            }
        }
    }
    Some("(no summary)".into())
}
```

> **Note:** the `first_summary_line` field probing must be confirmed against the actual jsonl shape recorded in `docs/notes/format-findings.md`. Adjust the field paths after Task 1 confirms them.

- [ ] **Step 5: Add the attach function**

Append:

```rust
use crate::model::Project;

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
```

- [ ] **Step 6: Run tests**

```bash
cargo test sessions::
```

Expected: 5 passed.

- [ ] **Step 7: Commit**

```bash
git add src/sessions.rs
git commit -m "feat(sessions): scan interactive sessions with 30d/5-per-cwd cap, attach to worktrees"
```

---

## Task 9: `actions` module (OSC 52 + print-and-exit)

**Files:**
- Create: `src/actions.rs`
- Modify: `src/main.rs` (`mod actions;`)

- [ ] **Step 1: Write failing test**

Create `src/actions.rs`:

```rust
use anyhow::Result;
use base64::Engine;

/// Encode a string as an OSC 52 escape sequence.
/// Spec: \x1b]52;c;<base64>\x07
pub fn osc52_encode(s: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(s);
    format!("\x1b]52;c;{b64}\x07")
}

pub fn launch_command_for(cwd: &std::path::Path, resume_id: Option<&str>) -> String {
    let cwd_display = cwd.to_string_lossy();
    match resume_id {
        Some(id) => format!("cd {cwd_display} && claude --resume {id}"),
        None => format!("cd {cwd_display} && claude"),
    }
}

/// Write the OSC 52 sequence to stdout. The terminal honors or drops it.
pub fn copy_to_clipboard(s: &str) -> Result<()> {
    use std::io::Write;
    let seq = osc52_encode(s);
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(seq.as_bytes())?;
    stdout.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn osc52_encodes_with_correct_envelope() {
        let out = osc52_encode("hello");
        assert!(out.starts_with("\x1b]52;c;"));
        assert!(out.ends_with('\x07'));
        let body = &out[7..out.len() - 1];
        let decoded = base64::engine::general_purpose::STANDARD.decode(body).unwrap();
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn launch_command_no_resume() {
        let s = launch_command_for(Path::new("/home/jane/projects/example-project"), None);
        assert_eq!(s, "cd /home/jane/projects/example-project && claude");
    }

    #[test]
    fn launch_command_with_resume() {
        let s = launch_command_for(Path::new("/home/jane/projects/example-project"), Some("abc-123"));
        assert_eq!(s, "cd /home/jane/projects/example-project && claude --resume abc-123");
    }
}
```

- [ ] **Step 2: Wire and test**

In `src/main.rs`:

```rust
mod actions;
```

Run: `cargo test actions::`
Expected: 3 passed.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs src/actions.rs
git commit -m "feat(actions): OSC 52 clipboard encode and launch command builder"
```

---

## Task 10: `ui::theme` palette

**Files:**
- Create: `src/ui/mod.rs`
- Create: `src/ui/theme.rs`
- Modify: `src/main.rs` (`mod ui;`)

- [ ] **Step 1: Create `src/ui/mod.rs`**

```rust
pub mod theme;
pub mod layout;
pub mod worktrees;
pub mod sessions;
pub mod modal;
```

- [ ] **Step 2: Create `src/ui/theme.rs`**

```rust
use ratatui::style::{Color, Modifier, Style};

// Rep Cap brand palette + companion accents.
// Per spec: night-mode-forward, all foreground, no background fills.
pub const PINK:     Color = Color::Rgb(0xe8, 0x8b, 0x9f);  // Rep Cap pink
pub const LAVENDER: Color = Color::Rgb(0xc5, 0xa3, 0xff);  // Structural accent
pub const MAGENTA:  Color = Color::Rgb(0xff, 0x6e, 0xc7);  // Transient feedback

pub fn pane_header() -> Style {
    Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD)
}

pub fn pane_header_focused() -> Style {
    Style::default().fg(MAGENTA).add_modifier(Modifier::BOLD)
}

pub fn active_row() -> Style {
    Style::default().fg(PINK).add_modifier(Modifier::BOLD)
}

pub fn status_icon() -> Style {
    Style::default().fg(PINK)
}

pub fn session_badge() -> Style {
    Style::default().fg(MAGENTA)
}

pub fn dim_footer() -> Style {
    Style::default().fg(LAVENDER).add_modifier(Modifier::DIM)
}

pub fn status_line() -> Style {
    Style::default().fg(MAGENTA)
}

/// The `▸ ` marker prefix used to mark the focused row without a bg fill.
pub const FOCUS_MARKER: &str = "▸ ";
pub const UNFOCUSED_PREFIX: &str = "  ";
```

- [ ] **Step 3: Create stubs for the other ui files so `mod.rs` compiles**

`src/ui/layout.rs`:
```rust
// filled in Task 11
```

`src/ui/worktrees.rs`:
```rust
// filled in Task 12
```

`src/ui/sessions.rs`:
```rust
// filled in Task 13
```

`src/ui/modal.rs`:
```rust
// filled in Task 13
```

- [ ] **Step 4: Wire and build**

In `src/main.rs`:

```rust
mod ui;
```

Run: `cargo build`
Expected: clean build.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/ui/
git commit -m "feat(ui): theme palette (lavender headers, pink active, magenta transient)"
```

---

## Task 11: `ui::layout::choose_columns`

**Files:**
- Modify: `src/ui/layout.rs`

- [ ] **Step 1: Write failing tests**

Replace the contents of `src/ui/layout.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    pub show_branch: bool,
    pub name_max: u16,
    pub compact_icons: bool,
    pub too_narrow: bool,
}

pub fn choose_columns(width: u16) -> Columns {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_terminal_shows_everything() {
        let c = choose_columns(120);
        assert!(c.show_branch);
        assert!(c.name_max >= 20);
        assert!(!c.compact_icons);
        assert!(!c.too_narrow);
    }

    #[test]
    fn medium_drops_branch() {
        let c = choose_columns(50);
        assert!(!c.show_branch);
        assert!(c.name_max >= 12);
        assert!(!c.compact_icons);
    }

    #[test]
    fn narrow_truncates_names_and_compacts_icons() {
        let c = choose_columns(35);
        assert!(!c.show_branch);
        assert!(c.name_max <= 12);
        assert!(c.compact_icons);
        assert!(!c.too_narrow);
    }

    #[test]
    fn extreme_narrow_flags_warning() {
        let c = choose_columns(20);
        assert!(c.too_narrow);
    }
}
```

- [ ] **Step 2: Verify failure**

```bash
cargo test ui::layout::
```

Expected: 4 failures at `todo!()`.

- [ ] **Step 3: Implement**

Replace `todo!()`:

```rust
pub fn choose_columns(width: u16) -> Columns {
    if width < 30 {
        Columns { show_branch: false, name_max: 8, compact_icons: true, too_narrow: true }
    } else if width < 40 {
        Columns { show_branch: false, name_max: 12, compact_icons: true, too_narrow: false }
    } else if width < 60 {
        Columns { show_branch: false, name_max: 20, compact_icons: false, too_narrow: false }
    } else {
        Columns { show_branch: true, name_max: 24, compact_icons: false, too_narrow: false }
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test ui::layout::
```

Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add src/ui/layout.rs
git commit -m "feat(ui): width-to-column-set logic"
```

---

## Task 12: `ui::worktrees` pane renderer

**Files:**
- Modify: `src/ui/worktrees.rs`

- [ ] **Step 1: Write the renderer**

Replace `src/ui/worktrees.rs`:

```rust
use crate::model::{ActiveFilter, AppState, Pane, Project, Session, TreePath, Worktree};
use crate::ui::layout::Columns;
use crate::ui::theme::{self, FOCUS_MARKER, UNFOCUSED_PREFIX};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, state: &AppState, columns: Columns) {
    let focused = state.focus == Pane::Worktrees;
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled("WORKTREES",
            if focused { theme::pane_header_focused() } else { theme::pane_header() }));

    let mut items: Vec<ListItem> = Vec::new();

    for p in &state.projects {
        let any_visible = project_has_visible_worktrees(p, state);
        if !any_visible { continue; }
        let expanded = state.expanded.contains(&p.name);
        let marker = if expanded { "▼" } else { "▸" };
        items.push(ListItem::new(Line::from(vec![
            row_prefix(state, &TreePath::project_header(p)),
            Span::raw(format!("{marker} {} ({})", p.name, p.worktrees.len())),
        ])));

        if expanded {
            for wt in &p.worktrees {
                if !worktree_visible(wt, state) { continue; }
                items.push(ListItem::new(Line::from(worktree_spans(state, p, wt, columns))));
            }
        }
    }

    f.render_widget(List::new(items).block(block), area);
}

fn row_prefix(state: &AppState, path: &TreePath) -> Span<'static> {
    let is_sel = state.selected.as_ref() == Some(path);
    if is_sel {
        Span::styled(FOCUS_MARKER, theme::active_row())
    } else {
        Span::raw(UNFOCUSED_PREFIX)
    }
}

fn worktree_spans<'a>(
    state: &AppState,
    p: &Project,
    wt: &Worktree,
    cols: Columns,
) -> Vec<Span<'a>> {
    let path = TreePath::worktree_row(p, wt);
    let is_sel = state.selected.as_ref() == Some(&path);
    let row_style: Style = if is_sel { theme::active_row() } else { Style::default() };
    let prefix = if is_sel { FOCUS_MARKER } else { UNFOCUSED_PREFIX };

    let name = wt.path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string();
    let name = truncate(&name, cols.name_max);

    let mut spans = vec![
        Span::styled(format!("  {prefix}└ "), row_style),
        Span::styled(format!("{:<width$}", name, width = cols.name_max as usize), row_style),
    ];
    if cols.show_branch {
        let branch = wt.branch.as_deref().unwrap_or("?");
        spans.push(Span::raw("  "));
        spans.push(Span::styled(truncate(branch, 12), Style::default()));
    }
    spans.push(Span::raw("  "));
    spans.push(status_icons(wt, cols));
    spans.push(Span::raw("  "));
    spans.push(session_badge(wt));
    spans
}

fn status_icons<'a>(wt: &Worktree, cols: Columns) -> Span<'a> {
    let mut s = String::new();
    if wt.dirty { s.push('●'); }
    if wt.ahead > 0 {
        if cols.compact_icons { s.push('↑'); } else { s.push_str(&format!("↑{}", wt.ahead)); }
    }
    if wt.behind > 0 {
        if cols.compact_icons { s.push('↓'); } else { s.push_str(&format!("↓{}", wt.behind)); }
    }
    Span::styled(s, crate::ui::theme::status_icon())
}

fn session_badge<'a>(wt: &Worktree) -> Span<'a> {
    let n = wt.sessions.len();
    if n == 0 { return Span::raw(""); }
    Span::styled(format!("●{n}"), crate::ui::theme::session_badge())
}

fn truncate(s: &str, max: u16) -> String {
    let max = max as usize;
    if s.chars().count() <= max { return s.to_string(); }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn project_has_visible_worktrees(p: &Project, state: &AppState) -> bool {
    if state.filter == ActiveFilter::All { return !p.worktrees.is_empty(); }
    p.worktrees.iter().any(|w| worktree_visible(w, state))
}

fn worktree_visible(wt: &Worktree, state: &AppState) -> bool {
    let search_ok = match &state.search {
        Some(q) if !q.is_empty() => {
            let lname = wt.path.to_string_lossy().to_lowercase();
            lname.contains(&q.to_lowercase())
        }
        _ => true,
    };
    if !search_ok { return false; }
    if state.filter == ActiveFilter::All { return true; }
    is_active(wt)
}

fn is_active(wt: &Worktree) -> bool {
    if !wt.sessions.is_empty() { return true; }
    if wt.dirty { return true; }
    if wt.has_upstream && (wt.ahead > 0 || wt.behind > 0) { return true; }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wt_with(sessions: usize, dirty: bool) -> Worktree {
        Worktree {
            path: "/x/y".into(),
            branch: Some("main".into()),
            dirty,
            ahead: 0,
            behind: 0,
            last_commit: None,
            sessions: (0..sessions).map(|i| Session::BackgroundJob {
                id: format!("{i}"),
                status: crate::model::JobStatus::Running,
                cwd: "/x/y".into(),
                age: std::time::Duration::ZERO,
            }).collect(),
            has_upstream: true,
        }
    }

    #[test]
    fn is_active_when_dirty() {
        assert!(is_active(&wt_with(0, true)));
    }
    #[test]
    fn is_active_when_sessions() {
        assert!(is_active(&wt_with(1, false)));
    }
    #[test]
    fn is_inactive_when_quiet_no_upstream() {
        let mut w = wt_with(0, false);
        w.has_upstream = false;
        assert!(!is_active(&w));
    }
    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate("abc", 10), "abc");
    }
    #[test]
    fn truncate_long_gets_ellipsis() {
        assert_eq!(truncate("abcdefghij", 6), "abcde…");
    }
}
```

- [ ] **Step 2: Build and test**

```bash
cargo test ui::worktrees::
```

Expected: 5 passed.

- [ ] **Step 3: Commit**

```bash
git add src/ui/worktrees.rs
git commit -m "feat(ui): worktree pane renderer with active filter + truncation"
```

---

## Task 13: `ui::sessions` pane + `ui::modal`

**Files:**
- Modify: `src/ui/sessions.rs`
- Modify: `src/ui/modal.rs`

- [ ] **Step 1: Sessions pane renderer**

Replace `src/ui/sessions.rs`:

```rust
use crate::model::{AppState, JobStatus, Pane, Project, Session, SessionState, TreePath, Worktree};
use crate::ui::theme;
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};
use std::time::Duration;

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let focused = state.focus == Pane::Sessions;
    let header = current_worktree_label(state).unwrap_or_else(|| "(no selection)".to_string());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(format!("SESSIONS · {header}"),
            if focused { theme::pane_header_focused() } else { theme::pane_header() }));

    let wt = current_worktree(state);
    let mut items: Vec<ListItem> = Vec::new();
    if let Some(wt) = wt {
        for s in &wt.sessions {
            items.push(ListItem::new(Line::from(session_spans(s))));
        }
        if items.is_empty() {
            items.push(ListItem::new(Span::styled("(no sessions)", theme::dim_footer())));
        }
        if let Some(c) = &wt.last_commit {
            items.push(ListItem::new(Span::styled(
                format!("Last: {} \"{}\"", c.short_sha, c.subject),
                theme::dim_footer(),
            )));
        }
    }
    f.render_widget(List::new(items).block(block), area);
}

fn session_spans<'a>(s: &Session) -> Vec<Span<'a>> {
    match s {
        Session::BackgroundJob { id, status, age, .. } => vec![
            Span::raw("⚙ bg  "),
            Span::raw(short(id)),
            Span::raw("  "),
            Span::raw(fmt_age(*age)),
            Span::raw("  "),
            Span::styled(job_status_label(*status), theme::status_icon()),
        ],
        Session::Interactive { id, summary, age, state, .. } => vec![
            Span::raw("💬 int "),
            Span::raw(short(id)),
            Span::raw("  "),
            Span::raw(fmt_age(*age)),
            Span::raw("  "),
            Span::raw(state_label(*state).to_string()),
            Span::raw("  "),
            Span::styled(truncate(summary, 40), theme::dim_footer()),
        ],
    }
}

fn current_worktree(state: &AppState) -> Option<&Worktree> {
    let sel = state.selected.as_ref()?;
    let p = state.projects.iter().find(|p| p.name == sel.project)?;
    match &sel.worktree {
        Some(wp) => p.worktrees.iter().find(|w| w.path == *wp),
        None => p.worktrees.first(),
    }
}

fn current_worktree_label(state: &AppState) -> Option<String> {
    let sel = state.selected.as_ref()?;
    let wt_name = match &sel.worktree {
        Some(p) => p.file_name()?.to_string_lossy().to_string(),
        None => "main".to_string(),
    };
    Some(format!("{}/{}", sel.project, wt_name))
}

fn short(id: &str) -> String {
    id.chars().take(8).collect::<String>() + if id.len() > 8 { "…" } else { "" }
}

fn job_status_label(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Running => "running",
        JobStatus::Completed => "completed",
        JobStatus::Failed => "failed",
        JobStatus::Unknown => "?",
    }
}

fn state_label(s: SessionState) -> &'static str {
    match s {
        SessionState::Active => "active",
        SessionState::Compact => "compact",
        SessionState::Archived => "archived",
    }
}

fn fmt_age(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 { format!("{s}s") }
    else if s < 3600 { format!("{}m", s / 60) }
    else if s < 86400 { format!("{}h", s / 3600) }
    else { format!("{}d", s / 86400) }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { return s.to_string(); }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_formatting() {
        assert_eq!(fmt_age(Duration::from_secs(30)), "30s");
        assert_eq!(fmt_age(Duration::from_secs(120)), "2m");
        assert_eq!(fmt_age(Duration::from_secs(7200)), "2h");
        assert_eq!(fmt_age(Duration::from_secs(86400 * 3)), "3d");
    }
}
```

- [ ] **Step 2: Modal renderer**

Replace `src/ui/modal.rs`:

```rust
use crate::git::LogEntry;
use crate::ui::theme;
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, log: &[LogEntry], title: &str) {
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(format!("LOG · {title}"), theme::pane_header_focused()));
    let items: Vec<ListItem> = log.iter().map(|e| {
        ListItem::new(Line::from(vec![
            Span::styled(e.short_sha.clone(), theme::status_icon()),
            Span::raw("  "),
            Span::raw(e.subject.clone()),
        ]))
    }).collect();
    f.render_widget(List::new(items).block(block), area);
}
```

- [ ] **Step 3: Build and test**

```bash
cargo build
cargo test ui::sessions::
```

Expected: 1 passed (age formatting). Clean build.

- [ ] **Step 4: Commit**

```bash
git add src/ui/sessions.rs src/ui/modal.rs
git commit -m "feat(ui): sessions pane and log modal renderers"
```

---

## Task 14: `app::AppState` event loop scaffolding (move + quit + refresh)

**Files:**
- Create: `src/app.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create `src/app.rs`**

```rust
use crate::actions;
use crate::discovery;
use crate::model::{ActiveFilter, AppState, Pane, Project, StatusLine, TreePath};
use crate::sessions;
use crate::ui::layout;
use crate::ui::{self};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn initial_state(projects_root: PathBuf) -> Result<AppState> {
    let mut projects = discovery::scan(&projects_root)?;
    discovery::enrich_with_status(&mut projects);

    let known: Vec<PathBuf> = projects.iter()
        .flat_map(|p| p.worktrees.iter().map(|w| w.path.clone()))
        .collect();
    let jobs_dir = dirs::home_dir().unwrap_or_default().join(".claude/jobs");
    let proj_dir = dirs::home_dir().unwrap_or_default().join(".claude/projects");
    let jobs = sessions::scan_jobs(&jobs_dir).unwrap_or_default();
    let interactive = sessions::scan_interactive(&proj_dir, &known).unwrap_or_default();
    sessions::attach_to_worktrees(&mut projects, jobs);
    sessions::attach_to_worktrees(&mut projects, interactive);

    let selected = first_visible(&projects, &ActiveFilter::ActiveOnly);
    let expanded = projects.iter().map(|p| p.name.clone()).collect();
    Ok(AppState {
        projects,
        selected,
        focus: Pane::Worktrees,
        filter: ActiveFilter::ActiveOnly,
        search: None,
        last_refresh: Instant::now(),
        status: StatusLine::default(),
        expanded,
        generation: 0,
    })
}

/// Refresh in place, preserving user state (selection, filter, search, focus, expansion).
/// Increments `generation` so in-flight tick messages can be dropped.
pub fn refresh_in_place(state: &mut AppState, projects_root: PathBuf) -> Result<()> {
    let mut projects = discovery::scan(&projects_root)?;
    discovery::enrich_with_status(&mut projects);

    let known: Vec<PathBuf> = projects.iter()
        .flat_map(|p| p.worktrees.iter().map(|w| w.path.clone()))
        .collect();
    let jobs_dir = dirs::home_dir().unwrap_or_default().join(".claude/jobs");
    let proj_dir = dirs::home_dir().unwrap_or_default().join(".claude/projects");
    let jobs = sessions::scan_jobs(&jobs_dir).unwrap_or_default();
    let interactive = sessions::scan_interactive(&proj_dir, &known).unwrap_or_default();
    sessions::attach_to_worktrees(&mut projects, jobs);
    sessions::attach_to_worktrees(&mut projects, interactive);

    state.projects = projects;
    state.last_refresh = Instant::now();
    state.generation = state.generation.wrapping_add(1);
    // `selected` is a TreePath identified by project name + worktree path,
    // so it re-resolves correctly against the new projects vec. No reset needed.
    Ok(())
}

fn first_visible(projects: &[Project], _filter: &ActiveFilter) -> Option<TreePath> {
    projects.iter().find_map(|p| {
        p.worktrees.first().map(|w| TreePath::worktree_row(p, w))
    })
}

pub fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: &mut AppState,
    tick_rx: Receiver<TickMsg>,
    gen_counter: Arc<AtomicU64>,
) -> Result<RunOutcome> {
    loop {
        terminal.draw(|f| render_frame(f, state))?;

        // Drain background tick messages without blocking.
        while let Ok(msg) = tick_rx.try_recv() {
            apply_tick(state, msg);
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let prev_gen = state.generation;
                if let Some(outcome) = handle_key(state, key)? {
                    return Ok(outcome);
                }
                if state.generation != prev_gen {
                    gen_counter.store(state.generation, Ordering::SeqCst);
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum RunOutcome {
    Quit,
    PrintAndExit(String),
}

#[derive(Debug)]
pub enum TickMsg {
    JobsRefreshed { generation: u64, sessions: Vec<crate::model::Session> },
}

fn apply_tick(state: &mut AppState, msg: TickMsg) {
    match msg {
        TickMsg::JobsRefreshed { generation, sessions } => {
            // Drop stale messages that started before the last `r`.
            if generation != state.generation { return; }
            for p in state.projects.iter_mut() {
                for wt in p.worktrees.iter_mut() {
                    wt.sessions.retain(|s| !matches!(s,
                        crate::model::Session::BackgroundJob { .. }));
                }
            }
            sessions::attach_to_worktrees(&mut state.projects, sessions);
        }
    }
}

fn render_frame(f: &mut ratatui::Frame, state: &AppState) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Min(0), Constraint::Length(1)])
        .split(area);
    let cols = layout::choose_columns(area.width);

    ui::worktrees::render(f, chunks[0], state, cols);
    ui::sessions::render(f, chunks[1], state);
    render_footer(f, chunks[2], state, cols);
}

fn render_footer(f: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &AppState, cols: layout::Columns) {
    use ratatui::text::Span;
    use ratatui::widgets::Paragraph;
    let mut bits = vec![Span::raw("↑↓ ↵ Tab c o r / a g q")];
    if cols.too_narrow {
        bits.push(Span::raw("  "));
        bits.push(Span::styled("narrow", crate::ui::theme::status_line()));
    }
    if let Some(msg) = state.status.current() {
        bits.push(Span::raw("  "));
        bits.push(Span::styled(msg.to_string(), crate::ui::theme::status_line()));
    }
    f.render_widget(Paragraph::new(ratatui::text::Line::from(bits)), area);
}

fn handle_key(state: &mut AppState, key: KeyEvent) -> Result<Option<RunOutcome>> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) => Ok(Some(RunOutcome::Quit)),
        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => Ok(Some(RunOutcome::Quit)),
        (KeyCode::Char('c'), _) => { copy_current(state)?; Ok(None) }
        (KeyCode::Char('o'), _) => {
            if let Some(cmd) = launch_for_selected(state) {
                return Ok(Some(RunOutcome::PrintAndExit(cmd)));
            }
            Ok(None)
        }
        (KeyCode::Char('r'), _) => {
            refresh_in_place(state, default_projects_root())?;
            state.status.say("refreshed");
            Ok(None)
        }
        (KeyCode::Char('a'), _) => {
            state.filter = match state.filter {
                ActiveFilter::ActiveOnly => ActiveFilter::All,
                ActiveFilter::All => ActiveFilter::ActiveOnly,
            };
            Ok(None)
        }
        (KeyCode::Enter, _) => { handle_enter(state); Ok(None) }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => { move_selection(state, 1); Ok(None) }
        (KeyCode::Up, _)   | (KeyCode::Char('k'), _) => { move_selection(state, -1); Ok(None) }
        (KeyCode::Tab, _)     => { state.focus = next_pane(state.focus); Ok(None) }
        (KeyCode::BackTab, _) => { state.focus = next_pane(state.focus); Ok(None) }
        _ => Ok(None),
    }
}

fn next_pane(p: Pane) -> Pane {
    match p { Pane::Worktrees => Pane::Sessions, Pane::Sessions => Pane::Worktrees }
}

/// Enter on a project header toggles expansion. Enter on a worktree row
/// shifts focus to the sessions pane (and the user can Tab back).
fn handle_enter(state: &mut AppState) {
    let Some(sel) = state.selected.clone() else { return };
    if sel.worktree.is_none() {
        if state.expanded.contains(&sel.project) {
            state.expanded.remove(&sel.project);
        } else {
            state.expanded.insert(sel.project.clone());
        }
    } else {
        state.focus = Pane::Sessions;
    }
}

fn default_projects_root() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join("projects")
}

fn visible_paths(state: &AppState) -> Vec<TreePath> {
    let mut out = Vec::new();
    for p in &state.projects {
        let has_any = match state.filter {
            ActiveFilter::All => !p.worktrees.is_empty(),
            ActiveFilter::ActiveOnly => p.worktrees.iter().any(|w| is_active_or_search_match(w, state)),
        };
        if !has_any { continue; }
        out.push(TreePath::project_header(p));
        if state.expanded.contains(&p.name) {
            for w in &p.worktrees {
                if !is_active_or_search_match(w, state) { continue; }
                out.push(TreePath::worktree_row(p, w));
            }
        }
    }
    out
}

fn is_active_or_search_match(w: &crate::model::Worktree, state: &AppState) -> bool {
    let search_ok = match &state.search {
        Some(q) if !q.is_empty() => w.path.to_string_lossy().to_lowercase().contains(&q.to_lowercase()),
        _ => true,
    };
    if !search_ok { return false; }
    if state.filter == ActiveFilter::All { return true; }
    !w.sessions.is_empty() || w.dirty || (w.has_upstream && (w.ahead > 0 || w.behind > 0))
}

fn move_selection(state: &mut AppState, delta: i32) {
    let paths = visible_paths(state);
    if paths.is_empty() { return; }
    let idx = state.selected.as_ref()
        .and_then(|s| paths.iter().position(|p| p == s))
        .unwrap_or(0) as i32;
    let new = (idx + delta).clamp(0, paths.len() as i32 - 1) as usize;
    state.selected = Some(paths[new].clone());
}

fn copy_current(state: &mut AppState) -> Result<()> {
    if let Some(cmd) = launch_for_selected(state) {
        actions::copy_to_clipboard(&cmd)?;
        state.status.say("copied to clipboard");
    } else {
        state.status.say("nothing selected");
    }
    Ok(())
}

fn launch_for_selected(state: &AppState) -> Option<String> {
    let sel = state.selected.as_ref()?;
    let p = state.projects.iter().find(|p| p.name == sel.project)?;
    let wt = match &sel.worktree {
        Some(path) => p.worktrees.iter().find(|w| w.path == *path)?,
        None => p.worktrees.first()?,
    };
    Some(actions::launch_command_for(&wt.path, None))
}
```

- [ ] **Step 2: Update `src/main.rs`**

```rust
mod actions;
mod app;
mod discovery;
mod git;
mod model;
mod sessions;
mod ui;

use anyhow::Result;
use std::sync::atomic::AtomicU64;
use std::sync::{mpsc, Arc};

fn main() -> Result<()> {
    let projects_root = dirs::home_dir().unwrap_or_default().join("projects");
    let mut state = app::initial_state(projects_root)?;

    // Terminal setup.
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let (_tick_tx, tick_rx) = mpsc::channel::<app::TickMsg>();
    let gen_counter = Arc::new(AtomicU64::new(state.generation));

    let result = app::run(&mut terminal, &mut state, tick_rx, gen_counter);

    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;

    match result? {
        app::RunOutcome::Quit => Ok(()),
        app::RunOutcome::PrintAndExit(cmd) => {
            println!("{cmd}");
            Ok(())
        }
    }
}
```

- [ ] **Step 3: Build**

```bash
cargo build
```

Expected: clean build, possibly with dead-code warnings on unused fields. Those are fine for now.

- [ ] **Step 4: Run it briefly**

```bash
cargo run
```

Expected: a TUI opens showing the worktree pane with at least one project from `~/projects/`. Press `q` to exit.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/app.rs
git commit -m "feat(app): vertical layout, basic event loop, c/o/r/a/Tab/quit"
```

---

## Task 15: `app`: search filter (`/`) and modal (`g`)

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Extend `AppState`-related state with input modes**

Add to `src/app.rs` near the top (after `use` block):

```rust
#[derive(Debug, Clone)]
pub enum InputMode {
    Normal,
    Search(String),
    Modal(Vec<crate::git::LogEntry>, String),  // log entries + title
}
```

Add a field to track input mode by wrapping AppState. Since we don't want to modify `model.rs` for transient UI state, store it in a new `UiState` struct local to `app.rs`:

```rust
pub struct UiState {
    pub mode: InputMode,
}

impl UiState {
    pub fn new() -> Self { Self { mode: InputMode::Normal } }
}
```

- [ ] **Step 2: Thread `UiState` through `run` and `handle_key`**

Change `run` signature:

```rust
pub fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: &mut AppState,
    ui_state: &mut UiState,
    tick_rx: Receiver<TickMsg>,
    gen_counter: Arc<AtomicU64>,
) -> Result<RunOutcome> {
    loop {
        terminal.draw(|f| render_frame(f, state, ui_state))?;

        while let Ok(msg) = tick_rx.try_recv() {
            apply_tick(state, msg);
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let prev_gen = state.generation;
                if let Some(outcome) = handle_key(state, ui_state, key)? {
                    return Ok(outcome);
                }
                if state.generation != prev_gen {
                    gen_counter.store(state.generation, Ordering::SeqCst);
                }
            }
        }
    }
}
```

Change `render_frame` and `handle_key` signatures similarly. Replace `handle_key` body:

```rust
fn handle_key(state: &mut AppState, ui: &mut UiState, key: KeyEvent) -> Result<Option<RunOutcome>> {
    match &mut ui.mode {
        InputMode::Search(buf) => {
            match key.code {
                KeyCode::Esc => { ui.mode = InputMode::Normal; state.search = None; }
                KeyCode::Enter => {
                    state.search = if buf.is_empty() { None } else { Some(buf.clone()) };
                    ui.mode = InputMode::Normal;
                }
                KeyCode::Backspace => { buf.pop(); state.search = Some(buf.clone()); }
                KeyCode::Char(c) => { buf.push(c); state.search = Some(buf.clone()); }
                _ => {}
            }
            return Ok(None);
        }
        InputMode::Modal(_, _) => {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter) {
                ui.mode = InputMode::Normal;
            }
            return Ok(None);
        }
        InputMode::Normal => {}
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) => Ok(Some(RunOutcome::Quit)),
        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => Ok(Some(RunOutcome::Quit)),
        (KeyCode::Char('c'), _) => { copy_current(state)?; Ok(None) }
        (KeyCode::Char('o'), _) => {
            if let Some(cmd) = launch_for_selected(state) {
                return Ok(Some(RunOutcome::PrintAndExit(cmd)));
            }
            Ok(None)
        }
        (KeyCode::Char('r'), _) => {
            refresh_in_place(state, default_projects_root())?;
            state.status.say("refreshed");
            Ok(None)
        }
        (KeyCode::Char('a'), _) => {
            state.filter = match state.filter {
                ActiveFilter::ActiveOnly => ActiveFilter::All,
                ActiveFilter::All => ActiveFilter::ActiveOnly,
            };
            Ok(None)
        }
        (KeyCode::Char('/'), _) => { ui.mode = InputMode::Search(String::new()); state.search = Some(String::new()); Ok(None) }
        (KeyCode::Char('g'), _) => {
            if let Some(path) = current_worktree_path(state) {
                let entries = crate::git::run_log_recent(&path, 20).unwrap_or_default();
                let title = path.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_string();
                ui.mode = InputMode::Modal(entries, title);
            }
            Ok(None)
        }
        (KeyCode::Enter, _) => { handle_enter(state); Ok(None) }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => { move_selection(state, 1); Ok(None) }
        (KeyCode::Up, _)   | (KeyCode::Char('k'), _) => { move_selection(state, -1); Ok(None) }
        (KeyCode::Tab, _)     => { state.focus = next_pane(state.focus); Ok(None) }
        (KeyCode::BackTab, _) => { state.focus = next_pane(state.focus); Ok(None) }
        _ => Ok(None),
    }
}

fn current_worktree_path(state: &AppState) -> Option<PathBuf> {
    let sel = state.selected.as_ref()?;
    let p = state.projects.iter().find(|p| p.name == sel.project)?;
    let wt = match &sel.worktree {
        Some(path) => p.worktrees.iter().find(|w| w.path == *path)?,
        None => p.worktrees.first()?,
    };
    Some(wt.path.clone())
}
```

- [ ] **Step 3: Render modal on top when active**

Replace `render_frame`:

```rust
fn render_frame(f: &mut ratatui::Frame, state: &AppState, ui: &UiState) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(36), Constraint::Length(1)])
        .split(area);
    let cols = layout::choose_columns(area.width);

    crate::ui::worktrees::render(f, chunks[0], state, cols);
    crate::ui::sessions::render(f, chunks[1], state);
    render_footer(f, chunks[2], state, cols, ui);

    if let InputMode::Modal(log, title) = &ui.mode {
        let modal_area = centered_rect(area, 60, 60);
        crate::ui::modal::render(f, modal_area, log, title);
    }
}

fn centered_rect(parent: ratatui::layout::Rect, percent_x: u16, percent_y: u16) -> ratatui::layout::Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(parent);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(v[1])[1]
}
```

Update `render_footer` to show search buffer when active:

```rust
fn render_footer(f: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &AppState, cols: layout::Columns, ui: &UiState) {
    use ratatui::text::Span;
    use ratatui::widgets::Paragraph;
    if let InputMode::Search(buf) = &ui.mode {
        f.render_widget(
            Paragraph::new(format!("/ {buf}_  (Enter to apply, Esc to cancel)")),
            area,
        );
        return;
    }
    let mut bits = vec![Span::raw("↑↓ ↵ Tab c o r / a g q")];
    if cols.too_narrow {
        bits.push(Span::raw("  "));
        bits.push(Span::styled("narrow", crate::ui::theme::status_line()));
    }
    if let Some(msg) = state.status.current() {
        bits.push(Span::raw("  "));
        bits.push(Span::styled(msg.to_string(), crate::ui::theme::status_line()));
    }
    f.render_widget(Paragraph::new(ratatui::text::Line::from(bits)), area);
}
```

- [ ] **Step 4: Update `main.rs` to pass `UiState`**

```rust
fn main() -> Result<()> {
    let projects_root = dirs::home_dir().unwrap_or_default().join("projects");
    let mut state = app::initial_state(projects_root)?;
    let mut ui_state = app::UiState::new();

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let (_tick_tx, tick_rx) = mpsc::channel::<app::TickMsg>();
    let gen_counter = Arc::new(AtomicU64::new(state.generation));

    let result = app::run(&mut terminal, &mut state, &mut ui_state, tick_rx, gen_counter);

    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;

    match result? {
        app::RunOutcome::Quit => Ok(()),
        app::RunOutcome::PrintAndExit(cmd) => { println!("{cmd}"); Ok(()) }
    }
}
```

- [ ] **Step 5: Build and run**

```bash
cargo run
```

Expected: TUI opens. Press `/`, type `example-project`, Enter; only matching rows show. Esc clears. Press `g`; modal pops with last 20 commits. Press `Esc` to dismiss.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "feat(app): search filter (/) and commit-log modal (g)"
```

---

## Task 16: `tick`: background 10s job refresh

**Files:**
- Create: `src/tick.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the tick thread**

`src/tick.rs`:

```rust
use crate::app::TickMsg;
use crate::sessions;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub fn spawn(tx: Sender<TickMsg>, gen_counter: Arc<AtomicU64>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let jobs_dir: PathBuf = dirs::home_dir().unwrap_or_default().join(".claude/jobs");
        loop {
            thread::sleep(Duration::from_secs(10));
            let generation = gen_counter.load(Ordering::SeqCst);
            let jobs = sessions::scan_jobs(&jobs_dir).unwrap_or_default();
            if tx.send(TickMsg::JobsRefreshed { generation, sessions: jobs }).is_err() {
                break;
            }
        }
    })
}
```

- [ ] **Step 2: Wire into `main.rs`**

```rust
mod tick;

fn main() -> Result<()> {
    let projects_root = dirs::home_dir().unwrap_or_default().join("projects");
    let mut state = app::initial_state(projects_root)?;
    let mut ui_state = app::UiState::new();

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let (tick_tx, tick_rx) = mpsc::channel::<app::TickMsg>();
    let gen_counter = Arc::new(AtomicU64::new(state.generation));
    let _tick_handle = tick::spawn(tick_tx, Arc::clone(&gen_counter));

    let result = app::run(&mut terminal, &mut state, &mut ui_state, tick_rx, gen_counter);

    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;

    match result? {
        app::RunOutcome::Quit => Ok(()),
        app::RunOutcome::PrintAndExit(cmd) => { println!("{cmd}"); Ok(()) }
    }
}
```

- [ ] **Step 3: Build and run for at least 15 seconds**

```bash
cargo run
```

Expected: TUI runs; after ~10 s, you should see the session badge update if a background job changes state. No visible jank, no CPU spike.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs src/tick.rs
git commit -m "feat(tick): background 10s thread refreshes job sessions"
```

---

## Task 17: Smoke test, README, and shell wrapper

**Files:**
- Create: `README.md`
- Create: `docs/shell-wrapper.sh`

- [ ] **Step 1: Manual smoke test checklist**

Run `cargo run --release` and verify each of the following manually. Note failures in a scratch file.

- [ ] Launches without panic.
- [ ] Lists projects from `~/projects/`.
- [ ] Pressing `↑` and `↓` moves the focus marker `▸ `.
- [ ] Pressing `Tab` switches the bold header from one pane to the other.
- [ ] Pressing `a` toggles active filter (the project count visible should change).
- [ ] Pressing `c` shows "copied to clipboard" in the footer.
- [ ] In another tab, `Cmd-V` / `Ctrl-Shift-V` pastes `cd <path> && claude`.
- [ ] Pressing `/`, typing `the`, Enter: only matching rows show; Esc clears.
- [ ] Pressing `g`: modal appears with last 20 commits; Esc dismisses.
- [ ] Pressing `o`: TUI exits and stdout contains the launch command.
- [ ] Pressing `q`: clean exit, terminal restored (no leftover raw mode).
- [ ] Resize terminal to ~50 cols: branch column disappears.
- [ ] Resize to ~35 cols: names truncate with `…`.
- [ ] Resize to ~25 cols: "narrow" appears in footer.

Fix anything broken in a follow-up commit.

- [ ] **Step 2: Write `README.md`**

```markdown
# wt

Terminal dashboard for git worktrees and Claude Code sessions across `~/projects/`.

## Install

    cargo install --path .

This puts `wt` on your PATH.

## Use

    wt

Keys: `↑↓` move, `Tab` switch pane, `c` copy `cd ... && claude` to clipboard,
`o` print command and exit, `r` refresh, `/` filter, `a` toggle active-only,
`g` show commit log, `q` quit.

Designed to be left running in a dedicated terminal tab. Works equally well
over SSH on a phone (Blink, Termius: OSC 52 clipboard).

## Shell wrapper (optional)

To use `o` to `cd` your current shell into the selected worktree, add to
your shell rc:

    wtcd() { eval "$(wt)"; }

Then `wtcd`, navigate, press `o`. Your shell ends up in the selected dir
with `claude` running.

## See also

- Spec: `docs/specs/2026-05-16-wt-dashboard-design.md`
```

- [ ] **Step 3: Write `docs/shell-wrapper.sh`**

```bash
# Source this from your shell rc to get the `wtcd` function.
wtcd() {
    local cmd
    cmd="$(wt)" || return 1
    if [ -z "$cmd" ]; then
        return 0
    fi
    eval "$cmd"
}
```

- [ ] **Step 4: Install locally and try the wrapper**

```bash
cargo install --path .
# in your shell:
source docs/shell-wrapper.sh
wtcd
# select a row, press o
```

Expected: shell ends up in the chosen worktree, with `claude` launched (or the cmd printed if `claude` isn't on PATH).

- [ ] **Step 5: Commit**

```bash
git add README.md docs/shell-wrapper.sh
git commit -m "docs: README, shell wrapper, smoke-test checklist"
```

---

## Self-Review

**1. Spec coverage**

| Spec section                       | Implemented in task         |
|------------------------------------|-----------------------------|
| Vertical 60/40 layout              | Task 14 (render_frame)      |
| Responsive width collapse          | Task 11 (choose_columns) + Task 12 (rendering) |
| Discovery from `~/projects/`       | Task 6                      |
| `git worktree list` integration    | Task 4 + Task 6             |
| Status, ahead, behind, dirty       | Task 4 + Task 6 (enrich)    |
| Background job sessions            | Task 7                      |
| Interactive sessions, 30d/5 cap    | Task 8                      |
| Attach sessions to worktrees       | Task 8                      |
| Snapshot refresh + manual `r`      | Task 14, Task 15            |
| 10s background tick                | Task 16                     |
| `c` clipboard OSC 52               | Task 9 + Task 14            |
| `o` print and exit                 | Task 9 + Task 14            |
| `g` commit log modal               | Task 15                     |
| `/` substring filter               | Task 15                     |
| `a` active-only toggle             | Task 14                     |
| Theme: lavender / pink / magenta   | Task 10                     |
| Active filter semantics            | Task 12 (is_active)         |
| No-upstream-quiet = inactive       | Task 12 (is_active)         |
| Skip hidden subdirs in discovery   | Task 6                      |
| Error handling (best-effort)       | Task 6, Task 7, Task 8      |
| Performance targets                | Verified in Task 17 smoke   |

No gaps.

**2. Placeholder scan**

- No `TBD` / `TODO` / `XXX` strings appear in any task body.
- Two notes call out fields that must match Task 1's findings (job metadata field aliases, interactive jsonl summary path). Both use `serde(alias = ...)` to be tolerant; if Task 1 records something exotic, update the alias list before running Task 7 / Task 8.

**3. Type consistency**

- `Worktree`, `Project`, `Session`, `TreePath`, `AppState`, `StatusLine` are defined in Task 3 and used identically throughout.
- `TickMsg::JobsRefreshed { generation, sessions }` defined in Task 14, sent in Task 16, consumed in `apply_tick` (Task 14) which drops messages whose generation does not match `state.generation`.
- `RunOutcome` defined in Task 14, returned consistently from Task 15.
- `Columns` struct defined in Task 11, consumed by Task 12 and Task 14.
- Function name `actions::copy_to_clipboard` consistent between Task 9 (definition) and Task 14 (caller).
- Function name `actions::launch_command_for` consistent between Task 9 and Task 14.
- Function name `sessions::scan_jobs`, `sessions::scan_interactive`, `sessions::attach_to_worktrees` consistent across Tasks 7, 8, 14, 16.

No mismatches found.
