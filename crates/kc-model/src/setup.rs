//! A key described in slots: what tapping it does, what holding it does,
//! what a second tap does, and how the key feels. This is the view the
//! key setup panel edits. Reading a key ([`KeySetup::of`]) turns its
//! binding and the definitions behind it into slots; writing one back
//! ([`Project::apply_setup`]) chooses how to store it and keeps the
//! definitions tidy, so the user never meets a behavior definition.
//!
//! A key is stored in one of three forms, chosen here:
//!
//! 1. a built-in behavior with its default feel (`kp`, `mt`, `lt`, `sk`,
//!    `sl`), which every firmware family has;
//! 2. a reference to a **template**, named by the user, with the key's own
//!    parameters;
//! 3. a **key-owned definition**, auto-labeled and hidden, for a custom
//!    feel or for slots a built-in cannot hold. It may follow a template
//!    for its settings.
//!
//! Slots nest in a fixed order when stored: a shift override around a
//! multi-tap around a hold-tap around a sticky key around the tap. A
//! binding nested any other way (as hand-written or imported keymaps may
//! be) cannot be shown as slots and stays an advanced binding.

use kc_zmk::behaviors;

use crate::behavior::{
    BehaviorDef, BehaviorKind, HoldTap, HoldTapSettings, ModMorph, ModMorphSettings, Scope,
    Settings, StickyKey, StickyKeySettings, TapDance, TapDanceSettings,
};
use crate::binding::{BehaviorRef, Binding, Param};
use crate::ids::{BehaviorId, LayerId};
use crate::project::{Location, ModelError, Project};

/// Where one part of a key's feel comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice<S> {
    /// The keyboard's own default: a built-in behavior where one exists.
    Default,
    /// A named template; the key follows its settings.
    Template(BehaviorId),
    /// Settings of this key's own.
    Custom(S),
}

impl<S> Choice<S> {
    pub fn template(&self) -> Option<BehaviorId> {
        match self {
            Choice::Template(id) => Some(*id),
            _ => None,
        }
    }
}

/// How a key feels: one choice per part of it that has settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feel {
    /// How hold and tap are told apart; used by the Hold and the Tap-then-
    /// hold slots.
    pub hold: Choice<HoldTapSettings>,
    /// How long a second tap may wait.
    pub taps: Choice<TapDanceSettings>,
    /// Which modifiers switch to the override.
    pub shifted: Choice<ModMorphSettings>,
    /// How a sticky key lets go.
    pub sticky: Choice<StickyKeySettings>,
}

impl Default for Feel {
    fn default() -> Self {
        Self {
            hold: Choice::Default,
            taps: Choice::Default,
            shifted: Choice::Default,
            sticky: Choice::Default,
        }
    }
}

/// One part of a key's feel, for saving it as a template or detaching it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    Hold,
    Taps,
    Shifted,
    Sticky,
}

/// The key-owned definitions a key used when it was read, so that writing
/// it back can edit them in place instead of making new ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Prior {
    shifted: Option<BehaviorId>,
    taps: Option<BehaviorId>,
    hold: Option<BehaviorId>,
    tap_hold: Option<BehaviorId>,
    sticky: Option<BehaviorId>,
}

/// A key as the key setup panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct KeySetup {
    /// What a press does. Any binding, including a transparent one.
    pub tap: Binding,
    /// What holding the key does instead.
    pub hold: Option<Binding>,
    /// What a second tap does.
    pub double_tap: Option<Binding>,
    /// What holding the key after one tap does.
    pub tap_hold: Option<Binding>,
    /// What a third tap and later taps do.
    pub more_taps: Vec<Binding>,
    /// What the key sends while the override modifiers (⇧ unless the feel
    /// says otherwise) are held.
    pub shifted: Option<Binding>,
    /// Whether the tap stays in effect for the next key.
    pub sticky: bool,
    pub feel: Feel,
    /// The user's note on the key, kept on its key-owned definition.
    pub note: String,
    prior: Prior,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SetupError {
    #[error("a hold, tap or sticky side must be a single key, layer or behavior")]
    NotASide,
    #[error("the override needs at least one modifier")]
    NoModifiers,
    #[error("that template no longer exists or is of another kind")]
    NoSuchTemplate,
    #[error("the key cannot be shown as slots")]
    NotSlots,
    #[error(transparent)]
    Model(#[from] ModelError),
}

impl KeySetup {
    /// A key that just does `tap`.
    pub fn plain(tap: Binding) -> Self {
        Self {
            tap,
            hold: None,
            double_tap: None,
            tap_hold: None,
            more_taps: Vec::new(),
            shifted: None,
            sticky: false,
            feel: Feel::default(),
            note: String::new(),
            prior: Prior::default(),
        }
    }

    /// The key at `position` on `layer` as slots, or `None` when its
    /// binding cannot be shown that way.
    pub fn of(project: &Project, layer: LayerId, position: usize) -> Option<Self> {
        Self::read(project, project.binding(layer, position)?)
    }

    /// A binding as slots, or `None` when it is raw or nested in a way the
    /// slots cannot show.
    pub fn read(project: &Project, binding: &Binding) -> Option<Self> {
        if matches!(binding, Binding::Raw { .. }) {
            return None;
        }
        let mut setup = Self::plain(binding.clone());
        let mut current = binding.clone();
        let mut note: Option<String> = None;
        let mut keep_note = |def: &BehaviorDef| {
            if note.is_none() && def.scope == Scope::Key && !def.name.is_empty() {
                note = Some(def.name.clone());
            }
        };

        if let Some((def, m)) = mod_morph(project, &current) {
            keep_note(def);
            setup.shifted = Some(m.morphed.clone());
            setup.feel.shifted = choice(def);
            setup.prior.shifted = owned(def);
            current = m.normal.clone();
        }

        if let Some((def, t)) = tap_dance(project, &current) {
            keep_note(def);
            let first = t.bindings.first()?;
            if let Some(second) = t.bindings.get(1) {
                match hold_tap(project, second) {
                    Some(side) => {
                        setup.tap_hold = Some(side.hold);
                        setup.double_tap = Some(side.tap).filter(|b| !is_none(b));
                        setup.feel.hold = side.choice;
                        setup.prior.tap_hold = side.owned;
                    }
                    None => setup.double_tap = Some(second.clone()),
                }
            }
            setup.more_taps = t.bindings.iter().skip(2).cloned().collect();
            setup.feel.taps = choice(def);
            setup.prior.taps = owned(def);
            current = first.clone();
        }

        if let Some(side) = hold_tap(project, &current) {
            if setup.tap_hold.is_some() && setup.feel.hold != side.choice {
                // Two hold-taps with different feels have no single slot.
                return None;
            }
            if let Some(def) = side.def(project) {
                keep_note(def);
            }
            setup.hold = Some(side.hold);
            setup.feel.hold = side.choice;
            setup.prior.hold = side.owned;
            current = side.tap;
        }

        if let Some((inner, choice, owned)) = sticky(project, &current) {
            if let Some(def) = owned.and_then(|id| project.behavior(id)) {
                keep_note(def);
            }
            setup.sticky = true;
            setup.feel.sticky = choice;
            setup.prior.sticky = owned;
            current = inner;
        }

        setup.tap = current;
        setup.note = note.unwrap_or_default();
        Some(setup)
    }

    /// Whether the key is more than a plain tap.
    pub fn has_extras(&self) -> bool {
        self.hold.is_some()
            || self.double_tap.is_some()
            || self.tap_hold.is_some()
            || !self.more_taps.is_empty()
            || self.shifted.is_some()
            || self.sticky
    }

    /// Whether a second tap or later does something.
    pub fn has_taps(&self) -> bool {
        self.double_tap.is_some() || self.tap_hold.is_some() || !self.more_taps.is_empty()
    }

    /// Whether a hold, on the first or the second press, does something.
    pub fn has_hold(&self) -> bool {
        self.hold.is_some() || self.tap_hold.is_some()
    }
}

fn is_none(binding: &Binding) -> bool {
    matches!(binding, Binding::Behavior { behavior: BehaviorRef::BuiltIn(l), .. } if l == "none")
}

fn owned(def: &BehaviorDef) -> Option<BehaviorId> {
    (def.scope == Scope::Key).then_some(def.id)
}

/// Where a definition's settings come from, as the slots show it.
fn choice<S>(def: &BehaviorDef) -> Choice<S>
where
    S: Clone + Default + PartialEq,
    Settings: TryInto<S>,
{
    if def.scope == Scope::Template {
        return Choice::Template(def.id);
    }
    if let Some(template) = def.follows {
        return Choice::Template(template);
    }
    let own: S = match def.kind.settings().try_into() {
        Ok(s) => s,
        Err(_) => return Choice::Default,
    };
    if own == S::default() {
        Choice::Default
    } else {
        Choice::Custom(own)
    }
}

macro_rules! settings_into {
    ($variant:ident, $type:ty) => {
        impl TryFrom<Settings> for $type {
            type Error = ();
            fn try_from(s: Settings) -> Result<Self, ()> {
                match s {
                    Settings::$variant(v) => Ok(v),
                    _ => Err(()),
                }
            }
        }
    };
}
settings_into!(HoldTap, HoldTapSettings);
settings_into!(TapDance, TapDanceSettings);
settings_into!(ModMorph, ModMorphSettings);
settings_into!(StickyKey, StickyKeySettings);

fn user_def<'a>(
    project: &'a Project,
    binding: &'a Binding,
) -> Option<(&'a BehaviorDef, &'a [Param])> {
    match binding {
        Binding::Behavior {
            behavior: BehaviorRef::User { user },
            params,
        } => Some((project.behavior(*user)?, params.as_slice())),
        _ => None,
    }
}

fn mod_morph<'a>(
    project: &'a Project,
    binding: &'a Binding,
) -> Option<(&'a BehaviorDef, &'a ModMorph)> {
    let (def, _) = user_def(project, binding)?;
    match &def.kind {
        BehaviorKind::ModMorph(m) => Some((def, m)),
        _ => None,
    }
}

fn tap_dance<'a>(
    project: &'a Project,
    binding: &'a Binding,
) -> Option<(&'a BehaviorDef, &'a TapDance)> {
    let (def, _) = user_def(project, binding)?;
    match &def.kind {
        BehaviorKind::TapDance(t) => Some((def, t)),
        _ => None,
    }
}

/// How many parameters a behavior takes.
fn param_count(project: &Project, behavior: &BehaviorRef) -> usize {
    match behavior {
        BehaviorRef::BuiltIn(label) => behaviors::built_in(label).map_or(0, |d| d.params.len()),
        BehaviorRef::User { user } => project.behavior(*user).map_or(0, |d| d.kind.param_count()),
    }
}

/// The binding one side of a hold-tap or sticky key stands for: the wrapped
/// behavior with the key's parameter, or without one when it takes none.
pub(crate) fn side_binding(
    project: &Project,
    behavior: &BehaviorRef,
    param: Option<&Param>,
) -> Binding {
    let params = if param_count(project, behavior) == 0 {
        Vec::new()
    } else {
        param.cloned().into_iter().collect()
    };
    Binding::Behavior {
        behavior: behavior.clone(),
        params,
    }
}

struct HoldTapSides {
    hold: Binding,
    tap: Binding,
    choice: Choice<HoldTapSettings>,
    owned: Option<BehaviorId>,
    def: Option<BehaviorId>,
}

impl HoldTapSides {
    fn def<'a>(&self, project: &'a Project) -> Option<&'a BehaviorDef> {
        project.behavior(self.def?)
    }
}

fn hold_tap(project: &Project, binding: &Binding) -> Option<HoldTapSides> {
    if let Binding::Behavior {
        behavior: BehaviorRef::BuiltIn(label),
        params,
    } = binding
    {
        return match (label.as_str(), params.as_slice()) {
            ("mt", [Param::Key(hold), Param::Key(tap)]) => Some(HoldTapSides {
                hold: Binding::kp(hold.clone()),
                tap: Binding::kp(tap.clone()),
                choice: Choice::Default,
                owned: None,
                def: None,
            }),
            ("lt", [Param::Layer(layer), Param::Key(tap)]) => Some(HoldTapSides {
                hold: Binding::layer("mo", *layer),
                tap: Binding::kp(tap.clone()),
                choice: Choice::Default,
                owned: None,
                def: None,
            }),
            _ => None,
        };
    }
    let (def, params) = user_def(project, binding)?;
    let BehaviorKind::HoldTap(h) = &def.kind else {
        return None;
    };
    Some(HoldTapSides {
        hold: side_binding(project, &h.hold, params.first()),
        tap: side_binding(project, &h.tap, params.get(1)),
        choice: choice(def),
        owned: owned(def),
        def: Some(def.id),
    })
}

fn sticky(
    project: &Project,
    binding: &Binding,
) -> Option<(Binding, Choice<StickyKeySettings>, Option<BehaviorId>)> {
    if let Binding::Behavior {
        behavior: BehaviorRef::BuiltIn(label),
        params,
    } = binding
    {
        return match (label.as_str(), params.as_slice()) {
            ("sk", [Param::Key(key)]) => Some((Binding::kp(key.clone()), Choice::Default, None)),
            ("sl", [Param::Layer(layer)]) => {
                Some((Binding::layer("mo", *layer), Choice::Default, None))
            }
            _ => None,
        };
    }
    let (def, params) = user_def(project, binding)?;
    let BehaviorKind::StickyKey(s) = &def.kind else {
        return None;
    };
    Some((
        side_binding(project, &s.behavior, params.first()),
        choice(def),
        owned(def),
    ))
}

/// A binding as one side of a hold-tap or sticky key: its behavior and its
/// single parameter, if any.
fn side(binding: &Binding) -> Result<(BehaviorRef, Option<Param>), SetupError> {
    match binding {
        Binding::Behavior { behavior, params } => match params.as_slice() {
            [] => Ok((behavior.clone(), None)),
            [Param::Command { args, .. }] if !args.is_empty() => Err(SetupError::NotASide),
            [param] => Ok((behavior.clone(), Some(param.clone()))),
            _ => Err(SetupError::NotASide),
        },
        Binding::Raw { .. } => Err(SetupError::NotASide),
    }
}

fn is_built_in(behavior: &BehaviorRef, label: &str) -> bool {
    matches!(behavior, BehaviorRef::BuiltIn(l) if l == label)
}

/// The label prefix of a key-owned definition of `kind`.
fn prefix(kind: &BehaviorKind) -> &'static str {
    match kind {
        BehaviorKind::HoldTap(_) => "ht",
        BehaviorKind::TapDance(_) => "td",
        BehaviorKind::ModMorph(_) => "mm",
        BehaviorKind::StickyKey(_) => "sk",
        BehaviorKind::Macro(_) => "macro",
    }
}

impl Project {
    /// Writes `setup` to the key at `position` on `layer`, choosing how to
    /// store it and tidying the definitions afterwards.
    pub fn apply_setup(
        &mut self,
        layer: LayerId,
        position: usize,
        setup: &KeySetup,
    ) -> Result<(), SetupError> {
        let here = Location::Key { layer, position };
        let mut binding = setup.tap.clone();
        let mut outermost: Option<BehaviorId> = None;
        let mut track = |id: Option<BehaviorId>| {
            if let Some(id) = id {
                outermost = Some(id);
            }
        };

        if setup.sticky {
            let (id, built) =
                self.build_sticky(&here, binding, &setup.feel.sticky, setup.prior.sticky)?;
            track(id);
            binding = built;
        }
        if let Some(hold) = &setup.hold {
            let (id, built) =
                self.build_hold_tap(&here, hold, &binding, &setup.feel.hold, setup.prior.hold)?;
            track(id);
            binding = built;
        }
        if setup.has_taps() {
            let mut list = vec![binding];
            let second = match (&setup.double_tap, &setup.tap_hold) {
                (Some(double), None) => double.clone(),
                (double, Some(hold)) => {
                    let tap = double.clone().unwrap_or_else(Binding::none);
                    self.build_hold_tap(&here, hold, &tap, &setup.feel.hold, setup.prior.tap_hold)?
                        .1
                }
                (None, None) => Binding::none(),
            };
            list.push(second);
            list.extend(setup.more_taps.iter().cloned());
            let (id, built) =
                self.build_tap_dance(&here, list, &setup.feel.taps, setup.prior.taps)?;
            track(id);
            binding = built;
        }
        if let Some(shifted) = &setup.shifted {
            let (id, built) = self.build_mod_morph(
                &here,
                binding,
                shifted.clone(),
                &setup.feel.shifted,
                setup.prior.shifted,
            )?;
            track(id);
            binding = built;
        }

        self.set_binding(layer, position, binding)?;
        if let Some(id) = outermost {
            if let Ok(def) = self.behavior_mut(id) {
                def.name = setup.note.clone();
            }
        }
        self.tidy();
        Ok(())
    }

    fn template_of(
        &self,
        id: BehaviorId,
        fits: impl FnOnce(&BehaviorKind) -> bool,
    ) -> Result<&BehaviorDef, SetupError> {
        self.behavior(id)
            .filter(|def| def.is_template() && fits(&def.kind))
            .ok_or(SetupError::NoSuchTemplate)
    }

    fn build_sticky(
        &mut self,
        here: &Location,
        inner: Binding,
        choice: &Choice<StickyKeySettings>,
        prior: Option<BehaviorId>,
    ) -> Result<(Option<BehaviorId>, Binding), SetupError> {
        let (behavior, param) = side(&inner)?;
        let (settings, follows) = match choice {
            Choice::Default => {
                match (&behavior, &param) {
                    (b, Some(Param::Key(key))) if is_built_in(b, "kp") => {
                        return Ok((None, Binding::new("sk", vec![Param::Key(key.clone())])));
                    }
                    (b, Some(Param::Layer(layer))) if is_built_in(b, "mo") => {
                        return Ok((None, Binding::layer("sl", *layer)));
                    }
                    _ => {}
                }
                (StickyKeySettings::default(), None)
            }
            Choice::Template(id) => {
                let template =
                    self.template_of(*id, |k| matches!(k, BehaviorKind::StickyKey(_)))?;
                let BehaviorKind::StickyKey(t) = &template.kind else {
                    unreachable!()
                };
                if t.behavior == behavior {
                    let params = param.into_iter().collect();
                    return Ok((None, Binding::user(*id, params)));
                }
                (t.settings(), Some(*id))
            }
            Choice::Custom(s) => (s.clone(), None),
        };
        let kind = BehaviorKind::StickyKey(StickyKey::with_settings(behavior, &settings));
        let id = self.own_definition(here, kind, follows, prior);
        let params = vec![param.unwrap_or(Param::Number(0))];
        Ok((Some(id), Binding::user(id, params)))
    }

    fn build_hold_tap(
        &mut self,
        here: &Location,
        hold: &Binding,
        tap: &Binding,
        choice: &Choice<HoldTapSettings>,
        prior: Option<BehaviorId>,
    ) -> Result<(Option<BehaviorId>, Binding), SetupError> {
        let (hold_ref, hold_param) = side(hold)?;
        let (tap_ref, tap_param) = side(tap)?;
        let (settings, follows) = match choice {
            Choice::Default => {
                match (&hold_ref, &hold_param, &tap_ref, &tap_param) {
                    (h, Some(Param::Key(hold_key)), t, Some(Param::Key(tap_key)))
                        if is_built_in(h, "kp") && is_built_in(t, "kp") =>
                    {
                        return Ok((
                            None,
                            Binding::new(
                                "mt",
                                vec![Param::Key(hold_key.clone()), Param::Key(tap_key.clone())],
                            ),
                        ));
                    }
                    (h, Some(Param::Layer(layer)), t, Some(Param::Key(tap_key)))
                        if is_built_in(h, "mo") && is_built_in(t, "kp") =>
                    {
                        return Ok((
                            None,
                            Binding::new(
                                "lt",
                                vec![Param::Layer(*layer), Param::Key(tap_key.clone())],
                            ),
                        ));
                    }
                    _ => {}
                }
                (HoldTapSettings::default(), None)
            }
            Choice::Template(id) => {
                let template = self.template_of(*id, |k| matches!(k, BehaviorKind::HoldTap(_)))?;
                let BehaviorKind::HoldTap(t) = &template.kind else {
                    unreachable!()
                };
                if t.hold == hold_ref && t.tap == tap_ref {
                    let params = vec![
                        hold_param.unwrap_or(Param::Number(0)),
                        tap_param.unwrap_or(Param::Number(0)),
                    ];
                    return Ok((None, Binding::user(*id, params)));
                }
                (t.settings(), Some(*id))
            }
            Choice::Custom(s) => (s.clone(), None),
        };
        let kind = BehaviorKind::HoldTap(HoldTap::with_settings(hold_ref, tap_ref, &settings));
        let id = self.own_definition(here, kind, follows, prior);
        let params = vec![
            hold_param.unwrap_or(Param::Number(0)),
            tap_param.unwrap_or(Param::Number(0)),
        ];
        Ok((Some(id), Binding::user(id, params)))
    }

    fn build_tap_dance(
        &mut self,
        here: &Location,
        bindings: Vec<Binding>,
        choice: &Choice<TapDanceSettings>,
        prior: Option<BehaviorId>,
    ) -> Result<(Option<BehaviorId>, Binding), SetupError> {
        let (settings, follows) = match choice {
            Choice::Default => (TapDanceSettings::default(), None),
            Choice::Template(id) => {
                let template = self.template_of(*id, |k| matches!(k, BehaviorKind::TapDance(_)))?;
                match template.kind.settings() {
                    Settings::TapDance(s) => (s, Some(*id)),
                    _ => unreachable!(),
                }
            }
            Choice::Custom(s) => (s.clone(), None),
        };
        let kind = BehaviorKind::TapDance(TapDance {
            tapping_term_ms: settings.tapping_term_ms,
            bindings,
        });
        let id = self.own_definition(here, kind, follows, prior);
        Ok((Some(id), Binding::user(id, vec![])))
    }

    fn build_mod_morph(
        &mut self,
        here: &Location,
        normal: Binding,
        morphed: Binding,
        choice: &Choice<ModMorphSettings>,
        prior: Option<BehaviorId>,
    ) -> Result<(Option<BehaviorId>, Binding), SetupError> {
        let (settings, follows) = match choice {
            Choice::Default => (ModMorphSettings::default(), None),
            Choice::Template(id) => {
                let template = self.template_of(*id, |k| matches!(k, BehaviorKind::ModMorph(_)))?;
                match template.kind.settings() {
                    Settings::ModMorph(s) => (s, Some(*id)),
                    _ => unreachable!(),
                }
            }
            Choice::Custom(s) => (s.clone(), None),
        };
        if settings.mods.is_empty() {
            return Err(SetupError::NoModifiers);
        }
        let kind = BehaviorKind::ModMorph(ModMorph {
            normal,
            morphed,
            mods: settings.mods,
            keep_mods: settings.keep_mods,
        });
        let id = self.own_definition(here, kind, follows, prior);
        Ok((Some(id), Binding::user(id, vec![])))
    }

    /// A key-owned definition with `kind`: the key's own one edited in
    /// place when it has one that nothing else uses, an identical existing
    /// one, or a new one.
    fn own_definition(
        &mut self,
        here: &Location,
        kind: BehaviorKind,
        follows: Option<BehaviorId>,
        prior: Option<BehaviorId>,
    ) -> BehaviorId {
        if let Some(id) = prior {
            let sole = self.behavior(id).is_some_and(|d| d.scope == Scope::Key)
                && self.top_owners(id) == [here.clone()];
            if sole {
                let def = self.behavior_mut(id).expect("checked above");
                def.kind = kind;
                def.follows = follows;
                return id;
            }
        }
        if let Some(existing) = self
            .behaviors
            .iter()
            .find(|d| d.scope == Scope::Key && d.follows == follows && d.kind == kind)
        {
            return existing.id;
        }
        let label = self.free_label(prefix(&kind));
        let id = self
            .add_behavior(label, "", kind)
            .expect("a free label is valid");
        let def = self.behavior_mut(id).expect("just added");
        def.scope = Scope::Key;
        def.follows = follows;
        id
    }

    /// `prefix_1`, `prefix_2`, …: the first label nothing uses.
    pub fn free_label(&self, prefix: &str) -> String {
        (1..)
            .map(|n| format!("{prefix}_{n}"))
            .find(|label| self.label_is_free(label))
            .expect("some number is free")
    }

    /// The label for a template named `name`: its slug, numbered only when
    /// that is taken.
    pub fn label_for(&self, name: &str) -> String {
        let slug = slug(name);
        if self.label_is_free(&slug) {
            return slug;
        }
        (2..)
            .map(|n| format!("{slug}_{n}"))
            .find(|label| self.label_is_free(label))
            .expect("some number is free")
    }

    /// Saves one part of the key's feel as a template named `name`, and
    /// makes the key use it.
    pub fn save_as_template(
        &mut self,
        layer: LayerId,
        position: usize,
        part: Part,
        name: impl Into<String>,
    ) -> Result<BehaviorId, SetupError> {
        let mut setup = KeySetup::of(self, layer, position).ok_or(SetupError::NotSlots)?;
        let name = name.into();
        let kind = match part {
            Part::Hold => {
                let hold = setup
                    .hold
                    .as_ref()
                    .or(setup.tap_hold.as_ref())
                    .ok_or(SetupError::NotSlots)?;
                let (hold_ref, _) = side(hold)?;
                let (tap_ref, _) = side(&setup.tap)?;
                let settings = match &setup.feel.hold {
                    Choice::Custom(s) => s.clone(),
                    Choice::Template(id) => return Ok(*id),
                    Choice::Default => HoldTapSettings::default(),
                };
                BehaviorKind::HoldTap(HoldTap::with_settings(hold_ref, tap_ref, &settings))
            }
            Part::Taps => {
                if !setup.has_taps() {
                    return Err(SetupError::NotSlots);
                }
                let settings = match &setup.feel.taps {
                    Choice::Custom(s) => s.clone(),
                    Choice::Template(id) => return Ok(*id),
                    Choice::Default => TapDanceSettings::default(),
                };
                BehaviorKind::TapDance(TapDance {
                    tapping_term_ms: settings.tapping_term_ms,
                    bindings: vec![Binding::none(), Binding::none()],
                })
            }
            Part::Shifted => {
                if setup.shifted.is_none() {
                    return Err(SetupError::NotSlots);
                }
                let settings = match &setup.feel.shifted {
                    Choice::Custom(s) => s.clone(),
                    Choice::Template(id) => return Ok(*id),
                    Choice::Default => ModMorphSettings::default(),
                };
                BehaviorKind::ModMorph(ModMorph {
                    normal: Binding::none(),
                    morphed: Binding::none(),
                    mods: settings.mods,
                    keep_mods: settings.keep_mods,
                })
            }
            Part::Sticky => {
                if !setup.sticky {
                    return Err(SetupError::NotSlots);
                }
                let (behavior, _) = side(&setup.tap)?;
                let settings = match &setup.feel.sticky {
                    Choice::Custom(s) => s.clone(),
                    Choice::Template(id) => return Ok(*id),
                    Choice::Default => StickyKeySettings::default(),
                };
                BehaviorKind::StickyKey(StickyKey::with_settings(behavior, &settings))
            }
        };
        let label = self.label_for(&name);
        let id = self.add_behavior(label, name, kind)?;
        match part {
            Part::Hold => setup.feel.hold = Choice::Template(id),
            Part::Taps => setup.feel.taps = Choice::Template(id),
            Part::Shifted => setup.feel.shifted = Choice::Template(id),
            Part::Sticky => setup.feel.sticky = Choice::Template(id),
        }
        self.apply_setup(layer, position, &setup)?;
        Ok(id)
    }

    /// Makes one part of the key's feel its own, with the settings it has
    /// now, no longer following a template.
    pub fn detach(
        &mut self,
        layer: LayerId,
        position: usize,
        part: Part,
    ) -> Result<(), SetupError> {
        let mut setup = KeySetup::of(self, layer, position).ok_or(SetupError::NotSlots)?;
        fn own<S: Clone>(
            choice: &Choice<S>,
            settings: impl FnOnce(BehaviorId) -> Option<S>,
        ) -> Choice<S> {
            match choice {
                Choice::Template(id) => settings(*id).map_or(Choice::Default, Choice::Custom),
                other => other.clone(),
            }
        }
        match part {
            Part::Hold => {
                setup.feel.hold = own(&setup.feel.hold, |id| {
                    self.behavior(id)?.kind.settings().try_into().ok()
                });
            }
            Part::Taps => {
                setup.feel.taps = own(&setup.feel.taps, |id| {
                    self.behavior(id)?.kind.settings().try_into().ok()
                });
            }
            Part::Shifted => {
                setup.feel.shifted = own(&setup.feel.shifted, |id| {
                    self.behavior(id)?.kind.settings().try_into().ok()
                });
            }
            Part::Sticky => {
                setup.feel.sticky = own(&setup.feel.sticky, |id| {
                    self.behavior(id)?.kind.settings().try_into().ok()
                });
            }
        }
        self.apply_setup(layer, position, &setup)
    }

    /// Gives the key its own copy of a template it uses directly, such as a
    /// macro, so that the template can change without the key.
    pub fn detach_from_template(
        &mut self,
        layer: LayerId,
        position: usize,
        template: BehaviorId,
    ) -> Result<(), SetupError> {
        let source = self
            .behavior(template)
            .filter(|d| d.is_template())
            .ok_or(SetupError::NoSuchTemplate)?
            .clone();
        let here = Location::Key { layer, position };
        let copy = self.own_definition(&here, source.kind.clone(), None, None);
        if let Ok(def) = self.behavior_mut(copy) {
            def.description = source.description.clone();
        }
        let mut binding = self
            .binding(layer, position)
            .ok_or(ModelError::NoSuchLayer(layer))?
            .clone();
        replace_in_binding(&mut binding, template, copy);
        self.set_binding(layer, position, binding)?;
        self.tidy();
        Ok(())
    }

    /// Turns a key-owned definition, such as a macro built on a key, into a
    /// template named `name`. The keys using it keep using it.
    pub fn make_template(
        &mut self,
        id: BehaviorId,
        name: impl Into<String>,
    ) -> Result<(), ModelError> {
        let name = name.into();
        let label = self.label_for(&name);
        let def = self.behavior_mut(id)?;
        def.scope = Scope::Template;
        def.follows = None;
        def.name = name;
        def.label = label;
        Ok(())
    }
}

/// A devicetree label made from a name: `Home-row mod` becomes `home_row_mod`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_').to_string();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        format!("t_{out}")
    } else {
        out
    }
}

fn replace_in_binding(binding: &mut Binding, from: BehaviorId, to: BehaviorId) {
    if let Binding::Behavior {
        behavior: BehaviorRef::User { user },
        ..
    } = binding
    {
        if *user == from {
            *user = to;
        }
    }
}
