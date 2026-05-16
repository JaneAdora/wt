use crate::git::LogEntry;
use crate::ui::theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

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
