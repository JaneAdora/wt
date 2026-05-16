use ratatui::style::{Color, Modifier, Style};

// Night-mode palette: all foreground, no background fills.
pub const PINK: Color = Color::Rgb(0xe8, 0x8b, 0x9f); // Rep Cap pink
pub const LAVENDER: Color = Color::Rgb(0xc5, 0xa3, 0xff); // Structural accent
pub const MAGENTA: Color = Color::Rgb(0xff, 0x6e, 0xc7); // Transient feedback

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

/// Prefix marker for the focused row; no background fill.
pub const FOCUS_MARKER: &str = "▸ ";
pub const UNFOCUSED_PREFIX: &str = "  ";
