//! The Curator firmware's chords: several keys pressed together sending
//! another key, which are the layout's combos.
//!
//! The format is a fact about the fork (mroboff/NeuronWireless_defy,
//! branch `curator`, CURATOR.md, "Focus commands the fork adds"); no code
//! is taken from it. `chords.map` is a list of 16-bit values: each chord
//! is `result layers timeout key1 ... keyN 0`, where `result` is a key
//! code in the `keymap.custom` vocabulary (a plain or modified key, a
//! layer action, a macro, a superkey), `layers` a bitmask of the layers
//! the chord works on with `0` for every layer, `timeout` the chord's own
//! timeout in milliseconds or `0` for `chords.timeout`, and the keys are
//! keymap slots counted from 1, two to six of them. The map ends with
//! `0 0`, like the superkey map, and an erased map reads as `0 0`. The
//! room is `chords.memory` bytes (512), two per value, and at most 32
//! chords are kept. Stock Dygma firmware has none of this; the commands
//! are sent only when `help` lists them.

use kc_boards::board::DygmaProfile;
use kc_model::{Binding, LayerId, Location, Problem, Project, Severity};

use crate::{codec, macros, superkeys};

/// How many chords the keyboard keeps.
pub const MAX_CHORDS: usize = 32;
/// The most keys a chord may have.
pub const MAX_KEYS: usize = 6;
/// The fewest keys a chord may have.
pub const MIN_KEYS: usize = 2;
/// The map's room in the keyboard, in bytes, when it does not say.
pub const MAP_BYTES: usize = 512;

/// One chord, with its keys as keymap slots counted from 0 (the wire
/// counts them from 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    /// The key code sent, in the keymap's vocabulary.
    pub result: u16,
    /// The layers the chord works on, bit `n` for layer `n`; `0` for all.
    pub layers: u16,
    /// The chord's own timeout in milliseconds; `0` for the keyboard's.
    pub timeout: u16,
    pub keys: Vec<u8>,
}

/// The map's chords, in order.
pub fn parse(raw: &[u16]) -> Vec<Chord> {
    let mut chords = Vec::new();
    let mut at = 0;
    // A chord starts with its result; a `0` there ends the map.
    while at + 3 <= raw.len() && raw[at] != 0 {
        let (result, layers, timeout) = (raw[at], raw[at + 1], raw[at + 2]);
        at += 3;
        let mut keys = Vec::new();
        while at < raw.len() && raw[at] != 0 {
            // Slots are counted from 1 on the wire.
            keys.push(u8::try_from(raw[at] - 1).unwrap_or(u8::MAX));
            at += 1;
        }
        // The `0` that ends the chord.
        at += 1;
        chords.push(Chord {
            result,
            layers,
            timeout,
            keys,
        });
    }
    chords
}

/// The map as the keyboard stores it: every chord closed with `0`, then
/// the `0` that ends the map; an empty map is `0 0`.
pub fn serialize(chords: &[Chord]) -> Vec<u16> {
    let mut out = Vec::with_capacity(chords.iter().map(|c| c.keys.len() + 4).sum::<usize>() + 2);
    for chord in chords {
        out.extend_from_slice(&[chord.result, chord.layers, chord.timeout]);
        out.extend(chord.keys.iter().map(|k| u16::from(*k) + 1));
        out.push(0);
    }
    out.push(0);
    if chords.is_empty() {
        out.push(0);
    }
    out
}

/// How many bytes `chords` take in the keyboard.
pub fn bytes(chords: &[Chord]) -> usize {
    serialize(chords).len() * 2
}

/// The layout's combos as chords, or why one cannot be held. `codes` has
/// the code of every macro placed in the keyboard's table, and a combo
/// that sends a key setup takes a superkey in `superkeys`. `memory` is
/// the map's room in bytes.
pub fn from_project(
    project: &Project,
    layers: &[LayerId],
    profile: &DygmaProfile,
    codes: &macros::Codes,
    superkeys: &mut Vec<superkeys::Actions>,
    memory: usize,
) -> Result<Vec<Chord>, (Location, String)> {
    let mut chords: Vec<Chord> = Vec::new();
    for combo in &project.combos {
        let location = Location::Combo(combo.id);
        let fail = |reason: String| (location.clone(), reason);
        if chords.len() == MAX_CHORDS {
            return Err(fail(format!(
                "the keyboard holds at most {MAX_CHORDS} combos, and this is one more"
            )));
        }
        if combo.key_positions.len() < MIN_KEYS || combo.key_positions.len() > MAX_KEYS {
            return Err(fail(format!(
                "a combo on this keyboard has {MIN_KEYS} to {MAX_KEYS} keys, and this one has {}",
                combo.key_positions.len()
            )));
        }
        let mut keys = Vec::with_capacity(combo.key_positions.len());
        for position in &combo.key_positions {
            let slot = profile
                .key_slots
                .get(*position)
                .and_then(|slot| u8::try_from(*slot).ok())
                .ok_or_else(|| fail("the combo uses a key this keyboard does not have".into()))?;
            keys.push(slot);
        }
        let binding = &combo.binding;
        let result = if crate::is_macro(project, binding) {
            crate::encode_any(binding, layers, codes).map_err(fail)?
        } else if binding.user_behavior().is_some() {
            let actions =
                superkeys::actions(project, binding, layers, codes, false).map_err(fail)?;
            let entry = superkeys::place(superkeys, actions).map_err(fail)?;
            superkeys::FIRST + u16::try_from(entry).expect("few superkeys")
        } else {
            codec::encode(binding, layers).map_err(fail)?
        };
        let mut mask: u16 = 0;
        for layer in &combo.layers {
            let index = layers
                .iter()
                .position(|l| l == layer)
                .filter(|i| *i < 16)
                .ok_or_else(|| fail("the combo names a layer that no longer exists".into()))?;
            mask |= 1 << index;
        }
        let timeout = combo
            .timeout_ms
            .map_or(0, |ms| u16::try_from(ms).unwrap_or(u16::MAX));
        chords.push(Chord {
            result,
            layers: mask,
            timeout,
            keys,
        });
        if bytes(&chords) > memory {
            return Err(fail(format!(
                "the keyboard has {memory} bytes for combos, and the layout's need more"
            )));
        }
    }
    Ok(chords)
}

/// What the firmware cannot do for the layout's combos, said at each one:
/// a pause before the combo and a slow release are kept in the layout for
/// firmware that has them.
pub fn warnings(project: &Project) -> Vec<Problem> {
    let mut problems = Vec::new();
    for combo in &project.combos {
        let mut lacking = Vec::new();
        if combo.require_prior_idle_ms.is_some() {
            lacking.push("\"Only after a pause in typing\"");
        }
        if combo.slow_release {
            lacking.push("\"Stay active until every key is released\"");
        }
        if !lacking.is_empty() {
            problems.push(Problem {
                severity: Severity::Warning,
                location: Location::Combo(combo.id),
                message: format!(
                    "the Curator firmware fires a combo once its keys land together: {} does not apply",
                    lacking.join(" and ")
                ),
            });
        }
    }
    problems
}

/// Adds the keyboard's chords to `project` as combos, with `macro_binding`
/// the binding for a macro code. A chord on a slot the layout has no key
/// for is left out, and a chord sending a superkey keeps it as a number;
/// both are said in the notes.
pub fn apply(
    project: &mut Project,
    chords: &[Chord],
    profile: &DygmaProfile,
    macro_binding: &dyn Fn(u16) -> Option<Binding>,
) -> Vec<String> {
    let layers: Vec<LayerId> = project.layers.iter().map(|l| l.id).collect();
    let mut notes = Vec::new();
    for (number, chord) in chords.iter().enumerate() {
        let number = number + 1;
        let positions: Option<Vec<usize>> = chord
            .keys
            .iter()
            .map(|slot| {
                profile
                    .key_slots
                    .iter()
                    .position(|s| *s == usize::from(*slot))
            })
            .collect();
        let Some(positions) = positions else {
            notes.push(format!(
                "Combo {number} on the keyboard uses a key the layout does not have, so it was left out."
            ));
            continue;
        };
        let binding =
            macro_binding(chord.result).unwrap_or_else(|| codec::decode(chord.result, &layers));
        if let Binding::Raw { raw } = &binding {
            notes.push(format!(
                "Combo {number} sends something the app cannot show ({raw}); it is kept as a number and written back unchanged."
            ));
        }
        let id = project.add_combo(format!("Combo {number}"), positions, binding);
        let on: Vec<LayerId> = layers
            .iter()
            .enumerate()
            .filter(|(index, _)| *index < 16 && chord.layers & (1 << index) != 0)
            .map(|(_, id)| *id)
            .collect();
        if let Ok(combo) = project.combo_mut(id) {
            combo.layers = on;
            combo.timeout_ms = (chord.timeout != 0).then_some(u32::from(chord.timeout));
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_parse_and_serialize_with_their_endings() {
        // Two chords: Escape on slots 1 and 2 of every layer, with the
        // keyboard's timeout; Tab on slots 3, 4 and 5 of layer 2 within
        // 80 ms.
        let raw = [41u16, 0, 0, 1, 2, 0, 43, 2, 80, 3, 4, 5, 0, 0];
        let chords = parse(&raw);
        assert_eq!(
            chords,
            [
                Chord {
                    result: 41,
                    layers: 0,
                    timeout: 0,
                    keys: vec![0, 1]
                },
                Chord {
                    result: 43,
                    layers: 2,
                    timeout: 80,
                    keys: vec![2, 3, 4]
                }
            ]
        );
        assert_eq!(serialize(&chords), raw);
        assert_eq!(bytes(&chords), 28);
        // The erased map, and an empty one.
        assert_eq!(parse(&[0, 0]), Vec::<Chord>::new());
        assert_eq!(parse(&[0]), Vec::<Chord>::new());
        assert_eq!(parse(&[]), Vec::<Chord>::new());
        assert_eq!(serialize(&[]), [0, 0]);
        // A map cut short keeps the chords that are whole.
        assert_eq!(
            parse(&[41, 0, 0, 1, 2, 0, 43, 2]),
            parse(&[41, 0, 0, 1, 2, 0, 0])
        );
        // A map with the most keys a chord may have, and one over the
        // keyboard's room.
        let wide = Chord {
            result: 4,
            layers: 0,
            timeout: 0,
            keys: (0..6).collect(),
        };
        assert_eq!(bytes(&[wide.clone()]), 22);
        let many: Vec<Chord> = (0..MAX_CHORDS).map(|_| wide.clone()).collect();
        assert!(bytes(&many) > MAP_BYTES);
        assert_eq!(parse(&serialize(&many)), many);
    }
}
