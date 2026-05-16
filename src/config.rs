//! Persistent UI state across launches.
//!
//! Stored at `~/.config/wt/state.json` (or `$XDG_CONFIG_HOME/wt/state.json`).
//! Loaded once at startup, saved on clean quit. Missing or malformed file
//! falls back to defaults silently. Only persists UI preferences, never
//! discovered project data.

use crate::model::{ActiveFilter, SessionWindow};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedState {
    #[serde(default = "default_filter", with = "filter_serde")]
    pub filter: ActiveFilter,
    #[serde(default)]
    pub expanded: Vec<String>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub selected_project: Option<String>,
    #[serde(default)]
    pub selected_worktree: Option<String>,
    #[serde(default = "default_window", with = "window_serde")]
    pub session_window: SessionWindow,
}

impl Default for SavedState {
    fn default() -> Self {
        Self {
            filter: default_filter(),
            expanded: Vec::new(),
            search: None,
            selected_project: None,
            selected_worktree: None,
            session_window: default_window(),
        }
    }
}

fn default_filter() -> ActiveFilter {
    ActiveFilter::ActiveOnly
}

fn default_window() -> SessionWindow {
    SessionWindow::Days30
}

pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("wt").join("state.json"))
}

pub fn load_or_default() -> SavedState {
    let Some(path) = config_path() else {
        return SavedState::default();
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return SavedState::default(),
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn save(state: &SavedState) -> std::io::Result<()> {
    let Some(path) = config_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(state)
        .unwrap_or_else(|_| b"{}".to_vec());
    std::fs::write(path, json)
}

// ----- enum serialization helpers -----

mod filter_serde {
    use super::ActiveFilter;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(f: &ActiveFilter, s: S) -> Result<S::Ok, S::Error> {
        let label = match f {
            ActiveFilter::ActiveOnly => "ActiveOnly",
            ActiveFilter::All => "All",
        };
        label.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<ActiveFilter, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "All" => ActiveFilter::All,
            _ => ActiveFilter::ActiveOnly,
        })
    }
}

mod window_serde {
    use super::SessionWindow;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(w: &SessionWindow, s: S) -> Result<S::Ok, S::Error> {
        let label = match w {
            SessionWindow::Days7 => "7d",
            SessionWindow::Days30 => "30d",
            SessionWindow::All => "all",
        };
        label.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SessionWindow, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "7d" => SessionWindow::Days7,
            "all" => SessionWindow::All,
            _ => SessionWindow::Days30,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ActiveFilter, SessionWindow};

    #[test]
    fn round_trip_json() {
        let s = SavedState {
            filter: ActiveFilter::All,
            expanded: vec!["alpha".into(), "beta".into()],
            search: Some("thelma".into()),
            selected_project: Some("alpha".into()),
            selected_worktree: Some("/home/jane/projects/alpha".into()),
            session_window: SessionWindow::Days7,
        };
        let json = serde_json::to_string(&s).unwrap();
        let parsed: SavedState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.filter, ActiveFilter::All);
        assert_eq!(parsed.expanded, vec!["alpha", "beta"]);
        assert_eq!(parsed.search.as_deref(), Some("thelma"));
        assert_eq!(parsed.selected_project.as_deref(), Some("alpha"));
        assert_eq!(parsed.session_window, SessionWindow::Days7);
    }

    #[test]
    fn empty_or_malformed_yields_defaults() {
        let parsed: SavedState = serde_json::from_str("{}").unwrap_or_default();
        assert_eq!(parsed.filter, ActiveFilter::ActiveOnly);
        assert_eq!(parsed.session_window, SessionWindow::Days30);
        let parsed: SavedState = serde_json::from_str("not json").unwrap_or_default();
        assert_eq!(parsed.filter, ActiveFilter::ActiveOnly);
    }
}

/// Snapshot the persistable parts of AppState into a SavedState.
pub fn snapshot(state: &crate::model::AppState) -> SavedState {
    SavedState {
        filter: state.filter,
        expanded: state.expanded.iter().cloned().collect(),
        search: state.search.clone(),
        selected_project: state.selected.as_ref().map(|s| s.project.clone()),
        selected_worktree: state
            .selected
            .as_ref()
            .and_then(|s| s.worktree.as_ref())
            .map(|p| p.to_string_lossy().to_string()),
        session_window: state.session_window,
    }
}
