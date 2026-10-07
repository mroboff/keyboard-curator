//! App state that outlives a session: the keyboard last worked on, the
//! window frame and whether the app is light or dark. Saved keyboards themselves live in the library.

use std::path::{Path, PathBuf};

use kc_model::KeyboardId;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowFrame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Whether the app is drawn light or dark.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    /// As the computer is set, changing when it does.
    #[default]
    System,
    Light,
    Dark,
}

/// The app's look. Each is a file under `assets/themes`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeId {
    #[default]
    Gallery,
    GoldenGate,
    Arena,
    Curator,
    Bench,
}

impl ThemeId {
    pub const ALL: [ThemeId; 5] = [
        ThemeId::Gallery,
        ThemeId::GoldenGate,
        ThemeId::Arena,
        ThemeId::Curator,
        ThemeId::Bench,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ThemeId::Gallery => "Gallery",
            ThemeId::GoldenGate => "Golden Gate",
            ThemeId::Arena => "Arena",
            ThemeId::Curator => "Curator",
            ThemeId::Bench => "Bench",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    /// The keyboard selected on the welcome screen.
    pub keyboard: Option<KeyboardId>,
    /// Projects opened before keyboards were saved. Each moves to its
    /// keyboard's own list when it is next opened.
    pub recent: Vec<PathBuf>,
    pub window: Option<WindowFrame>,
    pub appearance: Appearance,
    pub theme: ThemeId,
    /// The folder the ZMK config was last exported to.
    pub export_dir: Option<PathBuf>,
    /// Whether the app has been told not to look for a newer release when
    /// it starts. Stored the way round that makes a fresh install check.
    pub updates_off: bool,
    /// Firmware repository folders from before they were kept per
    /// keyboard, keyed by project path or `board:<id>`. A keyboard without
    /// a folder adopts the one its project used.
    pub repos: std::collections::BTreeMap<String, PathBuf>,
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

    pub fn forget_recent(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
    }

    /// The repository folder remembered for a project from before folders
    /// were kept per keyboard: the project's own, or else its board's.
    pub fn earlier_repo(&self, path: Option<&Path>, board: &str) -> Option<PathBuf> {
        path.and_then(|p| self.repos.get(&p.display().to_string()))
            .or_else(|| self.repos.get(&format!("board:{board}")))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_is_remembered_and_starts_as_the_systems() {
        // State saved before there was a choice follows the system.
        let earlier: AppState = serde_json::from_str(r#"{"recent": []}"#).unwrap();
        assert_eq!(earlier.appearance, Appearance::System);
        let state = AppState {
            appearance: Appearance::Dark,
            ..AppState::default()
        };
        let text = serde_json::to_string(&state).unwrap();
        assert!(text.contains(r#""appearance":"dark""#), "{text}");
        assert_eq!(serde_json::from_str::<AppState>(&text).unwrap(), state);
    }

    #[test]
    fn earlier_recent_projects_are_forgotten_one_at_a_time() {
        let mut state = AppState {
            recent: vec!["/p/a".into(), "/p/b".into()],
            ..AppState::default()
        };
        state.forget_recent(&PathBuf::from("/p/a"));
        assert_eq!(state.recent, [PathBuf::from("/p/b")]);
    }

    #[test]
    fn earlier_repository_folders_are_found_by_project_then_board() {
        let mut state = AppState::default();
        state.repos.insert("/p/a.kcproj".into(), "/repos/a".into());
        state
            .repos
            .insert("board:moergo-go60".into(), "/repos/go60".into());
        let a = PathBuf::from("/p/a.kcproj");
        let b = PathBuf::from("/p/b.kcproj");
        assert_eq!(
            state.earlier_repo(Some(&a), "moergo-go60"),
            Some("/repos/a".into())
        );
        assert_eq!(
            state.earlier_repo(Some(&b), "moergo-go60"),
            Some("/repos/go60".into())
        );
        assert_eq!(state.earlier_repo(Some(&b), "cyboard-imprint"), None);
        assert_eq!(
            state.earlier_repo(None, "moergo-go60"),
            Some("/repos/go60".into())
        );
    }
}
