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
                if let Some(e) = cur.as_mut() {
                    e.bare = true;
                }
            }
            "detached" => {
                if let Some(e) = cur.as_mut() {
                    e.detached = true;
                }
            }
            _ => {}
        }
    }
    if let Some(e) = cur.take() {
        out.push(e);
    }
    Ok(out)
}

pub fn parse_status_v2(porcelain: &str) -> Result<StatusSummary> {
    let mut s = StatusSummary::default();
    for line in porcelain.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            s.branch = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            s.upstream = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            let mut parts = rest.split_whitespace();
            let ahead = parts
                .next()
                .unwrap_or("+0")
                .trim_start_matches('+')
                .parse()
                .unwrap_or(0);
            let behind = parts
                .next()
                .unwrap_or("-0")
                .trim_start_matches('-')
                .parse()
                .unwrap_or(0);
            s.ahead = ahead;
            s.behind = behind;
        } else if !line.starts_with('#') && !line.is_empty() {
            s.dirty = true;
        }
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_worktree_list_one_entry() {
        let input = "worktree /home/jane/projects/thelma\nHEAD abc123def456789\nbranch refs/heads/main\n\n";
        let out = parse_worktree_list(input).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, PathBuf::from("/home/jane/projects/thelma"));
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
