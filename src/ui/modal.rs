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
    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        format!("LOG · {title}"),
        theme::pane_header_focused(),
    ));
    let items: Vec<ListItem> = log
        .iter()
        .map(|e| {
            ListItem::new(Line::from(vec![
                Span::styled(e.short_sha.clone(), theme::status_icon()),
                Span::raw("  "),
                Span::raw(e.subject.clone()),
            ]))
        })
        .collect();
    f.render_widget(List::new(items).block(block), area);
}
