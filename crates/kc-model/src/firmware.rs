//! A board's firmware configuration: which firmware it runs and how that
//! firmware is set up. It belongs to the board, never to a layout, so one
//! board can have many layouts and changing its firmware touches none of
//! them.

use std::collections::BTreeMap;

use kc_boards::Board;
use kc_zmk::settings::setting_for;
use kc_zmk::Feature;
use serde::{Deserialize, Serialize};

use crate::features::SettingValue;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FirmwareConfig {
    /// The firmware profile, by its ID in the board definition.
    pub profile: String,
    /// Kconfig options that differ from the board's defaults.
    #[serde(default)]
    pub settings: BTreeMap<String, SettingValue>,
    /// Extra lines for the `.conf` file, for anything the settings do not
    /// cover.
    #[serde(default)]
    pub raw_conf: String,
}

impl FirmwareConfig {
    /// The firmware `profile` with every setting at its default.
    pub fn new(profile: impl Into<String>) -> Self {
        Self {
            profile: profile.into(),
            settings: BTreeMap::new(),
            raw_conf: String::new(),
        }
    }

    /// The board's first firmware with every setting at its default.
    pub fn stock(board: &Board) -> Self {
        Self::new(board.firmware[0].id.clone())
    }

    /// What the chosen firmware can do; nothing when the board does not
    /// have it.
    pub fn features<'a>(&self, board: &'a Board) -> &'a [Feature] {
        board
            .profile(&self.profile)
            .map_or(&[], |p| p.capabilities.as_slice())
    }

    /// Whether the firmware has the feature a setting needs. Settings the
    /// firmware lacks are kept, out of sight and out of the `.conf` file.
    pub fn offers(&self, key: &str, board: &Board) -> bool {
        setting_for(key)
            .and_then(|setting| setting.requires)
            .is_none_or(|feature| self.features(board).contains(&feature))
    }

    /// Takes on settings found in an imported keymap or an old layout file.
    /// They replace the board's own where both set the same option.
    pub fn absorb(&mut self, carried: Carried) {
        self.settings.extend(carried.settings);
        let extra = carried.raw_conf.trim();
        if !extra.is_empty() && !self.raw_conf.contains(extra) {
            if !self.raw_conf.trim().is_empty() {
                self.raw_conf.push('\n');
            }
            self.raw_conf.push_str(extra);
            self.raw_conf.push('\n');
        }
    }
}

/// Firmware settings that arrived with a layout: from the `.conf` beside an
/// imported keymap, or from a layout file written before settings moved to
/// the board. The user decides whether the board takes them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Carried {
    pub settings: BTreeMap<String, SettingValue>,
    pub raw_conf: String,
}

impl Carried {
    pub fn is_empty(&self) -> bool {
        self.settings.is_empty() && self.raw_conf.trim().is_empty()
    }

    /// What is carried, in words: the names of the settings, then how many
    /// custom lines.
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = self
            .settings
            .keys()
            .map(|key| {
                setting_for(key)
                    .map_or(key.as_str(), |s| s.name)
                    .to_string()
            })
            .collect();
        match self
            .raw_conf
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count()
        {
            0 => {}
            1 => parts.push("1 custom line".into()),
            n => parts.push(format!("{n} custom lines")),
        }
        parts.join(", ")
    }
}
