//! Behaviors a user defines in their project: hold-taps, tap-dances,
//! mod-morphs, sticky keys and macros.
//!
//! A definition is either a **template**, named by the user and listed for
//! reuse, or **owned by a key**: made by the model when a key needs a node
//! of its own, hidden, and dropped when nothing uses it any more. Keys are
//! described in slots by [`crate::setup`], which does that bookkeeping.

use kc_zmk::Modifier;
use serde::{Deserialize, Serialize};

use crate::binding::{BehaviorRef, Binding};
use crate::ids::BehaviorId;

/// Who a definition belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Named by the user and listed on the Templates page for reuse.
    #[default]
    Template,
    /// Made for the keys that use it, hidden, and dropped when unused.
    Key,
}

impl Scope {
    fn is_template(&self) -> bool {
        *self == Scope::Template
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BehaviorDef {
    pub id: BehaviorId,
    /// The devicetree label, written `&label` in the generated keymap.
    pub label: String,
    /// The name shown in the app. For a key-owned definition it is the
    /// key's note, usually empty.
    #[serde(default)]
    pub name: String,
    /// What the behavior is for, in the user's words. Written above it in
    /// the generated keymap as a comment.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Scope::is_template")]
    pub scope: Scope,
    /// For a key-owned definition, the template whose settings it follows:
    /// its own content, the template's feel. Editing the template's
    /// settings updates it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follows: Option<BehaviorId>,
    pub kind: BehaviorKind,
}

impl BehaviorDef {
    /// The name to show when one is needed: the user's name, or else what
    /// kind of behavior it is.
    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            self.kind.name().to_string()
        } else {
            self.name.clone()
        }
    }

    pub fn is_template(&self) -> bool {
        self.scope == Scope::Template
    }
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
    /// What this kind of behavior is called in the app.
    pub fn name(&self) -> &'static str {
        match self {
            BehaviorKind::HoldTap(_) => "Hold & tap",
            BehaviorKind::TapDance(_) => "Multi-tap",
            BehaviorKind::ModMorph(_) => "Shift override",
            BehaviorKind::StickyKey(_) => "Sticky",
            BehaviorKind::Macro(_) => "Macro",
        }
    }

    /// One sentence on what this kind of behavior does.
    pub fn summary(&self) -> &'static str {
        match self {
            BehaviorKind::HoldTap(_) => "Does one thing when tapped and another when held.",
            BehaviorKind::TapDance(_) => "Does something different when tapped more than once.",
            BehaviorKind::ModMorph(_) => "Sends a different key while chosen modifiers are held.",
            BehaviorKind::StickyKey(_) => "Stays active for the next key after it is let go.",
            BehaviorKind::Macro(_) => "Types a sequence of keys.",
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

    pub fn behavior_refs_mut(&mut self) -> Vec<&mut BehaviorRef> {
        match self {
            BehaviorKind::HoldTap(h) => vec![&mut h.hold, &mut h.tap],
            BehaviorKind::StickyKey(s) => vec![&mut s.behavior],
            _ => vec![],
        }
    }

    /// The settings of this behavior apart from what it does: the part a
    /// template carries.
    pub fn settings(&self) -> Settings {
        match self {
            BehaviorKind::HoldTap(h) => Settings::HoldTap(h.settings()),
            BehaviorKind::TapDance(t) => Settings::TapDance(TapDanceSettings {
                tapping_term_ms: t.tapping_term_ms,
            }),
            BehaviorKind::ModMorph(m) => Settings::ModMorph(ModMorphSettings {
                mods: m.mods.clone(),
                keep_mods: m.keep_mods.clone(),
            }),
            BehaviorKind::StickyKey(s) => Settings::StickyKey(s.settings()),
            BehaviorKind::Macro(m) => Settings::Macro(MacroSettings {
                wait_ms: m.wait_ms,
                tap_ms: m.tap_ms,
            }),
        }
    }

    /// Takes over `settings`, leaving what the behavior does as it is.
    /// Settings of another kind are ignored.
    pub fn set_settings(&mut self, settings: &Settings) {
        match (self, settings) {
            (BehaviorKind::HoldTap(h), Settings::HoldTap(s)) => h.set_settings(s),
            (BehaviorKind::TapDance(t), Settings::TapDance(s)) => {
                t.tapping_term_ms = s.tapping_term_ms;
            }
            (BehaviorKind::ModMorph(m), Settings::ModMorph(s)) => {
                m.mods = s.mods.clone();
                m.keep_mods = s.keep_mods.clone();
            }
            (BehaviorKind::StickyKey(k), Settings::StickyKey(s)) => k.set_settings(s),
            (BehaviorKind::Macro(m), Settings::Macro(s)) => {
                m.wait_ms = s.wait_ms;
                m.tap_ms = s.tap_ms;
            }
            _ => {}
        }
    }
}

/// The settings of a behavior apart from what it does, by kind. A template
/// is one of these with a name; a key-owned definition carries its own.
#[derive(Debug, Clone, PartialEq)]
pub enum Settings {
    HoldTap(HoldTapSettings),
    TapDance(TapDanceSettings),
    ModMorph(ModMorphSettings),
    StickyKey(StickyKeySettings),
    Macro(MacroSettings),
}

impl Settings {
    /// Whether these are the settings a behavior of the kind starts with,
    /// which built-in behaviors such as `&mt` and `&sk` share.
    pub fn is_default(&self) -> bool {
        match self {
            Settings::HoldTap(s) => *s == HoldTapSettings::default(),
            Settings::TapDance(s) => *s == TapDanceSettings::default(),
            Settings::ModMorph(s) => *s == ModMorphSettings::default(),
            Settings::StickyKey(s) => *s == StickyKeySettings::default(),
            Settings::Macro(s) => *s == MacroSettings::default(),
        }
    }

    /// Whether `kind` can take these settings.
    pub fn fits(&self, kind: &BehaviorKind) -> bool {
        matches!(
            (self, kind),
            (Settings::HoldTap(_), BehaviorKind::HoldTap(_))
                | (Settings::TapDance(_), BehaviorKind::TapDance(_))
                | (Settings::ModMorph(_), BehaviorKind::ModMorph(_))
                | (Settings::StickyKey(_), BehaviorKind::StickyKey(_))
                | (Settings::Macro(_), BehaviorKind::Macro(_))
        )
    }
}

/// How a hold-tap decides between its two sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldTapSettings {
    pub flavor: Flavor,
    pub tapping_term_ms: u32,
    pub quick_tap_ms: Option<u32>,
    pub require_prior_idle_ms: Option<u32>,
    pub retro_tap: bool,
    pub hold_while_undecided: bool,
    pub hold_trigger_key_positions: Vec<usize>,
    pub hold_trigger_on_release: bool,
    pub opposite_hand_hold: bool,
}

impl Default for HoldTapSettings {
    fn default() -> Self {
        Self {
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

impl HoldTapSettings {
    /// The settings most people use for home-row modifiers: balanced,
    /// patient, and holding only for keys on the other hand.
    pub fn home_row() -> Self {
        Self {
            flavor: Flavor::Balanced,
            tapping_term_ms: 280,
            quick_tap_ms: Some(175),
            require_prior_idle_ms: Some(150),
            retro_tap: false,
            hold_while_undecided: false,
            hold_trigger_key_positions: Vec::new(),
            hold_trigger_on_release: true,
            opposite_hand_hold: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapDanceSettings {
    pub tapping_term_ms: u32,
}

impl Default for TapDanceSettings {
    fn default() -> Self {
        Self {
            tapping_term_ms: 200,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModMorphSettings {
    /// The modifiers that switch to the second binding.
    pub mods: Vec<Modifier>,
    /// Modifiers passed on to the second binding instead of being swallowed.
    pub keep_mods: Vec<Modifier>,
}

impl Default for ModMorphSettings {
    /// Either Shift, as a shifted key usually wants.
    fn default() -> Self {
        Self {
            mods: vec![Modifier::LShift, Modifier::RShift],
            keep_mods: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StickyKeySettings {
    pub release_after_ms: u32,
    pub quick_release: bool,
    pub lazy: bool,
    pub ignore_modifiers: bool,
}

impl Default for StickyKeySettings {
    /// ZMK's own defaults for `&sk` and `&sl`.
    fn default() -> Self {
        Self {
            release_after_ms: 1000,
            quick_release: false,
            lazy: false,
            ignore_modifiers: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MacroSettings {
    pub wait_ms: Option<u32>,
    pub tap_ms: Option<u32>,
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
        Self::with_settings(hold, tap, &HoldTapSettings::default())
    }

    pub fn with_settings(hold: BehaviorRef, tap: BehaviorRef, settings: &HoldTapSettings) -> Self {
        let mut hold_tap = Self {
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
        };
        hold_tap.set_settings(settings);
        hold_tap
    }

    pub fn settings(&self) -> HoldTapSettings {
        HoldTapSettings {
            flavor: self.flavor,
            tapping_term_ms: self.tapping_term_ms,
            quick_tap_ms: self.quick_tap_ms,
            require_prior_idle_ms: self.require_prior_idle_ms,
            retro_tap: self.retro_tap,
            hold_while_undecided: self.hold_while_undecided,
            hold_trigger_key_positions: self.hold_trigger_key_positions.clone(),
            hold_trigger_on_release: self.hold_trigger_on_release,
            opposite_hand_hold: self.opposite_hand_hold,
        }
    }

    pub fn set_settings(&mut self, s: &HoldTapSettings) {
        self.flavor = s.flavor;
        self.tapping_term_ms = s.tapping_term_ms;
        self.quick_tap_ms = s.quick_tap_ms;
        self.require_prior_idle_ms = s.require_prior_idle_ms;
        self.retro_tap = s.retro_tap;
        self.hold_while_undecided = s.hold_while_undecided;
        self.hold_trigger_key_positions = s.hold_trigger_key_positions.clone();
        self.hold_trigger_on_release = s.hold_trigger_on_release;
        self.opposite_hand_hold = s.opposite_hand_hold;
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

impl StickyKey {
    pub fn with_settings(behavior: BehaviorRef, s: &StickyKeySettings) -> Self {
        Self {
            behavior,
            release_after_ms: s.release_after_ms,
            quick_release: s.quick_release,
            lazy: s.lazy,
            ignore_modifiers: s.ignore_modifiers,
        }
    }

    pub fn settings(&self) -> StickyKeySettings {
        StickyKeySettings {
            release_after_ms: self.release_after_ms,
            quick_release: self.quick_release,
            lazy: self.lazy,
            ignore_modifiers: self.ignore_modifiers,
        }
    }

    pub fn set_settings(&mut self, s: &StickyKeySettings) {
        self.release_after_ms = s.release_after_ms;
        self.quick_release = s.quick_release;
        self.lazy = s.lazy;
        self.ignore_modifiers = s.ignore_modifiers;
    }
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
    /// Wait here for this many milliseconds, once.
    Pause(u32),
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
