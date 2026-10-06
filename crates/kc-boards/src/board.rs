//! Board definitions: everything the app needs to know about a keyboard,
//! kept as data so that supporting a new board needs no code.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::geometry::Key;

/// A keyboard the app can edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Board {
    /// Stable identifier stored in project files.
    pub id: String,
    pub name: String,
    pub vendor: String,
    /// Highest LED brightness (percent) the firmware may be configured for.
    pub brightness_cap: u8,
    /// The layout a new project starts with.
    pub default_layout: String,
    /// Keycode names for the base layer of a new project, one per key of
    /// the default layout; an empty name leaves the key transparent.
    #[serde(default)]
    pub starter_keys: Vec<String>,
    /// How the keyboard identifies itself over USB, for recognizing one
    /// that is connected.
    #[serde(default)]
    pub usb: Vec<UsbId>,
    pub flash: FlashInfo,
    pub halves: Vec<Half>,
    #[serde(default)]
    pub pointing: Vec<PointingDevice>,
    pub firmware: Vec<FirmwareProfile>,
    pub layouts: Vec<PhysicalLayout>,
}

/// What a board's stock firmware reports over USB. The IDs are often shared
/// between boards, and the name changes if the user renames the keyboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsbId {
    pub vendor: u16,
    pub product: u16,
    /// The USB product name.
    pub name: String,
}

/// One arrangement of keys. Boards with variants have several.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalLayout {
    /// The devicetree label of the `zmk,physical-layout` node.
    pub id: String,
    pub name: String,
    /// Keys in binding order.
    pub keys: Vec<Key>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Left,
    Right,
}

/// One half of a split keyboard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Half {
    pub side: Side,
    /// The central half holds the keymap and talks to the host.
    pub central: bool,
    /// Volume name the UF2 bootloader mounts as.
    pub bootloader_volume: String,
    /// The UF2 family ID this half's bootloader accepts, when the vendor
    /// uses a different one per half. Lets a wrong-half file be caught.
    pub uf2_family: Option<u32>,
    pub leds: Option<LedMap>,
}

/// Which key each LED on a half's chain sits under.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedMap {
    /// The layout whose key positions `chain` refers to.
    pub layout: String,
    /// False until the order has been confirmed on real hardware.
    pub verified: bool,
    /// Key position for each LED, in chain order.
    pub chain: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PointingKind {
    Trackball,
    Touchpad,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointingDevice {
    pub name: String,
    pub kind: PointingKind,
    pub side: Side,
    /// The devicetree label of the device's input listener.
    pub listener: String,
    /// Where to draw the device on the canvas: its center and its width, in
    /// layout units. Schematic, like the key layout itself.
    pub x: i32,
    pub y: i32,
    pub size: i32,
}

/// How firmware gets onto the board.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlashInfo {
    /// The order the vendor recommends flashing the halves in.
    pub order: Vec<Side>,
    /// How to put a half into its bootloader, in the user's terms.
    pub bootloader_entry: String,
    /// How to unlock ZMK Studio on the keyboard, when its firmware is built
    /// with Studio.
    pub studio_unlock: Option<String>,
}

/// A feature a firmware profile provides.
pub use kc_zmk::Feature as Capability;

/// A git repository pinned to a revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub url: String,
    pub revision: String,
}

/// An extra west module the firmware needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    pub name: String,
    pub url: String,
    pub revision: String,
    /// A west manifest inside the module to import, if it has dependencies.
    pub import: Option<String>,
}

/// One firmware image to build.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildTarget {
    pub side: Side,
    pub board: String,
    pub shield: Option<String>,
    #[serde(default)]
    pub snippets: Vec<String>,
    #[serde(default)]
    pub cmake_args: Vec<String>,
}

/// A firmware family: one way of putting a layout on a keyboard, with its
/// own vocabulary, settings and tools. A family is code; the firmwares
/// within it are data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Family {
    #[default]
    Zmk,
    Rmk,
    Dygma,
}

/// How a layout gets onto a keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Config files are generated, built into firmware and flashed.
    Build,
    /// The configuration is written to the running keyboard; there is no
    /// build.
    Live,
}

impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Family::Zmk => "ZMK",
            Family::Rmk => "RMK",
            Family::Dygma => "Dygma",
        }
    }

    pub fn delivery(self) -> Delivery {
        match self {
            Family::Zmk | Family::Rmk => Delivery::Build,
            Family::Dygma => Delivery::Live,
        }
    }

    /// What every firmware of the family can do, before a profile adds
    /// its own capabilities.
    pub fn base_features(self) -> &'static [Capability] {
        use Capability::{
            Build, Combos, Devicetree, HoldTaps, LayerRules, Macros, ModMorph, StickyKeys, TapDance,
        };
        match self {
            Family::Zmk => &[
                Build, Combos, LayerRules, Macros, TapDance, ModMorph, HoldTaps, StickyKeys,
                Devicetree,
            ],
            Family::Rmk => &[Build, Combos, Macros, TapDance],
            // Dygma's superkeys and macros are kept as the keyboard has
            // them, and are not edited yet.
            Family::Dygma => &[],
        }
    }
}

/// A firmware a board can run: where it comes from and what it can do.
/// Firmware differences within a family are expressed here as data, never
/// as code branches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirmwareProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub family: Family,
    /// Not yet proven on hardware, or missing things the board's usual
    /// firmware has. The app says so wherever the firmware is chosen.
    #[serde(default)]
    pub experimental: bool,
    /// What the user should know before choosing this firmware: what it
    /// lacks on this board, and any risk.
    #[serde(default)]
    pub notes: Vec<String>,
    /// Zephyr generation, for example `3.5`. ZMK only.
    #[serde(default)]
    pub zephyr: String,
    /// The base name of the keymap and `.conf` files in a zmk-config
    /// repository, which ZMK matches to the board or shield. For other
    /// build families, the base name of the firmware files.
    #[serde(default)]
    pub config_name: String,
    /// The reusable GitHub Actions workflow that builds this firmware.
    /// ZMK only.
    #[serde(default)]
    pub workflow: String,
    /// Where ZMK comes from. ZMK only, and required for it.
    pub zmk: Option<Source>,
    #[serde(default)]
    pub modules: Vec<Module>,
    /// What this firmware can do beyond its family's base.
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    /// How per-key lighting is written for this firmware, when it has it.
    pub lighting: Option<LightingBackend>,
    /// How this board's keys and lights are laid out in Dygma's firmware.
    /// Dygma only, and required for it.
    pub dygma: Option<DygmaProfile>,
    #[serde(default)]
    pub builds: Vec<BuildTarget>,
}

/// Where a board's keys and lights sit in Dygma's firmware, which stores a
/// keymap as a fixed grid of slots and colors as palette entries per light.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DygmaProfile {
    /// How many layers the keyboard holds. Always all of them.
    pub layers: usize,
    /// Slots per layer in the keymap grid, used or not.
    pub slots: usize,
    /// Colors in the palette.
    pub palette: usize,
    /// Whether palette colors carry a white channel after red, green, blue.
    pub rgbw: bool,
    /// Lights per layer in the color map: those under keys, then underglow.
    pub leds: usize,
    /// For each key of the default layout, its slot in the keymap grid.
    pub key_slots: Vec<usize>,
    /// For each key of the default layout, the light under it.
    pub key_leds: Vec<usize>,
}

impl FirmwareProfile {
    /// Everything this firmware can do: its family's base and its own
    /// capabilities.
    pub fn features(&self) -> Vec<Capability> {
        let mut features = self.family.base_features().to_vec();
        for capability in &self.capabilities {
            if !features.contains(capability) {
                features.push(*capability);
            }
        }
        features
    }
}

/// The `zmk,underglow-layer` implementation a firmware carries. Versions
/// of it differ, and upstream ZMK intends to replace it, so what varies is
/// described here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingBackend {
    /// Whether `&trans` shows the color from the layer below. Without
    /// it, a key that inherits is written as unlit.
    pub transparent: bool,
    /// The number of the per-key effect, when the firmware lets the
    /// keyboard start in it. Where it does not, the user reaches the
    /// effect once with the next-effect key and the keyboard remembers.
    pub start_effect: Option<u8>,
    /// Whether the generated config must tell the firmware which key each
    /// LED sits under, from this board definition's LED chains. False when
    /// the firmware's own board files already do.
    #[serde(default)]
    pub led_map_overlay: bool,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum BoardError {
    #[error("board definition is not valid TOML: {0}")]
    Parse(String),
    #[error("layout `{0}` is defined more than once")]
    DuplicateLayout(String),
    #[error("layout `{0}` has no keys")]
    EmptyLayout(String),
    #[error("`{field}` refers to layout `{layout}`, which does not exist")]
    UnknownLayout { field: &'static str, layout: String },
    #[error("a board needs exactly one central half, found {0}")]
    CentralCount(usize),
    #[error("the {0:?} half is defined more than once")]
    DuplicateHalf(Side),
    #[error("brightness cap {0} is above 100 percent")]
    BrightnessCap(u8),
    #[error("LED map for the {side:?} half points at key {position}, but layout `{layout}` has {keys} keys")]
    LedOutOfRange {
        side: Side,
        position: usize,
        layout: String,
        keys: usize,
    },
    #[error("key {0} has more than one LED mapped to it")]
    LedDuplicate(usize),
    #[error("{what} refers to the {side:?} half, which the board does not have")]
    UnknownHalf { what: String, side: Side },
    #[error("firmware profile `{0}` is defined more than once")]
    DuplicateProfile(String),
    #[error("a board needs at least one firmware profile")]
    NoProfiles,
    #[error("firmware profile `{0}` must have both the per-key-lighting capability and a lighting section, or neither")]
    LightingMismatch(String),
    #[error(
        "ZMK firmware profile `{0}` needs `zmk`, `config_name`, `workflow` and at least one build"
    )]
    IncompleteZmk(String),
    #[error("Dygma firmware profile `{0}` needs a `dygma` section giving every key of the default layout a different slot and light, within range")]
    BadDygma(String),
    #[error("starter_keys has {found} entries, but the default layout has {keys} keys")]
    StarterKeyCount { found: usize, keys: usize },
    #[error("starter key `{0}` is not a ZMK keycode")]
    UnknownStarterKey(String),
}

impl Board {
    /// Parses and validates a board definition.
    pub fn from_toml(src: &str) -> Result<Self, BoardError> {
        let board: Board = toml::from_str(src).map_err(|e| BoardError::Parse(e.to_string()))?;
        board.validate()?;
        Ok(board)
    }

    pub fn layout(&self, id: &str) -> Option<&PhysicalLayout> {
        self.layouts.iter().find(|l| l.id == id)
    }

    pub fn half(&self, side: Side) -> Option<&Half> {
        self.halves.iter().find(|h| h.side == side)
    }

    pub fn profile(&self, id: &str) -> Option<&FirmwareProfile> {
        self.firmware.iter().find(|p| p.id == id)
    }

    /// The families this board has firmware for, in the order they first
    /// appear.
    pub fn families(&self) -> Vec<Family> {
        let mut families = Vec::new();
        for profile in &self.firmware {
            if !families.contains(&profile.family) {
                families.push(profile.family);
            }
        }
        families
    }

    fn validate(&self) -> Result<(), BoardError> {
        if self.brightness_cap > 100 {
            return Err(BoardError::BrightnessCap(self.brightness_cap));
        }

        let mut layout_ids = HashSet::new();
        for layout in &self.layouts {
            if !layout_ids.insert(layout.id.as_str()) {
                return Err(BoardError::DuplicateLayout(layout.id.clone()));
            }
            if layout.keys.is_empty() {
                return Err(BoardError::EmptyLayout(layout.id.clone()));
            }
        }
        if self.layout(&self.default_layout).is_none() {
            return Err(BoardError::UnknownLayout {
                field: "default_layout",
                layout: self.default_layout.clone(),
            });
        }

        if let Some(layout) = self.layout(&self.default_layout) {
            if !self.starter_keys.is_empty() && self.starter_keys.len() != layout.keys.len() {
                return Err(BoardError::StarterKeyCount {
                    found: self.starter_keys.len(),
                    keys: layout.keys.len(),
                });
            }
        }
        let codes = kc_zmk::keycodes::keycodes();
        if let Some(unknown) = self
            .starter_keys
            .iter()
            .find(|k| !k.is_empty() && codes.get(k).is_none())
        {
            return Err(BoardError::UnknownStarterKey(unknown.clone()));
        }

        let mut sides = HashSet::new();
        for half in &self.halves {
            if !sides.insert(half.side) {
                return Err(BoardError::DuplicateHalf(half.side));
            }
        }
        let centrals = self.halves.iter().filter(|h| h.central).count();
        if centrals != 1 {
            return Err(BoardError::CentralCount(centrals));
        }

        let mut lit = HashSet::new();
        for half in &self.halves {
            let Some(leds) = &half.leds else { continue };
            let layout = self.layout(&leds.layout).ok_or(BoardError::UnknownLayout {
                field: "leds.layout",
                layout: leds.layout.clone(),
            })?;
            for &position in &leds.chain {
                if position >= layout.keys.len() {
                    return Err(BoardError::LedOutOfRange {
                        side: half.side,
                        position,
                        layout: layout.id.clone(),
                        keys: layout.keys.len(),
                    });
                }
                if !lit.insert(position) {
                    return Err(BoardError::LedDuplicate(position));
                }
            }
        }

        let has = |side| sides.contains(&side);
        for device in &self.pointing {
            if !has(device.side) {
                return Err(BoardError::UnknownHalf {
                    what: format!("pointing device `{}`", device.name),
                    side: device.side,
                });
            }
        }
        for &side in &self.flash.order {
            if !has(side) {
                return Err(BoardError::UnknownHalf {
                    what: "flash order".into(),
                    side,
                });
            }
        }

        if self.firmware.is_empty() {
            return Err(BoardError::NoProfiles);
        }
        let mut profile_ids = HashSet::new();
        for profile in &self.firmware {
            if !profile_ids.insert(profile.id.as_str()) {
                return Err(BoardError::DuplicateProfile(profile.id.clone()));
            }
            let capable = profile.capabilities.contains(&Capability::PerKeyLighting);
            // ZMK lighting needs its back end described; other families
            // carry their own lighting model.
            if profile.family == Family::Zmk && capable != profile.lighting.is_some() {
                return Err(BoardError::LightingMismatch(profile.id.clone()));
            }
            if profile.family == Family::Zmk
                && (profile.zmk.is_none()
                    || profile.config_name.is_empty()
                    || profile.workflow.is_empty()
                    || profile.builds.is_empty())
            {
                return Err(BoardError::IncompleteZmk(profile.id.clone()));
            }
            if profile.family == Family::Dygma {
                let keys = self
                    .layout(&self.default_layout)
                    .map_or(0, |l| l.keys.len());
                let sound = profile.dygma.as_ref().is_some_and(|d| {
                    let distinct = |list: &[usize], limit: usize| {
                        list.len() == keys
                            && list.iter().all(|n| *n < limit)
                            && list.iter().collect::<HashSet<_>>().len() == keys
                    };
                    d.layers > 0
                        && d.palette > 0
                        && distinct(&d.key_slots, d.slots)
                        && distinct(&d.key_leds, d.leds)
                });
                if !sound {
                    return Err(BoardError::BadDygma(profile.id.clone()));
                }
            }
            for build in &profile.builds {
                if !has(build.side) {
                    return Err(BoardError::UnknownHalf {
                        what: format!("firmware profile `{}`", profile.id),
                        side: build.side,
                    });
                }
            }
        }
        Ok(())
    }
}
