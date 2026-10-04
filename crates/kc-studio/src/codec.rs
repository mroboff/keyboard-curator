//! Converts between the project's bindings and the three numbers the
//! Studio protocol uses for one: a behaviour ID and two parameters.

use std::collections::HashMap;

use kc_model::behavior::BehaviorKind;
use kc_model::{BehaviorRef, Binding, KeyExpr, LayerId, Param, Project};
use kc_zmk::behaviors::{self, ParamKind};
use kc_zmk::keycodes::keycodes;
use kc_zmk::Modifier;

use crate::client::DeviceBehavior;
use crate::proto::keymap::{BehaviorBinding, Keymap};

/// The display names ZMK gives its built-in behaviours, which is how the
/// keyboard identifies them.
const DISPLAY_NAMES: [(&str, &str); 21] = [
    ("kp", "Key Press"),
    ("kt", "Key Toggle"),
    ("sk", "Sticky Key"),
    ("mt", "Mod-Tap"),
    ("lt", "Layer-Tap"),
    ("mo", "Momentary Layer"),
    ("to", "To Layer"),
    ("tog", "Toggle Layer"),
    ("sl", "Sticky Layer"),
    ("trans", "Transparent"),
    ("none", "None"),
    ("gresc", "Grave/Escape"),
    ("caps_word", "Caps Word"),
    ("key_repeat", "Key Repeat"),
    ("mkp", "Mouse Key Press"),
    ("bt", "Bluetooth"),
    ("out", "Output Selection"),
    ("rgb_ug", "Underglow"),
    ("ext_power", "External Power"),
    ("sys_reset", "Reset"),
    ("bootloader", "Bootloader"),
];

/// The values of the named constants and commands, from ZMK's headers.
const VALUES: [(&str, u32); 30] = [
    ("BT_CLR", 0),
    ("BT_NXT", 1),
    ("BT_PRV", 2),
    ("BT_SEL", 3),
    ("BT_CLR_ALL", 4),
    ("BT_DISC", 5),
    ("OUT_TOG", 0),
    ("OUT_USB", 1),
    ("OUT_BLE", 2),
    ("RGB_TOG", 0),
    ("RGB_ON", 1),
    ("RGB_OFF", 2),
    ("RGB_HUI", 3),
    ("RGB_HUD", 4),
    ("RGB_SAI", 5),
    ("RGB_SAD", 6),
    ("RGB_BRI", 7),
    ("RGB_BRD", 8),
    ("RGB_SPI", 9),
    ("RGB_SPD", 10),
    ("RGB_EFF", 11),
    ("RGB_EFR", 12),
    ("EP_OFF", 0),
    ("EP_ON", 1),
    ("EP_TOG", 2),
    ("LCLK", 1),
    ("RCLK", 2),
    ("MCLK", 4),
    ("MB4", 8),
    ("MB5", 16),
];

fn value_of(name: &str) -> Option<u32> {
    VALUES.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

/// A key expression as ZMK encodes it: modifiers, usage page, usage.
pub fn encode_key(expr: &KeyExpr) -> Option<u32> {
    let code = keycodes().get(&expr.key)?;
    let mods = expr
        .mods
        .iter()
        .chain(&code.implicit_mods)
        .fold(0u32, |bits, m| bits | u32::from(m.bit()));
    Some(mods << 24 | u32::from(code.page.id()) << 16 | u32::from(code.usage))
}

pub fn decode_key(value: u32) -> Option<KeyExpr> {
    let (bits, page, usage) = (
        (value >> 24) as u8,
        ((value >> 16) & 0xFF) as u16,
        (value & 0xFFFF) as u16,
    );
    let matching = || {
        keycodes()
            .all()
            .iter()
            .filter(move |k| k.page.id() == page && k.usage == usage)
    };
    // Prefer a name that already includes the modifiers, such as `EXCL`.
    let named = matching().find(|k| {
        !k.implicit_mods.is_empty() && k.implicit_mods.iter().fold(0u8, |b, m| b | m.bit()) == bits
    });
    if let Some(code) = named {
        return Some(KeyExpr::new(code.short_name()));
    }
    let code = matching().find(|k| k.implicit_mods.is_empty())?;
    let mods = Modifier::ALL
        .into_iter()
        .filter(|m| bits & m.bit() != 0)
        .collect();
    Some(KeyExpr {
        mods,
        key: code.short_name().to_string(),
    })
}

/// Which of the keyboard's behaviour IDs is which behaviour of the project.
#[derive(Debug, Clone, Default)]
pub struct BehaviorTable {
    to_device: HashMap<BehaviorRef, u32>,
    from_device: HashMap<u32, BehaviorRef>,
}

impl BehaviorTable {
    /// Matches the keyboard's behaviours to the project's: built-ins by
    /// ZMK's display names, the project's own by their label.
    pub fn new(project: &Project, device: &[DeviceBehavior]) -> Self {
        let mut table = Self::default();
        for behavior in device {
            let built_in = DISPLAY_NAMES
                .iter()
                .find(|(_, name)| *name == behavior.display_name)
                .map(|(label, _)| BehaviorRef::built_in(label));
            let user = project
                .behaviors
                .iter()
                .find(|b| b.label == behavior.display_name || b.name == behavior.display_name)
                .map(|b| BehaviorRef::User { user: b.id });
            if let Some(reference) = built_in.or(user) {
                table
                    .to_device
                    .entry(reference.clone())
                    .or_insert(behavior.id);
                table.from_device.insert(behavior.id, reference);
            }
        }
        table
    }
}

/// What kind of value each parameter of a behaviour is, as far as the
/// protocol needs to know.
fn param_kinds(project: &Project, behavior: &BehaviorRef) -> Option<Vec<Option<ParamKind>>> {
    match behavior {
        BehaviorRef::BuiltIn(label) => Some(
            behaviors::built_in(label)?
                .params
                .iter()
                .map(|p| Some(p.kind))
                .collect(),
        ),
        BehaviorRef::User { user } => {
            let first = |b: &BehaviorRef| match b {
                BehaviorRef::BuiltIn(label) => behaviors::built_in(label)
                    .and_then(|d| d.params.first())
                    .map(|p| p.kind),
                BehaviorRef::User { .. } => None,
            };
            Some(match &project.behavior(*user)?.kind {
                BehaviorKind::HoldTap(h) => vec![first(&h.hold), first(&h.tap)],
                BehaviorKind::StickyKey(s) => vec![first(&s.behavior)],
                BehaviorKind::Macro(m) => vec![None; m.params as usize],
                BehaviorKind::TapDance(_) | BehaviorKind::ModMorph(_) => vec![],
            })
        }
    }
}

/// A project binding as the protocol sends it, or `None` if it cannot be
/// sent this way and needs a firmware build.
pub fn encode(
    project: &Project,
    table: &BehaviorTable,
    binding: &Binding,
) -> Option<BehaviorBinding> {
    let Binding::Behavior { behavior, params } = binding else {
        return None;
    };
    let behavior_id = *table.to_device.get(behavior)? as i32;
    let mut values = [0u32; 2];
    match params.as_slice() {
        // A command takes both numbers: its code and its argument.
        [Param::Command { name, args }] => {
            values[0] = value_of(name)?;
            values[1] = match args.as_slice() {
                [] => 0,
                [argument] => *argument,
                _ => return None,
            };
        }
        params if params.len() <= 2 => {
            for (slot, param) in values.iter_mut().zip(params) {
                *slot = match param {
                    Param::Key(expr) => encode_key(expr)?,
                    // A layer's ID on the keyboard is its position in the
                    // keymap the firmware was built from.
                    Param::Layer(id) => project.layer_index(*id)? as u32,
                    Param::Constant(name) => value_of(name)?,
                    Param::Number(n) => u32::try_from(*n).ok()?,
                    Param::Command { .. } => return None,
                };
            }
        }
        _ => return None,
    }
    Some(BehaviorBinding {
        behavior_id,
        param1: values[0],
        param2: values[1],
    })
}

/// A binding from the keyboard as a project binding, or `None` if the
/// project has nothing that matches it.
pub fn decode(
    project: &Project,
    table: &BehaviorTable,
    binding: &BehaviorBinding,
) -> Option<Binding> {
    let behavior = table
        .from_device
        .get(&u32::try_from(binding.behavior_id).ok()?)?;
    let kinds = param_kinds(project, behavior)?;
    let values = [binding.param1, binding.param2];
    let name_of = |value: u32, names: Vec<&'static str>| {
        names
            .into_iter()
            .find(|n| value_of(n) == Some(value))
            .map(str::to_string)
    };
    let layer = |value: u32| {
        project
            .layers
            .get(value as usize)
            .map(|l: &kc_model::Layer| -> LayerId { l.id })
    };
    let mut params = Vec::new();
    for (kind, value) in kinds.iter().zip(values) {
        params.push(match kind {
            Some(ParamKind::Keycode) => Param::Key(decode_key(value)?),
            Some(ParamKind::Layer) => Param::Layer(layer(value)?),
            Some(ParamKind::Constant(constants)) => {
                Param::Constant(name_of(value, constants.iter().map(|c| c.name).collect())?)
            }
            Some(ParamKind::Command(commands)) => {
                let name = name_of(value, commands.iter().map(|c| c.name).collect())?;
                let command = commands.iter().find(|c| c.name == name)?;
                let args = match command.args.len() {
                    0 => vec![],
                    1 => vec![binding.param2],
                    _ => return None,
                };
                Param::Command { name, args }
            }
            None => Param::Number(i64::from(value)),
        });
    }
    Some(Binding::Behavior {
        behavior: behavior.clone(),
        params,
    })
}

/// One key to change on the keyboard.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub layer_id: u32,
    pub position: usize,
    pub binding: BehaviorBinding,
}

/// How a project differs from what a keyboard is running.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Comparison {
    /// Keys that can be changed on the keyboard directly.
    pub changes: Vec<Change>,
    /// How many keys differ in ways only a firmware build can apply.
    pub needs_build: usize,
    /// Why the two cannot be compared key by key, if they cannot.
    pub mismatch: Option<String>,
}

/// Compares a project with the keymap on a keyboard that runs firmware
/// built from it.
pub fn compare(project: &Project, table: &BehaviorTable, device: &Keymap) -> Comparison {
    let mut result = Comparison::default();
    if device.layers.len() != project.layers.len() {
        result.mismatch = Some(format!(
            "The keyboard has {} layers and the project has {}. Adding or removing layers needs a firmware build.",
            device.layers.len(),
            project.layers.len()
        ));
        return result;
    }
    for (index, layer) in project.layers.iter().enumerate() {
        // Layers on the keyboard are found by ID, which is the position
        // the layer had when the firmware was built.
        let Some(on_device) = device.layers.iter().find(|l| l.id as usize == index) else {
            result.mismatch = Some("The keyboard's layers have been rearranged. A firmware build will bring it back in line.".to_string());
            return result;
        };
        if on_device.bindings.len() != layer.bindings.len() {
            result.mismatch = Some("The keyboard has a different number of keys per layer. Is this the right keyboard?".to_string());
            return result;
        }
        for (position, (binding, current)) in
            layer.bindings.iter().zip(&on_device.bindings).enumerate()
        {
            match encode(project, table, binding) {
                Some(wanted) if wanted == *current => {}
                Some(wanted) => result.changes.push(Change {
                    layer_id: on_device.id,
                    position,
                    binding: wanted,
                }),
                None => {
                    // It may still be what the keyboard already has.
                    if decode(project, table, current).as_ref() != Some(binding) {
                        result.needs_build += 1;
                    }
                }
            }
        }
    }
    result
}
