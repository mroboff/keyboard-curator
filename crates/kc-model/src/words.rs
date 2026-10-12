//! The app's words for what keys do: one glossary that the picker, the
//! keycaps, the key setup panel, messages and the user guide all draw
//! from, so that no ZMK label reaches the screen outside Advanced.

use kc_zmk::behaviors::{self, ParamKind};
use kc_zmk::Modifier;

use crate::behavior::{BehaviorKind, Flavor, Settings};
use crate::binding::{BehaviorRef, Binding, Param};
use crate::keycap::key_legend;
use crate::project::Project;
use crate::setup::side_binding;

/// The symbol for a modifier, as keycaps and the panel show it.
pub fn modifier_symbol(modifier: Modifier) -> &'static str {
    match modifier {
        Modifier::LCtrl | Modifier::RCtrl => "⌃",
        Modifier::LShift | Modifier::RShift => "⇧",
        Modifier::LAlt | Modifier::RAlt => "⌥",
        Modifier::LGui | Modifier::RGui => "⌘",
    }
}

/// A modifier with its side, such as `Left ⇧`.
pub fn modifier_name(modifier: Modifier) -> String {
    let side = match modifier {
        Modifier::LCtrl | Modifier::LShift | Modifier::LAlt | Modifier::LGui => "Left",
        _ => "Right",
    };
    format!("{side} {}", modifier_symbol(modifier))
}

/// The symbols of several modifiers run together, such as `⌃⇧`.
pub fn modifier_symbols(mods: &[Modifier]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for m in mods {
        let symbol = modifier_symbol(*m);
        if !seen.contains(&symbol) {
            seen.push(symbol);
        }
    }
    seen.concat()
}

/// A command as a short keycap legend: `BT 2`, `Light on`, `Hue +`. Commands
/// without a short form are written out from their name.
pub fn command_legend(name: &str, args: &[u32]) -> String {
    let first = args.first().map(u32::to_string).unwrap_or_default();
    match name {
        "BT_SEL" => format!("BT {first}"),
        "BT_NXT" => "BT next".into(),
        "BT_PRV" => "BT prev".into(),
        "BT_CLR" => "BT clear".into(),
        "BT_CLR_ALL" => "BT clear all".into(),
        "BT_DISC" => format!("BT {first} off"),
        "OUT_USB" => "USB".into(),
        "OUT_BLE" => "Bluetooth".into(),
        "OUT_TOG" => "USB/BT".into(),
        "OUT_NONE" => "No output".into(),
        "RGB_TOG" | "BL_TOG" => "Light".into(),
        "RGB_ON" | "BL_ON" => "Light on".into(),
        "RGB_OFF" | "BL_OFF" => "Light off".into(),
        "RGB_HUI" => "Hue +".into(),
        "RGB_HUD" => "Hue −".into(),
        "RGB_SAI" => "Sat +".into(),
        "RGB_SAD" => "Sat −".into(),
        "RGB_BRI" | "BL_INC" => "Bright +".into(),
        "RGB_BRD" | "BL_DEC" => "Bright −".into(),
        "RGB_SPI" => "Speed +".into(),
        "RGB_SPD" => "Speed −".into(),
        "RGB_EFF" => "Effect ›".into(),
        "RGB_EFR" => "‹ Effect".into(),
        "RGB_COLOR_HSB" => "Color".into(),
        "RGB_STATUS" => "Status".into(),
        "BL_CYCLE" => "Bright ›".into(),
        "BL_SET" => format!("Bright {first}"),
        "EP_ON" => "Power on".into(),
        "EP_OFF" => "Power off".into(),
        "EP_TOG" => "Power".into(),
        _ => {
            let mut text = name.replace('_', " ");
            for arg in args {
                text.push_str(&format!(" {arg}"));
            }
            text
        }
    }
}

/// What a layer behavior does with a layer, in words: `Nav while held`.
pub fn layer_action(label: &str, layer: &str) -> Option<String> {
    Some(match label {
        "mo" => format!("{layer} while held"),
        "to" => format!("Switch to {layer}"),
        "tog" => format!("Toggle {layer}"),
        "sl" => format!("{layer} for the next key"),
        _ => return None,
    })
}

fn layer_name(project: &Project, id: crate::ids::LayerId) -> String {
    project
        .layer(id)
        .map_or_else(|| "a missing layer".to_string(), |l| l.name.clone())
}

fn built_in(project: &Project, label: &str, params: &[Param]) -> String {
    match (label, params) {
        ("trans", _) => return "Same as the layer below".into(),
        ("none", _) => return "Nothing".into(),
        ("kp", [Param::Key(key)]) => return key_legend(key),
        ("kt", [Param::Key(key)]) => return format!("Toggle {}", key_legend(key)),
        ("sk", [Param::Key(key)]) => return format!("Sticky {}", key_legend(key)),
        ("mt", [Param::Key(hold), Param::Key(tap)]) => {
            return format!("{}, hold for {}", key_legend(tap), key_legend(hold));
        }
        ("lt", [Param::Layer(layer), Param::Key(tap)]) => {
            return format!(
                "{}, hold for {}",
                key_legend(tap),
                layer_name(project, *layer)
            );
        }
        ("mo" | "to" | "tog" | "sl", [Param::Layer(layer)]) => {
            if let Some(text) = layer_action(label, &layer_name(project, *layer)) {
                return text;
            }
        }
        _ => {}
    }
    let Some(def) = behaviors::built_in(label) else {
        return format!("&{label}");
    };
    match (def.params, params) {
        ([param], [Param::Constant(name)]) => {
            if let ParamKind::Constant(constants) = param.kind {
                if let Some(c) = constants.iter().find(|c| c.name == name) {
                    return c.description.to_string();
                }
            }
        }
        // Commands read as their keycap legends, which the picker shows too.
        ([_], [Param::Command { name, args }]) => return command_legend(name, args),
        _ => {}
    }
    def.name.to_string()
}

/// What a binding does, in words: `A`, `Nav while held`, `A, hold for ⇧`,
/// `Bluetooth profile 2`.
pub fn describe(project: &Project, binding: &Binding) -> String {
    describe_at(project, binding, 0)
}

fn describe_at(project: &Project, binding: &Binding, depth: u8) -> String {
    let (behavior, params) = match binding {
        Binding::Raw { raw } => return raw.clone(),
        Binding::Behavior { behavior, params } => (behavior, params.as_slice()),
    };
    let user = match behavior {
        BehaviorRef::BuiltIn(label) => return built_in(project, label, params),
        BehaviorRef::User { user } => *user,
    };
    let Some(def) = project.behavior(user) else {
        return "A missing behavior".into();
    };
    if depth > 6 {
        return def.display_name();
    }
    let inner = |b: &Binding| describe_at(project, b, depth + 1);
    match &def.kind {
        BehaviorKind::HoldTap(h) => format!(
            "{}, hold for {}",
            inner(&side_binding(project, &h.tap, params.get(1))),
            hold_words(
                project,
                &side_binding(project, &h.hold, params.first()),
                depth
            )
        ),
        BehaviorKind::TapDance(t) => {
            let mut parts = Vec::new();
            for (index, b) in t.bindings.iter().enumerate() {
                parts.push(match index {
                    0 => inner(b),
                    1 => format!("tap twice for {}", inner(b)),
                    2 => format!("three times for {}", inner(b)),
                    n => format!("{} times for {}", n + 1, inner(b)),
                });
            }
            parts.join("; ")
        }
        BehaviorKind::ModMorph(m) => format!(
            "{}, with {} held {}",
            inner(&m.normal),
            modifier_symbols(&m.mods),
            inner(&m.morphed)
        ),
        BehaviorKind::StickyKey(s) => format!(
            "Sticky {}",
            inner(&side_binding(project, &s.behavior, params.first()))
        ),
        BehaviorKind::Macro(_) => {
            if def.name.is_empty() {
                "Macro".into()
            } else {
                format!("Macro: {}", def.name)
            }
        }
    }
}

/// What holding a key does, after "hold for": a layer by its name, since
/// "while held" goes without saying there.
fn hold_words(project: &Project, hold: &Binding, depth: u8) -> String {
    if let Binding::Behavior {
        behavior: BehaviorRef::BuiltIn(label),
        params,
    } = hold
    {
        if let ("mo", [Param::Layer(layer)]) = (label.as_str(), params.as_slice()) {
            return layer_name(project, *layer);
        }
    }
    describe_at(project, hold, depth + 1)
}

impl Flavor {
    /// What to call the way a hold-tap decides.
    pub fn name(self) -> &'static str {
        match self {
            Flavor::HoldPreferred => "Favor hold",
            Flavor::Balanced => "Balanced",
            Flavor::TapPreferred => "Favor tap",
            Flavor::TapUnlessInterrupted => "Tap unless another key is pressed",
        }
    }

    /// One sentence on when it holds.
    pub fn detail(self) -> &'static str {
        match self {
            Flavor::HoldPreferred => {
                "Holds once the hold time passes, or as soon as another key is pressed."
            }
            Flavor::Balanced => {
                "Holds once the hold time passes, or when another key is pressed and released first."
            }
            Flavor::TapPreferred => {
                "Holds only once the hold time passes; other keys make no difference."
            }
            Flavor::TapUnlessInterrupted => {
                "Taps unless another key is pressed within the hold time."
            }
        }
    }

    pub const ALL: [Flavor; 4] = [
        Flavor::HoldPreferred,
        Flavor::Balanced,
        Flavor::TapPreferred,
        Flavor::TapUnlessInterrupted,
    ];
}

impl Settings {
    /// The settings in one line, for a template card or a key's caption.
    pub fn summary(&self) -> String {
        match self {
            Settings::HoldTap(s) => {
                let mut parts = vec![
                    s.flavor.name().to_string(),
                    format!("counts as a hold after {} ms", s.tapping_term_ms),
                ];
                if s.quick_tap_ms.is_some() {
                    parts.push("tap twice quickly to repeat".into());
                }
                if s.require_prior_idle_ms.is_some() {
                    parts.push("no hold right after typing".into());
                }
                if s.opposite_hand_hold {
                    parts.push("only with keys on the other hand".into());
                } else if !s.hold_trigger_key_positions.is_empty() {
                    parts.push(format!(
                        "only with {} chosen keys",
                        s.hold_trigger_key_positions.len()
                    ));
                }
                if s.hold_trigger_on_release {
                    parts.push("decided when the other key is released".into());
                }
                if s.retro_tap {
                    parts.push("tap if released with nothing else pressed".into());
                }
                if s.hold_while_undecided {
                    parts.push("hold starts right away".into());
                }
                parts.join(", ")
            }
            Settings::TapDance(s) => format!("a second tap within {} ms", s.tapping_term_ms),
            Settings::ModMorph(s) => {
                let mut text = format!("with {} held", modifier_symbols(&s.mods));
                if !s.keep_mods.is_empty() {
                    text.push_str(&format!(", keeping {}", modifier_symbols(&s.keep_mods)));
                }
                text
            }
            Settings::StickyKey(s) => {
                let mut parts = vec![format!("lets go after {} ms", s.release_after_ms)];
                if s.quick_release {
                    parts.push("or as soon as the next key is pressed".into());
                }
                if s.lazy {
                    parts.push("pressed only when the next key is".into());
                }
                if s.ignore_modifiers {
                    parts.push("lets modifiers through".into());
                }
                parts.join(", ")
            }
            Settings::Macro(s) => match (s.wait_ms, s.tap_ms) {
                (None, None) => "default timing".into(),
                (wait, tap) => {
                    let mut parts = Vec::new();
                    if let Some(w) = wait {
                        parts.push(format!("{w} ms between steps"));
                    }
                    if let Some(t) = tap {
                        parts.push(format!("taps held {t} ms"));
                    }
                    parts.join(", ")
                }
            },
        }
    }
}
