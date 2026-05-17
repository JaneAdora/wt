//! Sessions view: alternative top-pane layout showing ALL Claude sessions
//! across the machine, grouped by their cwd. Each group header marks
//! whether wt found a matching git worktree for that cwd.

use crate::model::{AppState, JobStatus, Session, SessionGroup};
use crate::ui::theme::{self, FOCUS_MARKER, UNFOCUSED_PREFIX};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let total_sessions: usize = state.session_groups.iter().map(|g| g.sessions.len()).sum();
    let with_worktree = state.session_groups.iter().filter(|g| g.has_worktree).count();
    let title = format!(
        "SESSIONS · all · {} groups · {} sess · {} with worktree",
        state.session_groups.len(),
        total_sessions,
        with_worktree,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::pane_header_focused())
        .title(Span::styled(title, theme::pane_header_focused()))
        .title_top(
            Line::from(Span::styled("WT Wizard 🧙", theme::dim_footer())).right_aligned(),
        );

    let mut items: Vec<ListItem> = Vec::new();
    let mut selected_idx: Option<usize> = None;
    let mut row = 0usize;

    for (gi, group) in state.session_groups.iter().enumerate() {
        items.push(ListItem::new(Line::from(group_header_spans(group))));
        row += 1;
        for (si, sess) in group.sessions.iter().enumerate() {
            let is_sel = state.selected_in_view == Some((gi, si));
            if is_sel {
                selected_idx = Some(row);
            }
            items.push(ListItem::new(Line::from(session_spans(sess, is_sel, area.width))));
            row += 1;
        }
    }

    if items.is_empty() {
        items.push(ListItem::new(Span::styled(
            "(no Claude sessions found in ~/.claude)",
            theme::dim_footer(),
        )));
    }

    let mut list_state = ListState::default();
    list_state.select(selected_idx);
    f.render_stateful_widget(List::new(items).block(block), area, &mut list_state);
}

fn group_header_spans<'a>(group: &SessionGroup) -> Vec<Span<'a>> {
    let cwd_display = group.cwd.to_string_lossy().to_string();
    let marker = if group.has_worktree { "●" } else { "○" };
    let marker_style = if group.has_worktree {
        theme::status_icon()
    } else {
        theme::dim_footer()
    };
    vec![
        Span::raw("  "),
        Span::styled(marker, marker_style),
        Span::raw(" "),
        Span::styled(cwd_display, theme::pane_header()),
        Span::raw("  "),
        Span::styled(
            format!("({} sess)", group.sessions.len()),
            theme::dim_footer(),
        ),
    ]
}

fn session_spans<'a>(s: &Session, is_selected: bool, available_width: u16) -> Vec<Span<'a>> {
    let prefix = if is_selected {
        Span::styled(FOCUS_MARKER, theme::active_row())
    } else {
        Span::raw(UNFOCUSED_PREFIX)
    };
    // Indent under the group header.
    let indent = Span::raw("    ");
    // Approximate column overhead so the description has room.
    const FIXED: u16 = 2 + 4 + 7 + 8 + 2 + 8 + 2 + 9 + 2;
    let desc_width: usize = (available_width as i32 - FIXED as i32).max(0) as usize;

    match s {
        Session::BackgroundJob { id, status, mtime, intent, size_bytes, .. } => {
            let size_label = crate::ui::sessions::fmt_bytes_pub(*size_bytes);
            let local_desc = desc_width.saturating_sub(size_label.len() + 2);
            let intent_display = intent
                .as_deref()
                .map(|s| crate::ui::sessions::trunc_pub(s, local_desc))
                .unwrap_or_default();
            vec![
                prefix,
                indent,
                Span::raw("⚙ bg   "),
                Span::raw(short(id, 8)),
                Span::raw("  "),
                Span::raw(crate::ui::sessions::fmt_when_short(*mtime)),
                Span::raw("  "),
                Span::styled(job_status_label(*status), theme::status_icon()),
                Span::raw("  "),
                Span::styled(size_label, theme::dim_footer()),
                Span::raw("  "),
                Span::styled(intent_display, theme::dim_footer()),
            ]
        }
        Session::Interactive { id, summary, mtime, .. } => vec![
            prefix,
            indent,
            Span::raw("💬 int "),
            Span::raw(short(id, 8)),
            Span::raw("  "),
            Span::raw(crate::ui::sessions::fmt_when_short(*mtime)),
            Span::raw("  "),
            Span::styled(
                crate::ui::sessions::trunc_pub(summary, desc_width),
                theme::dim_footer(),
            ),
        ],
    }
}

fn short(id: &str, max: usize) -> String {
    let take: String = id.chars().take(max).collect();
    let count = id.chars().count();
    if count > max {
        format!("{take}…")
    } else {
        format!("{take:<max$}")
    }
}

fn job_status_label(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Running => "running",
        JobStatus::Completed => "done",
        JobStatus::Failed => "failed",
        JobStatus::Unknown => "?",
    }
}
