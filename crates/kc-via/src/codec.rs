//! Key bindings to and from QMK's 16-bit keycodes, as a VIA keyboard
//! stores them.
//!
//! The ranges are facts from QMK's `quantum/keycodes.h` (GPL-2.0), as
//! numbers: basic keys are HID keyboard usages, media and system keys
//! have QMK's own numbers (mapped to their HID usages after
//! `tmk_core/protocol/report.h`), and everything else sits in ranges
//! above 0x0100. Keycodes this app has no binding for are kept as they
//! are, as a raw binding carrying the number, and written back unchanged.

use kc_model::{BehaviorRef, Binding, KeyExpr, LayerId, Param};
use kc_zmk::keycodes::{keycodes, UsagePage};
use kc_zmk::Modifier;

pub const NONE: u16 = 0x0000;
pub const TRANSPARENT: u16 = 0x0001;

const MOD_TAP: u16 = 0x2000;
const LAYER_TAP: u16 = 0x4000;
const LAYER_MOD: u16 = 0x5000;
const TO: u16 = 0x5200;
const MOMENTARY: u16 = 0x5220;
const DEFAULT_LAYER: u16 = 0x5240;
const TOGGLE_LAYER: u16 = 0x5260;
const ONE_SHOT_LAYER: u16 = 0x5280;
const ONE_SHOT_MOD: u16 = 0x52A0;
const LAYER_TAP_TOGGLE: u16 = 0x52C0;
const PERSISTENT_DEFAULT_LAYER: u16 = 0x52E0;
/// Layer keys from `TO` on each have room for this many layers.
const LAYER_KEYS: u16 = 32;
/// Layer-taps reach only the first sixteen layers.
const TAP_LAYERS: u16 = 16;
pub const MACRO: u16 = 0x7700;
pub const MACROS: u16 = 128;
const BACKLIGHT: u16 = 0x7800;
const UNDERGLOW: u16 = 0x7820;
const RGB_MATRIX: u16 = 0x7840;
const BOOTLOADER: u16 = 0x7C00;
const REBOOT: u16 = 0x7C01;
const CAPS_WORD: u16 = 0x7C73;
const REPEAT_KEY: u16 = 0x7C79;
const CUSTOM: u16 = 0x7E00;
const CUSTOM_END: u16 = 0x7E40;

/// Modifier bits in the modified, mod-tap, layer-mod and one-shot ranges.
const CTRL: u16 = 0x01;
const SHIFT: u16 = 0x02;
const ALT: u16 = 0x04;
const GUI: u16 = 0x08;
const RIGHT: u16 = 0x10;

/// QMK's own numbers for system and media keys, and the HID usage each
/// stands for: generic desktop for the first three, consumer for the rest.
const MEDIA_KEYS: [(u16, UsagePage, u16); 30] = [
    (0x00A5, UsagePage::GenericDesktop, 0x81),
    (0x00A6, UsagePage::GenericDesktop, 0x82),
    (0x00A7, UsagePage::GenericDesktop, 0x83),
    (0x00A8, UsagePage::Consumer, 0xE2),
    (0x00A9, UsagePage::Consumer, 0xE9),
    (0x00AA, UsagePage::Consumer, 0xEA),
    (0x00AB, UsagePage::Consumer, 0xB5),
    (0x00AC, UsagePage::Consumer, 0xB6),
    (0x00AD, UsagePage::Consumer, 0xB7),
    (0x00AE, UsagePage::Consumer, 0xCD),
    (0x00AF, UsagePage::Consumer, 0x183),
    (0x00B0, UsagePage::Consumer, 0xCC),
    (0x00B1, UsagePage::Consumer, 0x18A),
    (0x00B2, UsagePage::Consumer, 0x192),
    (0x00B3, UsagePage::Consumer, 0x194),
    (0x00B4, UsagePage::Consumer, 0x221),
    (0x00B5, UsagePage::Consumer, 0x223),
    (0x00B6, UsagePage::Consumer, 0x224),
    (0x00B7, UsagePage::Consumer, 0x225),
    (0x00B8, UsagePage::Consumer, 0x226),
    (0x00B9, UsagePage::Consumer, 0x227),
    (0x00BA, UsagePage::Consumer, 0x22A),
    (0x00BB, UsagePage::Consumer, 0xB3),
    (0x00BC, UsagePage::Consumer, 0xB4),
    (0x00BD, UsagePage::Consumer, 0x6F),
    (0x00BE, UsagePage::Consumer, 0x70),
    (0x00BF, UsagePage::Consumer, 0x19F),
    (0x00C0, UsagePage::Consumer, 0x1CB),
    (0x00C1, UsagePage::Consumer, 0x29F),
    (0x00C2, UsagePage::Consumer, 0x2A0),
];

/// QMK's mouse keys and ZMK's behavior and constant for each.
const MOUSE_KEYS: [(u16, &str, &str); 13] = [
    (0x00CD, "mmv", "MOVE_UP"),
    (0x00CE, "mmv", "MOVE_DOWN"),
    (0x00CF, "mmv", "MOVE_LEFT"),
    (0x00D0, "mmv", "MOVE_RIGHT"),
    (0x00D1, "mkp", "LCLK"),
    (0x00D2, "mkp", "RCLK"),
    (0x00D3, "mkp", "MCLK"),
    (0x00D4, "mkp", "MB4"),
    (0x00D5, "mkp", "MB5"),
    (0x00D9, "msc", "SCRL_UP"),
    (0x00DA, "msc", "SCRL_DOWN"),
    (0x00DB, "msc", "SCRL_LEFT"),
    (0x00DC, "msc", "SCRL_RIGHT"),
];

/// Backlight keys, as offsets from `BACKLIGHT`, and ZMK's commands.
const BACKLIGHT_KEYS: [(u16, &str); 6] = [
    (0, "BL_ON"),
    (1, "BL_OFF"),
    (2, "BL_TOG"),
    (3, "BL_DEC"),
    (4, "BL_INC"),
    (5, "BL_CYCLE"),
];

/// RGB keys as offsets from `UNDERGLOW` (toggle first) and from
/// `RGB_MATRIX` (on and off first, toggle third), and ZMK's commands.
const RGB_KEYS: [(&str, Option<u16>, u16); 13] = [
    ("RGB_ON", None, 0),
    ("RGB_OFF", None, 1),
    ("RGB_TOG", Some(0), 2),
    ("RGB_EFF", Some(1), 3),
    ("RGB_EFR", Some(2), 4),
    ("RGB_HUI", Some(3), 5),
    ("RGB_HUD", Some(4), 6),
    ("RGB_SAI", Some(5), 7),
    ("RGB_SAD", Some(6), 8),
    ("RGB_BRI", Some(7), 9),
    ("RGB_BRD", Some(8), 10),
    ("RGB_SPI", Some(9), 11),
    ("RGB_SPD", Some(10), 12),
];

/// Keys with no parameters and ZMK's behavior for each.
const PLAIN_KEYS: [(u16, &str); 4] = [
    (BOOTLOADER, "bootloader"),
    (REBOOT, "sys_reset"),
    (CAPS_WORD, "caps_word"),
    (REPEAT_KEY, "key_repeat"),
];

/// What a keyboard's lighting is, which decides which RGB keys it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Lights {
    /// RGB matrix (per-key) rather than underglow keys.
    pub rgb_matrix: bool,
}

fn bits(modifier: Modifier) -> u16 {
    match modifier {
        Modifier::LCtrl => CTRL,
        Modifier::LShift => SHIFT,
        Modifier::LAlt => ALT,
        Modifier::LGui => GUI,
        Modifier::RCtrl => CTRL | RIGHT,
        Modifier::RShift => SHIFT | RIGHT,
        Modifier::RAlt => ALT | RIGHT,
        Modifier::RGui => GUI | RIGHT,
    }
}

/// The five modifier bits for a set of modifiers, which must all be on
/// one hand, as QMK packs them.
fn mod_bits(mods: &[Modifier]) -> Result<u16, String> {
    let packed = mods.iter().fold(0, |acc, m| acc | bits(*m));
    let right = mods.iter().any(|m| bits(*m) & RIGHT != 0);
    let left = mods.iter().any(|m| bits(*m) & RIGHT == 0);
    if right && left {
        return Err("QMK cannot hold left- and right-hand modifiers in one key".into());
    }
    Ok(packed)
}

/// The modifiers five bits stand for.
fn modifiers(packed: u16) -> Vec<Modifier> {
    let right = packed & RIGHT != 0;
    [
        (CTRL, Modifier::LCtrl, Modifier::RCtrl),
        (SHIFT, Modifier::LShift, Modifier::RShift),
        (ALT, Modifier::LAlt, Modifier::RAlt),
        (GUI, Modifier::LGui, Modifier::RGui),
    ]
    .into_iter()
    .filter_map(|(bit, left, r)| (packed & bit != 0).then_some(if right { r } else { left }))
    .collect()
}

/// The name of the key with a HID keyboard usage and nothing implied.
pub(crate) fn plain_name(usage: u16) -> Option<String> {
    keycodes()
        .all()
        .iter()
        .find(|k| k.page == UsagePage::Keyboard && k.usage == usage && k.implicit_mods.is_empty())
        .map(|k| k.short_name().to_string())
}

/// The modifier a usage from `0xE0` stands for.
fn modifier_of(usage: u16) -> Option<Modifier> {
    let index = usage.checked_sub(0xE0)?;
    Modifier::ALL.get(usize::from(index)).copied()
}

/// The usage of a modifier, from `0xE0`.
fn modifier_usage(modifier: Modifier) -> u16 {
    0xE0 + modifier.bit().trailing_zeros() as u16
}

/// A key expression as a basic keycode and modifier bits.
fn key_parts(expr: &KeyExpr) -> Result<(u16, u16), String> {
    let key = keycodes()
        .get(&expr.key)
        .ok_or_else(|| format!("`{}` is not a key this app knows", expr.key))?;
    let mods: Vec<Modifier> = expr
        .mods
        .iter()
        .chain(&key.implicit_mods)
        .copied()
        .collect();
    match key.page {
        // QMK gives 0xA5 to 0xDF to its own media and mouse keys, so the
        // keyboard page's usages there (keypad keys, mostly) have no code.
        UsagePage::Keyboard
            if (4..=0xA4).contains(&key.usage) || (0xE0..=0xE7).contains(&key.usage) =>
        {
            Ok((key.usage, mod_bits(&mods)?))
        }
        page => {
            let code = MEDIA_KEYS
                .iter()
                .find(|(_, p, usage)| *p == page && *usage == key.usage)
                .map(|(code, _, _)| *code)
                .ok_or_else(|| format!("QMK has no key for `{}`", expr.key))?;
            if !mods.is_empty() {
                return Err("QMK cannot add modifiers to a media key".into());
            }
            Ok((code, 0))
        }
    }
}

/// The keycode for a key expression: the basic code, or the modified one.
fn key_code(expr: &KeyExpr) -> Result<u16, String> {
    // A modified key is the modifier bits above the basic code; the range
    // starts at `MODS` only because at least one bit is set.
    let (usage, mods) = key_parts(expr)?;
    Ok(if mods == 0 {
        usage
    } else {
        (mods << 8) | usage
    })
}

/// The binding for a basic or modified keycode.
fn key_binding(code: u16) -> Option<Binding> {
    let (usage, packed) = (code & 0xFF, (code >> 8) & 0x1F);
    if let Some((_, page, hid)) = MEDIA_KEYS.iter().find(|(c, _, _)| *c == usage) {
        if packed != 0 {
            return None;
        }
        return keycodes()
            .all()
            .iter()
            .find(|k| k.page == *page && k.usage == *hid)
            .map(|k| Binding::kp(KeyExpr::new(k.short_name())));
    }
    if packed == 0 {
        return Some(Binding::kp(KeyExpr::new(plain_name(usage)?)));
    }
    let held = modifiers(packed);
    // A key whose own name already means these modifiers, such as `EXCL`
    // for shifted `1`, reads better than the spelled-out form.
    let named = keycodes().all().iter().find(|k| {
        k.page == UsagePage::Keyboard && k.usage == usage && !k.implicit_mods.is_empty() && {
            let mut implied = k.implicit_mods.clone();
            implied.sort();
            let mut wanted = held.clone();
            wanted.sort();
            implied == wanted
        }
    });
    if let Some(key) = named {
        return Some(Binding::kp(KeyExpr::new(key.short_name())));
    }
    let mut expr = KeyExpr::new(plain_name(usage)?);
    for modifier in held.into_iter().rev() {
        expr = expr.with(modifier);
    }
    Some(Binding::kp(expr))
}

/// A raw binding that carries a keycode this app has no binding for, with
/// a short description in front for the keycap.
pub fn kept(code: u16, custom: &[String]) -> Binding {
    let what = match code {
        c if (DEFAULT_LAYER..DEFAULT_LAYER + LAYER_KEYS).contains(&c) => {
            format!("Default layer {}", c - DEFAULT_LAYER + 1)
        }
        c if (LAYER_TAP_TOGGLE..LAYER_TAP_TOGGLE + LAYER_KEYS).contains(&c) => {
            format!("Tap-toggle layer {}", c - LAYER_TAP_TOGGLE + 1)
        }
        c if (PERSISTENT_DEFAULT_LAYER..PERSISTENT_DEFAULT_LAYER + LAYER_KEYS).contains(&c) => {
            format!("Default layer {} (kept)", c - PERSISTENT_DEFAULT_LAYER + 1)
        }
        c if (MACRO..MACRO + MACROS).contains(&c) => format!("Macro {}", c - MACRO + 1),
        c if (CUSTOM..CUSTOM_END).contains(&c) => custom
            .get(usize::from(c - CUSTOM))
            .filter(|n| !n.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Custom {}", c - CUSTOM + 1)),
        0x7800..=0x78FF => "Lighting".to_string(),
        _ => "QMK".to_string(),
    };
    Binding::Raw {
        raw: format!("{what} #{code}"),
    }
}

/// The keycode a kept raw binding carries.
pub fn kept_code(raw: &str) -> Option<u16> {
    raw.rsplit_once('#')?.1.trim().parse().ok()
}

fn layer_index(layer: LayerId, layers: &[LayerId], limit: u16) -> Result<u16, String> {
    let index = layers
        .iter()
        .position(|l| *l == layer)
        .ok_or("the layer no longer exists")?;
    u16::try_from(index)
        .ok()
        .filter(|i| *i < limit)
        .ok_or_else(|| format!("QMK reaches only the first {limit} layers this way"))
}

fn rgb_code(name: &str, lights: Lights) -> Result<u16, String> {
    let (_, underglow, matrix) = RGB_KEYS
        .iter()
        .find(|(command, _, _)| *command == name)
        .ok_or_else(|| format!("QMK has no RGB key for {name}"))?;
    if lights.rgb_matrix {
        return Ok(RGB_MATRIX + matrix);
    }
    underglow
        .map(|offset| UNDERGLOW + offset)
        .ok_or_else(|| format!("QMK's underglow has no key for {name}, only a toggle"))
}

/// The keycode for a binding. `layers` is the layout's layers in order.
/// Fails, with the reason, for a binding QMK has nothing for.
pub fn encode(binding: &Binding, layers: &[LayerId], lights: Lights) -> Result<u16, String> {
    let (label, params) = match binding {
        Binding::Raw { raw } => {
            return kept_code(raw)
                .ok_or_else(|| "devicetree text means nothing to QMK".to_string());
        }
        Binding::Behavior {
            behavior: BehaviorRef::BuiltIn(label),
            params,
        } => (label.as_str(), params.as_slice()),
        Binding::Behavior { .. } => {
            return Err("behaviors defined in the layout cannot be written to a VIA keyboard; only macros can".into());
        }
    };
    let modifier_bits = |expr: &KeyExpr| -> Result<u16, String> {
        let (usage, extra) = key_parts(expr)?;
        let own = modifier_of(usage).ok_or_else(|| format!("`{}` is not a modifier", expr.key))?;
        mod_bits(&[own]).map(|b| b | extra)
    };
    let plain_tap = |expr: &KeyExpr, what: &str| -> Result<u16, String> {
        match key_parts(expr)? {
            (usage, 0) => Ok(usage),
            _ => Err(format!(
                "on QMK a {what} taps a plain key, without modifiers"
            )),
        }
    };
    match (label, params) {
        ("trans", []) => Ok(TRANSPARENT),
        ("none", []) => Ok(NONE),
        ("kp", [Param::Key(expr)]) => key_code(expr),
        ("mo", [Param::Layer(layer)]) => Ok(MOMENTARY + layer_index(*layer, layers, LAYER_KEYS)?),
        ("tog", [Param::Layer(layer)]) => {
            Ok(TOGGLE_LAYER + layer_index(*layer, layers, LAYER_KEYS)?)
        }
        ("to", [Param::Layer(layer)]) => Ok(TO + layer_index(*layer, layers, LAYER_KEYS)?),
        ("sl", [Param::Layer(layer)]) => {
            Ok(ONE_SHOT_LAYER + layer_index(*layer, layers, LAYER_KEYS)?)
        }
        ("sk", [Param::Key(expr)]) => Ok(ONE_SHOT_MOD + modifier_bits(expr)?),
        ("mt", [Param::Key(hold), Param::Key(tap)]) => {
            let mods = modifier_bits(hold)?;
            Ok(MOD_TAP | (mods << 8) | plain_tap(tap, "mod-tap")?)
        }
        ("lt", [Param::Layer(layer), Param::Key(tap)]) => {
            let layer = layer_index(*layer, layers, TAP_LAYERS)?;
            Ok(LAYER_TAP | (layer << 8) | plain_tap(tap, "layer-tap")?)
        }
        ("mkp" | "mmv" | "msc", [Param::Constant(name)]) => MOUSE_KEYS
            .iter()
            .find(|(_, behavior, constant)| *behavior == label && constant == name)
            .map(|(code, _, _)| *code)
            .ok_or_else(|| format!("QMK has no mouse key for {name}")),
        ("bl", [Param::Command { name, args }]) if args.is_empty() => BACKLIGHT_KEYS
            .iter()
            .find(|(_, command)| command == name)
            .map(|(offset, _)| BACKLIGHT + offset)
            .ok_or_else(|| format!("QMK has no backlight key for {name}")),
        ("rgb_ug", [Param::Command { name, args }]) if args.is_empty() => rgb_code(name, lights),
        (label, []) if PLAIN_KEYS.iter().any(|(_, l)| *l == label) => Ok(PLAIN_KEYS
            .iter()
            .find(|(_, l)| *l == label)
            .map(|(code, _)| *code)
            .expect("checked above")),
        _ => {
            let name = kc_zmk::behaviors::built_in(label).map_or(label, |b| b.name);
            Err(format!("QMK has nothing for {name}"))
        }
    }
}

/// The binding for a keycode. Anything without one is kept as it is.
pub fn decode(code: u16, layers: &[LayerId], custom: &[String]) -> Binding {
    let layer = |index: u16| layers.get(usize::from(index)).copied();
    let command = |behavior: &str, name: &str| {
        Binding::new(
            behavior,
            vec![Param::Command {
                name: name.into(),
                args: Vec::new(),
            }],
        )
    };
    let mouse = MOUSE_KEYS
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, behavior, constant)| {
            Binding::new(behavior, vec![Param::Constant((*constant).into())])
        });
    let binding = match code {
        NONE => Some(Binding::none()),
        TRANSPARENT => Some(Binding::trans()),
        _ if mouse.is_some() => mouse,
        c if c < MOD_TAP => key_binding(c),
        c if (MOD_TAP..LAYER_TAP).contains(&c) => {
            let held = modifiers((c >> 8) & 0x1F);
            match held.as_slice() {
                [one] => plain_name(c & 0xFF).map(|tap| {
                    Binding::new(
                        "mt",
                        vec![
                            Param::Key(KeyExpr::new(one.keycode())),
                            Param::Key(KeyExpr::new(tap)),
                        ],
                    )
                }),
                _ => None,
            }
        }
        c if (LAYER_TAP..LAYER_MOD).contains(&c) => layer((c >> 8) & 0x0F)
            .zip(plain_name(c & 0xFF))
            .map(|(layer, tap)| {
                Binding::new(
                    "lt",
                    vec![Param::Layer(layer), Param::Key(KeyExpr::new(tap))],
                )
            }),
        c if (TO..TO + LAYER_KEYS).contains(&c) => layer(c - TO).map(|l| Binding::layer("to", l)),
        c if (MOMENTARY..MOMENTARY + LAYER_KEYS).contains(&c) => {
            layer(c - MOMENTARY).map(|l| Binding::layer("mo", l))
        }
        c if (TOGGLE_LAYER..TOGGLE_LAYER + LAYER_KEYS).contains(&c) => {
            layer(c - TOGGLE_LAYER).map(|l| Binding::layer("tog", l))
        }
        c if (ONE_SHOT_LAYER..ONE_SHOT_LAYER + LAYER_KEYS).contains(&c) => {
            layer(c - ONE_SHOT_LAYER).map(|l| Binding::layer("sl", l))
        }
        c if (ONE_SHOT_MOD..ONE_SHOT_MOD + LAYER_KEYS).contains(&c) => {
            match modifiers(c - ONE_SHOT_MOD).as_slice() {
                [one] => Some(Binding::new(
                    "sk",
                    vec![Param::Key(KeyExpr::new(one.keycode()))],
                )),
                _ => None,
            }
        }
        c if (BACKLIGHT..BACKLIGHT + 6).contains(&c) => BACKLIGHT_KEYS
            .iter()
            .find(|(offset, _)| BACKLIGHT + offset == c)
            .map(|(_, name)| command("bl", name)),
        c if (UNDERGLOW..UNDERGLOW + 11).contains(&c) => RGB_KEYS
            .iter()
            .find(|(_, underglow, _)| underglow.is_some_and(|o| UNDERGLOW + o == c))
            .map(|(name, _, _)| command("rgb_ug", name)),
        c if (RGB_MATRIX..RGB_MATRIX + 13).contains(&c) => RGB_KEYS
            .iter()
            .find(|(_, _, matrix)| RGB_MATRIX + matrix == c)
            .map(|(name, _, _)| command("rgb_ug", name)),
        c if PLAIN_KEYS.iter().any(|(code, _)| *code == c) => PLAIN_KEYS
            .iter()
            .find(|(code, _)| *code == c)
            .map(|(_, label)| Binding::new(label, vec![])),
        _ => None,
    };
    // Reading a code and writing it back must give the same code, so a
    // binding that would not is kept as the number instead.
    binding
        .filter(|b| {
            encode(
                b,
                layers,
                Lights {
                    rgb_matrix: (RGB_MATRIX..RGB_MATRIX + 13).contains(&code),
                },
            ) == Ok(code)
        })
        .unwrap_or_else(|| kept(code, custom))
}

/// The usage a modifier key name stands for, for the macro codec.
pub(crate) fn modifier_key_usage(name: &str) -> Option<u16> {
    Modifier::from_keycode(name).map(modifier_usage)
}
