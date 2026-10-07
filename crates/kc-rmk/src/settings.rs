//! The settings of RMK firmware configured over Rynk, as the board page
//! offers them: the global behavior timing and the lighting controls that
//! moergo-rmk keeps in its runtime configuration. They belong to the board,
//! like ZMK's Kconfig options, and are keyed `rmk.<section>.<name>` after
//! the sections of that configuration.
//!
//! A setting the board leaves at its default is not written to the
//! keyboard at all: whatever the keyboard holds for it stays. Only the
//! exported file has to put a number down, and uses the firmware's own
//! defaults there.

use kc_boards::board::RmkProfile;
use kc_model::features::SettingValue;
use kc_model::FirmwareConfig;
use kc_zmk::settings::{Setting, SettingKind};
use kc_zmk::Feature;

pub const BLUETOOTH_NAME: &str = "rmk.bluetooth_name";
pub const BRIGHTNESS: &str = "rmk.lighting.brightness";
pub const OUTPUT_MODE: &str = "rmk.lighting.output_mode";
pub const BACKGROUND_ENABLED: &str = "rmk.lighting.background.enabled";
pub const BACKGROUND_HUE: &str = "rmk.lighting.background.hue";
pub const BACKGROUND_SATURATION: &str = "rmk.lighting.background.saturation";
pub const BACKGROUND_VALUE: &str = "rmk.lighting.background.value";
pub const BACKGROUND_SPEED: &str = "rmk.lighting.background.speed";
pub const BACKGROUND_MODE: &str = "rmk.lighting.background.mode";
pub const EFFECT: &str = "rmk.lighting.effects.effect";
pub const PALETTE: &str = "rmk.lighting.effects.palette";
pub const EFFECT_VALUE: &str = "rmk.lighting.effects.value";
pub const EFFECT_SPEED: &str = "rmk.lighting.effects.speed";
pub const COMBO_TIMEOUT: &str = "rmk.behavior.combo_timeout_ms";
pub const ONESHOT_TIMEOUT: &str = "rmk.behavior.oneshot_timeout_ms";
pub const ONESHOT_QUICK_RELEASE: &str = "rmk.behavior.oneshot_quick_release";
pub const TAP_INTERVAL: &str = "rmk.behavior.tap_interval_ms";
pub const HOLD_TIMEOUT: &str = "rmk.behavior.morse.hold_timeout_ms";
pub const HOLD_MODE: &str = "rmk.behavior.morse.mode";
pub const QUICK_TAP: &str = "rmk.behavior.morse.quick_tap_ms";
pub const FLOW_TAP: &str = "rmk.behavior.morse.enable_flow_tap";
pub const PRIOR_IDLE: &str = "rmk.behavior.morse.prior_idle_ms";
pub const UNILATERAL_TAP: &str = "rmk.behavior.morse.unilateral_tap";
pub const OPPOSITE_HAND_HOLD: &str = "rmk.behavior.morse.opposite_hand_hold";

pub const OUTPUT_MODES: &[&str] = &["always-on", "always-off", "powered-only"];
pub const BACKGROUND_MODES: &[&str] = &["solid", "breathe"];
pub const HOLD_MODES: &[&str] = &[
    "normal",
    "permissive-hold",
    "hold-on-other-press",
    "tap-unless-interrupted",
];

const fn setting(
    group: &'static str,
    key: &'static str,
    name: &'static str,
    description: &'static str,
    kind: SettingKind,
) -> Setting {
    Setting {
        key,
        name,
        description,
        kind,
        group,
        requires: None,
    }
}

const fn int(min: i64, max: i64, step: i64) -> SettingKind {
    SettingKind::Int { min, max, step }
}

const fn lighting(
    key: &'static str,
    name: &'static str,
    description: &'static str,
    kind: SettingKind,
) -> Setting {
    Setting {
        requires: Some(Feature::RgbUnderglow),
        ..setting("Lighting", key, name, description, kind)
    }
}

/// The settings, grouped as the board page shows them.
pub const SETTINGS: &[Setting] = &[
    setting(
        "Keyboard",
        BLUETOOTH_NAME,
        "Keyboard name",
        "The name shown when pairing over Bluetooth. Write {slot} in it to add the profile's number.",
        SettingKind::Text { max_len: 16 },
    ),
    lighting(
        BRIGHTNESS,
        "Brightness",
        "How bright the lights are, as a share of what the firmware allows. The firmware keeps the board's own limit on top of this.",
        int(0, 100, 5),
    ),
    lighting(
        OUTPUT_MODE,
        "Lights on",
        "Always, never, or only while the keyboard is powered over USB. A layer with lock or battery lights wakes them while it is held.",
        SettingKind::Choice {
            options: OUTPUT_MODES,
        },
    ),
    lighting(
        EFFECT,
        "Animation",
        "The animation played under the colors a layout does not set.",
        SettingKind::Named,
    ),
    lighting(
        PALETTE,
        "Animation palette",
        "The colors the animation draws from.",
        SettingKind::Named,
    ),
    lighting(
        EFFECT_VALUE,
        "Animation brightness",
        "How bright the animation is, from 0 to 255.",
        int(0, 255, 15),
    ),
    lighting(
        EFFECT_SPEED,
        "Animation speed",
        "How fast the animation moves, from 0 to 255.",
        int(0, 255, 15),
    ),
    lighting(
        BACKGROUND_ENABLED,
        "Background color",
        "A single color behind everything else, in place of the animation.",
        SettingKind::Bool,
    ),
    lighting(
        BACKGROUND_HUE,
        "Background hue",
        "Where on the color wheel the background color sits, from 0 to 255.",
        int(0, 255, 8),
    ),
    lighting(
        BACKGROUND_SATURATION,
        "Background saturation",
        "How strong the background color is, from 0 (white) to 255.",
        int(0, 255, 15),
    ),
    lighting(
        BACKGROUND_VALUE,
        "Background brightness",
        "How bright the background color is, from 0 to 255.",
        int(0, 255, 15),
    ),
    lighting(
        BACKGROUND_SPEED,
        "Background breathing speed",
        "How fast the background breathes, when it does.",
        int(0, 255, 16),
    ),
    lighting(
        BACKGROUND_MODE,
        "Background style",
        "Steady, or breathing in and out.",
        SettingKind::Choice {
            options: BACKGROUND_MODES,
        },
    ),
    setting(
        "Typing",
        HOLD_TIMEOUT,
        "Hold time",
        "How long a mod-tap or layer-tap without its own timing is held before it counts as held.",
        int(50, 1000, 10),
    ),
    setting(
        "Typing",
        HOLD_MODE,
        "Hold decision",
        "How such a key decides between tap and hold when another key is pressed meanwhile: by the timer alone (normal), when the other key is released first (permissive-hold), as soon as another key is pressed (hold-on-other-press), or always as a tap (tap-unless-interrupted).",
        SettingKind::Choice {
            options: HOLD_MODES,
        },
    ),
    setting(
        "Typing",
        QUICK_TAP,
        "Quick tap",
        "Tapping then holding within this time repeats the tap instead of holding. 0 turns it off.",
        int(0, 500, 25),
    ),
    setting(
        "Typing",
        FLOW_TAP,
        "Only hold after a pause",
        "A mod-tap or layer-tap pressed while typing flows on as a tap; only one pressed after a pause can hold.",
        SettingKind::Bool,
    ),
    setting(
        "Typing",
        PRIOR_IDLE,
        "Pause before a hold",
        "How long typing must have paused for a hold to count, when the setting above is on.",
        int(0, 500, 10),
    ),
    setting(
        "Typing",
        UNILATERAL_TAP,
        "Tap for the same hand",
        "A hold-tap followed by a key on the same hand is a tap, whatever the timing.",
        SettingKind::Bool,
    ),
    setting(
        "Typing",
        OPPOSITE_HAND_HOLD,
        "Hold only for the other hand",
        "A hold-tap holds only when the next key is on the other hand; a key on the same hand settles it as a tap.",
        SettingKind::Bool,
    ),
    setting(
        "Typing",
        COMBO_TIMEOUT,
        "Combo window",
        "How close together a combo's keys must be pressed, in milliseconds. A combo with its own timing in the layout takes the longer of the two.",
        int(10, 500, 10),
    ),
    setting(
        "Typing",
        ONESHOT_TIMEOUT,
        "Sticky key timeout",
        "How long a sticky key or layer waits for the next key before letting go.",
        int(100, 5000, 100),
    ),
    setting(
        "Typing",
        ONESHOT_QUICK_RELEASE,
        "Sticky keys let go on the next key",
        "A sticky key releases as soon as the next key is pressed, rather than when it is released.",
        SettingKind::Bool,
    ),
    setting(
        "Typing",
        TAP_INTERVAL,
        "Tap length",
        "How long a tapped key is held down, in milliseconds.",
        int(5, 100, 5),
    ),
];

pub fn setting_for(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key)
}

/// The names a `Named` setting chooses from, from the firmware's profile.
pub fn options(profile: &RmkProfile, setting: &Setting) -> Vec<String> {
    match (setting.kind, setting.key) {
        (SettingKind::Choice { options }, _) => options.iter().map(|o| (*o).to_string()).collect(),
        (SettingKind::Named, EFFECT) => profile.effects.clone(),
        (SettingKind::Named, PALETTE) => profile.palettes.clone(),
        _ => Vec::new(),
    }
}

/// The firmware's own defaults, which the exported file uses for settings
/// the board leaves alone. Transcribed from moergo-rmk's
/// crates/go60-rmk/keyboard.toml (`[lighting.background]`) and from
/// moergo-config's `BehaviorConfig::default`.
pub mod defaults {
    pub const BRIGHTNESS: u8 = 255;
    pub const OUTPUT_MODE: &str = "always-on";
    pub const BACKGROUND_SPEED: u8 = 128;
    pub const COMBO_TIMEOUT_MS: u16 = 50;
    pub const ONESHOT_TIMEOUT_MS: u16 = 1000;
    pub const TAP_INTERVAL_MS: u16 = 20;
    pub const HOLD_TIMEOUT_MS: u16 = 250;
    pub const PRIOR_IDLE_MS: u16 = 120;
    pub const HOLD_MODE: &str = "normal";
}

/// The board's values for these settings, read with their types.
pub struct Values<'a>(pub &'a FirmwareConfig);

impl Values<'_> {
    pub fn is_set(&self, key: &str) -> bool {
        self.0.settings.contains_key(key)
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        match self.0.settings.get(key) {
            Some(SettingValue::Int(n)) => Some(*n),
            _ => None,
        }
    }

    /// An integer setting as the byte the firmware stores.
    pub fn byte(&self, key: &str) -> Option<u8> {
        self.int(key)
            .map(|n| u8::try_from(n.clamp(0, 255)).unwrap_or(u8::MAX))
    }

    /// An integer setting as the sixteen-bit count the firmware stores.
    pub fn u16(&self, key: &str) -> Option<u16> {
        self.int(key)
            .map(|n| u16::try_from(n.clamp(0, i64::from(u16::MAX))).unwrap_or(u16::MAX))
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.0.settings.get(key) {
            Some(SettingValue::Bool(b)) => Some(*b),
            _ => None,
        }
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        match self.0.settings.get(key) {
            Some(SettingValue::Text(text)) if !text.trim().is_empty() => Some(text.trim()),
            _ => None,
        }
    }

    /// Whether any setting of the lighting section is set.
    pub fn any_lighting(&self) -> bool {
        self.0
            .settings
            .keys()
            .any(|key| key.starts_with("rmk.lighting."))
    }
}

/// A brightness share in percent as the byte the firmware stores.
pub fn brightness_byte(percent: i64) -> u8 {
    let percent = percent.clamp(0, 100);
    u8::try_from((percent * 255 + 50) / 100).unwrap_or(u8::MAX)
}

/// The firmware's brightness byte as a share in percent.
pub fn brightness_percent(byte: u8) -> i64 {
    (i64::from(byte) * 100 + 127) / 255
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_well_formed_and_named_after_their_sections() {
        let mut keys: Vec<&str> = SETTINGS.iter().map(|s| s.key).collect();
        assert!(keys.iter().all(|k| k.starts_with("rmk.")));
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "a setting is listed twice");
        assert!(setting_for(BRIGHTNESS).is_some());
        assert!(setting_for("CONFIG_ZMK_SLEEP").is_none());
    }

    #[test]
    fn brightness_rounds_both_ways() {
        assert_eq!(brightness_byte(100), 255);
        assert_eq!(brightness_byte(0), 0);
        assert_eq!(brightness_byte(40), 102);
        assert_eq!(brightness_percent(102), 40);
        assert_eq!(brightness_percent(255), 100);
        for percent in (0..=100).step_by(5) {
            assert_eq!(brightness_percent(brightness_byte(percent)), percent);
        }
    }
}
