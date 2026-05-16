#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    pub show_branch: bool,
    pub name_max: u16,
    pub compact_icons: bool,
    pub too_narrow: bool,
}

pub fn choose_columns(width: u16) -> Columns {
    if width < 30 {
        Columns {
            show_branch: false,
            name_max: 8,
            compact_icons: true,
            too_narrow: true,
        }
    } else if width < 40 {
        Columns {
            show_branch: false,
            name_max: 12,
            compact_icons: true,
            too_narrow: false,
        }
    } else if width < 60 {
        Columns {
            show_branch: false,
            name_max: 20,
            compact_icons: false,
            too_narrow: false,
        }
    } else {
        Columns {
            show_branch: true,
            name_max: 24,
            compact_icons: false,
            too_narrow: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_terminal_shows_everything() {
        let c = choose_columns(120);
        assert!(c.show_branch);
        assert!(c.name_max >= 20);
        assert!(!c.compact_icons);
        assert!(!c.too_narrow);
    }

    #[test]
    fn medium_drops_branch() {
        let c = choose_columns(50);
        assert!(!c.show_branch);
        assert!(c.name_max >= 12);
        assert!(!c.compact_icons);
    }

    #[test]
    fn narrow_truncates_names_and_compacts_icons() {
        let c = choose_columns(35);
        assert!(!c.show_branch);
        assert!(c.name_max <= 12);
        assert!(c.compact_icons);
        assert!(!c.too_narrow);
    }

    #[test]
    fn extreme_narrow_flags_warning() {
        let c = choose_columns(20);
        assert!(c.too_narrow);
    }
}
