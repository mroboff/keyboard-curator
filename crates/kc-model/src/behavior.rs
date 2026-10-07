//! Behaviors a user defines in their project: hold-taps, tap-dances,
//! mod-morphs, sticky keys and macros.

use kc_zmk::Modifier;
use serde::{Deserialize, Serialize};

use crate::binding::{BehaviorRef, Binding};
use crate::ids::BehaviorId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BehaviorDef {
    pub id: BehaviorId,
    /// The devicetree label, written `&label` in the generated keymap.
    pub label: String,
    /// The name shown in the app.
    pub name: String,
    /// What the behavior is for, in the user's words. Written above it in
    /// the generated keymap as a comment.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub kind: BehaviorKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorKind {
    HoldTap(HoldTap),
    TapDance(TapDance),
    ModMorph(ModMorph),
    StickyKey(StickyKey),
    Macro(Macro),
}

impl BehaviorKind {
    /// What this kind of behavior is called.
    pub fn name(&self) -> &'static str {
        match self {
            BehaviorKind::HoldTap(_) => "Hold-tap",
            BehaviorKind::TapDance(_) => "Tap-dance",
            BehaviorKind::ModMorph(_) => "Mod-morph",
            BehaviorKind::StickyKey(_) => "Sticky key",
            BehaviorKind::Macro(_) => "Macro",
        }
    }

    /// One sentence on what this kind of behavior does.
    pub fn summary(&self) -> &'static str {
        match self {
            BehaviorKind::HoldTap(_) => "Does one thing when tapped and another when held.",
            BehaviorKind::TapDance(_) => {
                "Does something different depending on how many times it is tapped."
            }
            BehaviorKind::ModMorph(_) => {
                "Sends one key normally and another while chosen modifiers are held."
            }
            BehaviorKind::StickyKey(_) => {
                "Stays in effect after it is let go, until the next key is pressed."
            }
            BehaviorKind::Macro(_) => "Plays a sequence of key presses.",
        }
    }

    /// How many parameters a binding to this behavior takes.
    pub fn param_count(&self) -> usize {
        match self {
            BehaviorKind::HoldTap(_) => 2,
            BehaviorKind::TapDance(_) | BehaviorKind::ModMorph(_) => 0,
            BehaviorKind::StickyKey(_) => 1,
            BehaviorKind::Macro(m) => m.params as usize,
        }
    }

    /// The bindings held inside this behavior.
    pub fn bindings_mut(&mut self) -> Vec<&mut Binding> {
        match self {
            BehaviorKind::HoldTap(_) | BehaviorKind::StickyKey(_) => vec![],
            BehaviorKind::TapDance(t) => t.bindings.iter_mut().collect(),
            BehaviorKind::ModMorph(m) => vec![&mut m.normal, &mut m.morphed],
            BehaviorKind::Macro(m) => m
                .steps
                .iter_mut()
                .flat_map(|step| match step {
                    MacroStep::Tap(b) | MacroStep::Press(b) | MacroStep::Release(b) => {
                        b.iter_mut().collect()
                    }
                    _ => vec![],
                })
                .collect(),
        }
    }

    pub fn bindings(&self) -> Vec<&Binding> {
        match self {
            BehaviorKind::HoldTap(_) | BehaviorKind::StickyKey(_) => vec![],
            BehaviorKind::TapDance(t) => t.bindings.iter().collect(),
            BehaviorKind::ModMorph(m) => vec![&m.normal, &m.morphed],
            BehaviorKind::Macro(m) => m
                .steps
                .iter()
                .flat_map(|step| match step {
                    MacroStep::Tap(b) | MacroStep::Press(b) | MacroStep::Release(b) => {
                        b.iter().collect()
                    }
                    _ => vec![],
                })
                .collect(),
        }
    }

    /// The behaviors this one is built from, such as a hold-tap's two sides.
    pub fn behavior_refs(&self) -> Vec<&BehaviorRef> {
        match self {
            BehaviorKind::HoldTap(h) => vec![&h.hold, &h.tap],
            BehaviorKind::StickyKey(s) => vec![&s.behavior],
            _ => vec![],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Flavor {
    #[default]
    HoldPreferred,
    Balanced,
    TapPreferred,
    TapUnlessInterrupted,
}

/// A key that does one thing when held and another when tapped. A binding to
/// it supplies the parameter for each side, hold first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldTap {
    pub hold: BehaviorRef,
    pub tap: BehaviorRef,
    pub flavor: Flavor,
    pub tapping_term_ms: u32,
    pub quick_tap_ms: Option<u32>,
    pub require_prior_idle_ms: Option<u32>,
    pub retro_tap: bool,
    pub hold_while_undecided: bool,
    /// Key positions that let the hold trigger; empty means any key.
    pub hold_trigger_key_positions: Vec<usize>,
    pub hold_trigger_on_release: bool,
    /// Hold only when the next key is on the other hand, decided by the
    /// firmware from where the keys are. Firmware without the feature
    /// ignores it; ZMK uses the hold-trigger key positions instead.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub opposite_hand_hold: bool,
}

impl HoldTap {
    pub fn new(hold: BehaviorRef, tap: BehaviorRef) -> Self {
        Self {
            hold,
            tap,
            flavor: Flavor::default(),
            tapping_term_ms: 200,
            quick_tap_ms: None,
            require_prior_idle_ms: None,
            retro_tap: false,
            hold_while_undecided: false,
            hold_trigger_key_positions: Vec::new(),
            hold_trigger_on_release: false,
            opposite_hand_hold: false,
        }
    }
}

/// A different binding for one tap, two taps, and so on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TapDance {
    pub tapping_term_ms: u32,
    pub bindings: Vec<Binding>,
}

/// One binding normally, another while certain modifiers are held.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModMorph {
    pub normal: Binding,
    pub morphed: Binding,
    pub mods: Vec<Modifier>,
    pub keep_mods: Vec<Modifier>,
}

/// A sticky key or sticky layer with non-default timing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StickyKey {
    /// The behavior made sticky: `kp` for keys, `mo` for layers.
    pub behavior: BehaviorRef,
    pub release_after_ms: u32,
    pub quick_release: bool,
    pub lazy: bool,
    pub ignore_modifiers: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Macro {
    pub wait_ms: Option<u32>,
    pub tap_ms: Option<u32>,
    /// How many parameters a binding to this macro takes: 0, 1 or 2.
    pub params: u8,
    pub steps: Vec<MacroStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroStep {
    Tap(Vec<Binding>),
    Press(Vec<Binding>),
    Release(Vec<Binding>),
    /// Wait here until the macro's key is released.
    PauseForRelease,
    /// Change the wait between the steps that follow.
    WaitTime(u32),
    /// Change how long the taps that follow are held.
    TapTime(u32),
    /// Pass one of the macro's parameters to the next binding.
    Param {
        from: u8,
        to: u8,
    },
}
