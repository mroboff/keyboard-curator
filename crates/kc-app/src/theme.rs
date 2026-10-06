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
const FONTS: [&[u8]; 28] = [
    include_bytes!("../assets/fonts/chakra-petch/ChakraPetch-Medium.ttf"),
    include_bytes!("../assets/fonts/chakra-petch/ChakraPetch-SemiBold.ttf"),
    include_bytes!("../assets/fonts/chakra-petch/ChakraPetch-Bold.ttf"),
    include_bytes!("../assets/fonts/barlow/Barlow-Regular.ttf"),
    include_bytes!("../assets/fonts/barlow/Barlow-Medium.ttf"),
    include_bytes!("../assets/fonts/barlow/Barlow-SemiBold.ttf"),
    include_bytes!("../assets/fonts/barlow/Barlow-Bold.ttf"),
    include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Medium.ttf"),
    include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-SemiBold.ttf"),
    include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    include_bytes!("../assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"),
    include_bytes!("../assets/fonts/ibm-plex-sans/IBMPlexSans-Medium.ttf"),
    include_bytes!("../assets/fonts/ibm-plex-sans/IBMPlexSans-SemiBold.ttf"),
    include_bytes!("../assets/fonts/ibm-plex-sans/IBMPlexSans-Bold.ttf"),
    include_bytes!("../assets/fonts/archivo/Archivo-Regular.ttf"),
    include_bytes!("../assets/fonts/archivo/Archivo-Medium.ttf"),
    include_bytes!("../assets/fonts/archivo/Archivo-SemiBold.ttf"),
    include_bytes!("../assets/fonts/archivo/Archivo-Bold.ttf"),
    include_bytes!("../assets/fonts/dm-mono/DMMono-Regular.ttf"),
    include_bytes!("../assets/fonts/dm-mono/DMMono-Medium.ttf"),
    include_bytes!("../assets/fonts/figtree/Figtree-Regular.ttf"),
    include_bytes!("../assets/fonts/figtree/Figtree-Medium.ttf"),
    include_bytes!("../assets/fonts/figtree/Figtree-SemiBold.ttf"),
    include_bytes!("../assets/fonts/figtree/Figtree-Bold.ttf"),
    include_bytes!("../assets/fonts/bricolage-grotesque/BricolageGrotesque-Medium.ttf"),
    include_bytes!("../assets/fonts/bricolage-grotesque/BricolageGrotesque-SemiBold.ttf"),
    include_bytes!("../assets/fonts/bricolage-grotesque/BricolageGrotesque-Bold.ttf"),
];

/// The families those files hold, by the names themes use for them.
const FAMILIES: [&str; 8] = [
    "Figtree",
    "Bricolage Grotesque",
    "Chakra Petch",
    "Barlow",
    "JetBrains Mono",
    "IBM Plex Sans",
    "Archivo",
    "DM Mono",
];

impl ThemeId {
    fn source(self) -> &'static str {
        match self {
            ThemeId::Gallery => include_str!("../assets/themes/gallery.json"),
            ThemeId::GoldenGate => include_str!("../assets/themes/golden-gate.json"),
            ThemeId::Arena => include_str!("../assets/themes/arena.json"),
            ThemeId::Curator => include_str!("../assets/themes/curator.json"),
            ThemeId::Bench => include_str!("../assets/themes/bench.json"),
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

/// What a theme draws behind the welcome screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backdrop {
    #[default]
    None,
    /// The holes of a pegboard, for hanging keyboards on.
    Pegboard,
    /// A faint grid, like a stage floor.
    Grid,
}

/// How the window sits on the desktop.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowStyle {
    #[default]
    Opaque,
    /// See-through, with what is behind the window blurred: glass. The
    /// theme's background color carries how see-through.
    Blurred,
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
    /// The legend of a system key.
    #[serde(deserialize_with = "hex")]
    pub system_text: Hsla,
    /// Very large figures set behind the content, barely darker than it.
    #[serde(deserialize_with = "hex")]
    pub wash: Hsla,
    /// What the backdrop is drawn in.
    #[serde(deserialize_with = "hex")]
    pub mark: Hsla,
    /// The accent color for text on the background. The accent itself is
    /// chosen to sit under text, and can be too dark to be text.
    #[serde(deserialize_with = "hex")]
    pub accent_text: Hsla,
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
    /// Whether headings and tabs are set in capitals.
    #[serde(default)]
    uppercase: bool,
    /// Whether the editor's status line lists its keyboard shortcuts.
    #[serde(default)]
    hints: bool,
    #[serde(default)]
    backdrop: Backdrop,
    #[serde(default)]
    window: WindowStyle,
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
    /// Whether headings and tabs are set in capitals.
    pub uppercase: bool,
    /// Whether the editor's status line lists its keyboard shortcuts.
    pub hints: bool,
    pub backdrop: Backdrop,
    pub window: WindowStyle,
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
        uppercase: file.uppercase,
        hints: file.hints,
        backdrop: file.backdrop,
        window: file.window,
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

/// The theme's backdrop, to lay behind a page: nothing for most themes.
pub fn backdrop(cx: &App) -> Option<impl IntoElement> {
    let look = look(cx);
    let (kind, mark) = (look.backdrop, look.colors.mark);
    if kind == Backdrop::None {
        return None;
    }
    let drawn = canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let (left, top) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            match kind {
                Backdrop::None => {}
                Backdrop::Pegboard => {
                    const PITCH: f32 = 30.;
                    const HOLE: f32 = 6.;
                    let mut y = PITCH / 2.;
                    while y < height {
                        let mut x = PITCH / 2.;
                        while x < width {
                            let hole = Bounds::centered_at(
                                point(px(left + x), px(top + y)),
                                size(px(HOLE), px(HOLE)),
                            );
                            window.paint_quad(fill(hole, mark).corner_radii(px(HOLE / 2.)));
                            x += PITCH;
                        }
                        y += PITCH;
                    }
                }
                Backdrop::Grid => {
                    const PITCH: f32 = 44.;
                    let mut x = PITCH;
                    while x < width {
                        let line =
                            Bounds::new(point(px(left + x), px(top)), size(px(1.), px(height)));
                        window.paint_quad(fill(line, mark));
                        x += PITCH;
                    }
                    let mut y = PITCH;
                    while y < height {
                        let line =
                            Bounds::new(point(px(left), px(top + y)), size(px(width), px(1.)));
                        window.paint_quad(fill(line, mark));
                        y += PITCH;
                    }
                }
            }
        },
    );
    Some(drawn.absolute().top_0().left_0().size_full())
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
        // Gallery and Golden Gate come in light and dark; the others are
        // dark by nature.
        for theme in ThemeId::ALL {
            let both = matches!(theme, ThemeId::Gallery | ThemeId::GoldenGate);
            let only = ThemeFile::load(theme).only();
            assert_eq!(only.is_none(), both, "{theme:?}");
            if !both {
                assert_eq!(only, Some(ThemeMode::Dark), "{theme:?}");
            }
        }
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
