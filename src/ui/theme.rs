use ratatui::style::{Color, Modifier, Style};

use std::sync::OnceLock;

struct Palette {
    pink: Color,
    lavender: Color,
    magenta: Color,
}
static PALETTE: OnceLock<Palette> = OnceLock::new();

/// Core palette, overridable via ~/.config/dashboard-suite/theme.toml.
fn palette() -> &'static Palette {
    PALETTE.get_or_init(|| {
        let mut p = Palette {
            pink: Color::Rgb(0xe8, 0x8b, 0x9f),
            lavender: Color::Rgb(0xc5, 0xa3, 0xff),
            magenta: Color::Rgb(0xff, 0x6e, 0xc7),
        };
        if let Some(cfg) = suite_theme_path() {
            if let Ok(s) = std::fs::read_to_string(cfg) {
                for line in s.lines() {
                    let t = line.trim();
                    if t.starts_with('#') {
                        continue;
                    }
                    if let Some((k, v)) = t.split_once('=') {
                        if let Some(c) = parse_hex(v.trim().trim_matches('"')) {
                            match k.trim() {
                                "pink" => p.pink = c,
                                "lavender" => p.lavender = c,
                                "magenta" => p.magenta = c,
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        p
    })
}

fn suite_theme_path() -> Option<std::path::PathBuf> {
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        return Some(std::path::PathBuf::from(x).join("dashboard-suite/theme.toml"));
    }
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config/dashboard-suite/theme.toml"))
}

fn parse_hex(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 {
        return None;
    }
    Some(Color::Rgb(
        u8::from_str_radix(&s[0..2], 16).ok()?,
        u8::from_str_radix(&s[2..4], 16).ok()?,
        u8::from_str_radix(&s[4..6], 16).ok()?,
    ))
}

pub fn pink() -> Color {
    palette().pink
}
pub fn lavender() -> Color {
    palette().lavender
}
pub fn magenta() -> Color {
    palette().magenta
}

// Night-mode palette: all foreground, no background fills.

pub fn pane_header() -> Style {
    Style::default().fg(lavender()).add_modifier(Modifier::BOLD)
}

pub fn pane_header_focused() -> Style {
    Style::default().fg(magenta()).add_modifier(Modifier::BOLD)
}

pub fn active_row() -> Style {
    Style::default().fg(pink()).add_modifier(Modifier::BOLD)
}

pub fn status_icon() -> Style {
    Style::default().fg(pink())
}

pub fn session_badge() -> Style {
    Style::default().fg(magenta())
}

pub fn dim_footer() -> Style {
    Style::default().fg(lavender()).add_modifier(Modifier::DIM)
}

#[allow(dead_code)] // reserved for transient toasts; footer now uses default fg
pub fn status_line() -> Style {
    Style::default().fg(magenta())
}

/// Prefix marker for the focused row; no background fill.
pub const FOCUS_MARKER: &str = "▸ ";
pub const UNFOCUSED_PREFIX: &str = "  ";
