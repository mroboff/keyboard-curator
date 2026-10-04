//! App state that outlives a session: recent projects and the window frame.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const RECENT_LIMIT: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowFrame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    pub recent: Vec<PathBuf>,
    pub window: Option<WindowFrame>,
    /// The folder the ZMK config was last exported to.
    pub export_dir: Option<PathBuf>,
}

fn state_path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("Keyboard Curator")
            .join("state.json"),
    )
}

/// Where new projects are saved unless the user chooses elsewhere.
pub fn default_project_dir() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_default()
        .join("Keyboard Curator")
}

impl AppState {
    /// Loads the saved state; a missing or unreadable file gives defaults.
    pub fn load() -> Self {
        state_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Saves the state. Failing to is not worth interrupting the user for.
    pub fn save(&self) {
        let Some(path) = state_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }

    /// Moves `path` to the front of the recent list.
    pub fn note_recent(&mut self, path: PathBuf) {
        self.recent.retain(|p| *p != path);
        self.recent.insert(0, path);
        self.recent.truncate(RECENT_LIMIT);
    }

    pub fn forget_recent(&mut self, path: &PathBuf) {
        self.recent.retain(|p| p != path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_projects_are_deduplicated_and_capped() {
        let mut state = AppState::default();
        for i in 0..12 {
            state.note_recent(PathBuf::from(format!("/p/{i}")));
        }
        state.note_recent(PathBuf::from("/p/5"));
        assert_eq!(state.recent.len(), RECENT_LIMIT);
        assert_eq!(state.recent[0], PathBuf::from("/p/5"));
        assert_eq!(state.recent.iter().filter(|p| p.ends_with("5")).count(), 1);
        state.forget_recent(&PathBuf::from("/p/5"));
        assert_eq!(state.recent[0], PathBuf::from("/p/11"));
    }
}
