//! Layouts to and from what a VIA keyboard holds: the keymap through the
//! board's matrix positions, and macros through the macro buffer.

use std::collections::HashMap;

use kc_boards::board::ViaProfile;
use kc_boards::Board;
use kc_model::behavior::{BehaviorKind, Macro, MacroStep};
use kc_model::{BehaviorId, Binding, LayerId, Location, Problem, Project, Severity};

use crate::codec::{self, Lights};
use crate::{macros, Image, ViaError};

/// The RGB matrix channel's ID.
const RGB_MATRIX_CHANNEL: u8 = 3;

fn lights(profile: &ViaProfile) -> Lights {
    Lights {
        rgb_matrix: profile
            .lighting
            .iter()
            .any(|[channel, _]| *channel == RGB_MATRIX_CHANNEL),
    }
}

fn layer_ids(project: &Project) -> Vec<LayerId> {
    project.layers.iter().map(|l| l.id).collect()
}

fn macro_steps<'a>(project: &'a Project, binding: &Binding) -> Option<&'a [MacroStep]> {
    let def = project.behavior(binding.user_behavior()?)?;
    match &def.kind {
        BehaviorKind::Macro(m) => Some(&m.steps),
        _ => None,
    }
}

/// What the keyboard holds, as a new layout for `board`: every layer the
/// keyboard has, every key the app can name, and the macros its keys use
/// as macro behaviors. Keys the app has no binding for are kept as they
/// are, and written back unchanged.
pub fn to_project(
    image: &Image,
    board: &Board,
    profile: &ViaProfile,
    name: &str,
) -> Result<Project, ViaError> {
    if image.matrix.rows != profile.rows || image.matrix.cols != profile.cols {
        return Err(ViaError::Mismatch);
    }
    let mut project = Project::new(name, board);
    for index in 1..usize::from(image.layers) {
        if project.add_layer(format!("Layer {}", index + 1)).is_err() {
            break;
        }
    }
    let layers = layer_ids(&project);

    // The macros any key uses become behaviors; the rest stay in the
    // buffer, untouched, as nothing points at them.
    let entries = macros::parse(&image.macros, usize::from(image.macro_count));
    let mut defs: HashMap<usize, BehaviorId> = HashMap::new();
    let mut used: Vec<usize> = image
        .keymap
        .iter()
        .filter_map(|c| macros::index(*c))
        .collect();
    used.sort_unstable();
    used.dedup();
    for slot in used {
        let Some(actions) = entries.get(slot) else {
            continue;
        };
        let Some(steps) = macros::to_steps(actions) else {
            continue;
        };
        let label = project.free_label("macro");
        if let Ok(id) = project.add_behavior(
            label,
            "",
            BehaviorKind::Macro(Macro {
                wait_ms: None,
                tap_ms: None,
                params: 0,
                steps,
            }),
        ) {
            defs.insert(slot, id);
        }
    }

    for (index, layer) in layers.iter().enumerate() {
        for (position, [row, col]) in profile.matrix.iter().enumerate() {
            let Some(code) = image.keycode(index as u8, *row, *col) else {
                continue;
            };
            let binding = match macros::index(code).and_then(|slot| defs.get(&slot)) {
                Some(id) => Binding::user(*id, Vec::new()),
                None => codec::decode(code, &layers, &profile.custom_keycodes),
            };
            let _ = project.set_binding(*layer, position, binding);
        }
    }
    Ok(project)
}

/// Each placed macro's keycode, and the macro buffer holding them all.
type Placed = (HashMap<BehaviorId, u16>, Vec<u8>);

/// Places every macro a key uses into a slot, first use first, and
/// returns each macro's keycode with the new buffer.
fn place_macros(project: &Project, image: &Image) -> Result<Placed, (Location, String)> {
    let mut codes = HashMap::new();
    let mut table: Vec<Vec<macros::Action>> = Vec::new();
    for layer in &project.layers {
        for (position, binding) in layer.bindings.iter().enumerate() {
            let location = Location::Key {
                layer: layer.id,
                position,
            };
            let Some(steps) = macro_steps(project, binding) else {
                continue;
            };
            let id = binding.user_behavior().expect("a macro binding");
            if codes.contains_key(&id) {
                continue;
            }
            if image.macro_count == 0 {
                return Err((location, "this keyboard holds no macros".into()));
            }
            if table.len() >= usize::from(image.macro_count) {
                return Err((
                    location,
                    format!(
                        "the layout needs more macros than the keyboard's {}",
                        image.macro_count
                    ),
                ));
            }
            let actions = macros::from_steps(steps).map_err(|e| (location.clone(), e))?;
            codes.insert(id, macros::code(table.len()));
            table.push(actions);
        }
    }
    let mut buffer = macros::serialize(&table);
    if buffer.len() > image.macros.len() {
        return Err((
            Location::Project,
            format!(
                "the macros take {} bytes, and the keyboard holds {}",
                buffer.len(),
                image.macros.len()
            ),
        ));
    }
    buffer.resize(image.macros.len(), 0);
    Ok((codes, buffer))
}

/// The image the keyboard should hold for `project`, starting from what
/// it holds now. Layers beyond the layout's, matrix positions no key of
/// the layout sits at, the layout options and the lighting are left as
/// they are. Macros are placed anew, first use first; macros no key uses
/// are dropped.
pub fn with_project(
    image: &Image,
    project: &Project,
    profile: &ViaProfile,
) -> Result<Image, ViaError> {
    if image.matrix.rows != profile.rows || image.matrix.cols != profile.cols {
        return Err(ViaError::Mismatch);
    }
    if project.layers.len() > usize::from(image.layers) {
        return Err(ViaError::Unfit(format!(
            "the layout has {} layers, and this keyboard holds {}",
            project.layers.len(),
            image.layers
        )));
    }
    let layers = layer_ids(project);
    let lights = lights(profile);
    let (codes, macro_buffer) = place_macros(project, image)
        .map_err(|(location, reason)| ViaError::Unfit(describe(project, &location, &reason)))?;
    let mut keymap = image.keymap.clone();
    let (rows, cols) = (usize::from(profile.rows), usize::from(profile.cols));
    for (index, layer) in project.layers.iter().enumerate() {
        for (position, binding) in layer.bindings.iter().enumerate() {
            let Some([row, col]) = profile.matrix.get(position) else {
                break;
            };
            let code = match binding.user_behavior().and_then(|id| codes.get(&id)) {
                Some(code) => *code,
                None => codec::encode(binding, &layers, lights).map_err(|reason| {
                    ViaError::Unfit(describe(
                        project,
                        &Location::Key {
                            layer: layer.id,
                            position,
                        },
                        &reason,
                    ))
                })?,
            };
            keymap[(index * rows + usize::from(*row)) * cols + usize::from(*col)] = code;
        }
    }
    Ok(Image {
        keymap,
        macros: if image.macros.is_empty() {
            Vec::new()
        } else {
            macro_buffer
        },
        ..image.clone()
    })
}

fn describe(project: &Project, location: &Location, reason: &str) -> String {
    match location {
        Location::Key { layer, position } => {
            let name = project
                .layers
                .iter()
                .find(|l| l.id == *layer)
                .map_or_else(String::new, |l| l.name.clone());
            format!("key {} on layer {name}: {reason}", position + 1)
        }
        _ => reason.to_string(),
    }
}

/// What in `project` a VIA keyboard cannot take: `held` is how many
/// layers the keyboard has, when known.
pub fn check(project: &Project, profile: &ViaProfile, held: Option<u8>) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut error = |location: Location, message: String| {
        problems.push(Problem {
            severity: Severity::Error,
            location,
            message,
        });
    };
    if let Some(held) = held {
        if project.layers.len() > usize::from(held) {
            error(
                Location::Project,
                format!(
                    "the layout has {} layers, and this keyboard holds {held}",
                    project.layers.len()
                ),
            );
        }
    }
    let layers = layer_ids(project);
    let lights = lights(profile);
    let mut seen: Vec<BehaviorId> = Vec::new();
    for layer in &project.layers {
        for (position, binding) in layer.bindings.iter().enumerate() {
            let location = Location::Key {
                layer: layer.id,
                position,
            };
            let result = match macro_steps(project, binding) {
                Some(steps) => {
                    let id = binding.user_behavior().expect("a macro binding");
                    if !seen.contains(&id) {
                        seen.push(id);
                    }
                    macros::from_steps(steps).map(|_| ())
                }
                None => codec::encode(binding, &layers, lights).map(|_| ()),
            };
            if let Err(reason) = result {
                error(location, reason);
            }
        }
    }
    if seen.len() > 16 {
        error(
            Location::Project,
            format!(
                "the layout uses {} macros, and a VIA keyboard holds 16",
                seen.len()
            ),
        );
    }
    problems
}

/// Whether a binding can be written to a VIA keyboard at all, so that the
/// editor offers only what works.
pub fn expressible(binding: &Binding, project: &Project, profile: &ViaProfile) -> bool {
    if let Some(steps) = macro_steps(project, binding) {
        return macros::from_steps(steps).is_ok();
    }
    codec::encode(binding, &layer_ids(project), lights(profile)).is_ok()
}
