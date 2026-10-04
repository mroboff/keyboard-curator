//! The keycode catalogue, built from ZMK's own `keys.h`.
//!
//! Names, aliases, usages and descriptions come straight from the vendored
//! header, so the catalogue cannot drift from what the firmware accepts.
//! Picker categories and short keycap legends are derived here.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::modifiers::Modifier;

const KEYS_H: &str = include_str!("../vendor/zmk-v0.3.0/keys.h");
const HID_USAGE_H: &str = include_str!("../vendor/zmk-v0.3.0/hid_usage.h");

/// The HID usage pages ZMK keycodes live on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsagePage {
    GenericDesktop,
    Keyboard,
    Consumer,
}

impl UsagePage {
    pub fn id(self) -> u16 {
        match self {
            UsagePage::GenericDesktop => 0x01,
            UsagePage::Keyboard => 0x07,
            UsagePage::Consumer => 0x0C,
        }
    }
}

/// Where a keycode appears in the key picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Letters,
    Numbers,
    Symbols,
    Editing,
    Navigation,
    Function,
    Modifiers,
    Keypad,
    Media,
    Apps,
    International,
    System,
}

/// Another name ZMK accepts for a keycode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias {
    pub name: String,
    /// ZMK marks some aliases as deprecated; they still compile.
    pub deprecated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keycode {
    /// The primary name, as first defined in `keys.h`.
    pub name: String,
    pub aliases: Vec<Alias>,
    /// ZMK's own description, for example `a and A` or `Play/Pause`.
    pub description: String,
    pub page: UsagePage,
    pub usage: u16,
    /// Modifiers built into the keycode, such as shift in `EXCLAMATION`.
    pub implicit_mods: Vec<Modifier>,
    pub category: Category,
    /// Short text for a keycap.
    pub legend: String,
}

impl Keycode {
    /// The shortest current name, which is what generated keymaps use.
    pub fn short_name(&self) -> &str {
        self.aliases
            .iter()
            .filter(|a| !a.deprecated)
            .map(|a| a.name.as_str())
            .chain([self.name.as_str()])
            .min_by_key(|n| n.len())
            .unwrap_or(&self.name)
    }
}

pub struct Keycodes {
    all: Vec<Keycode>,
    by_name: HashMap<String, usize>,
}

impl Keycodes {
    pub fn all(&self) -> &[Keycode] {
        &self.all
    }

    /// Looks a keycode up by its primary name or any alias.
    pub fn get(&self, name: &str) -> Option<&Keycode> {
        self.by_name.get(name).map(|&ix| &self.all[ix])
    }

    pub fn in_category(&self, category: Category) -> impl Iterator<Item = &Keycode> {
        self.all.iter().filter(move |k| k.category == category)
    }
}

/// The catalogue for ZMK v0.3.0, parsed once on first use.
pub fn keycodes() -> &'static Keycodes {
    static CATALOGUE: OnceLock<Keycodes> = OnceLock::new();
    CATALOGUE.get_or_init(|| parse(KEYS_H, HID_USAGE_H))
}

/// `#define NAME (0x1234)` entries of `hid_usage.h`.
fn usage_ids(hid_usage_h: &str) -> HashMap<&str, u16> {
    hid_usage_h
        .lines()
        .filter_map(|line| {
            let mut parts = line.strip_prefix("#define ")?.split_whitespace();
            let (name, value) = (parts.next()?, parts.next()?);
            let hex = value
                .trim_matches(|c| c == '(' || c == ')')
                .strip_prefix("0x")?;
            Some((name, u16::from_str_radix(hex, 16).ok()?))
        })
        .collect()
}

fn parse(keys_h: &str, hid_usage_h: &str) -> Keycodes {
    let usages = usage_ids(hid_usage_h);
    let joined = keys_h.replace("\\\n", " ");
    let mut all: Vec<Keycode> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut description = String::new();

    for line in joined.lines() {
        let line = line.trim();
        if let Some(comment) = line.strip_prefix("/* ").and_then(|l| l.strip_suffix(" */")) {
            description = comment.to_string();
            continue;
        }
        let Some(rest) = line.strip_prefix("#define ") else {
            continue;
        };
        let Some((name, value)) = rest.split_once(char::is_whitespace) else {
            continue;
        };
        let deprecated = value.contains("DEPRECATED");
        let value = value.split("//").next().unwrap_or("").trim();
        let inner = value
            .strip_prefix('(')
            .and_then(|v| v.strip_suffix(')'))
            .unwrap_or(value)
            .trim();

        if let Some(&target) = by_name.get(inner) {
            all[target].aliases.push(Alias {
                name: name.to_string(),
                deprecated,
            });
            by_name.insert(name.to_string(), target);
            continue;
        }

        let (shifted, usage_call) =
            match inner.strip_prefix("LS(").and_then(|v| v.strip_suffix(')')) {
                Some(call) => (true, call),
                None => (false, inner),
            };
        let Some(args) = usage_call
            .strip_prefix("ZMK_HID_USAGE(")
            .and_then(|v| v.strip_suffix(')'))
        else {
            continue;
        };
        let Some((page, usage)) = args.split_once(',') else {
            continue;
        };
        let page = match page.trim() {
            "HID_USAGE_GD" => UsagePage::GenericDesktop,
            "HID_USAGE_KEY" => UsagePage::Keyboard,
            "HID_USAGE_CONSUMER" => UsagePage::Consumer,
            _ => continue,
        };
        let usage = usage.trim();
        let usage = match usage.strip_prefix("0x") {
            Some(hex) => u16::from_str_radix(hex, 16).ok(),
            None => usages.get(usage).copied(),
        };
        let Some(usage) = usage else { continue };

        let category = categorize(name, &description, page, usage, shifted);
        let legend = legend(name, &description, category);
        by_name.insert(name.to_string(), all.len());
        all.push(Keycode {
            name: name.to_string(),
            aliases: Vec::new(),
            description: strip_kind(&description).to_string(),
            page,
            usage,
            implicit_mods: if shifted {
                vec![Modifier::LShift]
            } else {
                vec![]
            },
            category,
            legend,
        });
    }
    Keycodes { all, by_name }
}

/// Drops the leading `Keyboard`, `Keypad` or `Consumer` word of a comment.
fn strip_kind(description: &str) -> &str {
    ["Keyboard ", "Consumer ", "Apple "]
        .iter()
        .find_map(|kind| description.strip_prefix(kind))
        .unwrap_or(description)
}

fn categorize(
    name: &str,
    description: &str,
    page: UsagePage,
    usage: u16,
    shifted: bool,
) -> Category {
    match page {
        UsagePage::GenericDesktop => return Category::System,
        UsagePage::Consumer => {
            return if name.starts_with("C_AL_") || name.starts_with("C_AC_") {
                Category::Apps
            } else {
                Category::Media
            };
        }
        UsagePage::Keyboard => {}
    }
    if description.starts_with("Keypad") {
        return Category::Keypad;
    }
    match usage {
        0x04..=0x1D => Category::Letters,
        0x1E..=0x27 if shifted => Category::Symbols,
        0x1E..=0x27 => Category::Numbers,
        0x28..=0x2C | 0x39 => Category::Editing,
        0x2D..=0x38 => Category::Symbols,
        0x3A..=0x45 | 0x68..=0x73 => Category::Function,
        0x46..=0x52 => Category::Navigation,
        0x64 | 0x87..=0x98 => Category::International,
        0x65 | 0x74..=0x7E => Category::Editing,
        0x66 | 0x82..=0x84 => Category::System,
        0x7F..=0x81 | 0xE8..=0xFB => Category::Media,
        0xE0..=0xE7 => Category::Modifiers,
        _ => Category::System,
    }
}

/// Legends for keys whose names are too long for a keycap.
const LEGENDS: &[(&str, &str)] = &[
    ("RETURN", "Enter"),
    ("ESCAPE", "Esc"),
    ("BACKSPACE", "Bksp"),
    ("TAB", "Tab"),
    ("SPACE", "Space"),
    ("CAPSLOCK", "Caps"),
    ("PRINTSCREEN", "PrtSc"),
    ("SCROLLLOCK", "ScrLk"),
    ("PAUSE_BREAK", "Pause"),
    ("INSERT", "Ins"),
    ("HOME", "Home"),
    ("PAGE_UP", "PgUp"),
    ("DELETE", "Del"),
    ("END", "End"),
    ("PAGE_DOWN", "PgDn"),
    ("RIGHT_ARROW", "→"),
    ("LEFT_ARROW", "←"),
    ("DOWN_ARROW", "↓"),
    ("UP_ARROW", "↑"),
    ("LEFT_CONTROL", "Ctrl"),
    ("LEFT_SHIFT", "Shift"),
    ("LEFT_ALT", "Alt"),
    ("LEFT_GUI", "Gui"),
    ("RIGHT_CONTROL", "RCtrl"),
    ("RIGHT_SHIFT", "RShift"),
    ("RIGHT_ALT", "RAlt"),
    ("RIGHT_GUI", "RGui"),
    ("KP_NUMLOCK", "Num"),
    ("KP_ENTER", "Enter"),
    ("K_APPLICATION", "Menu"),
    ("C_PLAY_PAUSE", "Play"),
    ("C_NEXT", "Next"),
    ("C_PREVIOUS", "Prev"),
    ("C_STOP", "Stop"),
    ("C_MUTE", "Mute"),
    ("C_VOLUME_UP", "Vol+"),
    ("C_VOLUME_DOWN", "Vol-"),
    ("C_BRIGHTNESS_INC", "Bri+"),
    ("C_BRIGHTNESS_DEC", "Bri-"),
];

fn legend(name: &str, description: &str, category: Category) -> String {
    if let Some((_, legend)) = LEGENDS.iter().find(|(n, _)| *n == name) {
        return legend.to_string();
    }
    let text = ["Keyboard ", "Keypad ", "Consumer ", "Apple "]
        .iter()
        .find_map(|kind| description.strip_prefix(kind))
        .unwrap_or(description);
    let first = text.split_whitespace().next().unwrap_or(name);
    match category {
        // `a and A` -> `A`; `1 and !` -> `1`; `- and _ (Minus and ...)` -> `-`.
        Category::Letters => first.to_uppercase(),
        Category::Numbers | Category::Symbols => first.to_string(),
        Category::Keypad if first.chars().count() <= 2 => first.to_string(),
        Category::Function => name.to_string(),
        _ => text.to_string(),
    }
}

/// The keycode for a key typed on the host keyboard, named the way GUI
/// toolkits report keystrokes (`a`, `1`, `enter`, `-`). Used to assign keys
/// by typing them.
pub fn from_typed(key: &str) -> Option<&'static str> {
    const NAMED: &[(&str, &str)] = &[
        ("enter", "RET"),
        ("space", "SPACE"),
        ("tab", "TAB"),
        ("backspace", "BSPC"),
        ("escape", "ESC"),
        ("delete", "DEL"),
        ("home", "HOME"),
        ("end", "END"),
        ("pageup", "PG_UP"),
        ("pagedown", "PG_DN"),
        ("-", "MINUS"),
        ("=", "EQUAL"),
        ("[", "LBKT"),
        ("]", "RBKT"),
        ("\\", "BSLH"),
        (";", "SEMI"),
        ("'", "SQT"),
        ("`", "GRAVE"),
        (",", "COMMA"),
        (".", "DOT"),
        ("/", "FSLH"),
    ];
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    const DIGITS: [&str; 10] = ["N0", "N1", "N2", "N3", "N4", "N5", "N6", "N7", "N8", "N9"];
    const FUNCTION: [&str; 12] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
    ];
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_lowercase() {
            return Some(LETTERS[(c as u8 - b'a') as usize]);
        }
        if c.is_ascii_digit() {
            return Some(DIGITS[(c as u8 - b'0') as usize]);
        }
    }
    if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<usize>().ok()) {
        return FUNCTION.get(n.wrapping_sub(1)).copied();
    }
    NAMED
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, code)| *code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_keys_map_to_real_keycodes() {
        for (typed, code) in [
            ("a", "A"),
            ("z", "Z"),
            ("0", "N0"),
            ("7", "N7"),
            ("enter", "RET"),
            ("f5", "F5"),
            ("\\", "BSLH"),
            ("/", "FSLH"),
        ] {
            assert_eq!(from_typed(typed), Some(code), "{typed}");
            assert!(keycodes().get(code).is_some(), "{code}");
        }
        for typed in ["", "ab", "f0", "f13", "left", "A"] {
            assert_eq!(from_typed(typed), None, "{typed}");
        }
    }

    #[test]
    fn letters_numbers_and_aliases_resolve() {
        let codes = keycodes();
        let a = codes.get("A").unwrap();
        assert_eq!(
            (a.page, a.usage, a.category),
            (UsagePage::Keyboard, 0x04, Category::Letters)
        );
        assert_eq!(a.legend, "A");

        let one = codes.get("N1").unwrap();
        assert_eq!(one.name, "NUMBER_1");
        assert_eq!(
            (one.usage, one.category, one.legend.as_str()),
            (0x1E, Category::Numbers, "1")
        );
        assert_eq!(one.short_name(), "N1");

        let ret = codes.get("RET").unwrap();
        assert_eq!(ret.name, "RETURN");
        assert!(std::ptr::eq(ret, codes.get("ENTER").unwrap()));
    }

    #[test]
    fn shifted_symbols_carry_an_implicit_shift() {
        let excl = keycodes().get("EXCL").unwrap();
        assert_eq!(excl.name, "EXCLAMATION");
        assert_eq!(excl.usage, 0x1E);
        assert_eq!(excl.implicit_mods, [Modifier::LShift]);
        assert_eq!(
            (excl.category, excl.legend.as_str()),
            (Category::Symbols, "!")
        );
        assert!(keycodes().get("MINUS").unwrap().implicit_mods.is_empty());
    }

    #[test]
    fn deprecated_aliases_are_flagged_and_never_preferred() {
        let space = keycodes().get("SPC").unwrap();
        assert_eq!(space.name, "SPACE");
        assert!(space
            .aliases
            .iter()
            .any(|a| a.name == "SPC" && a.deprecated));
        assert_eq!(space.short_name(), "SPACE");
    }

    #[test]
    fn other_pages_and_categories() {
        let codes = keycodes();
        let play = codes.get("C_PP").unwrap();
        assert_eq!(
            (play.page, play.usage, play.category),
            (UsagePage::Consumer, 0xCD, Category::Media)
        );
        assert_eq!(
            codes.get("SYS_SLEEP").unwrap().page,
            UsagePage::GenericDesktop
        );
        assert_eq!(codes.get("LSHFT").unwrap().category, Category::Modifiers);
        assert_eq!(codes.get("F13").unwrap().category, Category::Function);
        assert_eq!(codes.get("KP_N7").unwrap().category, Category::Keypad);
        assert_eq!(codes.get("LEFT").unwrap().legend, "←");
        assert_eq!(codes.get("C_AL_CALC").unwrap().category, Category::Apps);
        assert_eq!(codes.get("K_PLAY_PAUSE").unwrap().usage, 0xE8);
    }

    /// Every name `keys.h` defines must be in the catalogue.
    #[test]
    fn covers_every_define_in_the_header() {
        let codes = keycodes();
        let defined: Vec<&str> = KEYS_H
            .lines()
            .filter_map(|l| l.strip_prefix("#define ")?.split_whitespace().next())
            .collect();
        assert!(
            defined.len() > 600,
            "header parsed: {} defines",
            defined.len()
        );
        let missing: Vec<&&str> = defined.iter().filter(|n| codes.get(n).is_none()).collect();
        assert!(missing.is_empty(), "not in catalogue: {missing:?}");

        // Short names are what the emitter writes, so they must be unique.
        let mut short: Vec<&str> = codes.all().iter().map(Keycode::short_name).collect();
        short.sort_unstable();
        let before = short.len();
        short.dedup();
        assert_eq!(short.len(), before);
        assert!(codes.all().iter().all(|k| !k.legend.is_empty()));
    }
}
