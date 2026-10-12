//! Dygma's macros, to and from the model's macro steps.
//!
//! The format is a fact about the firmware, from Bazecor's parser
//! (src/api/parsers/macros.ts) and Dygma's Focus API document. The map is
//! a list of bytes: each action is a type byte and its argument, a macro
//! ends with `0`, and the list ends with `0 0`. Type 2 is a delay in
//! milliseconds (two bytes); 3, 4 and 5 press, release and tap a 16-bit
//! key code (two bytes); 6, 7 and 8 do the same with a one-byte code.
//! Other types are kept as they are, so a macro this app cannot read is
//! written back unchanged.

use std::collections::HashMap;

use kc_model::behavior::MacroStep;
use kc_model::{BehaviorId, Binding, LayerId};

use crate::codec;

/// The first macro's code on the keymap.
pub const FIRST: u16 = 53852;
/// How many macros the keymap can name.
pub const SLOTS: usize = 128;
/// The map's room when the keyboard does not say: Bazecor's figure.
pub const DEFAULT_MEMORY: usize = 2048;

/// One action of a macro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Wait this many milliseconds.
    Delay(u16),
    Down(u16),
    Up(u16),
    Tap(u16),
}

/// One macro: read, or kept as the bytes it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Known(Vec<Action>),
    Raw(Vec<u16>),
}

/// The map's entries, in order.
pub fn parse(raw: &[u16]) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut at = 0;
    // The list ends with `0 0`; an empty map is all 255 or empty.
    while at < raw.len() {
        if raw[at] == 0 {
            break;
        }
        if raw[at] == 255 {
            break;
        }
        let start = at;
        let mut actions = Vec::new();
        let mut known = true;
        while at < raw.len() && raw[at] != 0 {
            let kind = raw[at];
            let wide = |at: usize| -> Option<u16> {
                let (hi, lo) = (raw.get(at + 1)?, raw.get(at + 2)?);
                Some((hi << 8) | (lo & 0xFF))
            };
            match kind {
                2..=5 => {
                    let Some(value) = wide(at) else {
                        known = false;
                        at = raw.len();
                        break;
                    };
                    actions.push(match kind {
                        2 => Action::Delay(value),
                        3 => Action::Down(value),
                        4 => Action::Up(value),
                        _ => Action::Tap(value),
                    });
                    at += 3;
                }
                6..=8 => {
                    let Some(value) = raw.get(at + 1) else {
                        known = false;
                        at = raw.len();
                        break;
                    };
                    actions.push(match kind {
                        6 => Action::Down(*value),
                        7 => Action::Up(*value),
                        _ => Action::Tap(*value),
                    });
                    at += 2;
                }
                _ => {
                    // A type this app does not read: the macro is kept whole.
                    known = false;
                    while at < raw.len() && raw[at] != 0 {
                        at += 1;
                    }
                }
            }
        }
        entries.push(if known {
            Entry::Known(actions)
        } else {
            Entry::Raw(raw[start..at].to_vec())
        });
        // The `0` that ends the macro.
        at += 1;
    }
    entries
}

fn action_bytes(action: &Action, out: &mut Vec<u16>) {
    let (narrow, wide, code) = match action {
        Action::Delay(ms) => (None, 2, *ms),
        Action::Down(code) => (Some(6), 3, *code),
        Action::Up(code) => (Some(7), 4, *code),
        Action::Tap(code) => (Some(8), 5, *code),
    };
    match narrow {
        Some(kind) if code < 256 => out.extend_from_slice(&[kind, code]),
        _ => out.extend_from_slice(&[wide, code >> 8, code & 0xFF]),
    }
}

/// The map as the keyboard stores it.
pub fn serialize(entries: &[Entry]) -> Vec<u16> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            Entry::Known(actions) => {
                for action in actions {
                    action_bytes(action, &mut out);
                }
            }
            Entry::Raw(bytes) => out.extend_from_slice(bytes),
        }
        out.push(0);
    }
    out.push(0);
    out
}

/// How many bytes `entries` take in the keyboard.
pub fn bytes(entries: &[Entry]) -> usize {
    serialize(entries).len()
}

/// The model's steps for a macro's actions, with `layers` the layout's
/// layers in order. Taps in a row become one step. `None` when an action
/// uses a code this app has no binding for.
pub fn to_steps(actions: &[Action], layers: &[LayerId]) -> Option<Vec<MacroStep>> {
    let mut steps: Vec<MacroStep> = Vec::new();
    for action in actions {
        let key = |code: u16| -> Option<Binding> {
            let binding = codec::decode(code, layers);
            (!matches!(binding, Binding::Raw { .. })).then_some(binding)
        };
        match action {
            Action::Delay(ms) => steps.push(MacroStep::Pause(u32::from(*ms))),
            Action::Down(code) => steps.push(MacroStep::Press(vec![key(*code)?])),
            Action::Up(code) => steps.push(MacroStep::Release(vec![key(*code)?])),
            Action::Tap(code) => {
                let binding = key(*code)?;
                match steps.last_mut() {
                    Some(MacroStep::Tap(list)) => list.push(binding),
                    _ => steps.push(MacroStep::Tap(vec![binding])),
                }
            }
        }
    }
    Some(steps)
}

/// A macro's actions for the model's steps, or why the firmware cannot
/// hold them.
pub fn from_steps(steps: &[MacroStep], layers: &[LayerId]) -> Result<Vec<Action>, String> {
    let mut actions = Vec::new();
    for step in steps {
        let (bindings, make): (&[Binding], fn(u16) -> Action) = match step {
            MacroStep::Tap(b) => (b, Action::Tap),
            MacroStep::Press(b) => (b, Action::Down),
            MacroStep::Release(b) => (b, Action::Up),
            MacroStep::Pause(ms) => {
                actions.push(Action::Delay(u16::try_from(*ms).unwrap_or(u16::MAX)));
                continue;
            }
            MacroStep::WaitTime(_) => {
                return Err("Dygma macros pause only where a Pause step says; a change of wait time has no place".into());
            }
            MacroStep::TapTime(_) => {
                return Err("Dygma macros tap at the keyboard's own pace; a change of tap time has no place".into());
            }
            MacroStep::PauseForRelease => {
                return Err("Dygma macros cannot wait for the key to be released".into());
            }
            MacroStep::Param { .. } => {
                return Err("Dygma macros take no values from the key".into())
            }
        };
        for binding in bindings {
            actions.push(make(codec::encode(binding, layers)?));
        }
    }
    Ok(actions)
}

/// The entry for `actions`: an identical one already there, or a new
/// one, unless the keyboard is full.
pub fn place(
    entries: &mut Vec<Entry>,
    actions: Vec<Action>,
    memory: usize,
) -> Result<usize, String> {
    let wanted = Entry::Known(actions);
    if let Some(index) = entries.iter().position(|e| *e == wanted) {
        return Ok(index);
    }
    let mut grown = entries.clone();
    grown.push(wanted.clone());
    if grown.len() > SLOTS || bytes(&grown) > memory {
        return Err(format!(
            "the keyboard's macro memory is full ({} macros, {} bytes of {}); remove macros no key uses with Bazecor",
            entries.len(),
            bytes(entries),
            memory
        ));
    }
    entries.push(wanted);
    Ok(entries.len() - 1)
}

/// The key codes of macros placed in the table, by definition.
pub type Codes = HashMap<BehaviorId, u16>;

/// The code for a macro entry.
pub fn code(index: usize) -> u16 {
    FIRST + u16::try_from(index).expect("few macros")
}

/// The entry a macro code names, if it is one.
pub fn index(code: u16) -> Option<usize> {
    let index = usize::from(code.checked_sub(FIRST)?);
    (index < SLOTS).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_parse_and_serialize_like_bazecor() {
        // From Bazecor's virtual Defy: Shift held around H, then "ey, " …
        let raw: Vec<u16> =
            "6 229 8 11 7 229 8 8 8 28 8 54 8 44 6 229 8 7 7 229 8 28 8 10 8 16 8 4 8 23 8 8 0 0"
                .split(' ')
                .map(|v| v.parse().unwrap())
                .collect();
        let entries = parse(&raw);
        assert_eq!(entries.len(), 1);
        let Entry::Known(actions) = &entries[0] else {
            panic!("known")
        };
        assert_eq!(actions[0], Action::Down(229));
        assert_eq!(actions[1], Action::Tap(11));
        assert_eq!(actions[2], Action::Up(229));
        assert_eq!(serialize(&entries), raw);
        // Wide codes, delays and a type this app does not read.
        let raw = [5u16, 0x48, 0xE9, 2, 1, 0xF4, 0, 12, 1, 2, 0, 0];
        let entries = parse(&raw);
        assert_eq!(
            entries,
            [
                Entry::Known(vec![Action::Tap(0x48E9), Action::Delay(500)]),
                Entry::Raw(vec![12, 1, 2])
            ]
        );
        assert_eq!(serialize(&entries), raw);
        assert_eq!(parse(&[]), Vec::<Entry>::new());
        assert_eq!(parse(&[255, 255]), Vec::<Entry>::new());
    }
}
