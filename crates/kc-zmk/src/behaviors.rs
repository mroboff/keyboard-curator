//! ZMK's built-in behaviors and the parameters each one takes.
//!
//! The key picker, the key inspector and the emitter are all driven by this
//! table. User-defined behaviors (hold-taps, macros and so on) live in the
//! project model, not here.

use crate::feature::Feature;

/// A named constant accepted as a parameter, such as `LCLK` or `MOVE_UP`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Constant {
    pub name: &'static str,
    pub description: &'static str,
}

/// A numeric argument to a command, such as the profile in `BT_SEL 2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arg {
    pub name: &'static str,
    pub min: u32,
    pub max: u32,
}

/// How a command's arguments are written in a keymap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallStyle {
    /// `BT_SEL 2`
    Spaced,
    /// `RGB_COLOR_HSB(120,100,50)`
    Function,
}

/// One command of a command-style behavior such as `&bt` or `&rgb_ug`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    pub name: &'static str,
    pub description: &'static str,
    pub args: &'static [Arg],
    pub style: CallStyle,
    /// The firmware feature this command needs beyond its behavior's own.
    pub requires: Option<Feature>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// A keycode, optionally wrapped in modifier functions.
    Keycode,
    /// A layer of the keymap.
    Layer,
    /// One of a fixed set of named constants.
    Constant(&'static [Constant]),
    /// One of a fixed set of commands, each with its own arguments.
    Command(&'static [Command]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Param {
    pub name: &'static str,
    pub kind: ParamKind,
}

/// Where a behavior appears in the key picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Keys,
    Layers,
    Mouse,
    Connectivity,
    Lighting,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Behavior {
    /// The devicetree label, written as `&label` in a keymap.
    pub label: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub params: &'static [Param],
    pub group: Group,
    pub requires: Option<Feature>,
}

const fn command(name: &'static str, description: &'static str) -> Command {
    Command {
        name,
        description,
        args: &[],
        style: CallStyle::Spaced,
        requires: None,
    }
}

const KEY: Param = Param {
    name: "key",
    kind: ParamKind::Keycode,
};
const LAYER: Param = Param {
    name: "layer",
    kind: ParamKind::Layer,
};

const MOUSE_BUTTONS: &[Constant] = &[
    Constant {
        name: "LCLK",
        description: "Left click",
    },
    Constant {
        name: "RCLK",
        description: "Right click",
    },
    Constant {
        name: "MCLK",
        description: "Middle click",
    },
    Constant {
        name: "MB4",
        description: "Mouse button 4 (back)",
    },
    Constant {
        name: "MB5",
        description: "Mouse button 5 (forward)",
    },
];

const MOVES: &[Constant] = &[
    Constant {
        name: "MOVE_UP",
        description: "Move the pointer up",
    },
    Constant {
        name: "MOVE_DOWN",
        description: "Move the pointer down",
    },
    Constant {
        name: "MOVE_LEFT",
        description: "Move the pointer left",
    },
    Constant {
        name: "MOVE_RIGHT",
        description: "Move the pointer right",
    },
];

const SCROLLS: &[Constant] = &[
    Constant {
        name: "SCRL_UP",
        description: "Scroll up",
    },
    Constant {
        name: "SCRL_DOWN",
        description: "Scroll down",
    },
    Constant {
        name: "SCRL_LEFT",
        description: "Scroll left",
    },
    Constant {
        name: "SCRL_RIGHT",
        description: "Scroll right",
    },
];

const PROFILE: &[Arg] = &[Arg {
    name: "profile",
    min: 0,
    max: 4,
}];

const BT_COMMANDS: &[Command] = &[
    Command {
        args: PROFILE,
        ..command("BT_SEL", "Switch to a Bluetooth profile")
    },
    command("BT_NXT", "Switch to the next Bluetooth profile"),
    command("BT_PRV", "Switch to the previous Bluetooth profile"),
    command("BT_CLR", "Forget the host paired to the current profile"),
    command("BT_CLR_ALL", "Forget the hosts paired to every profile"),
    Command {
        args: PROFILE,
        ..command("BT_DISC", "Disconnect a profile without forgetting it")
    },
];

const OUT_COMMANDS: &[Command] = &[
    command("OUT_USB", "Send keys over USB"),
    command("OUT_BLE", "Send keys over Bluetooth"),
    command("OUT_TOG", "Switch between USB and Bluetooth"),
    Command {
        requires: Some(Feature::OutNone),
        ..command("OUT_NONE", "Send keys nowhere")
    },
];

const RGB_COMMANDS: &[Command] = &[
    command("RGB_TOG", "Turn the lighting on or off"),
    command("RGB_ON", "Turn the lighting on"),
    command("RGB_OFF", "Turn the lighting off"),
    command("RGB_HUI", "Increase hue"),
    command("RGB_HUD", "Decrease hue"),
    command("RGB_SAI", "Increase saturation"),
    command("RGB_SAD", "Decrease saturation"),
    command("RGB_BRI", "Increase brightness"),
    command("RGB_BRD", "Decrease brightness"),
    command("RGB_SPI", "Speed the effect up"),
    command("RGB_SPD", "Slow the effect down"),
    command("RGB_EFF", "Next effect"),
    command("RGB_EFR", "Previous effect"),
    Command {
        args: &[
            Arg {
                name: "hue",
                min: 0,
                max: 360,
            },
            Arg {
                name: "saturation",
                min: 0,
                max: 100,
            },
            Arg {
                name: "brightness",
                min: 0,
                max: 100,
            },
        ],
        style: CallStyle::Function,
        ..command("RGB_COLOR_HSB", "Set a specific color")
    },
    Command {
        requires: Some(Feature::RgbStatus),
        ..command(
            "RGB_STATUS",
            "Show battery, layer and connection status on the LEDs",
        )
    },
];

const BL_COMMANDS: &[Command] = &[
    command("BL_TOG", "Turn the backlight on or off"),
    command("BL_ON", "Turn the backlight on"),
    command("BL_OFF", "Turn the backlight off"),
    command("BL_INC", "Increase backlight brightness"),
    command("BL_DEC", "Decrease backlight brightness"),
    command("BL_CYCLE", "Step through backlight brightness levels"),
    Command {
        args: &[Arg {
            name: "brightness",
            min: 0,
            max: 100,
        }],
        ..command("BL_SET", "Set a specific backlight brightness")
    },
];

const EP_COMMANDS: &[Command] = &[
    command("EP_ON", "Turn external power on"),
    command("EP_OFF", "Turn external power off"),
    command("EP_TOG", "Toggle external power"),
];

const fn behavior(
    label: &'static str,
    name: &'static str,
    description: &'static str,
    params: &'static [Param],
    group: Group,
) -> Behavior {
    Behavior {
        label,
        name,
        description,
        params,
        group,
        requires: None,
    }
}

const fn commands(kind: &'static [Command]) -> [Param; 1] {
    [Param {
        name: "command",
        kind: ParamKind::Command(kind),
    }]
}

const fn constants(name: &'static str, kind: &'static [Constant]) -> [Param; 1] {
    [Param {
        name,
        kind: ParamKind::Constant(kind),
    }]
}

/// Every behavior ZMK v0.3.0 defines out of the box.
pub const BUILT_IN: &[Behavior] = &[
    behavior(
        "kp",
        "Key press",
        "Send a key while held",
        &[KEY],
        Group::Keys,
    ),
    behavior(
        "kt",
        "Key toggle",
        "Press a key and keep it held until tapped again",
        &[KEY],
        Group::Keys,
    ),
    behavior(
        "sk",
        "Sticky key",
        "Hold a key until the next key is pressed",
        &[KEY],
        Group::Keys,
    ),
    behavior(
        "mt",
        "Mod-tap",
        "A modifier when held, a key when tapped",
        &[
            Param {
                name: "hold",
                kind: ParamKind::Keycode,
            },
            Param {
                name: "tap",
                kind: ParamKind::Keycode,
            },
        ],
        Group::Keys,
    ),
    behavior(
        "gresc",
        "Grave escape",
        "Escape, or grave/tilde with Shift or Gui held",
        &[],
        Group::Keys,
    ),
    behavior(
        "caps_word",
        "Caps word",
        "Capitalize letters until a non-word key is pressed",
        &[],
        Group::Keys,
    ),
    behavior(
        "key_repeat",
        "Key repeat",
        "Send the last key again",
        &[],
        Group::Keys,
    ),
    behavior(
        "trans",
        "Transparent",
        "Use the binding from the next active layer down",
        &[],
        Group::Keys,
    ),
    behavior(
        "none",
        "None",
        "Do nothing, and block lower layers",
        &[],
        Group::Keys,
    ),
    behavior(
        "mo",
        "Momentary layer",
        "Activate a layer while held",
        &[LAYER],
        Group::Layers,
    ),
    behavior(
        "lt",
        "Layer-tap",
        "A layer when held, a key when tapped",
        &[
            Param {
                name: "hold",
                kind: ParamKind::Layer,
            },
            Param {
                name: "tap",
                kind: ParamKind::Keycode,
            },
        ],
        Group::Layers,
    ),
    behavior(
        "to",
        "To layer",
        "Switch to a layer and turn the others off",
        &[LAYER],
        Group::Layers,
    ),
    behavior(
        "tog",
        "Toggle layer",
        "Turn a layer on or off",
        &[LAYER],
        Group::Layers,
    ),
    // From the zmk-auto-layer add-on, when a board's build includes it.
    Behavior {
        requires: Some(Feature::AutoLayer),
        ..behavior(
            "num_word",
            "Num word",
            "Turn a layer on until something that is not a number is typed",
            &[LAYER],
            Group::Layers,
        )
    },
    behavior(
        "sl",
        "Sticky layer",
        "Activate a layer for the next key press",
        &[LAYER],
        Group::Layers,
    ),
    Behavior {
        requires: Some(Feature::Pointing),
        ..behavior(
            "mkp",
            "Mouse button",
            "Press a mouse button",
            &constants("button", MOUSE_BUTTONS),
            Group::Mouse,
        )
    },
    Behavior {
        requires: Some(Feature::Pointing),
        ..behavior(
            "mmv",
            "Mouse move",
            "Move the pointer while held",
            &constants("direction", MOVES),
            Group::Mouse,
        )
    },
    Behavior {
        requires: Some(Feature::Pointing),
        ..behavior(
            "msc",
            "Mouse scroll",
            "Scroll while held",
            &constants("direction", SCROLLS),
            Group::Mouse,
        )
    },
    behavior(
        "bt",
        "Bluetooth",
        "Manage Bluetooth profiles",
        &commands(BT_COMMANDS),
        Group::Connectivity,
    ),
    behavior(
        "out",
        "Output",
        "Choose USB or Bluetooth output",
        &commands(OUT_COMMANDS),
        Group::Connectivity,
    ),
    Behavior {
        requires: Some(Feature::RgbUnderglow),
        ..behavior(
            "rgb_ug",
            "RGB lighting",
            "Control the RGB lighting",
            &commands(RGB_COMMANDS),
            Group::Lighting,
        )
    },
    Behavior {
        requires: Some(Feature::Backlight),
        ..behavior(
            "bl",
            "Backlight",
            "Control the backlight",
            &commands(BL_COMMANDS),
            Group::Lighting,
        )
    },
    Behavior {
        requires: Some(Feature::ExtPower),
        ..behavior(
            "ext_power",
            "External power",
            "Switch power to LEDs and other peripherals",
            &commands(EP_COMMANDS),
            Group::System,
        )
    },
    behavior(
        "sys_reset",
        "Reset",
        "Restart the keyboard half this key is on",
        &[],
        Group::System,
    ),
    behavior(
        "bootloader",
        "Bootloader",
        "Restart this half into its bootloader for flashing",
        &[],
        Group::System,
    ),
    behavior(
        "soft_off",
        "Soft off",
        "Power the keyboard off until woken",
        &[],
        Group::System,
    ),
    Behavior {
        requires: Some(Feature::Studio),
        ..behavior(
            "studio_unlock",
            "Studio unlock",
            "Allow ZMK Studio to make changes",
            &[],
            Group::System,
        )
    },
];

/// Looks a built-in behavior up by its label (without the `&`).
pub fn built_in(label: &str) -> Option<&'static Behavior> {
    BUILT_IN.iter().find(|b| b.label == label)
}

impl Behavior {
    /// Whether a firmware with `features` can use this behavior.
    pub fn available(&self, features: &[Feature]) -> bool {
        self.requires.is_none_or(|f| features.contains(&f))
    }
}

impl Command {
    pub fn available(&self, features: &[Feature]) -> bool {
        self.requires.is_none_or(|f| features.contains(&f))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Names ZMK v0.3.0 defines in the headers behaviors take constants from.
    fn header_defines() -> Vec<&'static str> {
        [
            include_str!("../vendor/zmk-v0.3.0/bt.h"),
            include_str!("../vendor/zmk-v0.3.0/outputs.h"),
            include_str!("../vendor/zmk-v0.3.0/rgb.h"),
            include_str!("../vendor/zmk-v0.3.0/backlight.h"),
            include_str!("../vendor/zmk-v0.3.0/ext_power.h"),
            include_str!("../vendor/zmk-v0.3.0/pointing.h"),
        ]
        .iter()
        .flat_map(|h| h.lines())
        .filter_map(|l| {
            l.strip_prefix("#define ")?
                .split(|c: char| c == '(' || c.is_whitespace())
                .next()
        })
        .collect()
    }

    #[test]
    fn every_constant_and_command_exists_in_zmk() {
        let defines = header_defines();
        for behavior in BUILT_IN {
            for param in behavior.params {
                match param.kind {
                    ParamKind::Constant(constants) => {
                        for c in constants {
                            assert!(defines.contains(&c.name), "{} is not a ZMK define", c.name);
                        }
                    }
                    ParamKind::Command(commands) => {
                        for c in commands {
                            // Commands gated on a feature are extensions that
                            // stock v0.3.0 headers do not define.
                            assert_eq!(
                                defines.contains(&c.name),
                                c.requires.is_none(),
                                "{}",
                                c.name
                            );
                        }
                    }
                    ParamKind::Keycode | ParamKind::Layer => {}
                }
            }
        }
    }

    #[test]
    fn labels_are_unique_and_looked_up() {
        let mut labels: Vec<_> = BUILT_IN.iter().map(|b| b.label).collect();
        labels.sort_unstable();
        let before = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), before);
        assert_eq!(built_in("lt").unwrap().params.len(), 2);
        assert!(built_in("missing").is_none());
    }

    #[test]
    fn availability_follows_firmware_features() {
        let stock = [Feature::RgbUnderglow, Feature::Pointing];
        let rgb = built_in("rgb_ug").unwrap();
        assert!(rgb.available(&stock));
        assert!(!built_in("bl").unwrap().available(&stock));
        assert!(built_in("kp").unwrap().available(&[]));

        let ParamKind::Command(commands) = rgb.params[0].kind else {
            panic!("rgb_ug takes a command");
        };
        let status = commands.iter().find(|c| c.name == "RGB_STATUS").unwrap();
        assert!(!status.available(&stock));
        assert!(status.available(&[Feature::RgbStatus]));
    }
}
