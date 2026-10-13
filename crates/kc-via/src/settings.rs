//! The settings of a VIA keyboard, as the board page offers them: the
//! lighting values QMK exposes through VIA's custom channels. They belong
//! to the board, keyed `via.<channel>.<name>`, and each stands for one
//! value of one channel. Reading a keyboard carries them to the board;
//! applying writes those that differ, and only the values the keyboard's
//! definition reaches.
//!
//! Effects are numbers: which number is which effect is the keyboard's
//! firmware's own, and its definition names them for VIA.

use kc_model::features::SettingValue;
use kc_zmk::settings::{Setting, SettingKind};

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

const BYTE: SettingKind = SettingKind::Int {
    min: 0,
    max: 255,
    step: 1,
};

/// Every setting, in the order the board page shows them.
pub const SETTINGS: &[Setting] = &[
    setting(
        "Backlight",
        "via.backlight.brightness",
        "Brightness",
        "How bright the backlight is, from 0 to 255.",
        BYTE,
    ),
    setting(
        "Backlight",
        "via.backlight.effect",
        "Breathing",
        "Whether the backlight breathes.",
        SettingKind::Bool,
    ),
    setting(
        "Underglow",
        "via.rgblight.brightness",
        "Brightness",
        "How bright the underglow is, from 0 to 255.",
        BYTE,
    ),
    setting(
        "Underglow",
        "via.rgblight.effect",
        "Effect",
        "The underglow effect, by its number in the keyboard's firmware; 0 is off.",
        BYTE,
    ),
    setting(
        "Underglow",
        "via.rgblight.effect_speed",
        "Speed",
        "How fast the effect runs, from 0 to 255.",
        BYTE,
    ),
    setting(
        "Underglow",
        "via.rgblight.color_hue",
        "Hue",
        "The color's hue, from 0 to 255 around the wheel.",
        BYTE,
    ),
    setting(
        "Underglow",
        "via.rgblight.color_sat",
        "Saturation",
        "The color's saturation, from 0 (white) to 255.",
        BYTE,
    ),
    setting(
        "Per-key lighting",
        "via.rgb_matrix.brightness",
        "Brightness",
        "How bright the key lights are, from 0 to 255.",
        BYTE,
    ),
    setting(
        "Per-key lighting",
        "via.rgb_matrix.effect",
        "Effect",
        "The lighting effect, by its number in the keyboard's firmware; 0 is off.",
        BYTE,
    ),
    setting(
        "Per-key lighting",
        "via.rgb_matrix.effect_speed",
        "Speed",
        "How fast the effect runs, from 0 to 255.",
        BYTE,
    ),
    setting(
        "Per-key lighting",
        "via.rgb_matrix.color_hue",
        "Hue",
        "The color's hue, from 0 to 255 around the wheel.",
        BYTE,
    ),
    setting(
        "Per-key lighting",
        "via.rgb_matrix.color_sat",
        "Saturation",
        "The color's saturation, from 0 (white) to 255.",
        BYTE,
    ),
    setting(
        "Key lights",
        "via.led_matrix.brightness",
        "Brightness",
        "How bright the single-color key lights are, from 0 to 255.",
        BYTE,
    ),
    setting(
        "Key lights",
        "via.led_matrix.effect",
        "Effect",
        "The lighting effect, by its number in the keyboard's firmware; 0 is off.",
        BYTE,
    ),
    setting(
        "Key lights",
        "via.led_matrix.effect_speed",
        "Speed",
        "How fast the effect runs, from 0 to 255.",
        BYTE,
    ),
    setting(
        "Sound",
        "via.audio.enable",
        "Sound",
        "Whether the keyboard makes sounds.",
        SettingKind::Bool,
    ),
    setting(
        "Sound",
        "via.audio.clicky_enable",
        "Clicks",
        "Whether keys click.",
        SettingKind::Bool,
    ),
];

fn kind_of(key: &str) -> Option<SettingKind> {
    SETTINGS.iter().find(|s| s.key == key).map(|s| s.kind)
}

/// Which byte of the value a key reads: the second for a color's
/// saturation, the first otherwise.
fn byte_index(key: &str) -> usize {
    usize::from(key.ends_with("_sat"))
}

/// A setting's value from the bytes the keyboard gave for it.
pub fn value_of(key: &str, bytes: &[u8]) -> Option<SettingValue> {
    let byte = *bytes.get(byte_index(key))?;
    Some(match kind_of(key)? {
        SettingKind::Bool => SettingValue::Bool(byte != 0),
        SettingKind::Int { .. } => SettingValue::Int(i64::from(byte)),
        _ => return None,
    })
}

/// The bytes to send for a setting's value, with the other byte of a
/// color kept as `current` has it.
pub fn bytes_of(key: &str, value: &SettingValue, current: &[u8]) -> Option<Vec<u8>> {
    let byte = match (kind_of(key)?, value) {
        (SettingKind::Bool, SettingValue::Bool(on)) => u8::from(*on),
        (SettingKind::Int { min, max, .. }, SettingValue::Int(n)) => (*n).clamp(min, max) as u8,
        _ => return None,
    };
    let mut bytes = current.to_vec();
    let index = byte_index(key);
    if bytes.len() <= index {
        return None;
    }
    bytes[index] = byte;
    Some(bytes)
}
