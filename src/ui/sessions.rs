use crate::model::{AppState, JobStatus, Pane, Session, SessionState, Worktree};
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
    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        format!("SESSIONS · {header}"),
        if focused {
            theme::pane_header_focused()
        } else {
            theme::pane_header()
        },
    ));

    let wt = current_worktree(state);
    let mut items: Vec<ListItem> = Vec::new();
    if let Some(wt) = wt {
        for s in &wt.sessions {
            items.push(ListItem::new(Line::from(session_spans(s))));
        }
        if items.is_empty() {
            items.push(ListItem::new(Span::styled(
                "(no sessions)",
                theme::dim_footer(),
            )));
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
        Session::BackgroundJob {
            id, status, age, ..
        } => vec![
            Span::raw("⚙ bg  "),
            Span::raw(short(id)),
            Span::raw("  "),
            Span::raw(fmt_age(*age)),
            Span::raw("  "),
            Span::styled(job_status_label(*status), theme::status_icon()),
        ],
        Session::Interactive {
            id,
            summary,
            age,
            state,
            ..
        } => vec![
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

pub fn current_worktree(state: &AppState) -> Option<&Worktree> {
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
    let take: String = id.chars().take(8).collect();
    let count = id.chars().count();
    if count > 8 {
        format!("{take}…")
    } else {
        take
    }
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
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86400)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
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
