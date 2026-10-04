//! Helpers for per-key lighting: what a key shows once inheritance is
//! resolved, and a starting scheme derived from what the keys do.

use kc_zmk::keycodes::{keycodes, Category};

use crate::edit;
use crate::features::{KeyLight, Rgb};
use crate::ids::LayerId;
use crate::keycap::{keycap_for, KeycapKind};
use crate::project::Project;

/// The light a key shows on a layer, looking through keys that inherit to
/// the nearest layer below with a light of its own. The flag says whether
/// the light came from a lower layer.
pub fn effective(project: &Project, layer: LayerId, position: usize) -> (KeyLight, bool) {
    let Some(index) = project.layer_index(layer) else {
        return (KeyLight::Inherit, false);
    };
    for (depth, below) in project.layers[..=index].iter().rev().enumerate() {
        let light = project
            .lighting(below.id)
            .and_then(|l| l.keys.get(position))
            .copied()
            .unwrap_or_default();
        if light != KeyLight::Inherit {
            return (light, depth > 0);
        }
    }
    (KeyLight::Inherit, false)
}

/// The colour a light shows in its ordinary state: lock lights as when the
/// lock is on, battery lights as when the battery is charged.
pub fn display_color(light: KeyLight) -> Option<Rgb> {
    match light {
        KeyLight::Inherit | KeyLight::Off => None,
        KeyLight::Color(color) => Some(color),
        KeyLight::Lock { on, .. } => Some(on),
        KeyLight::Battery { above, .. } => Some(above),
    }
}

/// Colours for [`by_key_type`].
pub const KEY_COLOR: Rgb = Rgb(0x30, 0xA0, 0xA0);
pub const MODIFIER_COLOR: Rgb = Rgb(0xF7, 0x9A, 0x3E);
pub const LAYER_COLOR: Rgb = Rgb(0x8E, 0x4E, 0xC6);
pub const SYSTEM_COLOR: Rgb = Rgb(0xE5, 0x48, 0x4D);
pub const NAVIGATION_COLOR: Rgb = Rgb(0x46, 0xA7, 0x58);

/// A lighting scheme for a layer that colours keys by what they do, as a
/// starting point: modifiers, layer keys, navigation and system keys each
/// get a colour, transparent keys inherit and unused keys are unlit.
pub fn by_key_type(project: &Project, layer: LayerId) -> Vec<KeyLight> {
    let Some(layer) = project.layer(layer) else {
        return Vec::new();
    };
    layer
        .bindings
        .iter()
        .map(|binding| {
            let cap = keycap_for(project, binding);
            let category = edit::tap_key(binding)
                .and_then(|key| keycodes().get(&key.key))
                .map(|code| code.category);
            match (cap.kind, category) {
                (KeycapKind::Transparent, _) => KeyLight::Inherit,
                (KeycapKind::None, _) => KeyLight::Off,
                (KeycapKind::Layer, _) => KeyLight::Color(LAYER_COLOR),
                (KeycapKind::System | KeycapKind::Raw, _) => KeyLight::Color(SYSTEM_COLOR),
                (_, Some(Category::Modifiers)) => KeyLight::Color(MODIFIER_COLOR),
                (_, Some(Category::Navigation | Category::Function)) => {
                    KeyLight::Color(NAVIGATION_COLOR)
                }
                _ => KeyLight::Color(KEY_COLOR),
            }
        })
        .collect()
}
