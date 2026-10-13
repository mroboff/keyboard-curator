//! VIA's macros, to and from the model's macro steps.
//!
//! The format is a fact about QMK's `send_string` (GPL-2.0), as it reads
//! the dynamic macro buffer: a macro is a string of bytes, typed as ASCII
//! on a US layout, except that the byte 1 starts an action: 1 taps, 2
//! presses and 3 releases the one-byte keycode that follows; 4 waits for
//! the number of milliseconds spelled out in decimal digits, ended by any
//! other byte. Macros follow one another, each ended by a zero byte, in
//! a buffer the keyboard says the size of; the keyboard holds a fixed
//! number of them, named `M0` upward.

use std::collections::HashMap;
use std::sync::OnceLock;

use kc_model::behavior::MacroStep;
use kc_model::{Binding, KeyExpr, Param};
use kc_zmk::keycodes::{for_char, keycodes, UsagePage};
use kc_zmk::Modifier;

use crate::codec;

const PREFIX: u8 = 1;
const TAP: u8 = 1;
const DOWN: u8 = 2;
const UP: u8 = 3;
const DELAY: u8 = 4;
/// The byte that ends a delay's digits in what this app writes.
const DELAY_END: u8 = b'|';

/// One action of a macro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// A byte typed as text.
    Text(u8),
    Tap(u8),
    Down(u8),
    Up(u8),
    /// Wait this many milliseconds.
    Delay(u32),
}

/// The macros in a buffer: `count` of them, each a list of actions.
pub fn parse(buffer: &[u8], count: usize) -> Vec<Vec<Action>> {
    let mut macros = Vec::with_capacity(count);
    let mut bytes = buffer.iter().copied();
    for _ in 0..count {
        let mut actions = Vec::new();
        loop {
            let Some(byte) = bytes.next() else {
                break;
            };
            if byte == 0 {
                break;
            }
            if byte != PREFIX {
                actions.push(Action::Text(byte));
                continue;
            }
            match bytes.next() {
                Some(TAP) => actions.push(Action::Tap(bytes.next().unwrap_or(0))),
                Some(DOWN) => actions.push(Action::Down(bytes.next().unwrap_or(0))),
                Some(UP) => actions.push(Action::Up(bytes.next().unwrap_or(0))),
                Some(DELAY) => {
                    let mut ms: u32 = 0;
                    // The byte that ends the digits is used up, as the
                    // firmware uses it up.
                    loop {
                        match bytes.next() {
                            Some(digit @ b'0'..=b'9') => {
                                ms = ms
                                    .saturating_mul(10)
                                    .saturating_add(u32::from(digit - b'0'));
                            }
                            _ => break,
                        }
                    }
                    actions.push(Action::Delay(ms));
                }
                _ => break,
            }
        }
        macros.push(actions);
    }
    macros
}

/// The buffer for a list of macros, each ended by a zero byte.
pub fn serialize(macros: &[Vec<Action>]) -> Vec<u8> {
    let mut out = Vec::new();
    for actions in macros {
        for action in actions {
            match action {
                Action::Text(byte) => out.push(*byte),
                Action::Tap(code) => out.extend([PREFIX, TAP, *code]),
                Action::Down(code) => out.extend([PREFIX, DOWN, *code]),
                Action::Up(code) => out.extend([PREFIX, UP, *code]),
                Action::Delay(ms) => {
                    out.extend([PREFIX, DELAY]);
                    out.extend(ms.to_string().into_bytes());
                    out.push(DELAY_END);
                }
            }
        }
        out.push(0);
    }
    out
}

/// What a text byte types: the key and whether shift is held, after
/// ZMK's reading of a US layout, which is QMK's too.
fn text_keys() -> &'static HashMap<(u16, bool), u8> {
    static KEYS: OnceLock<HashMap<(u16, bool), u8>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let mut map = HashMap::new();
        for byte in (0x20..0x7Fu8).chain([b'\n', b'\t']) {
            let Some(expr) = for_char(byte as char).and_then(|e| e.parse::<KeyExpr>().ok()) else {
                continue;
            };
            let Some(key) = keycodes().get(&expr.key) else {
                continue;
            };
            if key.page != UsagePage::Keyboard {
                continue;
            }
            let mods: Vec<Modifier> = expr
                .mods
                .iter()
                .chain(&key.implicit_mods)
                .copied()
                .collect();
            let shifted = match mods.as_slice() {
                [] => false,
                [Modifier::LShift] => true,
                _ => continue,
            };
            map.entry((key.usage, shifted)).or_insert(byte);
        }
        map
    })
}

fn key_binding(code: u8) -> Option<Binding> {
    codec::plain_name(u16::from(code)).map(|name| Binding::kp(KeyExpr::new(name)))
}

/// The model's steps for a macro's actions: text as taps of the key that
/// types it. `None` when something has no step, which keeps the macro
/// as it is.
pub fn to_steps(actions: &[Action]) -> Option<Vec<MacroStep>> {
    let mut steps = Vec::new();
    for action in actions {
        let step = match action {
            Action::Text(byte) => {
                let expr: KeyExpr = for_char(*byte as char)?.parse().ok()?;
                MacroStep::Tap(vec![Binding::kp(expr)])
            }
            Action::Tap(code) => MacroStep::Tap(vec![key_binding(*code)?]),
            Action::Down(code) => MacroStep::Press(vec![key_binding(*code)?]),
            Action::Up(code) => MacroStep::Release(vec![key_binding(*code)?]),
            Action::Delay(ms) => MacroStep::Pause(*ms),
        };
        steps.push(step);
    }
    Some(steps)
}

/// A binding as the one-byte keycode and modifiers a macro can press.
fn basic_key(binding: &Binding) -> Result<(u8, Vec<Modifier>), String> {
    let Binding::Behavior { params, .. } = binding else {
        return Err("a VIA macro can only press keys".into());
    };
    let Some(Param::Key(expr)) = params.first() else {
        return Err("a VIA macro can only press keys".into());
    };
    if !matches!(
        binding,
        Binding::Behavior {
            behavior: kc_model::BehaviorRef::BuiltIn(label),
            ..
        } if label == "kp"
    ) {
        return Err("a VIA macro can only press keys".into());
    }
    let key = keycodes()
        .get(&expr.key)
        .ok_or_else(|| format!("`{}` is not a key this app knows", expr.key))?;
    if key.page != UsagePage::Keyboard || key.usage > 0xFF {
        return Err(format!(
            "a VIA macro cannot press `{}`: only plain keys",
            expr.key
        ));
    }
    let mods = expr
        .mods
        .iter()
        .chain(&key.implicit_mods)
        .copied()
        .collect();
    Ok((key.usage as u8, mods))
}

fn modifier_code(modifier: Modifier) -> u8 {
    codec::modifier_key_usage(modifier.keycode()).unwrap_or(0xE0) as u8
}

/// A macro's actions for the model's steps. Fails, with the reason, for a
/// step VIA cannot do.
pub fn from_steps(steps: &[MacroStep]) -> Result<Vec<Action>, String> {
    let mut actions = Vec::new();
    for step in steps {
        match step {
            MacroStep::Tap(bindings) => {
                for binding in bindings {
                    let (code, mods) = basic_key(binding)?;
                    let shifted = mods == [Modifier::LShift];
                    if let Some(byte) = (mods.is_empty() || shifted)
                        .then(|| text_keys().get(&(u16::from(code), shifted)))
                        .flatten()
                    {
                        actions.push(Action::Text(*byte));
                        continue;
                    }
                    for modifier in &mods {
                        actions.push(Action::Down(modifier_code(*modifier)));
                    }
                    actions.push(Action::Tap(code));
                    for modifier in mods.iter().rev() {
                        actions.push(Action::Up(modifier_code(*modifier)));
                    }
                }
            }
            MacroStep::Press(bindings) => {
                for binding in bindings {
                    let (code, mods) = basic_key(binding)?;
                    for modifier in &mods {
                        actions.push(Action::Down(modifier_code(*modifier)));
                    }
                    actions.push(Action::Down(code));
                }
            }
            MacroStep::Release(bindings) => {
                for binding in bindings {
                    let (code, mods) = basic_key(binding)?;
                    actions.push(Action::Up(code));
                    for modifier in mods.iter().rev() {
                        actions.push(Action::Up(modifier_code(*modifier)));
                    }
                }
            }
            MacroStep::Pause(ms) => actions.push(Action::Delay(*ms)),
            MacroStep::PauseForRelease => {
                return Err("a VIA macro cannot wait for the key to be released".into())
            }
            MacroStep::WaitTime(_) | MacroStep::TapTime(_) => {
                return Err("a VIA macro has one speed, set by the firmware".into())
            }
            MacroStep::Param { .. } => {
                return Err("a VIA macro takes no values from the key".into())
            }
        }
    }
    Ok(actions)
}

/// The keycode of macro slot `index`.
pub fn code(index: usize) -> u16 {
    codec::MACRO + index as u16
}

/// The macro slot a keycode names, if it names one.
pub fn index(code: u16) -> Option<usize> {
    (codec::MACRO..codec::MACRO + codec::MACROS)
        .contains(&code)
        .then(|| usize::from(code - codec::MACRO))
}
