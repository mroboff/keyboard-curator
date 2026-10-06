//! Light and dark: the app follows the computer, or is held to one.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

use crate::state::Appearance;

/// Draws the app as chosen. The parts of the window the system draws (the
/// title bar, menus and dialogs) follow, so the two never disagree.
pub fn apply(appearance: Appearance, cx: &mut App) {
    cx.set_window_appearance(match appearance {
        Appearance::System => None,
        Appearance::Light => Some(WindowAppearance::Light),
        Appearance::Dark => Some(WindowAppearance::Dark),
    });
    match appearance {
        Appearance::System => Theme::sync_system_appearance(None, cx),
        Appearance::Light => Theme::change(ThemeMode::Light, None, cx),
        Appearance::Dark => Theme::change(ThemeMode::Dark, None, cx),
    }
}

/// Follows the window when its appearance changes: the computer has gone
/// from day to night, or the choice above has taken effect.
pub fn follow(window: &mut Window, cx: &mut App) {
    Theme::sync_system_appearance(Some(window), cx);
}
