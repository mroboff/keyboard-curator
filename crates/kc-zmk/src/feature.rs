//! Firmware features that not every ZMK build provides.

use serde::{Deserialize, Serialize};

/// Something a firmware profile may or may not support. Catalogue entries
/// name the feature they need; the UI offers only what the selected profile
/// has, and the emitter refuses anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Feature {
    /// ZMK Studio can be enabled.
    Studio,
    /// Stock `&rgb_ug` underglow.
    RgbUnderglow,
    /// MoErgo's `RGB_STATUS` command and `zmk,underglow-indicators` node.
    RgbStatus,
    /// Per-key, per-layer colours through `zmk,underglow-layer`.
    PerKeyLighting,
    /// Single-channel backlight (`&bl`).
    Backlight,
    /// Switchable external power rail (`&ext_power`).
    ExtPower,
    /// Pointing devices, mouse keys and input processors.
    Pointing,
    /// `&out OUT_NONE`, added after ZMK v0.3.
    OutNone,
}
