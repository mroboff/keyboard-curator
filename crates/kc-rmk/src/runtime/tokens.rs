//! Key actions as moergo-rmk's runtime configuration spells them, and the
//! way back from what a keyboard holds.
//!
//! The configuration writes a key the way Vial does, as `KC_A`, `LT(1,
//! KC_SPC)` or `MO(2)`, plus moergo-rmk's own three-argument hold-taps
//! that name a timing profile: `MT(KC_A, LShift, hrm)`. moergo-config
//! parses those spellings, so this module produces text for the one
//! direction and reads wire actions for the other.
//!
//! Facts about the spellings are from moergo-config's `keycodes.rs` and
//! `rynk_keycode.rs`; the meanings of moergo-rmk's reserved user keys are
//! from its crates/moergo-rmk/src/remote_boot.rs and RMK's `process_user`.

use std::collections::HashMap;
use std::sync::OnceLock;

use kc_model::behavior::BehaviorKind;
use kc_model::{BehaviorId, BehaviorRef, Binding, KeyExpr, LayerId, Param, Project};
use kc_zmk::keycodes::{keycodes, UsagePage};
use kc_zmk::Modifier;
use moergo_config::keycodes::format_keycode;
use rynk::rmk_types::action::{Action, KeyAction, KeyboardAction, LightAction};
use rynk::rmk_types::keycode::{HidKeyCode, KeyCode, SpecialKey};
use rynk::rmk_types::modifier::ModifierCombination;

/// moergo-rmk's reserved user keys.
pub const USER_CLEAR_ACTIVE_PROFILE: u8 = 10;
pub const USER_CLEAR_ALL_PROFILES: u8 = 11;
pub const USER_PERIPHERAL_BOOTLOADER: u8 = 12;
pub const USER_SPLIT_TRANSPORT_TOGGLE: u8 = 13;

/// The most layers a key action can name: `MO(15)` is the last.
pub const MAX_LAYERS: usize = 16;

/// Vial's one-byte names for consumer and system keys: the byte, and the
/// HID usage page and usage it stands for. From rmk-types'
/// `ConsumerKey::to_hid_keycode` and `SystemControlKey::to_hid_keycode`.
const ALIASES: &[(u8, u16, u16)] = &[
    (0xA5, 0x01, 0x81),
    (0xA6, 0x01, 0x82),
    (0xA7, 0x01, 0x83),
    (0xA8, 0x0C, 0xE2),
    (0xA9, 0x0C, 0xE9),
    (0xAA, 0x0C, 0xEA),
    (0xAB, 0x0C, 0xB5),
    (0xAC, 0x0C, 0xB6),
    (0xAD, 0x0C, 0xB7),
    (0xAE, 0x0C, 0xCD),
    (0xAF, 0x0C, 0xB2),
    (0xB0, 0x0C, 0xB8),
    (0xB1, 0x0C, 0x18A),
    (0xB2, 0x0C, 0x192),
    (0xB3, 0x0C, 0x194),
    (0xB4, 0x0C, 0x221),
    (0xB5, 0x0C, 0x223),
    (0xB6, 0x0C, 0x224),
    (0xB7, 0x0C, 0x225),
    (0xB8, 0x0C, 0x226),
    (0xB9, 0x0C, 0x227),
    (0xBA, 0x0C, 0x22A),
    (0xBB, 0x0C, 0xB3),
    (0xBC, 0x0C, 0xB4),
    (0xBD, 0x0C, 0x6F),
    (0xBE, 0x0C, 0x70),
    (0xBF, 0x0C, 0x19F),
    (0xC0, 0x0C, 0x1CB),
    (0xC1, 0x0C, 0x29F),
    (0xC2, 0x0C, 0x2A0),
    (0xC3, 0x0C, 0x73),
    (0xC4, 0x0C, 0x74),
    (0xC5, 0x0C, 0x75),
];

/// ZMK's mouse constants and Vial's one-byte mouse keys.
const MOUSE: &[(&str, &str, u8)] = &[
    ("mmv", "MOVE_UP", 0xCD),
    ("mmv", "MOVE_DOWN", 0xCE),
    ("mmv", "MOVE_LEFT", 0xCF),
    ("mmv", "MOVE_RIGHT", 0xD0),
    ("mkp", "LCLK", 0xD1),
    ("mkp", "RCLK", 0xD2),
    ("mkp", "MCLK", 0xD3),
    ("mkp", "MB4", 0xD4),
    ("mkp", "MB5", 0xD5),
    ("msc", "SCRL_UP", 0xD9),
    ("msc", "SCRL_DOWN", 0xDA),
    ("msc", "SCRL_LEFT", 0xDB),
    ("msc", "SCRL_RIGHT", 0xDC),
];

/// ZMK's lighting commands and Vial's codes for what they do in
/// moergo-rmk, where the lighting output toggle (`BL_TOGG`) is what
/// MoErgo's `RGB_TOG` does and `UG_*` drive the animation.
const LIGHTING: &[(&str, u16)] = &[
    ("RGB_TOG", 0x7802),
    ("RGB_ON", 0x7800),
    ("RGB_OFF", 0x7801),
    ("RGB_HUI", 0x7823),
    ("RGB_HUD", 0x7824),
    ("RGB_SAI", 0x7825),
    ("RGB_SAD", 0x7826),
    ("RGB_BRI", 0x7827),
    ("RGB_BRD", 0x7828),
    ("RGB_SPI", 0x7829),
    ("RGB_SPD", 0x782A),
    ("RGB_EFF", 0x7821),
    ("RGB_EFR", 0x7822),
];

const MOD_CTRL: u8 = 0x01;
const MOD_SHIFT: u8 = 0x02;
const MOD_ALT: u8 = 0x04;
const MOD_GUI: u8 = 0x08;
const MOD_RIGHT: u8 = 0x10;

fn modifier_bit(modifier: Modifier) -> u8 {
    match modifier {
        Modifier::LCtrl => MOD_CTRL,
        Modifier::LShift => MOD_SHIFT,
        Modifier::LAlt => MOD_ALT,
        Modifier::LGui => MOD_GUI,
        Modifier::RCtrl => MOD_CTRL | MOD_RIGHT,
        Modifier::RShift => MOD_SHIFT | MOD_RIGHT,
        Modifier::RAlt => MOD_ALT | MOD_RIGHT,
        Modifier::RGui => MOD_GUI | MOD_RIGHT,
    }
}

/// Modifiers as Vial packs them: four bits and a hand. Vial has one hand
/// for all of them, so left and right cannot be mixed.
fn packed(mods: &[Modifier]) -> Result<u8, String> {
    let mut bits = 0;
    for modifier in mods {
        bits |= modifier_bit(*modifier);
    }
    let right = mods
        .iter()
        .filter(|m| modifier_bit(**m) & MOD_RIGHT != 0)
        .count();
    if right != 0 && right != mods.len() {
        return Err("RMK keeps a key's modifiers on one hand: left or right, not both".into());
    }
    Ok(bits)
}

/// The name a modifier has in moergo-rmk's three-argument hold-taps.
pub fn modifier_name(modifier: Modifier) -> &'static str {
    match modifier {
        Modifier::LCtrl => "LCtrl",
        Modifier::LShift => "LShift",
        Modifier::LAlt => "LAlt",
        Modifier::LGui => "LGui",
        Modifier::RCtrl => "RCtrl",
        Modifier::RShift => "RShift",
        Modifier::RAlt => "RAlt",
        Modifier::RGui => "RGui",
    }
}

/// A key as Vial spells it: a one-byte code and the modifiers around it,
/// the key's own included.
struct Basic {
    code: u8,
    mods: Vec<Modifier>,
}

fn basic(expr: &KeyExpr) -> Result<Basic, String> {
    let key = keycodes()
        .get(&expr.key)
        .ok_or_else(|| format!("`{}` is not a key this app knows", expr.key))?;
    let code = match key.page {
        UsagePage::Keyboard if key.usage < 0xA5 || (0xE0..=0xE7).contains(&key.usage) => {
            u8::try_from(key.usage).expect("checked range")
        }
        page => ALIASES
            .iter()
            .find(|(_, p, usage)| *p == page.id() && *usage == key.usage)
            .map(|(code, _, _)| *code)
            .ok_or_else(|| format!("RMK has no key for `{}`", expr.key))?,
    };
    let mut mods = expr.mods.clone();
    for modifier in &key.implicit_mods {
        if !mods.contains(modifier) {
            mods.push(*modifier);
        }
    }
    Ok(Basic { code, mods })
}

/// A key as a Vial code: the byte, or a modifier wrapper around it.
fn key_code(expr: &KeyExpr) -> Result<u16, String> {
    let basic = basic(expr)?;
    Ok(u16::from(packed(&basic.mods)?) << 8 | u16::from(basic.code))
}

/// The modifiers a hold-tap holds: those of the key named, which must be a
/// modifier, and any wrapped around it.
fn held_modifiers(expr: &KeyExpr) -> Result<Vec<Modifier>, String> {
    let own = Modifier::from_keycode(&expr.key)
        .ok_or_else(|| format!("`{}` is not a modifier", expr.key))?;
    let mut mods = vec![own];
    for modifier in &expr.mods {
        if !mods.contains(modifier) {
            mods.push(*modifier);
        }
    }
    Ok(mods)
}

/// What the translation needs to know about the layout: the layer order,
/// and the slot each behavior the layout defines has in the firmware's
/// tables.
pub struct Context<'a> {
    pub project: &'a Project,
    pub layers: Vec<LayerId>,
    pub ble_profiles: u8,
    /// Tap-dances, by their `TD(n)` slot.
    pub morses: HashMap<BehaviorId, u8>,
    /// Macros, by their `MACRO(n)` slot.
    pub macros: HashMap<BehaviorId, u8>,
    /// Hold-taps, by the name of the timing profile made from each.
    pub profiles: HashMap<BehaviorId, String>,
}

impl<'a> Context<'a> {
    pub fn new(project: &'a Project, ble_profiles: u8) -> Self {
        let mut cx = Self {
            project,
            layers: project.layers.iter().map(|l| l.id).collect(),
            ble_profiles,
            morses: HashMap::new(),
            macros: HashMap::new(),
            profiles: HashMap::new(),
        };
        // Macros first: a tap-dance may be made of them.
        for def in &project.behaviors {
            match &def.kind {
                BehaviorKind::Macro(m)
                    if profile_key_of_macro(m, &cx).is_none() && !macro_does_nothing(m, &cx) =>
                {
                    let slot = u8::try_from(cx.macros.len()).unwrap_or(u8::MAX);
                    cx.macros.insert(def.id, slot);
                }
                BehaviorKind::HoldTap(_) => {
                    cx.profiles.insert(def.id, def.label.clone());
                }
                _ => {}
            }
        }
        for def in &project.behaviors {
            if let BehaviorKind::TapDance(td) = &def.kind {
                if profile_key_of_tap_dance(td, &cx).is_none() {
                    let slot = u8::try_from(cx.morses.len()).unwrap_or(u8::MAX);
                    cx.morses.insert(def.id, slot);
                }
            }
        }
        cx
    }

    /// Whether a behavior the layout defines has a slot in the firmware's
    /// tables, rather than standing for one of the firmware's own keys.
    pub fn has_slot(&self, id: BehaviorId) -> bool {
        self.morses.contains_key(&id) || self.macros.contains_key(&id)
    }

    fn layer(&self, layer: LayerId) -> Result<u16, String> {
        let index = self
            .layers
            .iter()
            .position(|l| *l == layer)
            .ok_or("the layer no longer exists")?;
        if index >= MAX_LAYERS {
            return Err(format!("RMK's key actions reach only {MAX_LAYERS} layers"));
        }
        Ok(index as u16)
    }
}

/// The Bluetooth profile a MoErgo-style macro selects: one that switches
/// output to Bluetooth and selects a profile, in either order and nothing
/// else. The shape alone; whether the firmware has the profile is another
/// question.
pub fn bluetooth_macro(macro_def: &kc_model::behavior::Macro) -> Option<u32> {
    let mut profile = None;
    for step in &macro_def.steps {
        let kc_model::behavior::MacroStep::Tap(bindings) = step else {
            return None;
        };
        for binding in bindings {
            match built_in_of(binding)? {
                ("out", [Param::Command { name, .. }]) if name == "OUT_BLE" => {}
                ("bt", [Param::Command { name, args }]) if name == "BT_SEL" => {
                    let [n] = args.as_slice() else { return None };
                    if profile.replace(*n).is_some() {
                        return None;
                    }
                }
                _ => return None,
            }
        }
    }
    profile
}

/// Whether every step of a macro does nothing in RMK, such as MoErgo's
/// status macro, whose one step is `RGB_STATUS`. Such a macro is written
/// as nothing rather than as a macro of nothing.
pub fn macro_does_nothing(macro_def: &kc_model::behavior::Macro, cx: &Context) -> bool {
    use kc_model::behavior::MacroStep;
    macro_def.steps.iter().all(|step| match step {
        MacroStep::Tap(b) | MacroStep::Press(b) | MacroStep::Release(b) => {
            b.iter().all(|binding| via(binding, cx) == Ok(0))
        }
        MacroStep::WaitTime(_) | MacroStep::TapTime(_) => true,
        MacroStep::PauseForRelease | MacroStep::Param { .. } => false,
    })
}

/// The profile key a MoErgo-style Bluetooth macro stands for, when the
/// firmware has that profile. RMK's profile key switches to Bluetooth and
/// selects the profile at once.
pub fn profile_key_of_macro(macro_def: &kc_model::behavior::Macro, cx: &Context) -> Option<u8> {
    bluetooth_macro(macro_def)
        .filter(|n| *n < u32::from(cx.ble_profiles))
        .and_then(|n| u8::try_from(n).ok())
}

/// The Bluetooth profile a MoErgo-style tap-dance is about: one that
/// selects a profile on a tap and disconnects it on a double tap. The
/// shape alone.
pub fn bluetooth_tap_dance(
    tap_dance: &kc_model::behavior::TapDance,
    project: &Project,
) -> Option<u32> {
    let [first, second] = tap_dance.bindings.as_slice() else {
        return None;
    };
    let profile = match first {
        Binding::Behavior {
            behavior: BehaviorRef::User { user },
            ..
        } => match &project.behavior(*user)?.kind {
            BehaviorKind::Macro(m) => bluetooth_macro(m)?,
            _ => return None,
        },
        other => match built_in_of(other)? {
            ("bt", [Param::Command { name, args }]) if name == "BT_SEL" => *args.first()?,
            _ => return None,
        },
    };
    match built_in_of(second)? {
        ("bt", [Param::Command { name, args }])
            if name == "BT_DISC" && args.first().copied() == Some(profile) =>
        {
            Some(profile)
        }
        _ => None,
    }
}

/// The profile key a MoErgo-style Bluetooth tap-dance stands for, when
/// the firmware has that profile. RMK's profile key disconnects on a
/// double tap by itself.
pub fn profile_key_of_tap_dance(
    tap_dance: &kc_model::behavior::TapDance,
    cx: &Context,
) -> Option<u8> {
    bluetooth_tap_dance(tap_dance, cx.project)
        .filter(|n| *n < u32::from(cx.ble_profiles))
        .and_then(|n| u8::try_from(n).ok())
}

fn built_in_of(binding: &Binding) -> Option<(&str, &[Param])> {
    match binding {
        Binding::Behavior {
            behavior: BehaviorRef::BuiltIn(label),
            params,
        } => Some((label.as_str(), params.as_slice())),
        _ => None,
    }
}

/// A binding as a Vial code, for everything the configuration spells that
/// way: every key but a hold-tap with a timing profile of its own.
pub fn via(binding: &Binding, cx: &Context) -> Result<u16, String> {
    let (label, params) = match binding {
        Binding::Raw { .. } => return Err("devicetree text means nothing to RMK".into()),
        Binding::Behavior {
            behavior: BehaviorRef::User { user },
            params,
        } => return user_via(*user, params, cx),
        Binding::Behavior {
            behavior: BehaviorRef::BuiltIn(label),
            params,
        } => (label.as_str(), params.as_slice()),
    };
    let profiles = u16::from(cx.ble_profiles);
    Ok(match (label, params) {
        ("trans", []) => 0x0001,
        ("none", []) => 0x0000,
        ("kp", [Param::Key(expr)]) => key_code(expr)?,
        ("mo", [Param::Layer(l)]) => 0x5220 | cx.layer(*l)?,
        ("to", [Param::Layer(l)]) => 0x5200 | cx.layer(*l)?,
        ("tog", [Param::Layer(l)]) => 0x5260 | cx.layer(*l)?,
        ("sl", [Param::Layer(l)]) => 0x5280 | cx.layer(*l)?,
        ("sk", [Param::Key(expr)]) => {
            let mods = held_modifiers(expr)
                .map_err(|_| "in RMK a sticky key can only be modifiers".to_string())?;
            0x52A0 | u16::from(packed(&mods)?)
        }
        ("mt", [Param::Key(hold), Param::Key(tap)]) => {
            let mods =
                held_modifiers(hold).map_err(|_| "in RMK a mod-tap holds modifiers".to_string())?;
            let tap = basic(tap)?;
            if !tap.mods.is_empty() {
                return Err("in RMK a mod-tap taps a plain key".into());
            }
            0x2000 | u16::from(packed(&mods)?) << 8 | u16::from(tap.code)
        }
        ("lt", [Param::Layer(l), Param::Key(tap)]) => {
            let tap = basic(tap)?;
            if !tap.mods.is_empty() {
                return Err("in RMK a layer-tap taps a plain key".into());
            }
            0x4000 | cx.layer(*l)? << 8 | u16::from(tap.code)
        }
        ("bootloader", []) => 0x7C00,
        ("sys_reset", []) => 0x7C01,
        // The nearest thing to unlocking ZMK Studio: toggling moergo-rmk's
        // maintenance lock, which gates what a host may change.
        ("studio_unlock", []) => 0x7C04,
        ("caps_word", []) => 0x7C73,
        ("key_repeat", []) => 0x7C79,
        ("gresc", []) => 0x7C16,
        ("mkp" | "mmv" | "msc", [Param::Constant(name)]) => MOUSE
            .iter()
            .find(|(behavior, constant, _)| *behavior == label && constant == name)
            .map(|(_, _, code)| u16::from(*code))
            .ok_or_else(|| format!("RMK has nothing for &{label} {name}"))?,
        ("bt", [Param::Command { name, args }]) => match (name.as_str(), args.as_slice()) {
            // RMK's profile key also disconnects, on a double tap.
            ("BT_SEL" | "BT_DISC", [profile]) if *profile < profiles.into() => {
                0x7E00 | *profile as u16
            }
            ("BT_SEL" | "BT_DISC", _) => return Err(too_few_profiles(cx.ble_profiles)),
            ("BT_NXT", []) => 0x7E00 | profiles,
            ("BT_PRV", []) => 0x7E00 | (profiles + 1),
            // moergo-rmk's own key for clearing the active profile, the one
            // its MoErgo importer writes; RMK's `profiles + 2` reads the same.
            ("BT_CLR", []) => 0x7E00 | u16::from(USER_CLEAR_ACTIVE_PROFILE),
            ("BT_CLR_ALL", []) => 0x7E00 | u16::from(USER_CLEAR_ALL_PROFILES),
            _ => return Err(format!("RMK has nothing for &bt {name}")),
        },
        ("out", [Param::Command { name, .. }]) => match name.as_str() {
            "OUT_USB" => 0x7784,
            "OUT_BLE" => 0x7786,
            "OUT_TOG" => 0x7E00 | (profiles + 3),
            _ => return Err(format!("RMK has nothing for &out {name}")),
        },
        // Status is shown by a layer's lock and battery lights, not by a
        // key, so MoErgo's status key does nothing here.
        ("rgb_ug", [Param::Command { name, .. }]) if name == "RGB_STATUS" => 0x0000,
        ("rgb_ug", [Param::Command { name, .. }]) => LIGHTING
            .iter()
            .find(|(command, _)| command == name)
            .map(|(_, code)| *code)
            .ok_or_else(|| format!("RMK has nothing for &rgb_ug {name}"))?,
        _ => return Err(format!("RMK has nothing for &{label}")),
    })
}

/// Why a Bluetooth key for a profile the firmware does not keep fails.
pub fn too_few_profiles(profiles: u8) -> String {
    format!("this RMK firmware keeps {profiles} Bluetooth profiles")
}

/// A binding to a behavior the layout defines, as a Vial code.
fn user_via(id: BehaviorId, params: &[Param], cx: &Context) -> Result<u16, String> {
    let def = cx
        .project
        .behavior(id)
        .ok_or("uses a behavior that no longer exists")?;
    match &def.kind {
        BehaviorKind::TapDance(td) => match profile_key_of_tap_dance(td, cx) {
            Some(profile) => Ok(0x7E00 | u16::from(profile)),
            None => Ok(0x5700 | u16::from(cx.morses[&id])),
        },
        BehaviorKind::Macro(m) => {
            if let Some(profile) = profile_key_of_macro(m, cx) {
                return Ok(0x7E00 | u16::from(profile));
            }
            if macro_does_nothing(m, cx) {
                return Ok(0);
            }
            if m.params > 0 {
                return Err("RMK's macros take no parameters".into());
            }
            let slot = cx.macros[&id];
            if slot > 31 {
                return Err("RMK's keymap reaches 32 macros".into());
            }
            Ok(0x7700 | u16::from(slot))
        }
        // A fork is triggered by the key it replaces: the key is written
        // as its normal binding.
        BehaviorKind::ModMorph(m) => via(&m.normal, cx),
        BehaviorKind::StickyKey(s) => match (&s.behavior, params) {
            (BehaviorRef::BuiltIn(label), [Param::Key(expr)]) if label == "kp" => {
                via(&Binding::new("sk", vec![Param::Key(expr.clone())]), cx)
            }
            (BehaviorRef::BuiltIn(label), [Param::Layer(l)]) if label == "mo" => {
                via(&Binding::layer("sl", *l), cx)
            }
            _ => Err("RMK's sticky keys are modifiers or layers".into()),
        },
        BehaviorKind::HoldTap(_) => {
            Err("a hold-tap with its own timing is written only at a key in RMK".into())
        }
    }
}

/// A key as the configuration spells it.
pub fn token(binding: &Binding, cx: &Context) -> Result<String, String> {
    if let Binding::Behavior {
        behavior: BehaviorRef::User { user },
        params,
    } = binding
    {
        if let Some(BehaviorKind::HoldTap(hold_tap)) = cx.project.behavior(*user).map(|d| &d.kind) {
            return hold_tap_token(*user, hold_tap, params, cx);
        }
    }
    Ok(format_keycode(via(binding, cx)?))
}

/// `MT(tap, mods, profile)`, `LT(layer, tap, profile)` or `TH(tap, hold,
/// profile)`: a hold-tap with the layout's own timing, named after it. A
/// hold-tap that taps nothing is written as what it holds.
fn hold_tap_token(
    id: BehaviorId,
    hold_tap: &kc_model::behavior::HoldTap,
    params: &[Param],
    cx: &Context,
) -> Result<String, String> {
    let profile = &cx.profiles[&id];
    let [hold_param, tap_param] = params else {
        return Err("a hold-tap takes what it holds and what it taps".into());
    };
    let hold_binding = side_binding(&hold_tap.hold, hold_param);
    let tap_binding = side_binding(&hold_tap.tap, tap_param);
    let tap_code = via(&tap_binding, cx)?;
    if tap_code == 0 {
        return single(&hold_binding, cx);
    }
    let tap = single(&tap_binding, cx)?;
    match (&hold_tap.hold, hold_param) {
        (BehaviorRef::BuiltIn(label), Param::Key(expr)) if label == "kp" => {
            if let Ok(mods) = held_modifiers(expr) {
                let names: Vec<&str> = mods.iter().map(|m| modifier_name(*m)).collect();
                return Ok(format!("MT({tap}, {}, {profile})", names.join(" | ")));
            }
        }
        (BehaviorRef::BuiltIn(label), Param::Layer(l)) if label == "mo" => {
            return Ok(format!("LT({}, {tap}, {profile})", cx.layer(*l)?));
        }
        _ => {}
    }
    Ok(format!(
        "TH({tap}, {}, {profile})",
        single(&hold_binding, cx)?
    ))
}

/// One side of a hold-tap as a binding of its own: the side's behavior
/// with the parameter the key gave it, or a behavior of the layout's own
/// that takes none.
fn side_binding(behavior: &BehaviorRef, param: &Param) -> Binding {
    match behavior {
        BehaviorRef::BuiltIn(label) => Binding::new(label, vec![param.clone()]),
        BehaviorRef::User { user } => Binding::user(*user, Vec::new()),
    }
}

/// How a key is written differently from what the layout says, when it
/// is: a note for the user, or nothing.
pub fn degraded(binding: &Binding, cx: &Context) -> Option<&'static str> {
    const STATUS: &str = "MoErgo's status key does nothing in RMK; the Magic layer's lock and battery lights show status instead";
    const UNLOCK: &str = "the Studio unlock key toggles RMK's maintenance lock instead, which gates what a host may change on the keyboard";
    const PROFILE: &str = "a Bluetooth macro or tap-dance was written as RMK's own profile key, which switches to Bluetooth, selects the profile, and disconnects it on a double tap";
    match binding {
        Binding::Behavior {
            behavior: BehaviorRef::BuiltIn(label),
            params,
        } => match (label.as_str(), params.as_slice()) {
            ("rgb_ug", [Param::Command { name, .. }]) if name == "RGB_STATUS" => Some(STATUS),
            ("studio_unlock", []) => Some(UNLOCK),
            _ => None,
        },
        Binding::Behavior {
            behavior: BehaviorRef::User { user },
            params,
        } => match &cx.project.behavior(*user)?.kind {
            BehaviorKind::Macro(m) if profile_key_of_macro(m, cx).is_some() => Some(PROFILE),
            BehaviorKind::TapDance(td) if profile_key_of_tap_dance(td, cx).is_some() => {
                Some(PROFILE)
            }
            BehaviorKind::Macro(m) => m
                .steps
                .iter()
                .flat_map(|step| match step {
                    kc_model::behavior::MacroStep::Tap(b)
                    | kc_model::behavior::MacroStep::Press(b)
                    | kc_model::behavior::MacroStep::Release(b) => b.as_slice(),
                    _ => &[],
                })
                .find_map(|b| degraded(b, cx)),
            BehaviorKind::HoldTap(hold_tap) => {
                let [_, tap_param] = params.as_slice() else {
                    return None;
                };
                let tap = side_binding(&hold_tap.tap, tap_param);
                if degraded(&tap, cx).is_some() || via(&tap, cx) == Ok(0) {
                    Some("a hold-tap that taps nothing was written as what it holds")
                } else {
                    None
                }
            }
            _ => None,
        },
        Binding::Raw { .. } => None,
    }
}

/// A binding that must be one action, such as a tap-dance step, a combo's
/// output or a hold-tap's side: anything but another hold-tap.
pub fn single(binding: &Binding, cx: &Context) -> Result<String, String> {
    let code = via(binding, cx)?;
    if (0x2000..0x5000).contains(&code) {
        return Err("RMK takes a plain action here, not a hold-tap".into());
    }
    Ok(format_keycode(code))
}

/// The name of a key in a layout, by HID usage page and usage: the first
/// keycode of the catalog without modifiers built in.
fn name_for(page: u16, usage: u16) -> Option<&'static str> {
    static NAMES: OnceLock<HashMap<(u16, u16), &'static str>> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            let mut names: HashMap<(u16, u16), &'static str> = HashMap::new();
            for key in keycodes().all() {
                if key.implicit_mods.is_empty() {
                    names
                        .entry((key.page.id(), key.usage))
                        .or_insert(key.name.as_str());
                }
            }
            names
        })
        .get(&(page, usage))
        .copied()
}

/// The modifiers of a wire combination, left hand first.
pub fn modifiers_of(combination: ModifierCombination) -> Vec<Modifier> {
    [
        (combination.left_ctrl(), Modifier::LCtrl),
        (combination.left_shift(), Modifier::LShift),
        (combination.left_alt(), Modifier::LAlt),
        (combination.left_gui(), Modifier::LGui),
        (combination.right_ctrl(), Modifier::RCtrl),
        (combination.right_shift(), Modifier::RShift),
        (combination.right_alt(), Modifier::RAlt),
        (combination.right_gui(), Modifier::RGui),
    ]
    .into_iter()
    .filter_map(|(held, modifier)| held.then_some(modifier))
    .collect()
}

/// Held modifiers as a key: the first as the key, the rest wrapped
/// around it, as in `LS(LCTRL)`.
pub fn modifier_key(mods: &[Modifier]) -> Option<KeyExpr> {
    let (first, rest) = mods.split_first()?;
    let mut expr = KeyExpr::new(first.keycode());
    for modifier in rest {
        expr = expr.with(*modifier);
    }
    Some(expr)
}

/// A Vial one-byte key as a layout key, when the layout has a name for it.
pub fn hid_key(key: HidKeyCode) -> Option<KeyExpr> {
    let code = key as u16;
    let (page, usage) = match code {
        0x04..=0xA4 | 0xE0..=0xE7 => (UsagePage::Keyboard.id(), code),
        _ => ALIASES
            .iter()
            .find(|(alias, _, _)| u16::from(*alias) == code)
            .map(|(_, page, usage)| (*page, *usage))?,
    };
    name_for(page, usage).map(KeyExpr::new)
}

/// A mouse key as its ZMK behavior and constant.
fn mouse(key: HidKeyCode) -> Option<Binding> {
    let code = key as u16;
    MOUSE
        .iter()
        .find(|(_, _, mouse)| u16::from(*mouse) == code)
        .map(|(behavior, constant, _)| {
            Binding::new(behavior, vec![Param::Constant((*constant).to_string())])
        })
}

fn command(label: &str, name: &str, args: &[u32]) -> Binding {
    Binding::new(
        label,
        vec![Param::Command {
            name: name.to_string(),
            args: args.to_vec(),
        }],
    )
}

/// What the layers of a keyboard's configuration mean, for reading it.
pub struct Reverse<'a> {
    pub layers: &'a [LayerId],
    pub ble_profiles: u8,
}

/// A binding read from a keyboard, with a note when the keyboard's key
/// could not be kept as it was.
pub struct Read {
    pub binding: Binding,
    pub note: Option<String>,
}

impl Read {
    fn new(binding: Binding) -> Self {
        Self {
            binding,
            note: None,
        }
    }

    /// Nothing the layout can hold: `&none`, with the reason.
    fn none(what: impl std::fmt::Display) -> Self {
        Self {
            binding: Binding::none(),
            note: Some(format!(
                "{what} has no equivalent in a layout and was left unbound"
            )),
        }
    }

    fn noted(binding: Binding, note: impl Into<String>) -> Self {
        Self {
            binding,
            note: Some(note.into()),
        }
    }
}

/// A single wire action as a binding.
pub fn single_binding(action: Action, rx: &Reverse) -> Read {
    let layer = |index: u8| -> Result<LayerId, Read> {
        rx.layers.get(usize::from(index)).copied().ok_or_else(|| {
            Read::none(format!(
                "a key for layer {index}, which the configuration does not have,"
            ))
        })
    };
    let profiles = rx.ble_profiles;
    match action {
        Action::No => Read::new(Binding::none()),
        Action::Key(KeyCode::Hid(key)) => match (hid_key(key), mouse(key)) {
            (Some(expr), _) => Read::new(Binding::kp(expr)),
            (None, Some(binding)) => Read::new(binding),
            (None, None) => Read::none(format!("the key {key:?}")),
        },
        Action::Key(KeyCode::Consumer(key)) => {
            match name_for(UsagePage::Consumer.id(), u16::from(key)) {
                Some(name) => Read::new(Binding::kp(KeyExpr::new(name))),
                None => Read::none(format!("the consumer key {key:?}")),
            }
        }
        Action::Key(other) => Read::none(format!("the key {other:?}")),
        Action::KeyWithModifier(key, combination) => match hid_key(key) {
            Some(mut expr) => {
                for modifier in modifiers_of(combination).into_iter().rev() {
                    expr = expr.with(modifier);
                }
                Read::new(Binding::kp(expr))
            }
            None => Read::none(format!("the key {key:?}")),
        },
        Action::Modifier(combination) => match modifier_key(&modifiers_of(combination)) {
            Some(expr) => Read::new(Binding::kp(expr)),
            None => Read::new(Binding::none()),
        },
        Action::LayerOn(index) => {
            layer(index).map_or_else(|r| r, |l| Read::new(Binding::layer("mo", l)))
        }
        Action::LayerToggle(index) => {
            layer(index).map_or_else(|r| r, |l| Read::new(Binding::layer("tog", l)))
        }
        Action::LayerToggleOnly(index) => {
            layer(index).map_or_else(|r| r, |l| Read::new(Binding::layer("to", l)))
        }
        Action::DefaultLayer(index) | Action::PersistentDefaultLayer(index) => layer(index)
            .map_or_else(
                |r| r,
                |l| {
                    Read::noted(
                        Binding::layer("to", l),
                        "a default-layer key was read as a plain layer switch (&to)",
                    )
                },
            ),
        Action::OneShotLayer(index) => {
            layer(index).map_or_else(|r| r, |l| Read::new(Binding::layer("sl", l)))
        }
        Action::OneShotModifier(combination) => match modifier_key(&modifiers_of(combination)) {
            Some(expr) => Read::new(Binding::new("sk", vec![Param::Key(expr)])),
            None => Read::new(Binding::none()),
        },
        Action::OneShotKey(key) => match hid_key(key) {
            Some(expr) => Read::new(Binding::new("sk", vec![Param::Key(expr)])),
            None => Read::none(format!("the sticky key {key:?}")),
        },
        Action::LayerOnWithModifier(..) => Read::none("a layer-with-modifier key"),
        Action::TriLayerLower | Action::TriLayerUpper => Read::none("a tri-layer key"),
        Action::Light(light) => {
            let command_name = match light {
                LightAction::BacklightToggle => "RGB_TOG",
                LightAction::BacklightOn => "RGB_ON",
                LightAction::BacklightOff => "RGB_OFF",
                LightAction::RgbHui => "RGB_HUI",
                LightAction::RgbHud => "RGB_HUD",
                LightAction::RgbSai => "RGB_SAI",
                LightAction::RgbSad => "RGB_SAD",
                LightAction::RgbVai => "RGB_BRI",
                LightAction::RgbVad => "RGB_BRD",
                LightAction::RgbSpi => "RGB_SPI",
                LightAction::RgbSpd => "RGB_SPD",
                LightAction::RgbModeForward => "RGB_EFF",
                LightAction::RgbModeReverse => "RGB_EFR",
                LightAction::RgbTog => {
                    return Read::noted(
                        command("rgb_ug", "RGB_TOG", &[]),
                        "an animation toggle was read as the lighting toggle",
                    );
                }
                other => return Read::none(format!("the lighting key {other:?}")),
            };
            Read::new(command("rgb_ug", command_name, &[]))
        }
        Action::KeyboardControl(control) => match control {
            KeyboardAction::Bootloader => Read::new(Binding::new("bootloader", vec![])),
            KeyboardAction::Reboot => Read::new(Binding::new("sys_reset", vec![])),
            KeyboardAction::MaintenanceModeToggle => Read::noted(
                Binding::new("studio_unlock", vec![]),
                "the maintenance lock toggle was read as the Studio unlock key",
            ),
            KeyboardAction::CapsWordToggle => Read::new(Binding::new("caps_word", vec![])),
            KeyboardAction::OutputUsb => Read::new(command("out", "OUT_USB", &[])),
            KeyboardAction::OutputBluetooth => Read::new(command("out", "OUT_BLE", &[])),
            other => Read::none(format!("the keyboard control {other:?}")),
        },
        Action::Special(SpecialKey::GraveEscape) => Read::new(Binding::new("gresc", vec![])),
        Action::Special(SpecialKey::Repeat) => Read::new(Binding::new("key_repeat", vec![])),
        Action::Special(other) => Read::none(format!("the special key {other:?}")),
        Action::User(n) if n < profiles => Read::new(command("bt", "BT_SEL", &[u32::from(n)])),
        Action::User(n) if n == profiles => Read::new(command("bt", "BT_NXT", &[])),
        Action::User(n) if n == profiles + 1 => Read::new(command("bt", "BT_PRV", &[])),
        Action::User(n) if n == profiles + 2 || n == USER_CLEAR_ACTIVE_PROFILE => {
            Read::new(command("bt", "BT_CLR", &[]))
        }
        Action::User(n) if n == profiles + 3 => Read::new(command("out", "OUT_TOG", &[])),
        Action::User(USER_CLEAR_ALL_PROFILES) => Read::new(command("bt", "BT_CLR_ALL", &[])),
        Action::User(USER_PERIPHERAL_BOOTLOADER) => Read::new(Binding::new("bootloader", vec![])),
        Action::User(USER_SPLIT_TRANSPORT_TOGGLE) => Read::none("the split transport toggle"),
        Action::User(n) => Read::none(format!("user key {n}")),
        other => Read::none(format!("{other:?}")),
    }
}

/// The hold side of a wire hold-tap as a hold-tap's parameter and the
/// behavior that side uses.
pub fn hold_side(hold: Action, rx: &Reverse) -> Option<(BehaviorRef, Param)> {
    match hold {
        Action::Modifier(combination) => modifier_key(&modifiers_of(combination))
            .map(|expr| (BehaviorRef::built_in("kp"), Param::Key(expr))),
        other => match single_binding(other, rx).binding {
            Binding::Behavior {
                behavior: behavior @ BehaviorRef::BuiltIn(_),
                params,
            } if params.len() == 1 => Some((behavior, params[0].clone())),
            _ => None,
        },
    }
}

/// What a hold-tap taps, as a parameter of a `&kp`-tapping hold-tap.
pub fn tap_side(tap: Action) -> Option<Param> {
    match tap {
        Action::Key(KeyCode::Hid(key)) => hid_key(key).map(Param::Key),
        Action::KeyWithModifier(key, combination) => hid_key(key).map(|mut expr| {
            for modifier in modifiers_of(combination).into_iter().rev() {
                expr = expr.with(modifier);
            }
            Param::Key(expr)
        }),
        _ => None,
    }
}

/// Whether a wire action is a hold-tap on the firmware's default profile.
pub fn is_default_profile(profile: u8) -> bool {
    profile == u8::MAX
}

/// A wire action's text, for notes and reports.
pub fn describe(action: KeyAction) -> String {
    format_keycode(moergo_config::rynk_keycode::to_via_keycode(action))
}
