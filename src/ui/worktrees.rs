use crate::model::{ActiveFilter, AppState, Pane, Project, TreePath, Worktree};
use crate::ui::layout::Columns;
use crate::ui::theme::{self, FOCUS_MARKER, UNFOCUSED_PREFIX};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, state: &AppState, columns: Columns) {
    let focused = state.focus == Pane::Worktrees;

    // Count visible projects/worktrees for the title.
    let visible_projects: Vec<&crate::model::Project> = state
        .projects
        .iter()
        .filter(|p| project_has_visible_worktrees(p, state))
        .collect();
    let total_wts: usize = visible_projects
        .iter()
        .map(|p| p.worktrees.iter().filter(|w| worktree_visible(w, state)).count())
        .sum();
    let total_active_sessions: usize = visible_projects
        .iter()
        .flat_map(|p| p.worktrees.iter().filter(|w| worktree_visible(w, state)))
        .map(|w| w.sessions.len())
        .sum();

    let title = format!(
        "WORKTREES ({} wt · {} sess)",
        total_wts, total_active_sessions
    );
    let title_style = if focused {
        theme::pane_header_focused()
    } else {
        theme::pane_header()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if focused {
            theme::pane_header_focused()
        } else {
            Style::default()
        })
        .title(Span::styled(title, title_style))
        // Right-aligned wizard brand. Width-permitting; ratatui clips if
        // it overlaps the left title.
        .title_top(Line::from(Span::styled("🧙 ww", theme::dim_footer())).right_aligned());

    // Build items AND track the index of the currently-selected row so
    // ratatui can auto-scroll to keep it visible.
    let mut items: Vec<ListItem> = Vec::new();
    let mut selected_idx: Option<usize> = None;
    let mut idx = 0usize;

    for p in &visible_projects {
        let expanded = state.expanded.contains(&p.name);
        let marker = if expanded { "▼" } else { "▸" };
        let active = p
            .worktrees
            .iter()
            .filter(|w| worktree_visible(w, state))
            .map(|w| w.sessions.len())
            .sum::<usize>();
        let dirty_count = p
            .worktrees
            .iter()
            .filter(|w| worktree_visible(w, state) && w.dirty)
            .count();

        // Compose count_label with up to three pieces: worktree count,
        // session count, dirty count. Keep it short.
        let mut parts = vec![p.worktrees.len().to_string()];
        if active > 0 {
            parts.push(format!("{active} sess"));
        }
        let count_label = format!("({})", parts.join(" · "));

        let path = TreePath::project_header(p);
        if state.selected.as_ref() == Some(&path) {
            selected_idx = Some(idx);
        }
        let mut header_spans = vec![
            row_prefix(state, &path),
            Span::raw(format!("{marker} {} {}", p.name, count_label)),
        ];
        if dirty_count > 0 {
            header_spans.push(Span::raw("  "));
            header_spans.push(Span::styled(
                format!("●{dirty_count}"),
                theme::status_icon(),
            ));
        }
        items.push(ListItem::new(Line::from(header_spans)));
        idx += 1;

        if expanded {
            for wt in &p.worktrees {
                if !worktree_visible(wt, state) {
                    continue;
                }
                let wt_path = TreePath::worktree_row(p, wt);
                if state.selected.as_ref() == Some(&wt_path) {
                    selected_idx = Some(idx);
                }
                items.push(ListItem::new(Line::from(worktree_spans(
                    state, p, wt, columns,
                ))));
                idx += 1;
            }
        }
    }

    // Use a stateful list so ratatui scrolls to keep the selected row in
    // view. We don't apply a highlight style — the ▸ prefix marker is the
    // visual indicator. select() is purely to drive auto-scroll.
    let mut list_state = ListState::default();
    list_state.select(selected_idx);
    f.render_stateful_widget(List::new(items).block(block), area, &mut list_state);
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
    let row_style: Style = if is_sel {
        theme::active_row()
    } else {
        Style::default()
    };
    let prefix = if is_sel { FOCUS_MARKER } else { UNFOCUSED_PREFIX };

    let name = wt
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string();
    let name = truncate(&name, cols.name_max);

    let mut spans = vec![
        Span::styled(format!("  {prefix}└ "), row_style),
        Span::styled(
            format!("{:<width$}", name, width = cols.name_max as usize),
            row_style,
        ),
    ];
    if cols.show_branch {
        let branch = wt.branch.as_deref().unwrap_or("?");
        spans.push(Span::raw("  "));
        spans.push(Span::raw(truncate(branch, 12)));
    }
    spans.push(Span::raw("  "));
    spans.push(status_icons(wt, cols));
    spans.push(Span::raw("  "));
    spans.push(session_badge(wt));
    spans
}

fn status_icons<'a>(wt: &Worktree, cols: Columns) -> Span<'a> {
    let mut s = String::new();
    if wt.dirty {
        s.push('●');
    }
    if wt.ahead > 0 {
        if cols.compact_icons {
            s.push('↑');
        } else {
            s.push_str(&format!("↑{}", wt.ahead));
        }
    }
    if wt.behind > 0 {
        if cols.compact_icons {
            s.push('↓');
        } else {
            s.push_str(&format!("↓{}", wt.behind));
        }
    }
    Span::styled(s, theme::status_icon())
}

fn session_badge<'a>(wt: &Worktree) -> Span<'a> {
    let n = wt.sessions.len();
    if n == 0 {
        return Span::raw("");
    }
    Span::styled(format!("●{n}"), theme::session_badge())
}

fn truncate(s: &str, max: u16) -> String {
    let max = max as usize;
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn project_has_visible_worktrees(p: &Project, state: &AppState) -> bool {
    if state.filter == ActiveFilter::All {
        return !p.worktrees.is_empty();
    }
    p.worktrees.iter().any(|w| worktree_visible(w, state))
}

fn worktree_visible(wt: &Worktree, state: &AppState) -> bool {
    let search_ok = match &state.search {
        Some(q) if !q.is_empty() => {
            wt.path
                .to_string_lossy()
                .to_lowercase()
                .contains(&q.to_lowercase())
        }
        _ => true,
    };
    if !search_ok {
        return false;
    }
    if state.filter == ActiveFilter::All {
        return true;
    }
    is_active(wt)
}

pub fn is_active(wt: &Worktree) -> bool {
    if !wt.sessions.is_empty() {
        return true;
    }
    if wt.dirty {
        return true;
    }
    if wt.has_upstream && (wt.ahead > 0 || wt.behind > 0) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{JobStatus, Session};

    fn wt_with(sessions: usize, dirty: bool) -> Worktree {
        Worktree {
            path: "/x/y".into(),
            branch: Some("main".into()),
            dirty,
            ahead: 0,
            behind: 0,
            recent_commits: vec![],
            sessions: (0..sessions)
                .map(|i| Session::BackgroundJob {
                    id: format!("{i}"),
                    status: JobStatus::Running,
                    cwd: "/x/y".into(),
                    mtime: std::time::SystemTime::now(),
                    intent: None,
                })
                .collect(),
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
