use crate::git::LogEntry;
use crate::ui::theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

const HELP_BODY: &str = include_str!("help.txt");

/// Number of source lines in the embedded help text. Used by the
/// scroll-key handler to clamp scroll position to the actual content.
/// This is an approximation; wrap on narrow widths will add more visual
/// lines, so users may still scroll a bit past on a phone, but never
/// infinitely.
pub fn help_line_count() -> u16 {
    HELP_BODY.lines().count() as u16
}

/// Parse one help.txt line into a styled Line:
/// - Empty line: blank
/// - Header (no leading whitespace): bold lavender
/// - Indented key+description: bold pink key, plain description,
///   split at first 3+ space gap after the key text
/// - Anything else (e.g., continuation lines): plain
fn parse_help_line(raw: &str) -> Line<'static> {
    if raw.is_empty() {
        return Line::from("");
    }
    if !raw.starts_with(' ') {
        return Line::from(Span::styled(
            raw.to_string(),
            Style::default().add_modifier(Modifier::BOLD).fg(theme::LAVENDER),
        ));
    }
    let chars: Vec<char> = raw.chars().collect();
    let key_start = match chars.iter().position(|c| !c.is_whitespace()) {
        Some(i) => i,
        None => return Line::from(raw.to_string()),
    };

    // Find the first run of 3+ consecutive spaces *after* the key text
    // starts. That's the column gap between key and description.
    let mut key_end: Option<usize> = None;
    let mut run = 0usize;
    for i in key_start..chars.len() {
        if chars[i] == ' ' {
            run += 1;
            if run >= 3 {
                key_end = Some(i - run + 1);
                break;
            }
        } else {
            run = 0;
        }
    }

    if let Some(end) = key_end {
        let indent: String = chars[..key_start].iter().collect();
        let key: String = chars[key_start..end].iter().collect();
        let rest: String = chars[end..].iter().collect();
        Line::from(vec![
            Span::raw(indent),
            Span::styled(
                key,
                Style::default().add_modifier(Modifier::BOLD).fg(theme::PINK),
            ),
            Span::raw(rest),
        ])
    } else {
        Line::from(raw.to_string())
    }
}

/// Render the commit-log modal as a wrapped, scrollable paragraph.
///
/// Each entry becomes two lines: a header with the short SHA (highlighted
/// pink) followed by the subject, which wraps as needed at the modal's
/// inner width. `scroll` is the vertical line offset and is clamped by
/// ratatui to the rendered content.
pub fn render(f: &mut Frame, area: Rect, log: &[LogEntry], title: &str, scroll: u16) {
    f.render_widget(Clear, area);

    let footer_keys = "j/k scroll · g top · G bottom · Esc close";
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            format!(" LOG · {title} "),
            theme::pane_header_focused(),
        ))
        .title_bottom(Line::from(Span::styled(
            format!(" {footer_keys} "),
            theme::dim_footer(),
        )));

    // Build a Text of Lines so each commit becomes a labeled, wrappable
    // entry. Subjects wrap; the SHA stays on the leading line.
    let mut lines: Vec<Line> = Vec::with_capacity(log.len() * 2);
    for e in log {
        lines.push(Line::from(vec![
            Span::styled(e.short_sha.clone(), theme::status_icon()),
            Span::raw("  "),
            Span::styled(e.subject.clone(), Style::default().add_modifier(Modifier::BOLD)),
        ]));
        // Blank line between entries for breathing room.
        lines.push(Line::from(""));
    }

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(p, area);
}

/// Render the help overlay. Reuses the modal envelope (Clear, bordered
/// block, footer keymap) and word-wraps the help text body.
pub fn render_help(f: &mut Frame, area: Rect, scroll: u16) {
    f.render_widget(Clear, area);
    let footer_keys = "j/k scroll · g top · G bottom · Esc close";
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " HELP · Worktree Wizard ",
            theme::pane_header_focused(),
        ))
        .title_bottom(Line::from(Span::styled(
            format!(" {footer_keys} "),
            theme::dim_footer(),
        )));

    let lines: Vec<Line> = HELP_BODY.lines().map(parse_help_line).collect();

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(p, area);
}
