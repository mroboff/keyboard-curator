//! Firmware features that not every ZMK build provides.

use serde::{Deserialize, Serialize};

/// Something a firmware profile may or may not support. Catalog entries
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
    /// Per-key, per-layer colors through `zmk,underglow-layer`.
    PerKeyLighting,
    /// Single-channel backlight (`&bl`).
    Backlight,
    /// Switchable external power rail (`&ext_power`).
    ExtPower,
    /// Pointing devices, mouse keys and input processors.
    Pointing,
    /// `&out OUT_NONE`, added after ZMK v0.3.
    OutNone,
    /// Combos: several keys pressed together doing something else.
    Combos,
    /// Layers that switch on while a set of other layers is active.
    LayerRules,
    /// Macros defined by the user.
    Macros,
    /// Tap-dances: a key that does different things by tap count.
    TapDance,
    /// Mod-morphs: a key that changes while modifiers are held.
    ModMorph,
    /// Hold-taps with their own timing and flavor.
    HoldTaps,
    /// Sticky keys and layers with their own settings.
    StickyKeys,
    /// Devicetree text carried into the generated keymap as it is.
    Devicetree,
    /// The firmware is built from generated files, which can be shown.
    Build,
    /// `&num_word` and auto layers, from the zmk-auto-layer add-on.
    AutoLayer,
    /// Leader keys, from the zmk-leader-key add-on.
    LeaderKey,
    /// Adaptive keys, from the zmk-adaptive-key add-on.
    AdaptiveKey,
}
