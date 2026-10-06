//! The key tester: which keys of a layout the computer has seen pressed.
//!
//! The computer only reports the key a press sends, not the physical key
//! behind it. So a press is matched to every key of the layout that can
//! send it, on any layer. Keys that send nothing to the computer (layer
//! keys, Bluetooth keys and the like) cannot be tested this way.

use std::collections::BTreeSet;

use kc_boards::geometry::Key;
use kc_zmk::keycodes::{keycodes, UsagePage};
use kc_zmk::Modifier;

use crate::binding::{Binding, Param};
use crate::project::Project;

/// A key as the computer identifies it: its HID usage page and number.
type Usage = (UsagePage, u16);

fn usage(name: &str) -> Option<Usage> {
    keycodes().get(name).map(|key| (key.page, key.usage))
}

fn modifier_usage(modifier: Modifier) -> Usage {
    let index = Modifier::ALL
        .iter()
        .position(|m| *m == modifier)
        .unwrap_or(0) as u16;
    (UsagePage::Keyboard, 0xE0 + index)
}

/// Every key a binding can send to the computer: its own key or keys, and
/// any modifiers it holds with them. A mod-tap sends both its modifier and
/// its tapped key; a layer key sends nothing.
fn sends(binding: &Binding) -> Vec<Usage> {
    let Binding::Behavior { params, .. } = binding else {
        return Vec::new();
    };
    let mut sent = Vec::new();
    for param in params {
        let Param::Key(expr) = param else { continue };
        let Some(key) = keycodes().get(&expr.key) else {
            continue;
        };
        sent.push((key.page, key.usage));
        sent.extend(
            expr.mods
                .iter()
                .chain(&key.implicit_mods)
                .map(|m| modifier_usage(*m)),
        );
    }
    sent
}

/// The key positions of `project` that can send the key named `key` (a
/// keycode name such as `A` or `LSHFT`), on any layer. Positions that send
/// it on an earlier layer come first.
pub fn positions_sending(project: &Project, key: &str) -> Vec<usize> {
    let Some(wanted) = usage(key) else {
        return Vec::new();
    };
    let mut positions = Vec::new();
    for layer in &project.layers {
        for (position, binding) in layer.bindings.iter().enumerate() {
            if !positions.contains(&position) && sends(binding).contains(&wanted) {
                positions.push(position);
            }
        }
    }
    positions
}

/// For each key position, whether it sends anything to the computer on any
/// layer, and so can be tested at all.
pub fn testable(project: &Project) -> Vec<bool> {
    (0..project.key_count)
        .map(|position| {
            project
                .layers
                .iter()
                .filter_map(|layer| layer.bindings.get(position))
                .any(|binding| !sends(binding).is_empty())
        })
        .collect()
}

/// Which keys are down now, and which have been seen at least once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tester {
    pressed: BTreeSet<usize>,
    seen: BTreeSet<usize>,
}

impl Tester {
    pub fn press(&mut self, positions: &[usize]) {
        self.pressed.extend(positions);
        self.seen.extend(positions);
    }

    pub fn release(&mut self, positions: &[usize]) {
        for position in positions {
            self.pressed.remove(position);
        }
    }

    /// Lets go of every key, as when the window loses the keyboard.
    pub fn release_all(&mut self) {
        self.pressed.clear();
    }

    /// Forgets which keys have been seen, to start a test over.
    pub fn reset(&mut self) {
        self.pressed.clear();
        self.seen.clear();
    }

    pub fn is_pressed(&self, position: usize) -> bool {
        self.pressed.contains(&position)
    }

    pub fn is_seen(&self, position: usize) -> bool {
        self.seen.contains(&position)
    }

    /// How many of the keys that can be tested have been seen.
    pub fn progress(&self, testable: &[bool]) -> (usize, usize) {
        let total = testable.iter().filter(|t| **t).count();
        let seen = self
            .seen
            .iter()
            .filter(|p| testable.get(**p).copied().unwrap_or(false))
            .count();
        (seen, total)
    }
}

/// Where each key sits on the board as an instrument: its column counted
/// from the left and its row counted from the bottom, in whole keys, from
/// the key's center. Used to give each key a note.
pub fn grid(keys: &[Key]) -> Vec<(i32, i32)> {
    let center = |key: &Key| (key.x + key.w / 2, key.y + key.h / 2);
    let left = keys.iter().map(|k| center(k).0).min().unwrap_or(0);
    let bottom = keys.iter().map(|k| center(k).1).max().unwrap_or(0);
    let whole = |units: i32| (units + 50).div_euclid(100);
    keys.iter()
        .map(|key| {
            let (x, y) = center(key);
            (whole(x - left), whole(bottom - y))
        })
        .collect()
}
