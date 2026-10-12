//! Copying keys and layers as text, so they can be pasted into another
//! layer or another project for the same board.
//!
//! Bindings travel with the definitions they need and a table of the
//! layers they name, because IDs mean nothing outside the project they
//! came from: on paste, layers are matched by name and definitions are
//! recreated, merging with any identical ones already there.

use serde::{Deserialize, Serialize};

use crate::behavior::{BehaviorDef, Scope};
use crate::binding::{BehaviorRef, Binding, Param};
use crate::ids::{BehaviorId, LayerId};
use crate::project::{ModelError, Project};
use crate::text::{format_binding, LayerStyle};

/// The clipboard payload version this build writes.
const VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ClipKind {
    Keys,
    Layer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ClipKey {
    position: usize,
    binding: Binding,
    /// The binding as keymap text with layers by name, kept for a target
    /// that lacks a layer the binding names.
    text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Clip {
    /// Marks the text as ours, and versions the payload.
    keyboard_curator: u32,
    board: String,
    kind: ClipKind,
    keys: Vec<ClipKey>,
    /// The layers the bindings and definitions refer to, by ID and name.
    #[serde(default)]
    layers: Vec<(LayerId, String)>,
    /// The definitions the bindings need, nested ones included.
    #[serde(default)]
    behaviors: Vec<BehaviorDef>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PasteError {
    #[error("the clipboard does not hold keys copied from Keyboard Curator")]
    NotKeys,
    #[error("those keys were copied from a different keyboard (`{0}`)")]
    OtherBoard(String),
    #[error("select a key to paste onto")]
    NoTarget,
    #[error(transparent)]
    Model(#[from] ModelError),
}

/// Every definition `binding` reaches, nested ones included, in an order
/// that defines each before what uses it.
fn needed(project: &Project, binding: &Binding, out: &mut Vec<BehaviorDef>) {
    let Some(id) = binding.user_behavior() else {
        return;
    };
    if out.iter().any(|d| d.id == id) {
        return;
    }
    let Some(def) = project.behavior(id) else {
        return;
    };
    for inner in def.kind.bindings() {
        needed(project, inner, out);
    }
    for behavior in def.kind.behavior_refs() {
        if let BehaviorRef::User { user } = behavior {
            needed(project, &Binding::user(*user, vec![]), out);
        }
    }
    if let Some(template) = def.follows {
        needed(project, &Binding::user(template, vec![]), out);
    }
    out.push(def.clone());
}

fn clip(project: &Project, layer: LayerId, kind: ClipKind, positions: &[usize]) -> Option<String> {
    let bindings = &project.layer(layer)?.bindings;
    let mut positions = positions.to_vec();
    positions.sort_unstable();
    positions.dedup();
    let mut behaviors = Vec::new();
    let keys: Vec<ClipKey> = positions
        .into_iter()
        .filter_map(|position| {
            let binding = bindings.get(position)?;
            needed(project, binding, &mut behaviors);
            Some(ClipKey {
                position,
                binding: binding.clone(),
                text: format_binding(project, binding, LayerStyle::Constant),
            })
        })
        .collect();
    if keys.is_empty() {
        return None;
    }
    serde_json::to_string(&Clip {
        keyboard_curator: VERSION,
        board: project.board.clone(),
        kind,
        keys,
        layers: project
            .layers
            .iter()
            .map(|l| (l.id, l.name.clone()))
            .collect(),
        behaviors,
    })
    .ok()
}

/// Clipboard text for the keys at `positions` on a layer.
pub fn copy_keys(project: &Project, layer: LayerId, positions: &[usize]) -> Option<String> {
    clip(project, layer, ClipKind::Keys, positions)
}

/// Clipboard text for a whole layer.
pub fn copy_layer(project: &Project, layer: LayerId) -> Option<String> {
    let all: Vec<usize> = (0..project.key_count).collect();
    clip(project, layer, ClipKind::Layer, &all)
}

/// Maps a clip's IDs onto the target project's.
struct Remap {
    layers: Vec<(LayerId, Option<LayerId>)>,
    behaviors: Vec<(BehaviorId, BehaviorId)>,
}

impl Remap {
    fn layer(&self, id: LayerId) -> Option<LayerId> {
        self.layers
            .iter()
            .find(|(from, _)| *from == id)
            .map_or(Some(id), |(_, to)| *to)
    }

    fn behavior(&self, id: BehaviorId) -> BehaviorId {
        self.behaviors
            .iter()
            .find(|(from, _)| *from == id)
            .map_or(id, |(_, to)| *to)
    }

    /// `binding` with its IDs mapped, or `None` when it names a layer the
    /// target does not have.
    fn binding(&self, binding: &Binding) -> Option<Binding> {
        let Binding::Behavior { behavior, params } = binding else {
            return Some(binding.clone());
        };
        let behavior = match behavior {
            BehaviorRef::User { user } => BehaviorRef::User {
                user: self.behavior(*user),
            },
            built_in => built_in.clone(),
        };
        let params = params
            .iter()
            .map(|p| match p {
                Param::Layer(id) => self.layer(*id).map(Param::Layer),
                other => Some(other.clone()),
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Binding::Behavior { behavior, params })
    }
}

/// Recreates a clip's definitions in `project` and returns how its IDs map
/// onto the project's. Identical definitions already there are reused; a
/// definition that names a missing layer is still brought over, with that
/// binding written as text.
fn bring_definitions(project: &mut Project, clip: &Clip, remap: &mut Remap) {
    for source in &clip.behaviors {
        let mut kind = source.kind.clone();
        for binding in kind.bindings_mut() {
            *binding = remap.binding(binding).unwrap_or_else(|| Binding::Raw {
                raw: format_binding(project, binding, LayerStyle::Constant),
            });
        }
        for behavior in kind.behavior_refs_mut() {
            if let BehaviorRef::User { user } = behavior {
                *user = remap.behavior(*user);
            }
        }
        let follows = source.follows.map(|id| remap.behavior(id));
        let existing = project.behaviors.iter().find(|d| {
            d.kind == kind
                && d.follows == follows
                && (d.scope == Scope::Key || d.label == source.label)
                && d.scope == source.scope
        });
        let id = match existing {
            Some(def) => def.id,
            None => {
                let label = if project.label_is_free(&source.label) {
                    source.label.clone()
                } else {
                    project.label_for(&source.label)
                };
                let id = project
                    .add_behavior(label, source.name.clone(), kind)
                    .expect("a free label is valid");
                let def = project.behavior_mut(id).expect("just added");
                def.description = source.description.clone();
                def.scope = source.scope;
                def.follows = follows;
                id
            }
        };
        remap.behaviors.push((source.id, id));
    }
}

/// Pastes clipboard text onto a layer and returns how many keys changed.
///
/// A whole layer replaces the layer. A single key goes onto every target.
/// Several keys keep their arrangement, anchored at the first target; any
/// that would land off the keyboard are skipped.
pub fn paste(
    project: &mut Project,
    layer: LayerId,
    targets: &[usize],
    text: &str,
) -> Result<usize, PasteError> {
    let clip: Clip = serde_json::from_str(text).map_err(|_| PasteError::NotKeys)?;
    if clip.keyboard_curator != VERSION {
        return Err(PasteError::NotKeys);
    }
    if clip.board != project.board {
        return Err(PasteError::OtherBoard(clip.board));
    }
    let mut remap = Remap {
        layers: clip
            .layers
            .iter()
            .map(|(id, name)| {
                let target = project
                    .layers
                    .iter()
                    .find(|l| l.name == *name)
                    .map(|l| l.id);
                (*id, target)
            })
            .collect(),
        behaviors: Vec::new(),
    };
    bring_definitions(project, &clip, &mut remap);
    let placements: Vec<(usize, &ClipKey)> = match (clip.kind, clip.keys.as_slice()) {
        (ClipKind::Layer, keys) => keys.iter().map(|k| (k.position, k)).collect(),
        (ClipKind::Keys, [single]) => {
            if targets.is_empty() {
                return Err(PasteError::NoTarget);
            }
            targets.iter().map(|p| (*p, single)).collect()
        }
        (ClipKind::Keys, keys) => {
            let anchor = *targets.iter().min().ok_or(PasteError::NoTarget)?;
            let first = keys.iter().map(|k| k.position).min().unwrap_or(0);
            keys.iter()
                .map(|k| (anchor + k.position - first, k))
                .collect()
        }
    };
    let mut changed = 0;
    for (position, key) in placements {
        if position >= project.key_count {
            continue;
        }
        let binding = remap.binding(&key.binding).unwrap_or_else(|| Binding::Raw {
            raw: key.text.clone(),
        });
        if project.binding(layer, position) != Some(&binding) {
            project.set_binding(layer, position, binding)?;
            changed += 1;
        }
    }
    project.tidy();
    Ok(changed)
}
