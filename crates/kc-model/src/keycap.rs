//! What to print on a keycap for a binding.

use kc_zmk::keycodes::keycodes;
use kc_zmk::Modifier;

use crate::behavior::BehaviorKind;
use crate::binding::{BehaviorRef, Binding, KeyExpr, Param};
use crate::ids::LayerId;
use crate::project::Project;

/// How a keycap should be styled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeycapKind {
    /// Sends a key.
    Key,
    /// Changes the active layer.
    Layer,
    /// Falls through to the layer below; the legend is what it inherits.
    Transparent,
    /// Does nothing.
    None,
    /// Bluetooth, lighting, reset and other keyboard functions.
    System,
    /// Binding text the model does not understand.
    Raw,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keycap {
    /// The main legend: what a tap does.
    pub legend: String,
    /// What holding the key does, for hold-tap style bindings.
    pub hold: Option<String>,
    pub kind: KeycapKind,
}

impl Keycap {
    fn new(legend: impl Into<String>, kind: KeycapKind) -> Self {
        Self {
            legend: legend.into(),
            hold: None,
            kind,
        }
    }
}

fn modifier_symbol(modifier: Modifier) -> &'static str {
    match modifier {
        Modifier::LCtrl | Modifier::RCtrl => "⌃",
        Modifier::LShift | Modifier::RShift => "⇧",
        Modifier::LAlt | Modifier::RAlt => "⌥",
        Modifier::LGui | Modifier::RGui => "⌘",
    }
}

/// A key expression as keycap text, such as `⌃⇧K`.
pub fn key_legend(expr: &KeyExpr) -> String {
    let mods: String = expr.mods.iter().copied().map(modifier_symbol).collect();
    let key = keycodes()
        .get(&expr.key)
        .map_or(expr.key.as_str(), |k| k.legend.as_str());
    format!("{mods}{key}")
}

fn layer_name(project: &Project, id: LayerId) -> String {
    project
        .layer(id)
        .map_or_else(|| "?".to_string(), |l| l.name.clone())
}

fn param_legend(project: &Project, param: &Param) -> String {
    match param {
        Param::Key(expr) => key_legend(expr),
        Param::Layer(id) => layer_name(project, *id),
        Param::Constant(name) => name.clone(),
        Param::Command { name, args } => {
            // `BT_SEL 2` reads better as `BT SEL 2` on a keycap.
            let mut text = name.replace('_', " ");
            for arg in args {
                text.push_str(&format!(" {arg}"));
            }
            text
        }
        Param::Number(n) => n.to_string(),
    }
}

fn built_in(project: &Project, label: &str, params: &[Param]) -> Keycap {
    let first = params.first().map(|p| param_legend(project, p));
    let second = params.get(1).map(|p| param_legend(project, p));
    match label {
        "trans" => Keycap::new("", KeycapKind::Transparent),
        "none" => Keycap::new("", KeycapKind::None),
        "kp" => Keycap::new(first.unwrap_or_default(), KeycapKind::Key),
        "mt" | "lt" => Keycap {
            legend: second.unwrap_or_default(),
            hold: first,
            kind: KeycapKind::Key,
        },
        "mo" | "to" | "tog" | "sl" => Keycap {
            legend: first.unwrap_or_default(),
            hold: Some(
                match label {
                    "mo" => "hold",
                    "to" => "go to",
                    "tog" => "toggle",
                    _ => "sticky",
                }
                .to_string(),
            ),
            kind: KeycapKind::Layer,
        },
        "sk" | "kt" => Keycap {
            legend: first.unwrap_or_default(),
            hold: Some(if label == "sk" { "sticky" } else { "toggle" }.to_string()),
            kind: KeycapKind::Key,
        },
        "mkp" | "mmv" | "msc" => Keycap::new(first.unwrap_or_default(), KeycapKind::Key),
        "bt" | "out" | "rgb_ug" | "bl" | "ext_power" => {
            Keycap::new(first.unwrap_or_default(), KeycapKind::System)
        }
        other => {
            let name = kc_zmk::behaviors::built_in(other).map_or(other, |b| b.name);
            Keycap::new(name, KeycapKind::System)
        }
    }
}

/// The keycap for a binding, without resolving transparency.
pub fn keycap_for(project: &Project, binding: &Binding) -> Keycap {
    let (behavior, params) = match binding {
        Binding::Raw { raw } => return Keycap::new(raw.clone(), KeycapKind::Raw),
        Binding::Behavior { behavior, params } => (behavior, params.as_slice()),
    };
    match behavior {
        BehaviorRef::BuiltIn(label) => built_in(project, label, params),
        BehaviorRef::User { user } => {
            let Some(def) = project.behavior(*user) else {
                return Keycap::new("?", KeycapKind::Raw);
            };
            match &def.kind {
                BehaviorKind::HoldTap(_) => Keycap {
                    legend: params
                        .get(1)
                        .map(|p| param_legend(project, p))
                        .unwrap_or_default(),
                    hold: params.first().map(|p| param_legend(project, p)),
                    kind: KeycapKind::Key,
                },
                BehaviorKind::StickyKey(_) => Keycap {
                    legend: params
                        .first()
                        .map(|p| param_legend(project, p))
                        .unwrap_or_default(),
                    hold: Some("sticky".to_string()),
                    kind: KeycapKind::Key,
                },
                _ => Keycap::new(def.name.clone(), KeycapKind::System),
            }
        }
    }
}

/// The keycap shown at a key on a layer. A transparent key shows what it
/// inherits from the nearest layer below that binds the key, keeping the
/// [`KeycapKind::Transparent`] kind so it can be drawn ghosted.
pub fn keycap(project: &Project, layer: LayerId, position: usize) -> Option<Keycap> {
    let index = project.layer_index(layer)?;
    let own = keycap_for(project, project.layers[index].bindings.get(position)?);
    if own.kind != KeycapKind::Transparent {
        return Some(own);
    }
    let inherited = project.layers[..index]
        .iter()
        .rev()
        .filter_map(|l| l.bindings.get(position))
        .map(|b| keycap_for(project, b))
        .find(|k| k.kind != KeycapKind::Transparent);
    Some(Keycap {
        kind: KeycapKind::Transparent,
        ..inherited.unwrap_or(own)
    })
}
