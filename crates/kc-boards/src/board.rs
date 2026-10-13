//! Board definitions: everything the app needs to know about a keyboard,
//! kept as data so that supporting a new board needs no code.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::geometry::{Key, Spot};

/// A keyboard the app can edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Board {
    /// Stable identifier stored in project files.
    pub id: String,
    pub name: String,
    pub vendor: String,
    /// Whether this definition has been checked on a real keyboard. A board
    /// made from a firmware's own files starts out unverified, and the app
    /// says so where the board is chosen.
    #[serde(default)]
    pub verified: bool,
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
    /// Where the underglow lights are drawn, in the firmware's order, for
    /// keyboards whose underglow is painted light by light.
    #[serde(default)]
    pub underglow: Vec<Spot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Left,
    Right,
    /// A keyboard that is not split: the one unit there is to flash.
    Whole,
}

impl Side {
    /// The side as it appears in file and artifact names.
    pub fn name(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
            Side::Whole => "whole",
        }
    }

    /// The side in a sentence: "the left half", "the keyboard".
    pub fn label(self) -> &'static str {
        match self {
            Side::Left => "left half",
            Side::Right => "right half",
            Side::Whole => "keyboard",
        }
    }

    /// The side as a title: "Left Half", "Keyboard".
    pub fn title(self) -> &'static str {
        match self {
            Side::Left => "Left Half",
            Side::Right => "Right Half",
            Side::Whole => "Keyboard",
        }
    }
}

/// One half of a split keyboard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Half {
    pub side: Side,
    /// The central half holds the keymap and talks to the host.
    pub central: bool,
    /// Volume name the UF2 bootloader mounts as. Left out when the firmware
    /// profile names a controller, which carries it instead.
    #[serde(default)]
    pub bootloader_volume: Option<String>,
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
    /// How to put a half into its bootloader, in the user's terms. Empty
    /// when the controller's own words apply.
    #[serde(default)]
    pub bootloader_entry: String,
    /// How to unlock ZMK Studio on the keyboard, when its firmware is built
    /// with Studio.
    pub studio_unlock: Option<String>,
}

/// A controller board that a shield plugs into: the module that carries
/// the microcontroller and the bootloader. The table is `controllers.toml`
/// beside the boards; a firmware profile names one by ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Controller {
    pub id: String,
    pub name: String,
    /// The ZMK board name a build for it names.
    pub board: String,
    /// The connector it exposes, in ZMK's words: `pro_micro`, `seeed_xiao`.
    pub connector: String,
    /// The volume names its UF2 bootloader mounts as. Some have shipped
    /// with more than one.
    pub bootloader_volumes: Vec<String>,
    pub uf2_family: Option<u32>,
    pub bootloader_entry: String,
    /// What the user should know: clones built as it, what it lacks.
    #[serde(default)]
    pub notes: Vec<String>,
}

/// How one unit of a keyboard is flashed: what its bootloader mounts as,
/// which firmware it accepts and how to reach it, from the half itself or
/// from the controller its firmware profile names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootloaderFacts {
    pub volumes: Vec<String>,
    pub uf2_family: Option<u32>,
    pub entry: String,
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
    /// QMK with VIA: the vendor's firmware stays, and the layout is written
    /// to the running keyboard over VIA's raw HID protocol.
    Via,
}

/// How a layout gets onto a keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Config files are generated, built into firmware and flashed.
    Build,
    /// The configuration is written to the running keyboard; there is no
    /// build.
    Live,
    /// The firmware itself is taken ready-made from the project's releases
    /// and flashed once; the configuration is then written to the running
    /// keyboard, as with [`Delivery::Live`].
    Released,
    /// The firmware stays as it is, whatever it was built from; the layers
    /// and keys are written to the running keyboard over ZMK Studio.
    /// Nothing is built, so nothing the firmware lacks can be added.
    Studio,
}

impl Delivery {
    /// Whether the layout is written to the running keyboard.
    pub fn is_live(self) -> bool {
        matches!(self, Delivery::Live | Delivery::Released | Delivery::Studio)
    }
}

impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Family::Zmk => "ZMK",
            Family::Rmk => "RMK",
            Family::Dygma => "Dygma",
            Family::Via => "VIA",
        }
    }

    /// How the family's firmwares usually reach the keyboard. A profile
    /// can differ: see [`FirmwareProfile::delivery`].
    pub fn delivery(self) -> Delivery {
        match self {
            Family::Zmk | Family::Rmk => Delivery::Build,
            Family::Dygma | Family::Via => Delivery::Live,
        }
    }

    /// What every firmware of the family can do, before a profile adds
    /// its own capabilities.
    pub fn base_features(self) -> &'static [Capability] {
        use Capability::{
            Build, Combos, Devicetree, DygmaKeys, HoldTaps, LayerRules, Macros, ModMorph,
            StickyKeys, TapDance,
        };
        match self {
            Family::Zmk => &[
                Build, Combos, LayerRules, Macros, TapDance, ModMorph, HoldTaps, StickyKeys,
                Devicetree,
            ],
            // Behaviors defined in a layout are not translated to RMK yet.
            Family::Rmk => &[Build, Combos],
            // Dygma's superkeys and macros are kept as the keyboard has
            // them until KEY-89 and KEY-90; its own keys are offered.
            Family::Dygma => &[DygmaKeys],
            // QMK's built-in mod-taps, layer-taps, one-shots and VIA's
            // sixteen macros; anything else needs firmware built.
            Family::Via => &[HoldTaps, StickyKeys, Macros],
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
    /// What an RMK build for this board is made from. RMK only, and
    /// required for it.
    pub rmk: Option<RmkProfile>,
    /// How this board's keys sit in its switch matrix, and what its VIA
    /// definition says it has. VIA only, and required for it.
    #[serde(default)]
    pub via: Option<ViaProfile>,
    /// The controller board the keyboard's shield is built for, by its ID
    /// in the controller table, when the keyboard is a shield. The
    /// bootloader facts then come from the controller.
    #[serde(default)]
    pub controller: Option<String>,
    /// Firmware the app never builds: the keyboard is edited over ZMK
    /// Studio as it is. Such a profile has no source and no builds, and
    /// only the capabilities it lists.
    #[serde(default)]
    pub studio_only: bool,
    #[serde(default)]
    pub builds: Vec<BuildTarget>,
}

/// How an RMK firmware for a board is put together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RmkFlavor {
    /// RMK itself: the app generates the whole project, which is built and
    /// flashed.
    Upstream,
    /// colonelpanic's moergo-rmk, a firmware built on RMK for MoErgo's
    /// boards. It is taken ready-made from the project's releases and
    /// flashed once; the layout is then written to the running keyboard
    /// over Rynk, RMK's host protocol, as the project's own tools do.
    MoergoRmk,
}

/// What an RMK firmware for a board is made from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RmkProfile {
    pub flavor: RmkFlavor,
    /// Where RMK, or the firmware built on it, comes from.
    pub source: Source,
    /// How many Bluetooth profiles the firmware keeps, which decides the
    /// numbering of its profile keys.
    pub ble_profiles: u8,
    /// `keyboard.toml` up to the keymap: the keyboard's identity, matrix,
    /// split halves and input devices. For the upstream flavor.
    #[serde(default)]
    pub hardware: String,
    /// Where the firmware starts in flash, and how much it may take. Kept
    /// to the vendor's own layout, so that the vendor's firmware can be
    /// flashed again. For the upstream flavor.
    #[serde(default)]
    pub flash_origin: u32,
    #[serde(default)]
    pub flash_length: u32,
    /// How many layers the firmware holds. Zero means as many as the
    /// layout has.
    #[serde(default)]
    pub layers: usize,
    /// The RMK commit the firmware was built with. Rynk, its host
    /// protocol, can change in any release, so the app speaks it with a
    /// client pinned to this same commit and checks a release against it.
    /// For the moergo-rmk flavor.
    #[serde(default)]
    pub rmk_revision: String,
    /// Where the ready-made firmware comes from. For the moergo-rmk flavor.
    pub release: Option<Release>,
    /// The key matrix the firmware's configuration is written in, and
    /// where each key of the default layout sits in it. For the moergo-rmk
    /// flavor.
    pub matrix: Option<RmkMatrix>,
    /// The firmware's number for each of the board's pointing devices.
    #[serde(default)]
    pub pointing: Vec<RmkPointingDevice>,
    /// Where each half's LEDs begin in the firmware's numbering, which
    /// follows the halves' LED chains. For the moergo-rmk flavor.
    #[serde(default)]
    pub leds: Vec<RmkLeds>,
    /// The lighting effects and palettes the firmware offers, by the
    /// names it announces them under.
    #[serde(default)]
    pub effects: Vec<String>,
    #[serde(default)]
    pub palettes: Vec<String>,
}

/// A GitHub release holding a board's firmware, pinned to one tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    /// The repository, as `owner/name`.
    pub repository: String,
    pub tag: String,
    /// The release asset holding the firmware, when it is one zip archive
    /// with the UF2 files, a `SHA256SUMS` list and a `manifest.json`.
    /// Without it, each file, `SHA256SUMS` and `manifest.json` are assets
    /// of their own.
    pub asset: Option<String>,
    /// The UF2 file for each half.
    pub files: Vec<ReleaseFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseFile {
    pub side: Side,
    pub name: String,
}

/// The key matrix a firmware's configuration is written in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RmkMatrix {
    pub rows: u8,
    pub cols: u8,
    /// `[row, col]` for each key of the default layout, in binding order.
    pub positions: Vec<[u8; 2]>,
}

/// A pointing device as the firmware numbers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RmkPointingDevice {
    /// The device's input listener label in the board definition.
    pub listener: String,
    pub device: u8,
}

/// Where one half's LEDs begin in the firmware's numbering. The firmware
/// numbers a half's LEDs in chain order from here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RmkLeds {
    pub side: Side,
    pub first: u16,
}

impl RmkMatrix {
    /// The matrix position of a key of the default layout.
    pub fn position(&self, key: usize) -> Option<[u8; 2]> {
        self.positions.get(key).copied()
    }

    /// The key of the default layout at a matrix position, if any.
    pub fn key_at(&self, row: u8, col: u8) -> Option<usize> {
        self.positions.iter().position(|p| *p == [row, col])
    }
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
    /// The light of the default layout's first underglow spot; the rest
    /// follow in order. Without it, the underglow is left as it is.
    #[serde(default)]
    pub underglow_first: Option<usize>,
}

/// How a VIA keyboard's keys map onto its switch matrix, and what its
/// definition says the keyboard has. Taken from the definition file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViaProfile {
    pub rows: u8,
    pub cols: u8,
    /// For each key of the default layout, its `[row, col]`.
    pub matrix: Vec<[u8; 2]>,
    /// The definition's layout options, in its order; choice 0 of each is
    /// what the default layout shows.
    #[serde(default)]
    pub options: Vec<LayoutOption>,
    /// The lighting values the definition's menus reach, as
    /// `[channel, value]` IDs of VIA's custom channels.
    #[serde(default)]
    pub lighting: Vec<[u8; 2]>,
    /// Keycodes the keyboard's own firmware adds, in the order of
    /// `CUSTOM(n)`.
    #[serde(default)]
    pub custom_keycodes: Vec<String>,
}

/// A lighting (or audio) channel QMK exposes through VIA's custom values:
/// its ID, and each value with its name and length in bytes. Facts from
/// QMK's `quantum/via.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViaChannel {
    pub name: &'static str,
    pub id: u8,
    /// `(value ID, name, bytes)`.
    pub values: &'static [(u8, &'static str, usize)],
}

/// QMK's channels: brightness, effect, speed and color where the kind of
/// lighting has them; audio has its two switches.
pub const VIA_CHANNELS: [ViaChannel; 5] = [
    ViaChannel {
        name: "backlight",
        id: 1,
        values: &[(1, "brightness", 1), (2, "effect", 1)],
    },
    ViaChannel {
        name: "rgblight",
        id: 2,
        values: &[
            (1, "brightness", 1),
            (2, "effect", 1),
            (3, "effect_speed", 1),
            (4, "color", 2),
        ],
    },
    ViaChannel {
        name: "rgb_matrix",
        id: 3,
        values: &[
            (1, "brightness", 1),
            (2, "effect", 1),
            (3, "effect_speed", 1),
            (4, "color", 2),
        ],
    },
    ViaChannel {
        name: "audio",
        id: 4,
        values: &[(1, "enable", 1), (2, "clicky_enable", 1)],
    },
    ViaChannel {
        name: "led_matrix",
        id: 5,
        values: &[
            (1, "brightness", 1),
            (2, "effect", 1),
            (3, "effect_speed", 1),
        ],
    },
];

/// The `[channel, value]` IDs a VIA setting key names, such as
/// `via.rgb_matrix.brightness`; a color's hue and saturation share one
/// value.
pub fn via_value(key: &str) -> Option<[u8; 2]> {
    let mut parts = key.strip_prefix("via.")?.splitn(2, '.');
    let (channel, value) = (parts.next()?, parts.next()?);
    let value = value
        .strip_suffix("_hue")
        .or_else(|| value.strip_suffix("_sat"))
        .unwrap_or(value);
    let channel = VIA_CHANNELS.iter().find(|c| c.name == channel)?;
    let (id, _, _) = channel.values.iter().find(|v| v.1 == value)?;
    Some([channel.id, *id])
}

/// One of a VIA keyboard's layout options: a label and its choices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutOption {
    pub label: String,
    pub choices: Vec<String>,
}

impl FirmwareProfile {
    /// Everything this firmware can do: its family's base and its own
    /// capabilities.
    pub fn features(&self) -> Vec<Capability> {
        if self.studio_only {
            return self.capabilities.clone();
        }
        let mut features = self.family.base_features().to_vec();
        for capability in &self.capabilities {
            if !features.contains(capability) {
                features.push(*capability);
            }
        }
        features
    }

    /// How a layout reaches a keyboard running this firmware: the family's
    /// way, unless the firmware is taken from releases and configured live.
    pub fn delivery(&self) -> Delivery {
        if self.studio_only {
            return Delivery::Studio;
        }
        match self.rmk.as_ref().map(|rmk| rmk.flavor) {
            Some(RmkFlavor::MoergoRmk) => Delivery::Released,
            _ => self.family.delivery(),
        }
    }

    /// The LED a key of the default layout sits under, in the firmware's
    /// numbering, for firmware that numbers LEDs by half.
    pub fn led_of(&self, board: &Board, key: usize) -> Option<u16> {
        let rmk = self.rmk.as_ref()?;
        for half in &board.halves {
            let first = rmk.leds.iter().find(|l| l.side == half.side)?.first;
            if let Some(index) = half.leds.as_ref()?.chain.iter().position(|k| *k == key) {
                return Some(first + u16::try_from(index).ok()?);
            }
        }
        None
    }

    /// The key of the default layout under an LED, in the firmware's
    /// numbering.
    pub fn key_of_led(&self, board: &Board, led: u16) -> Option<usize> {
        let rmk = self.rmk.as_ref()?;
        for half in &board.halves {
            let first = rmk.leds.iter().find(|l| l.side == half.side)?.first;
            let chain = &half.leds.as_ref()?.chain;
            if let Some(index) = led.checked_sub(first) {
                if let Some(key) = chain.get(usize::from(index)) {
                    return Some(*key);
                }
            }
        }
        None
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
    #[error("RMK firmware profile `{0}` needs an `rmk` section and a `config_name`")]
    IncompleteRmk(String),
    #[error("VIA firmware profile `{0}` needs a `via` section giving every key of the default layout a different place in the matrix, within its rows and columns")]
    BadVia(String),
    #[error("RMK firmware profile `{profile}`: {problem}")]
    BadRmk {
        profile: String,
        problem: &'static str,
    },
    #[error("starter_keys has {found} entries, but the default layout has {keys} keys")]
    StarterKeyCount { found: usize, keys: usize },
    #[error("starter key `{0}` is not a ZMK keycode")]
    UnknownStarterKey(String),
    #[error("a whole keyboard has no left or right half besides")]
    WholeAndHalves,
    #[error("firmware profile `{profile}` names controller `{controller}`, which the controller table does not have")]
    UnknownController { profile: String, controller: String },
    #[error("firmware profile `{profile}` builds for board `{board}`, but its controller `{controller}` is built as `{expected}`")]
    ControllerMismatch {
        profile: String,
        controller: String,
        board: String,
        expected: String,
    },
    #[error("firmware profile `{profile}` builds for the {side:?} half, which has no bootloader volume and no controller to take one from")]
    NoBootloader { profile: String, side: Side },
}

impl Board {
    /// Parses and validates a board definition.
    pub fn from_toml(src: &str) -> Result<Self, BoardError> {
        let board: Board = toml::from_str(src).map_err(|e| BoardError::Parse(e.to_string()))?;
        board.validate()?;
        Ok(board)
    }

    /// A board built in code, checked the way a board file is.
    pub fn checked(self) -> Result<Self, BoardError> {
        self.validate()?;
        Ok(self)
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

    /// A board for a keyboard the app has no file for, from what the
    /// keyboard reports over ZMK Studio: its name, its layouts with the
    /// keys in binding order, and which layout it is using. The keyboard
    /// is edited as it is; its firmware is never built here.
    pub fn from_studio(
        name: &str,
        usb: Option<UsbId>,
        layouts: &[(String, Vec<Key>)],
        active: usize,
    ) -> Result<Self, BoardError> {
        let slug: String = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let layouts: Vec<PhysicalLayout> = layouts
            .iter()
            .enumerate()
            .map(|(index, (name, keys))| PhysicalLayout {
                id: format!("layout_{index}"),
                name: if name.trim().is_empty() {
                    format!("Layout {}", index + 1)
                } else {
                    name.clone()
                },
                keys: keys.clone(),
                underglow: Vec::new(),
            })
            .collect();
        let board = Board {
            id: format!("studio-{slug}"),
            name: name.to_string(),
            vendor: "ZMK Studio".into(),
            verified: false,
            brightness_cap: 100,
            default_layout: format!("layout_{}", active.min(layouts.len().saturating_sub(1))),
            starter_keys: Vec::new(),
            usb: usb.into_iter().collect(),
            flash: FlashInfo {
                order: Vec::new(),
                bootloader_entry: String::new(),
                studio_unlock: None,
            },
            halves: vec![Half {
                side: Side::Whole,
                central: true,
                bootloader_volume: None,
                uf2_family: None,
                leds: None,
            }],
            pointing: Vec::new(),
            firmware: vec![FirmwareProfile {
                id: "studio".into(),
                name: "The firmware on the keyboard, over ZMK Studio".into(),
                family: Family::Zmk,
                experimental: false,
                notes: vec![
                    "The keyboard's firmware stays as it is. Layers and keys are written to it over ZMK Studio; combos, new behaviors, settings and lighting would need firmware built for the keyboard, which needs a board file.".into(),
                ],
                zephyr: String::new(),
                config_name: String::new(),
                workflow: String::new(),
                zmk: None,
                modules: Vec::new(),
                capabilities: vec![Capability::Studio],
                lighting: None,
                dygma: None,
                rmk: None,
                via: None,
                controller: None,
                studio_only: true,
                builds: Vec::new(),
            }],
            layouts,
        };
        board.validate()?;
        Ok(board)
    }

    /// The controller a firmware profile is built for, when it names one.
    pub fn controller(profile: &FirmwareProfile) -> Option<&'static Controller> {
        profile.controller.as_deref().and_then(crate::controller)
    }

    /// How the `side` unit is flashed when it runs `profile`: the half's
    /// own bootloader facts, or failing those the profile's controller's.
    pub fn bootloader(&self, profile: &FirmwareProfile, side: Side) -> Option<BootloaderFacts> {
        let half = self.half(side)?;
        let controller = Self::controller(profile);
        let volumes = match (&half.bootloader_volume, controller) {
            (Some(volume), _) => vec![volume.clone()],
            (None, Some(controller)) => controller.bootloader_volumes.clone(),
            (None, None) => return None,
        };
        Some(BootloaderFacts {
            volumes,
            uf2_family: half
                .uf2_family
                .or_else(|| controller.and_then(|c| c.uf2_family)),
            entry: self.bootloader_entry(profile),
        })
    }

    /// How to put the keyboard into its bootloader: the board's own words,
    /// or failing those the controller's.
    pub fn bootloader_entry(&self, profile: &FirmwareProfile) -> String {
        if !self.flash.bootloader_entry.is_empty() {
            return self.flash.bootloader_entry.clone();
        }
        Self::controller(profile).map_or_else(String::new, |c| c.bootloader_entry.clone())
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
        if sides.contains(&Side::Whole) && sides.len() > 1 {
            return Err(BoardError::WholeAndHalves);
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
                && !profile.studio_only
                && (profile.zmk.is_none()
                    || profile.config_name.is_empty()
                    || profile.workflow.is_empty()
                    || profile.builds.is_empty())
            {
                return Err(BoardError::IncompleteZmk(profile.id.clone()));
            }
            if profile.family == Family::Rmk {
                let sound = !profile.config_name.is_empty()
                    && profile.rmk.as_ref().is_some_and(|rmk| match rmk.flavor {
                        RmkFlavor::Upstream => {
                            !rmk.hardware.trim().is_empty()
                                && rmk.flash_origin > 0
                                && rmk.flash_length > 0
                        }
                        RmkFlavor::MoergoRmk => !rmk.rmk_revision.is_empty(),
                    });
                if !sound {
                    return Err(BoardError::IncompleteRmk(profile.id.clone()));
                }
                if let Some(rmk) = profile
                    .rmk
                    .as_ref()
                    .filter(|r| r.flavor == RmkFlavor::MoergoRmk)
                {
                    self.check_moergo_rmk(profile, rmk)?;
                }
            }
            if profile.family == Family::Dygma {
                let (keys, spots) = self
                    .layout(&self.default_layout)
                    .map_or((0, 0), |l| (l.keys.len(), l.underglow.len()));
                let sound = profile.dygma.as_ref().is_some_and(|d| {
                    let distinct = |list: &[usize], limit: usize| {
                        list.len() == keys
                            && list.iter().all(|n| *n < limit)
                            && list.iter().collect::<HashSet<_>>().len() == keys
                    };
                    // The underglow lights fit after the keys' and overlap
                    // none of them.
                    let underglow = d.underglow_first.is_none_or(|first| {
                        first + spots <= d.leds
                            && !d
                                .key_leds
                                .iter()
                                .any(|led| (first..first + spots).contains(led))
                    });
                    d.layers > 0
                        && d.palette > 0
                        && distinct(&d.key_slots, d.slots)
                        && distinct(&d.key_leds, d.leds)
                        && underglow
                });
                if !sound {
                    return Err(BoardError::BadDygma(profile.id.clone()));
                }
            }
            if profile.family == Family::Via {
                let keys = self
                    .layout(&self.default_layout)
                    .map_or(0, |l| l.keys.len());
                let sound = profile.via.as_ref().is_some_and(|via| {
                    via.matrix.len() == keys
                        && via
                            .matrix
                            .iter()
                            .all(|[r, c]| *r < via.rows && *c < via.cols)
                        && via.matrix.iter().collect::<HashSet<_>>().len() == keys
                });
                if !sound {
                    return Err(BoardError::BadVia(profile.id.clone()));
                }
            }
            let controller =
                match &profile.controller {
                    Some(id) => Some(crate::controller(id).ok_or_else(|| {
                        BoardError::UnknownController {
                            profile: profile.id.clone(),
                            controller: id.clone(),
                        }
                    })?),
                    None => None,
                };
            for build in &profile.builds {
                if !has(build.side) {
                    return Err(BoardError::UnknownHalf {
                        what: format!("firmware profile `{}`", profile.id),
                        side: build.side,
                    });
                }
                if let Some(controller) = controller {
                    if build.board != controller.board {
                        return Err(BoardError::ControllerMismatch {
                            profile: profile.id.clone(),
                            controller: controller.id.clone(),
                            board: build.board.clone(),
                            expected: controller.board.clone(),
                        });
                    }
                }
                if self.bootloader(profile, build.side).is_none() {
                    return Err(BoardError::NoBootloader {
                        profile: profile.id.clone(),
                        side: build.side,
                    });
                }
            }
        }
        Ok(())
    }

    /// A firmware taken from releases and configured over Rynk needs the
    /// data that ties the layout to the keyboard: the matrix each key sits
    /// in, a release with a file per half, and the halves' LED numbering.
    fn check_moergo_rmk(
        &self,
        profile: &FirmwareProfile,
        rmk: &RmkProfile,
    ) -> Result<(), BoardError> {
        let bad = |problem: &'static str| BoardError::BadRmk {
            profile: profile.id.clone(),
            problem,
        };
        let keys = self
            .layout(&self.default_layout)
            .map_or(0, |l| l.keys.len());
        let matrix = rmk
            .matrix
            .as_ref()
            .ok_or_else(|| bad("needs a `matrix` with every key's position"))?;
        if matrix.positions.len() != keys {
            return Err(bad(
                "the matrix positions must cover every key of the default layout",
            ));
        }
        if matrix
            .positions
            .iter()
            .any(|[row, col]| *row >= matrix.rows || *col >= matrix.cols)
        {
            return Err(bad("a matrix position is outside the matrix"));
        }
        if matrix.positions.iter().collect::<HashSet<_>>().len() != keys {
            return Err(bad("two keys share a matrix position"));
        }
        let release = rmk
            .release
            .as_ref()
            .ok_or_else(|| bad("needs a `release` to take the firmware from"))?;
        if release.repository.split('/').count() != 2
            || release.tag.is_empty()
            || release.asset.as_ref().is_some_and(String::is_empty)
        {
            return Err(bad(
                "the release needs an `owner/name` repository and a tag",
            ));
        }
        for half in &self.halves {
            if release.files.iter().filter(|f| f.side == half.side).count() != 1 {
                return Err(bad("the release needs exactly one file for each half"));
            }
            if rmk.leds.iter().filter(|l| l.side == half.side).count() != 1 {
                return Err(bad("`leds` must say where each half's LEDs begin"));
            }
        }
        for device in &rmk.pointing {
            if !self.pointing.iter().any(|p| p.listener == device.listener) {
                return Err(bad(
                    "a pointing device names a listener the board does not have",
                ));
            }
        }
        Ok(())
    }
}
