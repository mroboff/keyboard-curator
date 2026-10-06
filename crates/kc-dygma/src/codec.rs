//! Key bindings to and from the 16-bit key codes Dygma's firmware stores.
//!
//! The numbers are facts about the firmware's keymap format, taken from the
//! tables in Dygma's Bazecor (src/api/keymap/db) and checked against a Defy
//! on firmware v2.2.1.
//!
//! Codes this app has no binding for (superkeys, macros, mouse and media
//! keys, LED and wireless keys) are kept as they are: they become a raw
//! binding that carries the number, and are written back unchanged.

use kc_model::{BehaviorRef, Binding, KeyExpr, LayerId, Param};
use kc_zmk::keycodes::{keycodes, UsagePage};
use kc_zmk::Modifier;

pub const NONE: u16 = 0;
pub const TRANSPARENT: u16 = 65535;

const CTRL: u16 = 256;
const ALT: u16 = 512;
const ALT_GR: u16 = 1024;
const SHIFT: u16 = 2048;
const OS: u16 = 4096;
/// Plain keys with modifier flags stay below this.
const MODIFIED_END: u16 = 8192;

const LOCK_LAYER: u16 = 17408;
const SHIFT_TO_LAYER: u16 = 17450;
const MOVE_TO_LAYER: u16 = 17492;
/// Lock, shift-to and move-to each have room for this many layers.
const LAYER_KEYS: u16 = 42;

const CONSUMER: u16 = 0x4800;
const CONSUMER_END: u16 = 0x4C00;

const ONE_SHOT_MODIFIER: u16 = 49153;
const ONE_SHOT_LAYER: u16 = 49161;
const MOD_TAP: u16 = 49169;
const LAYER_TAP: u16 = 51218;
const DUAL_USE_END: u16 = 53266;
/// One-shot layers and layer-taps reach only the first eight layers.
const SHORT_LAYERS: u16 = 8;

const MACRO: u16 = 53852;
const SUPERKEY: u16 = 53980;
const SLOTS: u16 = 128;

fn flag(modifier: Modifier) -> u16 {
    match modifier {
        Modifier::LCtrl | Modifier::RCtrl => CTRL,
        Modifier::LAlt => ALT,
        Modifier::RAlt => ALT_GR,
        Modifier::LShift | Modifier::RShift => SHIFT,
        Modifier::LGui | Modifier::RGui => OS,
    }
}

/// The modifiers a flag set stands for, as the left-hand ones.
fn modifiers(flags: u16) -> Vec<Modifier> {
    [
        (CTRL, Modifier::LCtrl),
        (SHIFT, Modifier::LShift),
        (ALT, Modifier::LAlt),
        (ALT_GR, Modifier::RAlt),
        (OS, Modifier::LGui),
    ]
    .into_iter()
    .filter_map(|(flag, modifier)| (flags & flag != 0).then_some(modifier))
    .collect()
}

/// The HID keyboard usage of a key name with nothing implied, such as `A`
/// or `LSHFT`.
fn plain_usage(name: &str) -> Option<u16> {
    let key = keycodes().get(name)?;
    (key.page == UsagePage::Keyboard
        && key.implicit_mods.is_empty()
        && (4..=255).contains(&key.usage))
    .then_some(key.usage)
}

/// The name of the key with a HID keyboard usage and nothing implied.
fn plain_name(usage: u16) -> Option<String> {
    keycodes()
        .all()
        .iter()
        .find(|k| k.page == UsagePage::Keyboard && k.usage == usage && k.implicit_mods.is_empty())
        .map(|k| k.short_name().to_string())
}

fn key_code(expr: &KeyExpr) -> Result<u16, String> {
    let key = keycodes()
        .get(&expr.key)
        .ok_or_else(|| format!("`{}` is not a key this app knows", expr.key))?;
    match key.page {
        UsagePage::Keyboard if (4..=255).contains(&key.usage) => {
            let flags = expr
                .mods
                .iter()
                .chain(&key.implicit_mods)
                .fold(0, |flags, m| flags | flag(*m));
            Ok(key.usage | flags)
        }
        UsagePage::Consumer if key.usage < 0x400 && expr.mods.is_empty() => {
            Ok(CONSUMER | key.usage)
        }
        UsagePage::Consumer if key.usage < 0x400 => {
            Err("Dygma firmware cannot add modifiers to a media key".into())
        }
        _ => Err(format!("Dygma firmware has no key for `{}`", expr.key)),
    }
}

/// The binding for a plain or modified key code.
fn key_binding(code: u16) -> Option<Binding> {
    let (usage, flags) = (code & 0xFF, code & !0xFF);
    if flags == 0 {
        return Some(Binding::kp(KeyExpr::new(plain_name(usage)?)));
    }
    let held = modifiers(flags);
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
    // The outermost wrapper is written first.
    for modifier in held.into_iter().rev() {
        expr = expr.with(modifier);
    }
    Some(Binding::kp(expr))
}

/// A raw binding that carries a code this app has no binding for, with a
/// short description in front for the keycap.
pub fn kept(code: u16) -> Binding {
    let what = match code {
        17152 => "LED next".to_string(),
        17153 => "LED previous".to_string(),
        17154 => "LED toggle".to_string(),
        c if (MACRO..MACRO + SLOTS).contains(&c) => format!("Macro {}", c - MACRO + 1),
        c if (SUPERKEY..SUPERKEY + SLOTS).contains(&c) => format!("Superkey {}", c - SUPERKEY + 1),
        20481..=20576 => "Mouse".to_string(),
        54108 => "Battery".to_string(),
        54109 => "Bluetooth pairing".to_string(),
        54111 => "Energy".to_string(),
        54112 => "RF".to_string(),
        _ => "Dygma".to_string(),
    };
    Binding::Raw {
        raw: format!("{what} #{code}"),
    }
}

/// The code a kept raw binding carries.
fn kept_code(raw: &str) -> Option<u16> {
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
        .ok_or_else(|| format!("Dygma firmware reaches only the first {limit} layers this way"))
}

/// The key code for a binding. `layers` is the layout's layers in order.
/// Fails, with the reason, for a binding Dygma's firmware has nothing for.
pub fn encode(binding: &Binding, layers: &[LayerId]) -> Result<u16, String> {
    let (label, params) = match binding {
        Binding::Raw { raw } => {
            return kept_code(raw)
                .ok_or_else(|| "devicetree text means nothing to Dygma firmware".to_string());
        }
        Binding::Behavior {
            behavior: BehaviorRef::BuiltIn(label),
            params,
        } => (label.as_str(), params.as_slice()),
        Binding::Behavior { .. } => {
            return Err(
                "behaviors defined in the layout cannot be written to Dygma firmware yet".into(),
            );
        }
    };
    let all = u16::try_from(layers.len())
        .unwrap_or(u16::MAX)
        .min(LAYER_KEYS);
    match (label, params) {
        ("trans", []) => Ok(TRANSPARENT),
        ("none", []) => Ok(NONE),
        ("kp", [Param::Key(expr)]) => key_code(expr),
        ("mo", [Param::Layer(layer)]) => Ok(SHIFT_TO_LAYER + layer_index(*layer, layers, all)?),
        ("tog", [Param::Layer(layer)]) => Ok(LOCK_LAYER + layer_index(*layer, layers, all)?),
        ("to", [Param::Layer(layer)]) => Ok(MOVE_TO_LAYER + layer_index(*layer, layers, all)?),
        ("sl", [Param::Layer(layer)]) => {
            Ok(ONE_SHOT_LAYER + layer_index(*layer, layers, SHORT_LAYERS)?)
        }
        ("sk", [Param::Key(expr)]) => match plain_usage(&expr.key) {
            Some(usage @ 224..=231) if expr.mods.is_empty() => Ok(ONE_SHOT_MODIFIER + usage - 224),
            _ => Err("on Dygma firmware a sticky key can only be a single modifier".into()),
        },
        ("mt", [Param::Key(hold), Param::Key(tap)]) => {
            let modifier = plain_usage(&hold.key)
                .filter(|usage| (224..=231).contains(usage) && hold.mods.is_empty())
                .ok_or("on Dygma firmware a mod-tap holds a single modifier")?;
            let tap = plain_usage(&tap.key)
                .filter(|_| tap.mods.is_empty())
                .ok_or("on Dygma firmware a mod-tap taps a plain key, without modifiers")?;
            Ok(MOD_TAP + 256 * (modifier - 224) + tap)
        }
        ("lt", [Param::Layer(layer), Param::Key(tap)]) => {
            let tap = plain_usage(&tap.key)
                .filter(|_| tap.mods.is_empty())
                .ok_or("on Dygma firmware a layer-tap taps a plain key, without modifiers")?;
            Ok(LAYER_TAP + 256 * layer_index(*layer, layers, SHORT_LAYERS)? + tap)
        }
        _ => Err(format!("Dygma firmware has nothing for &{label}")),
    }
}

/// The binding for a key code. Anything without one is kept as it is.
pub fn decode(code: u16, layers: &[LayerId]) -> Binding {
    let layer = |index: u16| layers.get(usize::from(index)).copied();
    let modifier_key = |index: u16| plain_name(224 + index).map(KeyExpr::new);
    let binding = match code {
        NONE => Some(Binding::none()),
        TRANSPARENT => Some(Binding::trans()),
        4..MODIFIED_END => key_binding(code),
        c if (LOCK_LAYER..LOCK_LAYER + LAYER_KEYS).contains(&c) => {
            layer(c - LOCK_LAYER).map(|l| Binding::layer("tog", l))
        }
        c if (SHIFT_TO_LAYER..SHIFT_TO_LAYER + LAYER_KEYS).contains(&c) => {
            layer(c - SHIFT_TO_LAYER).map(|l| Binding::layer("mo", l))
        }
        c if (MOVE_TO_LAYER..MOVE_TO_LAYER + LAYER_KEYS).contains(&c) => {
            layer(c - MOVE_TO_LAYER).map(|l| Binding::layer("to", l))
        }
        c if (CONSUMER..CONSUMER_END).contains(&c) => keycodes()
            .all()
            .iter()
            .find(|k| k.page == UsagePage::Consumer && k.usage == c - CONSUMER)
            .map(|k| Binding::kp(KeyExpr::new(k.short_name()))),
        c if (ONE_SHOT_MODIFIER..ONE_SHOT_LAYER).contains(&c) => {
            modifier_key(c - ONE_SHOT_MODIFIER).map(|key| Binding::new("sk", vec![Param::Key(key)]))
        }
        c if (ONE_SHOT_LAYER..MOD_TAP).contains(&c) => {
            layer(c - ONE_SHOT_LAYER).map(|l| Binding::layer("sl", l))
        }
        c if (MOD_TAP..LAYER_TAP).contains(&c) => {
            let (index, tap) = ((c - MOD_TAP) / 256, (c - MOD_TAP) % 256);
            modifier_key(index).zip(plain_name(tap)).map(|(hold, tap)| {
                Binding::new("mt", vec![Param::Key(hold), Param::Key(KeyExpr::new(tap))])
            })
        }
        c if (LAYER_TAP..DUAL_USE_END).contains(&c) => {
            let (index, tap) = ((c - LAYER_TAP) / 256, (c - LAYER_TAP) % 256);
            layer(index).zip(plain_name(tap)).map(|(layer, tap)| {
                Binding::new(
                    "lt",
                    vec![Param::Layer(layer), Param::Key(KeyExpr::new(tap))],
                )
            })
        }
        _ => None,
    };
    // Reading a code and writing it back must give the same code, so a
    // binding that would not is kept as the number instead.
    binding
        .filter(|binding| encode(binding, layers) == Ok(code))
        .unwrap_or_else(|| kept(code))
}
