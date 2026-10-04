//! The eight HID modifiers and ZMK's modifier wrapper functions.

use serde::{Deserialize, Serialize};

/// A keyboard modifier. `LC(...)`, `LS(...)` and friends wrap a keycode so
/// that it is sent with the modifier held; they nest freely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Modifier {
    LCtrl,
    LShift,
    LAlt,
    LGui,
    RCtrl,
    RShift,
    RAlt,
    RGui,
}

impl Modifier {
    pub const ALL: [Modifier; 8] = [
        Modifier::LCtrl,
        Modifier::LShift,
        Modifier::LAlt,
        Modifier::LGui,
        Modifier::RCtrl,
        Modifier::RShift,
        Modifier::RAlt,
        Modifier::RGui,
    ];

    /// The wrapper function name, as in `LC(A)`.
    pub fn function(self) -> &'static str {
        match self {
            Modifier::LCtrl => "LC",
            Modifier::LShift => "LS",
            Modifier::LAlt => "LA",
            Modifier::LGui => "LG",
            Modifier::RCtrl => "RC",
            Modifier::RShift => "RS",
            Modifier::RAlt => "RA",
            Modifier::RGui => "RG",
        }
    }

    /// The flag name used in `mods` properties, as in `MOD_LSFT`.
    pub fn flag(self) -> &'static str {
        match self {
            Modifier::LCtrl => "MOD_LCTL",
            Modifier::LShift => "MOD_LSFT",
            Modifier::LAlt => "MOD_LALT",
            Modifier::LGui => "MOD_LGUI",
            Modifier::RCtrl => "MOD_RCTL",
            Modifier::RShift => "MOD_RSFT",
            Modifier::RAlt => "MOD_RALT",
            Modifier::RGui => "MOD_RGUI",
        }
    }

    /// The modifier's bit in ZMK's modifier byte.
    pub fn bit(self) -> u8 {
        1 << Modifier::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }

    pub fn from_function(name: &str) -> Option<Self> {
        Modifier::ALL.into_iter().find(|m| m.function() == name)
    }

    pub fn from_flag(name: &str) -> Option<Self> {
        Modifier::ALL.into_iter().find(|m| m.flag() == name)
    }
}
