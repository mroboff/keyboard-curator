//! Dygma's superkeys: a key with five actions, as the slots of a key setup.
//!
//! The format is a fact about the firmware, from Bazecor's parser
//! (src/api/parsers/superkeys.ts) and Dygma's Focus API document. The map
//! is a list of 16-bit values: each superkey's five actions in the order
//! tap, hold, tap-then-hold, double tap, double-tap-then-hold, closed with
//! `0`; the list ends with `0 0`. A `1` is an action left empty. A keyboard
//! with no superkeys holds 512 values of `65535` instead. Superkey timing
//! is one set of values for the whole keyboard, so a key's feel here is
//! the keyboard's default.

use kc_model::setup::KeySetup;
use kc_model::{Binding, LayerId, Project};

use crate::codec;

/// The first superkey's code on the keymap.
pub const FIRST: u16 = 53980;
/// How many superkeys the keymap can name.
pub const SLOTS: usize = 128;
/// The map's room in the keyboard, in bytes: each value takes two.
pub const MAP_BYTES: usize = 1024;
/// An action left empty.
const EMPTY: u16 = 1;

/// One superkey: its five actions as key codes, `1` for an empty one.
pub type Actions = [u16; 5];

/// The map's entries, in order.
pub fn parse(raw: &[u16]) -> Vec<Actions> {
    let mut entries = Vec::new();
    let mut current: Vec<u16> = Vec::new();
    let mut values = raw.iter().copied().peekable();
    while let Some(value) = values.next() {
        if value == 0 {
            if current.is_empty() && values.peek().is_none_or(|v| *v == 0) {
                // `0 0`: the end of the list, or an empty map's padding.
                break;
            }
            let mut actions = [EMPTY; 5];
            for (slot, code) in actions.iter_mut().zip(current.drain(..)) {
                *slot = code;
            }
            entries.push(actions);
        } else if value == 65535 && current.is_empty() && entries.is_empty() {
            // The empty map: 512 of these and nothing else.
            return Vec::new();
        } else {
            current.push(value);
        }
    }
    entries
}

/// The map as the keyboard stores it.
pub fn serialize(entries: &[Actions]) -> Vec<u16> {
    if entries.is_empty() {
        return vec![65535; 512];
    }
    let mut out = Vec::with_capacity(entries.len() * 6 + 1);
    for actions in entries {
        out.extend_from_slice(actions);
        out.push(0);
    }
    out.push(0);
    out
}

/// How many bytes `entries` take in the keyboard.
pub fn bytes(entries: &[Actions]) -> usize {
    if entries.is_empty() {
        512 * 2
    } else {
        (entries.len() * 6 + 1) * 2
    }
}

/// The key setup a superkey's actions describe, with `layers` the
/// layout's layers in order. `None` when an action is a code this app
/// cannot place in a slot.
pub fn to_setup(actions: &Actions, layers: &[LayerId]) -> Option<KeySetup> {
    let action = |code: u16| -> Option<Option<Binding>> {
        if code == EMPTY {
            return Some(None);
        }
        let binding = codec::decode(code, layers);
        // A code with no binding has no place in a slot.
        if matches!(binding, Binding::Raw { .. }) {
            return None;
        }
        Some(Some(binding))
    };
    let tap = action(actions[0])?.unwrap_or_else(Binding::none);
    let mut setup = KeySetup::plain(tap);
    // A one-shot modifier or layer on the tap is the sticky switch.
    if let Some(inner) = sticky_inner(&setup.tap) {
        setup.tap = inner;
        setup.sticky = true;
    }
    setup.hold = action(actions[1])?;
    setup.tap_hold = action(actions[2])?;
    setup.double_tap = action(actions[3])?;
    setup.double_tap_hold = action(actions[4])?;
    Some(setup)
}

/// What a one-shot binding makes sticky: the key for `sk`, the layer
/// for `sl`.
fn sticky_inner(binding: &Binding) -> Option<Binding> {
    let Binding::Behavior { behavior, params } = binding else {
        return None;
    };
    match (behavior, params.as_slice()) {
        (b, [kc_model::Param::Key(key)]) if *b == kc_model::BehaviorRef::built_in("sk") => {
            Some(Binding::kp(key.clone()))
        }
        (b, [kc_model::Param::Layer(layer)]) if *b == kc_model::BehaviorRef::built_in("sl") => {
            Some(Binding::layer("mo", *layer))
        }
        _ => None,
    }
}

/// The five actions for a key that is more than one code, or why the
/// firmware cannot hold it.
pub fn actions(
    project: &Project,
    binding: &Binding,
    layers: &[LayerId],
) -> Result<Actions, String> {
    let setup = KeySetup::read(project, binding)
        .ok_or_else(|| "this key is written in a way Dygma firmware cannot hold".to_string())?;
    // Settings of the key's own that still match the defaults are the
    // keyboard's timing too; a template or changed settings are not.
    fn default_feel<S: Default + PartialEq>(choice: &kc_model::setup::Choice<S>) -> bool {
        match choice {
            kc_model::setup::Choice::Default => true,
            kc_model::setup::Choice::Custom(s) => *s == S::default(),
            kc_model::setup::Choice::Template(_) => false,
        }
    }
    if !default_feel(&setup.feel.hold)
        || !default_feel(&setup.feel.taps)
        || !default_feel(&setup.feel.sticky)
    {
        return Err("Dygma firmware keeps one timing for every superkey: choose Keyboard default for this key's feel, and set the timing under Settings".into());
    }
    if setup.shifted.is_some() {
        return Err("Dygma firmware has no shift override".into());
    }
    if !setup.more_taps.is_empty() {
        return Err("Dygma superkeys have five actions: tap, hold, tap then hold, double tap, and double tap then hold".into());
    }
    let code = |slot: &Option<Binding>| -> Result<u16, String> {
        match slot {
            None => Ok(EMPTY),
            Some(binding) => codec::encode(binding, layers),
        }
    };
    let tap = if setup.sticky {
        let sticky = match &setup.tap {
            Binding::Behavior { behavior, params }
                if *behavior == kc_model::BehaviorRef::built_in("mo") =>
            {
                Binding::Behavior {
                    behavior: kc_model::BehaviorRef::built_in("sl"),
                    params: params.clone(),
                }
            }
            Binding::Behavior { params, .. } => Binding::Behavior {
                behavior: kc_model::BehaviorRef::built_in("sk"),
                params: params.clone(),
            },
            other => other.clone(),
        };
        codec::encode(&sticky, layers)?
    } else {
        codec::encode(&setup.tap, layers)?
    };
    Ok([
        tap,
        code(&setup.hold)?,
        code(&setup.tap_hold)?,
        code(&setup.double_tap)?,
        code(&setup.double_tap_hold)?,
    ])
}

/// The entry for `actions` in `entries`: an identical one already there,
/// or a new one, unless the keyboard is full.
pub fn place(entries: &mut Vec<Actions>, actions: Actions) -> Result<usize, String> {
    if let Some(index) = entries.iter().position(|e| *e == actions) {
        return Ok(index);
    }
    let mut grown = entries.clone();
    grown.push(actions);
    if grown.len() > SLOTS || bytes(&grown) > MAP_BYTES {
        return Err(format!(
            "the keyboard's superkey table is full ({} superkeys, {} bytes of {}); remove superkeys no key uses with Bazecor",
            entries.len(),
            bytes(entries),
            MAP_BYTES
        ));
    }
    entries.push(actions);
    Ok(entries.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_parse_and_serialize_like_bazecor() {
        // Bazecor's own example: Ctrl+C, Ctrl+V, Ctrl+Shift+V, Ctrl+X, nothing.
        let raw = [262u16, 281, 2329, 283, 1, 0, 0];
        let entries = parse(&raw);
        assert_eq!(entries, [[262, 281, 2329, 283, 1]]);
        assert_eq!(serialize(&entries), raw);
        // Short entries are padded with the empty action.
        assert_eq!(
            parse(&[4, 0, 5, 6, 0, 0]),
            [[4, 1, 1, 1, 1], [5, 6, 1, 1, 1]]
        );
        // The empty map.
        assert_eq!(parse(&vec![65535; 512]), Vec::<Actions>::new());
        assert_eq!(serialize(&[]), vec![65535; 512]);
        assert_eq!(bytes(&entries), 14);
    }
}
