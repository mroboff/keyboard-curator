//! The settings of Dygma's firmware, as the board page offers them: typing
//! and superkey timing, mouse keys, lights, idle behavior and the wireless
//! parts. They belong to the board, keyed `dygma.<section>.<name>`, and
//! each stands for one Focus command. Reading a keyboard carries them to
//! the board; applying writes those that differ from what the keyboard
//! holds, and only the commands the keyboard lists.
//!
//! The commands, ranges and meanings are facts from Dygma's Focus API
//! document and the Preferences screen of Bazecor (src/renderer/views/
//! Preferences.tsx and modules/Settings/KeyboardSettings.tsx).

use kc_model::features::SettingValue;
use kc_zmk::settings::{Setting, SettingKind};

/// A setting and the Focus command it stands for.
pub struct Mapping {
    pub key: &'static str,
    pub command: &'static str,
}

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

pub const LED_MODES: &[&str] = &[
    "Layer colors",
    "Rainbow wave",
    "Rainbow",
    "Stalker",
    "Heatmap",
    "Digital rain",
    "Sprinkling water",
];

pub const RF_POWERS: &[&str] = &["Low", "Medium", "High"];

/// Every setting, with its Focus command, in the order the board page
/// shows them.
pub const MAPPINGS: &[Mapping] = &[
    Mapping {
        key: "dygma.keymap.default_layer",
        command: "settings.defaultLayer",
    },
    Mapping {
        key: "dygma.keymap.only_custom",
        command: "keymap.onlyCustom",
    },
    Mapping {
        key: "dygma.typing.hold_timeout",
        command: "qukeys.holdTimeout",
    },
    Mapping {
        key: "dygma.typing.overlap",
        command: "qukeys.overlapThreshold",
    },
    Mapping {
        key: "dygma.typing.minimum_hold",
        command: "qukeys.minimumHoldTime",
    },
    Mapping {
        key: "dygma.typing.minimum_prior",
        command: "qukeys.minimumPriorInterval",
    },
    Mapping {
        key: "dygma.superkeys.hold_start",
        command: "superkeys.holdstart",
    },
    Mapping {
        key: "dygma.superkeys.timeout",
        command: "superkeys.timeout",
    },
    Mapping {
        key: "dygma.superkeys.overlap",
        command: "superkeys.overlap",
    },
    Mapping {
        key: "dygma.superkeys.wait_for",
        command: "superkeys.waitfor",
    },
    Mapping {
        key: "dygma.superkeys.repeat",
        command: "superkeys.repeat",
    },
    Mapping {
        key: "dygma.combos.timeout",
        command: "chords.timeout",
    },
    Mapping {
        key: "dygma.mouse.speed",
        command: "mouse.speed",
    },
    Mapping {
        key: "dygma.mouse.acceleration",
        command: "mouse.accelSpeed",
    },
    Mapping {
        key: "dygma.mouse.speed_limit",
        command: "mouse.speedLimit",
    },
    Mapping {
        key: "dygma.mouse.wheel_speed",
        command: "mouse.wheelSpeed",
    },
    Mapping {
        key: "dygma.lights.mode",
        command: "led.mode",
    },
    Mapping {
        key: "dygma.lights.brightness",
        command: "led.brightness",
    },
    Mapping {
        key: "dygma.lights.underglow_brightness",
        command: "led.brightnessUG",
    },
    Mapping {
        key: "dygma.lights.brightness_wireless",
        command: "led.brightness.wireless",
    },
    Mapping {
        key: "dygma.lights.underglow_brightness_wireless",
        command: "led.brightnessUG.wireless",
    },
    Mapping {
        key: "dygma.idle.time_limit",
        command: "idleleds.time_limit",
    },
    Mapping {
        key: "dygma.idle.time_limit_wireless",
        command: "idleleds.wireless",
    },
    Mapping {
        key: "dygma.idle.true_sleep",
        command: "idleleds.true_sleep",
    },
    Mapping {
        key: "dygma.idle.true_sleep_time",
        command: "idleleds.true_sleep_time",
    },
    Mapping {
        key: "dygma.wireless.battery_saving",
        command: "wireless.battery.savingMode",
    },
    Mapping {
        key: "dygma.wireless.rf_power",
        command: "wireless.rf.power",
    },
    Mapping {
        key: "dygma.wireless.rf_channel_hop",
        command: "wireless.rf.channelHop",
    },
    Mapping {
        key: "dygma.wireless.bluetooth_name",
        command: "wireless.bluetooth.deviceName",
    },
];

/// The settings, grouped as the board page shows them. Same order as
/// [`MAPPINGS`].
pub const SETTINGS: &[Setting] = &[
    setting(
        "Keyboard",
        "dygma.keymap.default_layer",
        "Layer on start",
        "The layer the keyboard is on when it powers up, counted from 1.",
        int(1, 10, 1),
    ),
    setting(
        "Keyboard",
        "dygma.keymap.only_custom",
        "Hide the factory layers",
        "Keep the two layers Dygma ships out of the way, so only the layout's layers are reachable.",
        SettingKind::Bool,
    ),
    setting(
        "Typing",
        "dygma.typing.hold_timeout",
        "Counts as a hold after",
        "How long a key with a hold must be down to hold, in milliseconds. Every such key shares it.",
        int(1, 255, 5),
    ),
    setting(
        "Typing",
        "dygma.typing.overlap",
        "Overlap allowed while typing",
        "How much of the next key's press may overlap a key with a hold before it counts as a hold, in percent.",
        int(0, 100, 5),
    ),
    setting(
        "Typing",
        "dygma.typing.minimum_hold",
        "Shortest hold",
        "A key with a hold released sooner than this, in milliseconds, is always a tap.",
        int(1, 254, 5),
    ),
    setting(
        "Typing",
        "dygma.typing.minimum_prior",
        "No hold right after typing",
        "A key pressed within this many milliseconds of the previous key is always a tap.",
        int(1, 254, 5),
    ),
    setting(
        "Superkeys",
        "dygma.superkeys.hold_start",
        "Superkey counts as a hold after",
        "How long a superkey must be down to hold, in milliseconds.",
        int(120, 500, 10),
    ),
    setting(
        "Superkeys",
        "dygma.superkeys.timeout",
        "Time allowed between a superkey's taps",
        "How long the keyboard waits for another tap before acting, in milliseconds.",
        int(1, 500, 10),
    ),
    setting(
        "Superkeys",
        "dygma.superkeys.overlap",
        "Superkey overlap while typing",
        "How much of the next key's press may overlap a superkey before it counts as a hold, in percent.",
        int(0, 100, 5),
    ),
    setting(
        "Superkeys",
        "dygma.superkeys.wait_for",
        "Hold repeats after",
        "How long a held superkey waits before its hold action repeats, in milliseconds.",
        int(0, 5000, 50),
    ),
    setting(
        "Superkeys",
        "dygma.superkeys.repeat",
        "Repeat every",
        "The interval between repeats of a held superkey's action, in milliseconds.",
        int(0, 1000, 10),
    ),
    // The Curator firmware's `chords.timeout`: offered only where the
    // board's firmware has combos, and sent only where the keyboard lists
    // the command.
    Setting {
        group: "Combos",
        key: "dygma.combos.timeout",
        name: "Combo timeout",
        description: "How long the keys of a combo may arrive apart and still count as pressed together, in milliseconds. A combo with a timeout of its own uses that instead.",
        kind: int(10, 1000, 5),
        requires: Some(kc_zmk::Feature::Combos),
    },
    setting(
        "Mouse keys",
        "dygma.mouse.speed",
        "Pointer speed",
        "How far the pointer moves for each step.",
        int(0, 127, 1),
    ),
    setting(
        "Mouse keys",
        "dygma.mouse.acceleration",
        "Acceleration",
        "How much the pointer speeds up while a mouse key is held.",
        int(0, 254, 2),
    ),
    setting(
        "Mouse keys",
        "dygma.mouse.speed_limit",
        "Top speed",
        "The pointer's speed limit under acceleration.",
        int(0, 255, 5),
    ),
    setting(
        "Mouse keys",
        "dygma.mouse.wheel_speed",
        "Wheel speed",
        "How far a scroll key scrolls for each step.",
        int(1, 15, 1),
    ),
    setting(
        "Lights",
        "dygma.lights.mode",
        "Effect",
        "What the lights show: the layout's per-layer colors, or one of the firmware's animations.",
        SettingKind::Choice { options: LED_MODES },
    ),
    setting(
        "Lights",
        "dygma.lights.brightness",
        "Key brightness",
        "How bright the keys' lights are over USB, from 0 to 255.",
        int(0, 255, 5),
    ),
    setting(
        "Lights",
        "dygma.lights.underglow_brightness",
        "Underglow brightness",
        "How bright the underglow is over USB, from 0 to 255.",
        int(0, 255, 5),
    ),
    setting(
        "Lights",
        "dygma.lights.brightness_wireless",
        "Key brightness, wireless",
        "How bright the keys' lights are on battery, from 0 to 255.",
        int(0, 255, 5),
    ),
    setting(
        "Lights",
        "dygma.lights.underglow_brightness_wireless",
        "Underglow brightness, wireless",
        "How bright the underglow is on battery, from 0 to 255.",
        int(0, 255, 5),
    ),
    setting(
        "Idle",
        "dygma.idle.time_limit",
        "Lights off after",
        "Seconds without a key press before the lights go out over USB; 0 keeps them on.",
        int(0, 65000, 30),
    ),
    setting(
        "Idle",
        "dygma.idle.time_limit_wireless",
        "Lights off after, wireless",
        "Seconds without a key press before the lights go out on battery; 0 keeps them on.",
        int(0, 65000, 30),
    ),
    setting(
        "Idle",
        "dygma.idle.true_sleep",
        "Deep sleep",
        "Let the keyboard sleep deeply after the lights have gone out, to save the battery.",
        SettingKind::Bool,
    ),
    setting(
        "Idle",
        "dygma.idle.true_sleep_time",
        "Deep sleep after",
        "Seconds after the lights go out before deep sleep begins.",
        int(0, 65000, 30),
    ),
    setting(
        "Wireless",
        "dygma.wireless.battery_saving",
        "Battery saving",
        "Spend less power at some cost in response. Wireless keyboards only.",
        SettingKind::Bool,
    ),
    setting(
        "Wireless",
        "dygma.wireless.rf_power",
        "Radio power",
        "The strength of the radio link between the halves and the Neuron. Wireless keyboards only.",
        SettingKind::Choice { options: RF_POWERS },
    ),
    setting(
        "Wireless",
        "dygma.wireless.rf_channel_hop",
        "Radio channel hopping",
        "Let the radio link change channel to avoid interference. Wireless keyboards only.",
        SettingKind::Bool,
    ),
    setting(
        "Wireless",
        "dygma.wireless.bluetooth_name",
        "Bluetooth name",
        "The name the keyboard shows when pairing. Wireless keyboards only.",
        SettingKind::Text { max_len: 32 },
    ),
];

/// The Focus command a setting stands for.
pub fn command_of(key: &str) -> Option<&'static str> {
    MAPPINGS.iter().find(|m| m.key == key).map(|m| m.command)
}

/// The setting a Focus command stands for.
pub fn key_of(command: &str) -> Option<&'static str> {
    MAPPINGS
        .iter()
        .find(|m| m.command == command)
        .map(|m| m.key)
}

/// Whether a command is one of the settings'.
pub fn is_command(command: &str) -> bool {
    key_of(command).is_some()
}

fn kind_of(key: &str) -> Option<&'static SettingKind> {
    SETTINGS.iter().find(|s| s.key == key).map(|s| &s.kind)
}

/// A setting's value from the text the keyboard answered with.
pub fn value_of(key: &str, text: &str) -> Option<SettingValue> {
    let text = text.trim();
    Some(match kind_of(key)? {
        SettingKind::Bool => SettingValue::Bool(matches!(text, "1" | "true")),
        SettingKind::Int { .. } => {
            let number: i64 = text.parse().ok()?;
            // The keyboard counts layers from 0; people count from 1.
            if key == "dygma.keymap.default_layer" {
                SettingValue::Int(number + 1)
            } else {
                SettingValue::Int(number)
            }
        }
        SettingKind::Choice { options } => {
            let index: usize = text.parse().ok()?;
            SettingValue::Text((*options.get(index)?).to_string())
        }
        SettingKind::Text { .. } => SettingValue::Text(text.to_string()),
        _ => return None,
    })
}

/// The text to send the keyboard for a setting's value, written the way
/// the keyboard wrote `current` (words or digits for a switch).
pub fn text_of(key: &str, value: &SettingValue, current: &str) -> Option<String> {
    Some(match (kind_of(key)?, value) {
        (SettingKind::Bool, SettingValue::Bool(on)) => {
            let words = matches!(current.trim(), "true" | "false");
            match (on, words) {
                (true, true) => "true".into(),
                (false, true) => "false".into(),
                (true, false) => "1".into(),
                (false, false) => "0".into(),
            }
        }
        (SettingKind::Int { min, max, .. }, SettingValue::Int(n)) => {
            let n = (*n).clamp(*min, *max);
            if key == "dygma.keymap.default_layer" {
                (n - 1).to_string()
            } else {
                n.to_string()
            }
        }
        (SettingKind::Choice { options }, SettingValue::Text(name)) => {
            options.iter().position(|o| o == name)?.to_string()
        }
        (SettingKind::Text { max_len }, SettingValue::Text(text)) => {
            text.chars().take(*max_len).collect()
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_has_a_command_and_values_round_trip() {
        for setting in SETTINGS {
            let command = command_of(setting.key).expect(setting.key);
            assert_eq!(key_of(command), Some(setting.key));
        }
        assert_eq!(MAPPINGS.len(), SETTINGS.len());
        assert_eq!(
            value_of("dygma.keymap.default_layer", "0"),
            Some(SettingValue::Int(1))
        );
        assert_eq!(
            text_of("dygma.keymap.default_layer", &SettingValue::Int(3), "0"),
            Some("2".into())
        );
        assert_eq!(
            value_of("dygma.lights.mode", "3"),
            Some(SettingValue::Text("Stalker".into()))
        );
        assert_eq!(
            text_of(
                "dygma.lights.mode",
                &SettingValue::Text("Heatmap".into()),
                "0"
            ),
            Some("4".into())
        );
        assert_eq!(
            value_of("dygma.wireless.rf_channel_hop", "false"),
            Some(SettingValue::Bool(false))
        );
        assert_eq!(
            text_of(
                "dygma.wireless.rf_channel_hop",
                &SettingValue::Bool(true),
                "false"
            ),
            Some("true".into())
        );
        assert_eq!(
            text_of("dygma.idle.true_sleep", &SettingValue::Bool(true), "0"),
            Some("1".into())
        );
    }
}
