use crate::model::{AppState, JobStatus, Pane, Session, SessionState, Worktree};
use crate::ui::theme::{self, FOCUS_MARKER, UNFOCUSED_PREFIX};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};
use std::time::SystemTime;

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    use ratatui::style::Style;
    let focused = state.focus == Pane::Sessions;
    let header = current_worktree_label(state).unwrap_or_else(|| "(no selection)".to_string());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if focused {
            theme::pane_header_focused()
        } else {
            Style::default()
        })
        .title(Span::styled(
            format!("SESSIONS · {header}"),
            if focused {
                theme::pane_header_focused()
            } else {
                theme::pane_header()
            },
        ));

    // Block borders consume 2 cols (1 each side); reserve a 1-col safety margin
    // for terminals that quirk on the last column.
    let inner_width = area.width.saturating_sub(3);

    let wt = current_worktree(state);
    let mut items: Vec<ListItem> = Vec::new();
    if let Some(wt) = wt {
        let sel_idx = if state.focus == Pane::Sessions {
            state.selected_session
        } else {
            None
        };
        for (i, s) in wt.sessions.iter().enumerate() {
            let is_sel = sel_idx == Some(i);
            items.push(ListItem::new(Line::from(session_spans(s, is_sel, inner_width))));
        }
        if items.is_empty() {
            items.push(ListItem::new(Span::styled(
                "(no sessions)",
                theme::dim_footer(),
            )));
        }
        if !wt.recent_commits.is_empty() {
            // Blank separator + RECENT header + commits, each width-aware.
            items.push(ListItem::new(Span::raw("")));
            items.push(ListItem::new(Span::styled(
                "RECENT",
                theme::pane_header(),
            )));
            for c in &wt.recent_commits {
                let when = fmt_when_epoch(c.committed_at);
                // Overhead: 2 indent + sha + 2 + when(8) + 2
                let overhead = 2 + c.short_sha.chars().count() + 2 + 8 + 2;
                let subj_width = (inner_width as usize).saturating_sub(overhead);
                items.push(ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(c.short_sha.clone(), theme::status_icon()),
                    Span::raw("  "),
                    Span::raw(when),
                    Span::raw("  "),
                    Span::styled(
                        truncate_chars(&c.subject, subj_width),
                        theme::dim_footer(),
                    ),
                ])));
            }
        }
    }
    f.render_widget(List::new(items).block(block), area);
}

/// Build session row spans, width-aware. Reserves fixed columns for type,
/// id, when (date or relative age), state, then fills the remainder with
/// the description (interactive summary or bg-job intent).
fn session_spans<'a>(s: &Session, is_selected: bool, available_width: u16) -> Vec<Span<'a>> {
    let prefix = if is_selected {
        Span::styled(FOCUS_MARKER, theme::active_row())
    } else {
        Span::raw(UNFOCUSED_PREFIX)
    };

    // Column widths. Roughly:
    //   prefix(2) type(7) id(8) sp(2) when(8) sp(2) state(8) sp(2) desc(rest)
    // Type "💬 int " is 7 chars on the wire but emoji renders 2-wide visually;
    // we budget by ASCII width and trust most modern terminals.
    const FIXED_OVERHEAD: u16 = 2 + 7 + 8 + 2 + 8 + 2 + 9 + 2; // 40
    let desc_width: usize = (available_width as i32 - FIXED_OVERHEAD as i32).max(0) as usize;

    match s {
        Session::BackgroundJob {
            id,
            status,
            mtime,
            intent,
            size_bytes,
            ..
        } => {
            // Make room for the size column (~5 chars: "12K", "3.4M", "999K").
            // Subtract from desc width.
            let size_label = fmt_bytes(*size_bytes);
            let local_desc_width = desc_width.saturating_sub(size_label.len() + 2);
            let intent_display = intent
                .as_deref()
                .map(|s| truncate_chars(s, local_desc_width))
                .unwrap_or_default();
            vec![
                prefix,
                Span::raw("⚙ bg   "),
                Span::raw(short(id, 8)),
                Span::raw("  "),
                Span::raw(fmt_when(*mtime)),
                Span::raw("  "),
                Span::styled(job_status_label(*status), theme::status_icon()),
                Span::raw("  "),
                Span::styled(size_label, theme::dim_footer()),
                Span::raw("  "),
                Span::styled(intent_display, theme::dim_footer()),
            ]
        }
        Session::Interactive {
            id,
            summary,
            mtime,
            state,
            ..
        } => vec![
            prefix,
            Span::raw("💬 int "),
            Span::raw(short(id, 8)),
            Span::raw("  "),
            Span::raw(fmt_when(*mtime)),
            Span::raw("  "),
            Span::raw(state_label(*state).to_string()),
            Span::raw("  "),
            Span::styled(truncate_chars(summary, desc_width), theme::dim_footer()),
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

/// Format a timestamp as either relative ("5m", "2h", "23h") for things
/// under 24h, or an absolute month-day ("May 14", "Apr 02") for older.
/// Always pads to the same column width for nice alignment.
fn fmt_when(mtime: SystemTime) -> String {
    let epoch = mtime
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    fmt_when_epoch(epoch)
}

/// Same as fmt_when but takes Unix epoch seconds directly.
pub fn fmt_when_epoch(epoch_secs: i64) -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let age = (now - epoch_secs).max(0) as u64;
    let label = if age < 60 {
        format!("{age}s")
    } else if age < 3600 {
        format!("{}m", age / 60)
    } else if age < 86400 {
        format!("{}h", age / 3600)
    } else {
        ymd_month_day(epoch_secs)
    };
    format!("{label:<8}")
}

/// Convert seconds-since-epoch (UTC) to "MMM DD" (e.g., "May 14").
/// Self-contained civil-time arithmetic so we don't pull in a date crate.
fn ymd_month_day(epoch_secs: i64) -> String {
    // Days since 1970-01-01 (UTC).
    let days = epoch_secs.div_euclid(86_400);
    // Howard Hinnant's days-to-civil algorithm.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let _ = y; // year suppressed in the display label
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = MONTHS[(m - 1) as usize];
    format!("{month} {d:02}")
}

/// Format bytes as "12B", "3.4K", "999K", "1.2M", "47M", "2.1G".
fn fmt_bytes(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if n < KB {
        format!("{n}B")
    } else if n < MB {
        let kb = (n as f64) / KB as f64;
        if kb < 10.0 { format!("{kb:.1}K") } else { format!("{}K", kb as u64) }
    } else if n < GB {
        let mb = (n as f64) / MB as f64;
        if mb < 10.0 { format!("{mb:.1}M") } else { format!("{}M", mb as u64) }
    } else {
        let gb = (n as f64) / GB as f64;
        format!("{gb:.1}G")
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
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
    use std::time::Duration;

    #[test]
    fn when_relative_under_24h() {
        let now = SystemTime::now();
        let five_min_ago = now - Duration::from_secs(300);
        assert_eq!(fmt_when(five_min_ago).trim(), "5m");
        let two_h_ago = now - Duration::from_secs(7200);
        assert_eq!(fmt_when(two_h_ago).trim(), "2h");
    }

    #[test]
    fn when_absolute_for_old_dates() {
        // 2025-04-02 00:00:00 UTC = 1743552000
        let date = SystemTime::UNIX_EPOCH + Duration::from_secs(1_743_552_000);
        assert_eq!(fmt_when(date).trim(), "Apr 02");
    }

    #[test]
    fn ymd_known_dates() {
        // 2026-01-01 UTC = 1767225600
        assert_eq!(ymd_month_day(1_767_225_600), "Jan 01");
        // 2026-05-16 UTC = 1778889600
        assert_eq!(ymd_month_day(1_778_889_600), "May 16");
        // 2025-04-02 UTC = 1743552000
        assert_eq!(ymd_month_day(1_743_552_000), "Apr 02");
    }

    #[test]
    fn fmt_bytes_examples() {
        assert_eq!(fmt_bytes(0), "0B");
        assert_eq!(fmt_bytes(512), "512B");
        assert_eq!(fmt_bytes(1024), "1.0K");
        assert_eq!(fmt_bytes(15_360), "15K");
        assert_eq!(fmt_bytes(1_500_000), "1.4M");
        assert_eq!(fmt_bytes(50_000_000), "47M");
    }

    #[test]
    fn truncate_respects_zero_width() {
        assert_eq!(truncate_chars("abc", 0), "");
        assert_eq!(truncate_chars("abc", 1), "…");
        assert_eq!(truncate_chars("abcdef", 4), "abc…");
    }
}
