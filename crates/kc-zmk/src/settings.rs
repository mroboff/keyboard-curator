//! The firmware settings (Kconfig options) the app offers as typed controls.
//!
//! Anything not listed here can still be set as a raw `.conf` line.

use crate::feature::Feature;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    Bool,
    Int { min: i64, max: i64, step: i64 },
    Text { max_len: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    /// The Kconfig symbol, such as `CONFIG_ZMK_SLEEP`.
    pub key: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub kind: SettingKind,
    pub group: &'static str,
    pub requires: Option<Feature>,
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

const fn rgb(
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

use SettingKind::Bool;

/// The settings that limit LED brightness. Boards cap these.
pub const BRIGHTNESS_SETTINGS: [&str; 2] = [
    "CONFIG_ZMK_RGB_UNDERGLOW_BRT_MAX",
    "CONFIG_ZMK_RGB_UNDERGLOW_BRT_START",
];

pub const SETTINGS: &[Setting] = &[
    setting(
        "Keyboard",
        "CONFIG_ZMK_KEYBOARD_NAME",
        "Keyboard name",
        "The name shown when pairing over Bluetooth.",
        SettingKind::Text { max_len: 16 },
    ),
    setting(
        "Bluetooth",
        "CONFIG_BT_CTLR_TX_PWR_PLUS_8",
        "Stronger signal",
        "Transmit at higher power. Helps a weak connection, at some cost in battery life.",
        Bool,
    ),
    setting(
        "Bluetooth",
        "CONFIG_ZMK_BLE_EXPERIMENTAL_CONN",
        "Faster reconnection",
        "Use ZMK's experimental connection settings, which can improve reconnecting.",
        Bool,
    ),
    setting(
        "Bluetooth",
        "CONFIG_ZMK_BLE_PASSKEY_ENTRY",
        "Require a passkey",
        "Ask for a code typed on the keyboard when pairing.",
        Bool,
    ),
    setting(
        "Power",
        "CONFIG_ZMK_SLEEP",
        "Deep sleep",
        "Sleep after a period without typing. A key press wakes the keyboard.",
        Bool,
    ),
    setting(
        "Power",
        "CONFIG_ZMK_IDLE_TIMEOUT",
        "Idle after (ms)",
        "How long without typing before the keyboard is idle and dims its lights.",
        int(5_000, 3_600_000, 5_000),
    ),
    setting(
        "Power",
        "CONFIG_ZMK_IDLE_SLEEP_TIMEOUT",
        "Sleep after (ms)",
        "How long without typing before deep sleep.",
        int(60_000, 7_200_000, 60_000),
    ),
    setting(
        "Power",
        "CONFIG_ZMK_BATTERY_REPORT_INTERVAL",
        "Battery report interval (s)",
        "How often the battery level is measured and reported.",
        int(10, 600, 10),
    ),
    setting(
        "USB",
        "CONFIG_ZMK_HID_REPORT_TYPE_NKRO",
        "N-key rollover",
        "Report any number of keys at once. A few hosts do not accept it.",
        Bool,
    ),
    setting(
        "USB",
        "CONFIG_ZMK_HID_INDICATORS",
        "Lock indicators",
        "Receive Caps Lock and similar states from the host.",
        Bool,
    ),
    setting(
        "Typing",
        "CONFIG_ZMK_KSCAN_DEBOUNCE_PRESS_MS",
        "Press debounce (ms)",
        "How long a key must be down before it counts as pressed.",
        int(0, 50, 1),
    ),
    setting(
        "Typing",
        "CONFIG_ZMK_KSCAN_DEBOUNCE_RELEASE_MS",
        "Release debounce (ms)",
        "How long a key must be up before it counts as released.",
        int(0, 50, 1),
    ),
    setting(
        "Typing",
        "CONFIG_ZMK_BEHAVIORS_QUEUE_SIZE",
        "Macro queue size",
        "How many queued actions a macro can hold. Long macros need more.",
        int(16, 1024, 16),
    ),
    setting(
        "Combos",
        "CONFIG_ZMK_COMBO_MAX_PRESSED_COMBOS",
        "Combos held at once",
        "The most combos that can be active together.",
        int(1, 16, 1),
    ),
    setting(
        "Combos",
        "CONFIG_ZMK_COMBO_MAX_COMBOS_PER_KEY",
        "Combos per key",
        "The most combos one key can take part in.",
        int(1, 32, 1),
    ),
    setting(
        "Combos",
        "CONFIG_ZMK_COMBO_MAX_KEYS_PER_COMBO",
        "Keys per combo",
        "The most keys one combo can use.",
        int(2, 16, 1),
    ),
    setting(
        "Split",
        "CONFIG_ZMK_SPLIT_BLE_CENTRAL_BATTERY_LEVEL_FETCHING",
        "Report the other half's battery",
        "Let the main half read the other half's battery level.",
        Bool,
    ),
    setting(
        "Split",
        "CONFIG_ZMK_SPLIT_PERIPHERAL_HID_INDICATORS",
        "Lock indicators on the other half",
        "Pass Caps Lock and similar states to the other half.",
        Bool,
    ),
    rgb(
        "CONFIG_ZMK_RGB_UNDERGLOW_ON_START",
        "On at power-up",
        "Turn the lights on when the keyboard starts.",
        Bool,
    ),
    rgb(
        "CONFIG_ZMK_RGB_UNDERGLOW_AUTO_OFF_IDLE",
        "Off when idle",
        "Turn the lights off when the keyboard is idle.",
        Bool,
    ),
    rgb(
        "CONFIG_ZMK_RGB_UNDERGLOW_AUTO_OFF_USB",
        "Off without USB",
        "Turn the lights off when USB is unplugged.",
        Bool,
    ),
    rgb(
        "CONFIG_ZMK_RGB_UNDERGLOW_BRT_MAX",
        "Maximum brightness (%)",
        "The brightest the lights can be set.",
        int(0, 100, 5),
    ),
    rgb(
        "CONFIG_ZMK_RGB_UNDERGLOW_BRT_START",
        "Starting brightness (%)",
        "Brightness at power-up.",
        int(0, 100, 1),
    ),
    rgb(
        "CONFIG_ZMK_RGB_UNDERGLOW_BRT_STEP",
        "Brightness step (%)",
        "How much each brightness key press changes.",
        int(1, 50, 1),
    ),
    rgb(
        "CONFIG_ZMK_RGB_UNDERGLOW_HUE_START",
        "Starting hue",
        "Color at power-up, in degrees around the color wheel.",
        int(0, 359, 5),
    ),
    rgb(
        "CONFIG_ZMK_RGB_UNDERGLOW_SAT_START",
        "Starting saturation (%)",
        "Color intensity at power-up.",
        int(0, 100, 5),
    ),
    rgb(
        "CONFIG_ZMK_RGB_UNDERGLOW_EFF_START",
        "Starting effect",
        "Effect at power-up: 0 solid, 1 breathe, 2 spectrum, 3 swirl.",
        int(0, 3, 1),
    ),
    rgb(
        "CONFIG_ZMK_RGB_UNDERGLOW_SPD_START",
        "Starting effect speed",
        "Animation speed at power-up.",
        int(1, 5, 1),
    ),
    Setting {
        requires: Some(Feature::Pointing),
        ..setting(
            "Pointing",
            "CONFIG_ZMK_POINTING_SMOOTH_SCROLLING",
            "Smooth scrolling",
            "Send high-resolution scroll steps.",
            Bool,
        )
    },
    Setting {
        requires: Some(Feature::Studio),
        ..setting(
            "ZMK Studio",
            "CONFIG_ZMK_STUDIO_LOCKING",
            "Require unlocking",
            "Keep ZMK Studio locked until the unlock key is pressed.",
            Bool,
        )
    },
    // Settings of add-ons, offered once the add-on is in the build.
    Setting {
        requires: Some(Feature::LeaderKey),
        ..setting(
            "Add-ons",
            "CONFIG_ZMK_LEADER_MAX_KEYS_PER_SEQUENCE",
            "Leader key: longest sequence",
            "The most keys one leader sequence can have.",
            int(1, 16, 1),
        )
    },
    Setting {
        requires: Some(Feature::LeaderKey),
        ..setting(
            "Add-ons",
            "CONFIG_ZMK_LEADER_MAX_SEQUENCES",
            "Leader key: most sequences",
            "The most sequences one leader key can have.",
            int(1, 64, 1),
        )
    },
    Setting {
        requires: Some(Feature::AdaptiveKey),
        ..setting(
            "Add-ons",
            "CONFIG_ZMK_ADAPTIVE_KEY_MAX_TRIGGER_CONDITIONS",
            "Adaptive keys: most triggers",
            "The most trigger conditions one adaptive key can have.",
            int(1, 64, 1),
        )
    },
    Setting {
        requires: Some(Feature::AdaptiveKey),
        ..setting(
            "Add-ons",
            "CONFIG_ZMK_ADAPTIVE_KEY_MAX_BINDINGS",
            "Adaptive keys: most bindings",
            "The most bindings one trigger of an adaptive key can have.",
            int(1, 16, 1),
        )
    },
];

pub fn setting_for(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_well_formed() {
        let mut keys: Vec<&str> = SETTINGS.iter().map(|s| s.key).collect();
        assert!(keys.iter().all(|k| k.starts_with("CONFIG_")));
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "no duplicate settings");
        for setting in SETTINGS {
            if let SettingKind::Int { min, max, step } = setting.kind {
                assert!(min < max && step > 0, "{}", setting.key);
            }
        }
        assert!(BRIGHTNESS_SETTINGS.iter().all(|k| setting_for(k).is_some()));
        assert!(setting_for("CONFIG_NOPE").is_none());
    }
}
