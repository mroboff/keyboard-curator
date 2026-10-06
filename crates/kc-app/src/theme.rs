//! Themes: the app's whole look, chosen from the View menu.
//!
//! A theme is a data file under `assets/themes`. It carries the toolkit's
//! own theme (colors, type, corner radius) for light, dark or both, and a
//! "look": what the toolkit has no word for, such as how a keycap is drawn.
//! A theme with both follows the light or dark choice; a theme with one is
//! that way by nature and stays so.

use std::borrow::Cow;
use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig, ThemeMode};
use gpui_kit::*;
use serde::{Deserialize, Deserializer};

use crate::state::{Appearance, ThemeId};

/// The typefaces themes use, carried in the app so that a theme looks the
/// same on every computer. Each is under the SIL Open Font License, kept
/// beside it.
const FONTS: [&[u8]; 7] = [
    include_bytes!("../assets/fonts/figtree/Figtree-Regular.ttf"),
    include_bytes!("../assets/fonts/figtree/Figtree-Medium.ttf"),
    include_bytes!("../assets/fonts/figtree/Figtree-SemiBold.ttf"),
    include_bytes!("../assets/fonts/figtree/Figtree-Bold.ttf"),
    include_bytes!("../assets/fonts/bricolage-grotesque/BricolageGrotesque-Medium.ttf"),
    include_bytes!("../assets/fonts/bricolage-grotesque/BricolageGrotesque-SemiBold.ttf"),
    include_bytes!("../assets/fonts/bricolage-grotesque/BricolageGrotesque-Bold.ttf"),
];

/// The families those files hold, by the names themes use for them.
const FAMILIES: [&str; 2] = ["Figtree", "Bricolage Grotesque"];

impl ThemeId {
    fn source(self) -> &'static str {
        match self {
            ThemeId::Gallery => include_str!("../assets/themes/gallery.json"),
        }
    }
}

/// How a keycap is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapStyle {
    /// A filled outline.
    Flat,
    /// A raised cap: a top face over a darker lower edge, with a shadow.
    Sculpted,
}

fn hex<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Hsla, D::Error> {
    let text = String::deserialize(deserializer)?;
    parse_hex(&text).ok_or_else(|| serde::de::Error::custom(format!("`{text}` is not a color")))
}

/// A color written `#RRGGBB` or `#RRGGBBAA`.
fn parse_hex(text: &str) -> Option<Hsla> {
    let digits = text.strip_prefix('#')?;
    let value = u32::from_str_radix(digits, 16).ok()?;
    match digits.len() {
        6 => Some(rgb(value).into()),
        8 => Some(rgba(value).into()),
        _ => None,
    }
}

/// The colors of a look, for one of light and dark.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct LookColors {
    #[serde(deserialize_with = "hex")]
    pub key: Hsla,
    #[serde(deserialize_with = "hex")]
    pub key_border: Hsla,
    /// The lower edge of a sculpted cap.
    #[serde(deserialize_with = "hex")]
    pub key_lip: Hsla,
    #[serde(deserialize_with = "hex")]
    pub key_text: Hsla,
    /// Keys that work the keyboard itself: Bluetooth, lighting, reset.
    #[serde(deserialize_with = "hex")]
    pub system_key: Hsla,
    #[serde(deserialize_with = "hex")]
    pub system_lip: Hsla,
    #[serde(deserialize_with = "hex")]
    pub layer_key: Hsla,
    #[serde(deserialize_with = "hex")]
    pub layer_lip: Hsla,
    #[serde(deserialize_with = "hex")]
    pub layer_text: Hsla,
    /// The surface a keyboard is shown on.
    #[serde(deserialize_with = "hex")]
    pub plinth: Hsla,
    #[serde(deserialize_with = "hex")]
    pub plinth_border: Hsla,
    /// Very large figures set behind the content, barely darker than it.
    #[serde(deserialize_with = "hex")]
    pub wash: Hsla,
}

#[derive(Debug, Clone, Deserialize)]
struct Variant {
    toolkit: ThemeConfig,
    look: LookColors,
}

#[derive(Debug, Clone, Deserialize)]
struct ThemeFile {
    display_font: String,
    caps: CapStyle,
    /// Corner radius of buttons and chips; a large number makes pills.
    control_radius: f32,
    /// Whether very large figures are set behind the content.
    #[serde(default)]
    figures: bool,
    light: Option<Variant>,
    dark: Option<Variant>,
}

impl ThemeFile {
    fn load(theme: ThemeId) -> Self {
        serde_json::from_str(theme.source()).expect("built-in themes are valid")
    }

    /// The one appearance the theme has, when it has only one.
    fn only(&self) -> Option<ThemeMode> {
        match (&self.light, &self.dark) {
            (Some(_), Some(_)) => None,
            (None, Some(_)) => Some(ThemeMode::Dark),
            _ => Some(ThemeMode::Light),
        }
    }

    fn variant(&self, mode: ThemeMode) -> &Variant {
        let (first, second) = if mode.is_dark() {
            (&self.dark, &self.light)
        } else {
            (&self.light, &self.dark)
        };
        first
            .as_ref()
            .or(second.as_ref())
            .expect("a theme has a light or a dark variant")
    }
}

/// The part of the active theme the toolkit has no word for. Views read it
/// beside the toolkit's theme.
#[derive(Debug, Clone)]
pub struct Look {
    pub theme: ThemeId,
    /// The typeface of headings and key legends.
    pub display_font: SharedString,
    pub caps: CapStyle,
    pub control_radius: Pixels,
    /// Whether very large figures are set behind the content.
    pub figures: bool,
    pub colors: LookColors,
}

impl Global for Look {}

/// The active look.
pub fn look(cx: &App) -> &Look {
    cx.global::<Look>()
}

/// Makes the bundled typefaces available. Called once, before any theme is
/// applied.
pub fn init(cx: &mut App) {
    let fonts = FONTS.iter().map(|font| Cow::Borrowed(*font)).collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("The themes' typefaces could not be loaded: {error}.");
    }
    // A typeface that did not register would be swapped for the system's
    // without a word; say so instead.
    let known = cx.text_system().all_font_names();
    for family in FAMILIES {
        if !known.iter().any(|name| name == family) {
            eprintln!("The typeface {family} is missing; the system's is used in its place.");
        }
    }
}

fn install(theme: ThemeId, file: &ThemeFile, mode: ThemeMode, cx: &mut App) {
    let variant = file.variant(mode);
    cx.set_global(Look {
        theme,
        display_font: file.display_font.clone().into(),
        caps: file.caps,
        control_radius: px(file.control_radius),
        figures: file.figures,
        colors: variant.look,
    });
    let toolkit = Theme::global_mut(cx);
    if let Some(light) = &file.light {
        toolkit.light_theme = Rc::new(light.toolkit.clone());
    }
    if let Some(dark) = &file.dark {
        toolkit.dark_theme = Rc::new(dark.toolkit.clone());
    }
    Theme::change(mode, None, cx);
}

/// Draws the app in a theme, light or dark as chosen where the theme has
/// both. The parts of the window the system draws (the title bar, menus
/// and dialogs) follow, so the two never disagree.
pub fn apply(theme: ThemeId, appearance: Appearance, cx: &mut App) {
    let file = ThemeFile::load(theme);
    let held = file.only().or(match appearance {
        Appearance::System => None,
        Appearance::Light => Some(ThemeMode::Light),
        Appearance::Dark => Some(ThemeMode::Dark),
    });
    cx.set_window_appearance(held.map(|mode| {
        if mode.is_dark() {
            WindowAppearance::Dark
        } else {
            WindowAppearance::Light
        }
    }));
    let mode = held.unwrap_or_else(|| cx.window_appearance().into());
    install(theme, &file, mode, cx);
}

/// Follows the window when its appearance changes: the computer has gone
/// from day to night, or the choice above has taken effect.
pub fn follow(window: &mut Window, cx: &mut App) {
    let theme = look(cx).theme;
    let file = ThemeFile::load(theme);
    let mode = file.only().unwrap_or_else(|| window.appearance().into());
    install(theme, &file, mode, cx);
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::ThemeMode;

    use super::{parse_hex, ThemeFile};
    use crate::state::ThemeId;

    #[test]
    fn every_theme_is_a_valid_file_with_an_appearance() {
        for theme in ThemeId::ALL {
            let file = ThemeFile::load(theme);
            assert!(file.light.is_some() || file.dark.is_some(), "{theme:?}");
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                // A theme asked for the appearance it lacks gives the one
                // it has.
                let variant = file.variant(mode);
                if file.only().is_none() {
                    assert_eq!(variant.toolkit.mode, mode, "{theme:?}");
                }
            }
            assert!(!file.display_font.is_empty());
        }
        assert!(ThemeFile::load(ThemeId::default()).only().is_none());
    }

    #[test]
    fn colors_are_six_or_eight_hex_digits() {
        assert!(parse_hex("#2B3BD6").is_some());
        assert!(parse_hex("#2B3BD64D").is_some());
        for bad in ["2B3BD6", "#2B3", "#GGGGGG", ""] {
            assert!(parse_hex(bad).is_none(), "{bad}");
        }
    }
}
