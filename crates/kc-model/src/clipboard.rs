//! Copying keys and layers as text, so they can be pasted into another
//! layer or another project for the same board.
//!
//! Bindings travel as ZMK text with layers written by name, because IDs
//! mean nothing outside the project they came from.

use serde::{Deserialize, Serialize};

use crate::ids::LayerId;
use crate::project::{ModelError, Project};
use crate::text::{format_binding, parse_binding, LayerStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ClipKind {
    Keys,
    Layer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Clip {
    /// Marks the text as ours, and versions the payload.
    keyboard_curator: u32,
    board: String,
    kind: ClipKind,
    /// Key position and binding text.
    keys: Vec<(usize, String)>,
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

fn clip(project: &Project, layer: LayerId, kind: ClipKind, positions: &[usize]) -> Option<String> {
    let bindings = &project.layer(layer)?.bindings;
    let mut positions = positions.to_vec();
    positions.sort_unstable();
    positions.dedup();
    let keys = positions
        .into_iter()
        .filter_map(|p| {
            Some((
                p,
                format_binding(project, bindings.get(p)?, LayerStyle::Constant),
            ))
        })
        .collect::<Vec<_>>();
    if keys.is_empty() {
        return None;
    }
    serde_json::to_string(&Clip {
        keyboard_curator: 1,
        board: project.board.clone(),
        kind,
        keys,
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
    if clip.keyboard_curator != 1 {
        return Err(PasteError::NotKeys);
    }
    if clip.board != project.board {
        return Err(PasteError::OtherBoard(clip.board));
    }
    let placements: Vec<(usize, &str)> = match (clip.kind, clip.keys.as_slice()) {
        (ClipKind::Layer, keys) => keys.iter().map(|(p, t)| (*p, t.as_str())).collect(),
        (ClipKind::Keys, [(_, single)]) => {
            if targets.is_empty() {
                return Err(PasteError::NoTarget);
            }
            targets.iter().map(|p| (*p, single.as_str())).collect()
        }
        (ClipKind::Keys, keys) => {
            let anchor = *targets.iter().min().ok_or(PasteError::NoTarget)?;
            let first = keys.iter().map(|(p, _)| *p).min().unwrap_or(0);
            keys.iter()
                .map(|(p, t)| (anchor + p - first, t.as_str()))
                .collect()
        }
    };
    let mut changed = 0;
    for (position, text) in placements {
        if position >= project.key_count {
            continue;
        }
        let binding = parse_binding(project, text);
        if project.binding(layer, position) != Some(&binding) {
            project.set_binding(layer, position, binding)?;
            changed += 1;
        }
    }
    Ok(changed)
}
