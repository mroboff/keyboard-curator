//! The key tester: what the computer sees of a keyboard, matched back to
//! the layout's keys, layers and features.
//!
//! The computer only sees what a keyboard sends: keys, the modifiers held,
//! pointer movement, buttons and scrolling. It never sees which physical
//! key was pressed or which layer is active. So the tester keeps a layer
//! it is showing, matches each thing the computer reports to the functions
//! of that layer's keys first, and, when no key of that layer can have sent
//! it, moves to the first layer that can and marks the keys that lead to
//! that layer as held. The shown layer can be chosen by hand too.
//!
//! Every function of a key is tracked on its own: a hold-tap's tap and
//! hold, each count of a tap-dance, each side of a mod-morph. Combos and
//! macros are matched by what they send. Functions that send nothing to
//! the computer (Bluetooth, lighting, reset) cannot be tested and are
//! marked as such. Which functions have been seen is kept per layer.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use kc_boards::geometry::Key;
use kc_zmk::keycodes::{keycodes, UsagePage};
use kc_zmk::Modifier;

use crate::behavior::{BehaviorKind, MacroStep};
use crate::binding::{BehaviorRef, Binding, KeyExpr, Param};
use crate::ids::{BehaviorId, ComboId, LayerId};
use crate::keycap::{key_legend, keycap_for};
use crate::project::Project;

/// A key as the computer identifies it: its HID usage page and number.
pub type Usage = (UsagePage, u16);

/// How long a burst of keys may be spread out and still be a macro: the
/// gap between two of its keys.
const MACRO_GAP: Duration = Duration::from_millis(300);
/// A modifier that comes down this close before a key that needs it was
/// part of that key, not a key of its own.
const IMPLICIT_MODIFIER: Duration = Duration::from_millis(60);
/// How many recent keys are kept for matching macros.
const RECENT: usize = 24;

/// The four kinds of modifier the computer reports, without sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ModClass {
    Shift,
    Control,
    Alt,
    Gui,
}

impl ModClass {
    pub const ALL: [ModClass; 4] = [
        ModClass::Shift,
        ModClass::Control,
        ModClass::Alt,
        ModClass::Gui,
    ];

    /// The name macOS gives the key.
    pub fn name(self) -> &'static str {
        match self {
            ModClass::Shift => "Shift",
            ModClass::Control => "Control",
            ModClass::Alt => "Option",
            ModClass::Gui => "Command",
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            ModClass::Shift => "⇧",
            ModClass::Control => "⌃",
            ModClass::Alt => "⌥",
            ModClass::Gui => "⌘",
        }
    }

    fn of(modifier: Modifier) -> ModClass {
        match modifier {
            Modifier::LShift | Modifier::RShift => ModClass::Shift,
            Modifier::LCtrl | Modifier::RCtrl => ModClass::Control,
            Modifier::LAlt | Modifier::RAlt => ModClass::Alt,
            Modifier::LGui | Modifier::RGui => ModClass::Gui,
        }
    }

    /// The class of a modifier keycode, by its HID usage.
    fn of_usage(usage: Usage) -> Option<ModClass> {
        if usage.0 != UsagePage::Keyboard {
            return None;
        }
        let index = usage.1.checked_sub(0xE0)?;
        Modifier::ALL.get(index as usize).copied().map(ModClass::of)
    }
}

/// Which modifier classes are held, as the computer reports them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub gui: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        shift: false,
        control: false,
        alt: false,
        gui: false,
    };

    pub fn has(self, class: ModClass) -> bool {
        match class {
            ModClass::Shift => self.shift,
            ModClass::Control => self.control,
            ModClass::Alt => self.alt,
            ModClass::Gui => self.gui,
        }
    }

    fn set(&mut self, class: ModClass, down: bool) {
        match class {
            ModClass::Shift => self.shift = down,
            ModClass::Control => self.control = down,
            ModClass::Alt => self.alt = down,
            ModClass::Gui => self.gui = down,
        }
    }

    pub fn with(mut self, class: ModClass) -> Self {
        self.set(class, true);
        self
    }

    fn count(self) -> usize {
        ModClass::ALL.iter().filter(|c| self.has(**c)).count()
    }

    /// Whether every modifier in `self` is also in `other`.
    fn within(self, other: Mods) -> bool {
        ModClass::ALL.iter().all(|c| !self.has(*c) || other.has(*c))
    }

    fn from_modifiers<'a>(modifiers: impl IntoIterator<Item = &'a Modifier>) -> Self {
        let mut mods = Mods::NONE;
        for modifier in modifiers {
            mods.set(ModClass::of(*modifier), true);
        }
        mods
    }

    fn union(self, other: Mods) -> Mods {
        let mut mods = self;
        for class in ModClass::ALL {
            if other.has(class) {
                mods.set(class, true);
            }
        }
        mods
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub fn name(self) -> &'static str {
        match self {
            Direction::Up => "up",
            Direction::Down => "down",
            Direction::Left => "left",
            Direction::Right => "right",
        }
    }
}

/// What the computer sees when a function of a key fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// A key, with the modifiers that must be held with it.
    Key {
        usage: Usage,
        mods: Mods,
    },
    /// A modifier held on its own.
    Modifier(ModClass),
    /// A layer made active, seen only through the keys pressed afterwards.
    Layer(LayerId),
    /// A burst of keys, as a macro types them.
    Sequence(Vec<Usage>),
    Pointer(Direction),
    Scroll(Direction),
    /// A mouse button, numbered from 1.
    Button(u8),
    /// Nothing the computer can see.
    Unseen,
}

/// One thing a key can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facet {
    /// What to print for it: a legend, a layer name, a macro's name.
    pub label: String,
    /// How it is reached, for messages: `tap`, `hold`, `2 taps`, `with ⇧`,
    /// or nothing for a key that does one thing.
    pub role: String,
    pub signal: Signal,
}

impl Facet {
    fn new(label: impl Into<String>, signal: Signal) -> Self {
        Self {
            label: label.into(),
            role: String::new(),
            signal,
        }
    }

    fn with_role(mut self, role: &str) -> Self {
        if self.role.is_empty() {
            self.role = role.to_string();
        } else if !role.is_empty() {
            self.role = format!("{role}, {}", self.role);
        }
        self
    }

    pub fn testable(&self) -> bool {
        self.signal != Signal::Unseen
    }
}

fn usage(name: &str) -> Option<Usage> {
    keycodes().get(name).map(|key| (key.page, key.usage))
}

/// The signal of a key expression: a modifier on its own, or a key with
/// the modifiers wrapped around it and built into it.
fn key_signal(expr: &KeyExpr) -> Option<Signal> {
    let key = keycodes().get(&expr.key)?;
    let usage = (key.page, key.usage);
    if expr.mods.is_empty() {
        if let Some(class) = ModClass::of_usage(usage) {
            return Some(Signal::Modifier(class));
        }
    }
    let mods = Mods::from_modifiers(expr.mods.iter().chain(&key.implicit_mods));
    Some(Signal::Key { usage, mods })
}

fn key_facet(expr: &KeyExpr) -> Facet {
    Facet::new(key_legend(expr), key_signal(expr).unwrap_or(Signal::Unseen))
}

fn layer_facet(project: &Project, id: LayerId) -> Facet {
    let name = project
        .layer(id)
        .map_or_else(|| "?".to_string(), |l| l.name.clone());
    Facet::new(name, Signal::Layer(id))
}

fn constant_facet(name: &str) -> Facet {
    let signal = match name {
        "LCLK" => Signal::Button(1),
        "RCLK" => Signal::Button(2),
        "MCLK" => Signal::Button(3),
        "MB4" => Signal::Button(4),
        "MB5" => Signal::Button(5),
        "MOVE_UP" => Signal::Pointer(Direction::Up),
        "MOVE_DOWN" => Signal::Pointer(Direction::Down),
        "MOVE_LEFT" => Signal::Pointer(Direction::Left),
        "MOVE_RIGHT" => Signal::Pointer(Direction::Right),
        "SCRL_UP" => Signal::Scroll(Direction::Up),
        "SCRL_DOWN" => Signal::Scroll(Direction::Down),
        "SCRL_LEFT" => Signal::Scroll(Direction::Left),
        "SCRL_RIGHT" => Signal::Scroll(Direction::Right),
        _ => Signal::Unseen,
    };
    Facet::new(name, signal)
}

fn unseen(project: &Project, binding: &Binding) -> Vec<Facet> {
    let label = keycap_for(project, binding).legend;
    vec![Facet::new(label, Signal::Unseen)]
}

fn built_in_facets(
    project: &Project,
    label: &str,
    params: &[Param],
    binding: &Binding,
) -> Vec<Facet> {
    let key = |index: usize| match params.get(index) {
        Some(Param::Key(expr)) => Some(key_facet(expr)),
        _ => None,
    };
    let layer = |index: usize| match params.get(index) {
        Some(Param::Layer(id)) => Some(layer_facet(project, *id)),
        _ => None,
    };
    match label {
        "trans" | "none" => Vec::new(),
        "kp" | "kt" => key(0).into_iter().collect(),
        "sk" => key(0).map(|f| f.with_role("sticky")).into_iter().collect(),
        "mt" => key(1)
            .map(|f| f.with_role("tap"))
            .into_iter()
            .chain(key(0).map(|f| f.with_role("hold")))
            .collect(),
        "lt" => key(1)
            .map(|f| f.with_role("tap"))
            .into_iter()
            .chain(layer(0).map(|f| f.with_role("hold")))
            .collect(),
        "mo" => layer(0).map(|f| f.with_role("hold")).into_iter().collect(),
        "to" => layer(0).map(|f| f.with_role("go to")).into_iter().collect(),
        "tog" => layer(0)
            .map(|f| f.with_role("toggle"))
            .into_iter()
            .collect(),
        "sl" => layer(0)
            .map(|f| f.with_role("sticky"))
            .into_iter()
            .collect(),
        "gresc" => vec![
            key_facet(&KeyExpr::new("ESC")),
            key_facet(&KeyExpr::new("GRAVE").with(Modifier::LShift)).with_role("with ⇧"),
        ],
        "mkp" | "mmv" | "msc" => match params.first() {
            Some(Param::Constant(name)) => vec![constant_facet(name)],
            _ => unseen(project, binding),
        },
        _ => unseen(project, binding),
    }
}

/// The parameters a hold-tap passes on: its first to the hold behavior, its
/// second to the tap behavior. A behavior that takes none gets none.
fn sub_binding(behavior: &BehaviorRef, param: Option<&Param>) -> Binding {
    Binding::Behavior {
        behavior: behavior.clone(),
        params: param.cloned().into_iter().collect(),
    }
}

/// Fills a macro's placeholders: a `param` step hands one of the macro's
/// own parameters to the next binding.
fn with_params(binding: &Binding, pending: &[(u8, u8)], params: &[Param]) -> Binding {
    let Binding::Behavior {
        behavior,
        params: own,
    } = binding
    else {
        return binding.clone();
    };
    let mut own = own.clone();
    for (from, to) in pending {
        let (Some(value), Some(slot)) = (
            params.get(from.saturating_sub(1) as usize),
            own.get_mut(to.saturating_sub(1) as usize),
        ) else {
            continue;
        };
        *slot = value.clone();
    }
    Binding::Behavior {
        behavior: behavior.clone(),
        params: own,
    }
}

fn macro_facets(
    project: &Project,
    name: &str,
    steps: &[MacroStep],
    params: &[Param],
    depth: u8,
) -> Vec<Facet> {
    // The keys the macro types, each with the modifiers it holds down for
    // them, and any layers it moves to.
    let mut keys: Vec<(Usage, Mods)> = Vec::new();
    let mut layers: Vec<Facet> = Vec::new();
    let mut held = Mods::NONE;
    let mut pending: Vec<(u8, u8)> = Vec::new();
    for step in steps {
        let (bindings, press, release) = match step {
            MacroStep::Tap(b) => (b, false, false),
            MacroStep::Press(b) => (b, true, false),
            MacroStep::Release(b) => (b, false, true),
            MacroStep::Param { from, to } => {
                pending.push((*from, *to));
                continue;
            }
            _ => continue,
        };
        for (index, binding) in bindings.iter().enumerate() {
            let binding = if index == 0 {
                with_params(binding, &pending, params)
            } else {
                binding.clone()
            };
            for facet in facets_of(project, &binding, depth + 1) {
                match facet.signal {
                    Signal::Modifier(class) => {
                        if press {
                            held.set(class, true);
                        } else if release {
                            held.set(class, false);
                        }
                    }
                    Signal::Key { usage, mods } if !release => keys.push((usage, mods.union(held))),
                    Signal::Sequence(sequence) if !release => {
                        keys.extend(sequence.into_iter().map(|usage| (usage, held)));
                    }
                    Signal::Layer(_) => layers.push(facet),
                    _ => {}
                }
            }
        }
        pending.clear();
    }
    let mut facets = match keys.as_slice() {
        [] => match params.first() {
            // A macro whose steps cannot be read but which takes a key is
            // taken to type that key.
            Some(Param::Key(expr)) if layers.is_empty() => vec![key_facet(expr)],
            _ if layers.is_empty() => vec![Facet::new(name, Signal::Unseen)],
            _ => Vec::new(),
        },
        [(usage, mods)] => {
            let label = legend_for(*usage, *mods).unwrap_or_else(|| name.to_string());
            vec![Facet::new(
                label,
                Signal::Key {
                    usage: *usage,
                    mods: *mods,
                },
            )]
        }
        many => vec![Facet::new(
            name,
            Signal::Sequence(many.iter().map(|(usage, _)| *usage).collect()),
        )],
    };
    facets.extend(layers);
    facets
}

/// The legend of a key by its usage, with the modifiers held for it.
fn legend_for(usage: Usage, mods: Mods) -> Option<String> {
    let key = keycodes()
        .all()
        .iter()
        .find(|k| (k.page, k.usage) == usage && k.implicit_mods.is_empty())?;
    let mut legend = String::new();
    for class in ModClass::ALL {
        if mods.has(class) {
            legend.push_str(class.symbol());
        }
    }
    legend.push_str(&key.legend);
    Some(legend)
}

fn user_facets(project: &Project, id: BehaviorId, params: &[Param], depth: u8) -> Vec<Facet> {
    let Some(def) = project.behavior(id) else {
        return vec![Facet::new("?", Signal::Unseen)];
    };
    match &def.kind {
        BehaviorKind::HoldTap(ht) => {
            let tap = facets_of(project, &sub_binding(&ht.tap, params.get(1)), depth + 1);
            let hold = facets_of(project, &sub_binding(&ht.hold, params.first()), depth + 1);
            tap.into_iter()
                .map(|f| f.with_role("tap"))
                .chain(hold.into_iter().map(|f| f.with_role("hold")))
                .collect()
        }
        BehaviorKind::TapDance(td) => td
            .bindings
            .iter()
            .enumerate()
            .flat_map(|(index, binding)| {
                let role = match index {
                    0 => "1 tap".to_string(),
                    n => format!("{} taps", n + 1),
                };
                facets_of(project, binding, depth + 1)
                    .into_iter()
                    .map(move |f| f.with_role(&role))
            })
            .collect(),
        BehaviorKind::ModMorph(mm) => {
            let needed = Mods::from_modifiers(&mm.mods);
            let role = format!(
                "with {}",
                ModClass::ALL
                    .iter()
                    .filter(|c| needed.has(**c))
                    .map(|c| c.symbol())
                    .collect::<String>()
            );
            facets_of(project, &mm.normal, depth + 1)
                .into_iter()
                .chain(
                    facets_of(project, &mm.morphed, depth + 1)
                        .into_iter()
                        .map(|mut f| {
                            if let Signal::Key { mods, .. } = &mut f.signal {
                                *mods = mods.union(needed);
                            }
                            f.with_role(&role)
                        }),
                )
                .collect()
        }
        BehaviorKind::StickyKey(sk) => facets_of(
            project,
            &Binding::Behavior {
                behavior: sk.behavior.clone(),
                params: params.to_vec(),
            },
            depth + 1,
        )
        .into_iter()
        .map(|f| f.with_role("sticky"))
        .collect(),
        BehaviorKind::Macro(m) => macro_facets(project, &def.name, &m.steps, params, depth),
    }
}

/// Every function of a binding the computer could see, and those it
/// cannot. A transparent binding has none; its layer resolves it.
pub fn facets_of(project: &Project, binding: &Binding, depth: u8) -> Vec<Facet> {
    if depth > 6 {
        return Vec::new();
    }
    match binding {
        Binding::Raw { raw } => vec![Facet::new(raw.clone(), Signal::Unseen)],
        Binding::Behavior { behavior, params } => match behavior {
            BehaviorRef::BuiltIn(label) => built_in_facets(project, label, params, binding),
            BehaviorRef::User { user } => user_facets(project, *user, params, depth),
        },
    }
}

fn is_transparent(binding: &Binding) -> bool {
    matches!(binding, Binding::Behavior { behavior: BehaviorRef::BuiltIn(label), .. } if label == "trans")
}

/// The functions of the key at `position` on the layer at `index`, taking
/// a transparent key from the nearest layer below that binds it, and
/// whether it was taken from below.
fn resolved_facets(project: &Project, index: usize, position: usize) -> (Vec<Facet>, bool) {
    let own = project.layers[index].bindings.get(position);
    let inherited = own.is_some_and(is_transparent);
    project.layers[..=index]
        .iter()
        .rev()
        .filter_map(|layer| layer.bindings.get(position))
        .find(|binding| !is_transparent(binding))
        .map(|binding| (facets_of(project, binding, 0), inherited))
        .unwrap_or_default()
}

/// One function of one key on one layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hit {
    pub layer: LayerId,
    pub position: usize,
    pub facet: usize,
}

/// What presses are matched against first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// The keys of the shown layer; combos and macros only when no key
    /// sends what arrived.
    #[default]
    Keys,
    /// Combos and macros before plain keys.
    CombosAndMacros,
}

/// What the tester made of something the computer reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Keys lit, on `layer`; `switched` when the tester moved to that layer
    /// to find them; `completed` a macro whose last key this was.
    Keys {
        layer: LayerId,
        hits: Vec<Hit>,
        switched: bool,
        completed: Option<Hit>,
    },
    Combo {
        combo: ComboId,
    },
    /// A macro's keys all arrived; the key bound to it is lit.
    Macro {
        hit: Hit,
    },
    /// Nothing in the layout sends it.
    Unmatched,
    /// Nothing changed: the key was already held, or it is not one the
    /// tester tracks.
    Nothing,
}

/// How one function of a key stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacetState {
    /// Sends nothing to the computer; cannot be tested here.
    Untestable,
    Unseen,
    Seen,
    /// Down now, or firing.
    Pressed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetView {
    pub label: String,
    pub role: String,
    pub state: FacetState,
}

/// A key as the shown layer presents it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyView {
    pub facets: Vec<FacetView>,
    /// The physical key is down for a function of another layer, or is
    /// taken to be held because it leads to the shown layer.
    pub outlined: bool,
    /// The key is transparent on this layer: its functions are the layer
    /// below's.
    pub inherited: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    /// Keys with every testable function seen, and keys with any.
    pub keys_seen: usize,
    pub keys: usize,
    pub functions_seen: usize,
    pub functions: usize,
}

/// A modifier that came down, with what it lit and whether those were
/// already seen: if a key that needs the modifier follows at once, the
/// modifier was part of that key and the marks are taken back.
#[derive(Debug, Clone)]
struct ModifierPress {
    at: Instant,
    hits: Vec<(Hit, bool)>,
}

pub struct Tester {
    project: Project,
    /// The functions of every key of every layer, transparency resolved.
    facets: HashMap<LayerId, Vec<Vec<Facet>>>,
    /// Which keys of each layer are transparent there.
    inherited: HashMap<LayerId, Vec<bool>>,
    combo_facets: Vec<(ComboId, Vec<Facet>)>,
    shown: LayerId,
    mode: Mode,
    mods: Mods,
    /// What each thing held on the computer's side lit, by the name the
    /// toolkit gives it.
    held: HashMap<String, Vec<Hit>>,
    held_combos: HashMap<String, ComboId>,
    modifier_presses: HashMap<ModClass, ModifierPress>,
    pressed: BTreeSet<Hit>,
    seen: BTreeSet<Hit>,
    /// Keys taken to be held because they lead to the shown layer: the
    /// physical position, and the function on the layer it was pressed on.
    activators: Vec<(usize, Hit)>,
    activator_combo: Option<ComboId>,
    combos_seen: BTreeSet<ComboId>,
    combo_pressed: Option<ComboId>,
    /// Mouse functions lit by movement, until it stops.
    moving: Vec<Hit>,
    recent: VecDeque<(Usage, Instant)>,
}

impl Tester {
    pub fn new(project: Project) -> Self {
        let mut facets = HashMap::new();
        let mut inherited = HashMap::new();
        for (index, layer) in project.layers.iter().enumerate() {
            let (per_key, from_below): (Vec<Vec<Facet>>, Vec<bool>) = (0..project.key_count)
                .map(|position| resolved_facets(&project, index, position))
                .unzip();
            facets.insert(layer.id, per_key);
            inherited.insert(layer.id, from_below);
        }
        let combo_facets = project
            .combos
            .iter()
            .map(|combo| (combo.id, facets_of(&project, &combo.binding, 0)))
            .collect();
        let shown = project.layers.first().map_or(LayerId(0), |l| l.id);
        Self {
            project,
            facets,
            inherited,
            combo_facets,
            shown,
            mode: Mode::Keys,
            mods: Mods::NONE,
            held: HashMap::new(),
            held_combos: HashMap::new(),
            modifier_presses: HashMap::new(),
            pressed: BTreeSet::new(),
            seen: BTreeSet::new(),
            activators: Vec::new(),
            activator_combo: None,
            combos_seen: BTreeSet::new(),
            combo_pressed: None,
            moving: Vec::new(),
            recent: VecDeque::new(),
        }
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Tests against another layout, keeping nothing: marks mean nothing
    /// against other keys. The same layout keeps its marks.
    pub fn set_project(&mut self, project: Project) {
        if project != self.project {
            *self = Self::new(project);
        }
    }

    pub fn shown(&self) -> LayerId {
        self.shown
    }

    /// Shows a layer by hand. What was held for the layer before is let go.
    pub fn show(&mut self, layer: LayerId) {
        if self.project.layer(layer).is_none() || layer == self.shown {
            return;
        }
        self.shown = layer;
        self.release_activators();
        self.moving.clear();
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    pub fn mods(&self) -> Mods {
        self.mods
    }

    pub fn combo_pressed(&self) -> Option<ComboId> {
        self.combo_pressed.or(self.activator_combo)
    }

    pub fn facets(&self, layer: LayerId, position: usize) -> &[Facet] {
        self.facets
            .get(&layer)
            .and_then(|keys| keys.get(position))
            .map_or(&[], |f| f.as_slice())
    }

    fn facet(&self, hit: Hit) -> Option<&Facet> {
        self.facets(hit.layer, hit.position).get(hit.facet)
    }

    /// Every function on `layer` whose signal passes `want`, with how
    /// specific it is, so that the most specific matches win.
    fn hits_on(&self, layer: LayerId, want: impl Fn(&Signal) -> Option<usize>) -> Vec<Hit> {
        let Some(keys) = self.facets.get(&layer) else {
            return Vec::new();
        };
        let mut found: Vec<(Hit, usize)> = Vec::new();
        for (position, facets) in keys.iter().enumerate() {
            for (index, facet) in facets.iter().enumerate() {
                if let Some(rank) = want(&facet.signal) {
                    found.push((
                        Hit {
                            layer,
                            position,
                            facet: index,
                        },
                        rank,
                    ));
                }
            }
        }
        let best = found.iter().map(|(_, rank)| *rank).max().unwrap_or(0);
        found
            .into_iter()
            .filter(|(_, rank)| *rank == best)
            .map(|(hit, _)| hit)
            .collect()
    }

    /// The shown layer, then the others in keymap order.
    fn layer_order(&self) -> Vec<LayerId> {
        std::iter::once(self.shown)
            .chain(
                self.project
                    .layers
                    .iter()
                    .map(|l| l.id)
                    .filter(|id| *id != self.shown),
            )
            .collect()
    }

    /// Finds what sends a signal: on the shown layer, or else on the first
    /// layer that has it.
    fn find(&self, want: impl Fn(&Signal) -> Option<usize>) -> Option<(LayerId, Vec<Hit>)> {
        self.layer_order().into_iter().find_map(|layer| {
            let hits = self.hits_on(layer, &want);
            (!hits.is_empty()).then_some((layer, hits))
        })
    }

    fn key_wanted(usage: Usage, held: Mods) -> impl Fn(&Signal) -> Option<usize> {
        move |signal| match signal {
            Signal::Key { usage: u, mods } if *u == usage && mods.within(held) => {
                Some(mods.count())
            }
            _ => None,
        }
    }

    /// Moves to `layer` as the keyboard must have, marking what leads
    /// there as held: a key of the layer shown before, else of the first
    /// layer, else of any, else a combo.
    fn switch(&mut self, layer: LayerId) {
        let from = self.shown;
        self.shown = layer;
        self.release_activators();
        self.moving.clear();
        let leads =
            |signal: &Signal| matches!(signal, Signal::Layer(id) if *id == layer).then_some(0);
        let base = self.project.layers.first().map(|l| l.id);
        let mut candidates: Vec<LayerId> = vec![from];
        candidates.extend(base.filter(|b| *b != from));
        candidates.extend(
            self.project
                .layers
                .iter()
                .map(|l| l.id)
                .filter(|id| *id != from && Some(*id) != base),
        );
        for candidate in candidates {
            let hits = self.hits_on(candidate, leads);
            if !hits.is_empty() {
                for hit in hits {
                    self.pressed.insert(hit);
                    self.seen.insert(hit);
                    self.activators.push((hit.position, hit));
                }
                return;
            }
        }
        if let Some((id, _)) = self
            .combo_facets
            .iter()
            .find(|(_, facets)| facets.iter().any(|f| leads(&f.signal).is_some()))
        {
            self.activator_combo = Some(*id);
            self.combos_seen.insert(*id);
        }
    }

    fn release_activators(&mut self) {
        for (_, hit) in self.activators.drain(..) {
            self.pressed.remove(&hit);
        }
        self.activator_combo = None;
    }

    fn light(&mut self, name: &str, hits: &[Hit]) {
        for hit in hits {
            self.pressed.insert(*hit);
            self.seen.insert(*hit);
        }
        self.held.insert(name.to_string(), hits.to_vec());
    }

    /// The macro, if any, whose keys have all just arrived, ending with the
    /// latest: the longest such, from the shown layer first.
    fn completed_sequence(&self) -> Option<Hit> {
        let mut best: Option<(Hit, usize)> = None;
        for layer in self.layer_order() {
            let Some(keys) = self.facets.get(&layer) else {
                continue;
            };
            for (position, facets) in keys.iter().enumerate() {
                for (index, facet) in facets.iter().enumerate() {
                    let Signal::Sequence(sequence) = &facet.signal else {
                        continue;
                    };
                    if best.is_some_and(|(_, len)| len >= sequence.len()) {
                        continue;
                    }
                    if self.recent_ends_with(sequence) {
                        best = Some((
                            Hit {
                                layer,
                                position,
                                facet: index,
                            },
                            sequence.len(),
                        ));
                    }
                }
            }
        }
        best.map(|(hit, _)| hit)
    }

    fn recent_ends_with(&self, sequence: &[Usage]) -> bool {
        if sequence.len() < 2 || self.recent.len() < sequence.len() {
            return false;
        }
        let tail: Vec<&(Usage, Instant)> = self
            .recent
            .iter()
            .rev()
            .take(sequence.len())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        tail.iter()
            .zip(sequence)
            .all(|((usage, _), want)| usage == want)
            && tail
                .windows(2)
                .all(|pair| pair[1].1.duration_since(pair[0].1) <= MACRO_GAP)
    }

    /// Combos that send `usage` with the modifiers held, on the shown layer.
    fn combos_sending(&self, usage: Usage, held: Mods) -> Vec<ComboId> {
        let wanted = Self::key_wanted(usage, held);
        self.combo_facets
            .iter()
            .filter(|(id, facets)| {
                let on_layer = self
                    .project
                    .combos
                    .iter()
                    .find(|c| c.id == *id)
                    .is_some_and(|c| c.layers.is_empty() || c.layers.contains(&self.shown));
                on_layer && facets.iter().any(|f| wanted(&f.signal).is_some())
            })
            .map(|(id, _)| *id)
            .collect()
    }

    fn press_combo(&mut self, name: &str, combo: ComboId) -> Outcome {
        self.combos_seen.insert(combo);
        self.combo_pressed = Some(combo);
        self.held_combos.insert(name.to_string(), combo);
        Outcome::Combo { combo }
    }

    /// A key came down on the computer's side: `name` is what the toolkit
    /// calls it, for letting it go later; `key` its keycode name.
    pub fn key_down(&mut self, name: &str, key: &str, mods: Mods, now: Instant) -> Outcome {
        if self.held.contains_key(name) || self.held_combos.contains_key(name) {
            return Outcome::Nothing;
        }
        let Some(usage) = usage(key) else {
            return Outcome::Unmatched;
        };
        self.recent.push_back((usage, now));
        while self.recent.len() > RECENT {
            self.recent.pop_front();
        }
        if self.mode == Mode::CombosAndMacros {
            if let Some(hit) = self.completed_sequence() {
                self.light(name, &[hit]);
                return Outcome::Macro { hit };
            }
            if let Some(combo) = self.combos_sending(usage, mods).first().copied() {
                return self.press_combo(name, combo);
            }
        }
        let Some((layer, hits)) = self.find(Self::key_wanted(usage, mods)) else {
            if let Some(combo) = self.combos_sending(usage, mods).first().copied() {
                return self.press_combo(name, combo);
            }
            return Outcome::Unmatched;
        };
        let switched = layer != self.shown;
        if switched {
            self.switch(layer);
        }
        self.take_back_implicit_modifiers(&hits, now);
        self.light(name, &hits);
        let completed = self.completed_sequence();
        if let Some(hit) = completed {
            self.seen.insert(hit);
        }
        Outcome::Keys {
            layer,
            hits,
            switched,
            completed,
        }
    }

    /// A modifier that came down just before a key that needs it was built
    /// into that key (a shifted symbol, a Cmd shortcut), not held on a key
    /// of its own: its marks are taken back.
    fn take_back_implicit_modifiers(&mut self, hits: &[Hit], now: Instant) {
        let mut needed = Mods::NONE;
        for hit in hits {
            if let Some(Facet {
                signal: Signal::Key { mods, .. },
                ..
            }) = self.facet(*hit)
            {
                needed = needed.union(*mods);
            }
        }
        for class in ModClass::ALL {
            if !needed.has(class) {
                continue;
            }
            let Some(press) = self.modifier_presses.get(&class) else {
                continue;
            };
            if now.duration_since(press.at) > IMPLICIT_MODIFIER {
                continue;
            }
            let press = self.modifier_presses.remove(&class).unwrap();
            for (hit, was_seen) in press.hits {
                self.pressed.remove(&hit);
                if !was_seen {
                    self.seen.remove(&hit);
                }
            }
            self.held.remove(class.name());
        }
    }

    /// A key on the computer's side was let go.
    pub fn key_up(&mut self, name: &str) {
        if let Some(hits) = self.held.remove(name) {
            let still: Vec<Hit> = self.held.values().flatten().copied().collect();
            for hit in hits {
                if !still.contains(&hit) && !self.activators.iter().any(|(_, a)| *a == hit) {
                    self.pressed.remove(&hit);
                }
            }
        }
        if let Some(combo) = self.held_combos.remove(name) {
            if self.combo_pressed == Some(combo) && !self.held_combos.values().any(|c| *c == combo)
            {
                self.combo_pressed = None;
            }
        }
    }

    /// The modifiers the computer reports changed.
    pub fn modifiers(&mut self, mods: Mods, now: Instant) -> Vec<Outcome> {
        let before = self.mods;
        self.mods = mods;
        let mut outcomes = Vec::new();
        for class in ModClass::ALL {
            if mods.has(class) && !before.has(class) {
                outcomes.push(self.press_modifier(class, now));
            } else if !mods.has(class) && before.has(class) {
                self.modifier_presses.remove(&class);
                self.key_up(class.name());
            }
        }
        outcomes
    }

    fn press_modifier(&mut self, class: ModClass, now: Instant) -> Outcome {
        let name = class.name();
        let wanted =
            |signal: &Signal| matches!(signal, Signal::Modifier(c) if *c == class).then_some(0);
        if self.mode == Mode::CombosAndMacros {
            let combo = self
                .combo_facets
                .iter()
                .find(|(_, facets)| facets.iter().any(|f| wanted(&f.signal).is_some()))
                .map(|(id, _)| *id);
            if let Some(combo) = combo {
                return self.press_combo(name, combo);
            }
        }
        let Some((layer, hits)) = self.find(wanted) else {
            return Outcome::Unmatched;
        };
        let switched = layer != self.shown;
        if switched {
            self.switch(layer);
        }
        let was_seen: Vec<(Hit, bool)> = hits.iter().map(|h| (*h, self.seen.contains(h))).collect();
        self.light(name, &hits);
        self.modifier_presses.insert(
            class,
            ModifierPress {
                at: now,
                hits: was_seen,
            },
        );
        Outcome::Keys {
            layer,
            hits,
            switched,
            completed: None,
        }
    }

    fn mouse(&mut self, want: impl Fn(&Signal) -> Option<usize>) -> Outcome {
        let hits = self.hits_on(self.shown, want);
        if hits.is_empty() {
            self.moving.clear();
            return Outcome::Unmatched;
        }
        for hit in &hits {
            self.seen.insert(*hit);
        }
        self.moving = hits.clone();
        Outcome::Keys {
            layer: self.shown,
            hits,
            switched: false,
            completed: None,
        }
    }

    /// The pointer moved, mostly in `direction`. Mouse functions are only
    /// matched on the shown layer: the computer's own mouse moves too.
    pub fn pointer(&mut self, direction: Direction) -> Outcome {
        self.mouse(|signal| matches!(signal, Signal::Pointer(d) if *d == direction).then_some(0))
    }

    pub fn scroll(&mut self, direction: Direction) -> Outcome {
        self.mouse(|signal| matches!(signal, Signal::Scroll(d) if *d == direction).then_some(0))
    }

    /// Movement stopped.
    pub fn pointer_stop(&mut self) {
        self.moving.clear();
    }

    /// A mouse button, numbered from 1, went down or up.
    pub fn button(&mut self, button: u8, down: bool) -> Outcome {
        let name = format!("button{button}");
        if !down {
            self.key_up(&name);
            return Outcome::Nothing;
        }
        if self.held.contains_key(&name) {
            return Outcome::Nothing;
        }
        let hits = self.hits_on(self.shown, |signal| {
            matches!(signal, Signal::Button(b) if *b == button).then_some(0)
        });
        if hits.is_empty() {
            return Outcome::Unmatched;
        }
        self.light(&name, &hits);
        Outcome::Keys {
            layer: self.shown,
            hits,
            switched: false,
            completed: None,
        }
    }

    /// Lets go of everything, as when the window loses the keyboard.
    pub fn release_all(&mut self) {
        self.pressed.clear();
        self.held.clear();
        self.held_combos.clear();
        self.modifier_presses.clear();
        self.activators.clear();
        self.activator_combo = None;
        self.combo_pressed = None;
        self.moving.clear();
        self.mods = Mods::NONE;
    }

    /// Forgets what has been seen, to start a test over.
    pub fn reset(&mut self) {
        self.release_all();
        self.seen.clear();
        self.combos_seen.clear();
        self.recent.clear();
    }

    fn is_pressed(&self, hit: &Hit) -> bool {
        self.pressed.contains(hit) || self.moving.contains(hit)
    }

    pub fn is_seen(&self, hit: &Hit) -> bool {
        self.seen.contains(hit)
    }

    /// A key as the shown layer presents it.
    pub fn key_view(&self, position: usize) -> KeyView {
        let facets = self
            .facets(self.shown, position)
            .iter()
            .enumerate()
            .map(|(index, facet)| {
                let hit = Hit {
                    layer: self.shown,
                    position,
                    facet: index,
                };
                let state = if !facet.testable() {
                    FacetState::Untestable
                } else if self.is_pressed(&hit) {
                    FacetState::Pressed
                } else if self.seen.contains(&hit) {
                    FacetState::Seen
                } else {
                    FacetState::Unseen
                };
                FacetView {
                    label: facet.label.clone(),
                    role: facet.role.clone(),
                    state,
                }
            })
            .collect();
        let outlined = self.activators.iter().any(|(p, _)| *p == position)
            || self
                .pressed
                .iter()
                .any(|hit| hit.position == position && hit.layer != self.shown);
        let inherited = self
            .inherited
            .get(&self.shown)
            .and_then(|keys| keys.get(position))
            .copied()
            .unwrap_or(false);
        KeyView {
            facets,
            outlined,
            inherited,
        }
    }

    /// Which keys the shown layer's pressed combo, if any, is made of.
    pub fn combo_keys(&self) -> Option<(ComboId, Vec<usize>)> {
        let id = self.combo_pressed()?;
        let combo = self.project.combos.iter().find(|c| c.id == id)?;
        Some((id, combo.key_positions.clone()))
    }

    pub fn layer_progress(&self, layer: LayerId) -> Progress {
        let Some(keys) = self.facets.get(&layer) else {
            return Progress::default();
        };
        let mut progress = Progress::default();
        for (position, facets) in keys.iter().enumerate() {
            let testable: Vec<Hit> = facets
                .iter()
                .enumerate()
                .filter(|(_, f)| f.testable())
                .map(|(index, _)| Hit {
                    layer,
                    position,
                    facet: index,
                })
                .collect();
            if testable.is_empty() {
                continue;
            }
            progress.keys += 1;
            progress.functions += testable.len();
            let seen = testable.iter().filter(|h| self.seen.contains(h)).count();
            progress.functions_seen += seen;
            if seen == testable.len() {
                progress.keys_seen += 1;
            }
        }
        progress
    }

    pub fn total_progress(&self) -> Progress {
        self.project
            .layers
            .iter()
            .map(|l| self.layer_progress(l.id))
            .fold(Progress::default(), |a, b| Progress {
                keys_seen: a.keys_seen + b.keys_seen,
                keys: a.keys + b.keys,
                functions_seen: a.functions_seen + b.functions_seen,
                functions: a.functions + b.functions,
            })
    }

    /// Combos seen, and combos that can be.
    pub fn combo_progress(&self) -> (usize, usize) {
        let testable: Vec<ComboId> = self
            .combo_facets
            .iter()
            .filter(|(_, facets)| facets.iter().any(|f| f.testable()))
            .map(|(id, _)| *id)
            .collect();
        (
            testable
                .iter()
                .filter(|id| self.combos_seen.contains(id))
                .count(),
            testable.len(),
        )
    }

    /// Macros seen, and macros there are, counted by what they type: the
    /// same macro on several keys or layers is one macro.
    pub fn macro_progress(&self) -> (usize, usize) {
        let mut all: HashSet<&Vec<Usage>> = HashSet::new();
        let mut seen: HashSet<&Vec<Usage>> = HashSet::new();
        for (layer, keys) in &self.facets {
            for (position, facets) in keys.iter().enumerate() {
                for (index, facet) in facets.iter().enumerate() {
                    let Signal::Sequence(sequence) = &facet.signal else {
                        continue;
                    };
                    all.insert(sequence);
                    let hit = Hit {
                        layer: *layer,
                        position,
                        facet: index,
                    };
                    if self.seen.contains(&hit) {
                        seen.insert(sequence);
                    }
                }
            }
        }
        (seen.len(), all.len())
    }

    /// Keys on `layer` that send nothing to the computer at all.
    pub fn untestable_keys(&self, layer: LayerId) -> usize {
        self.facets.get(&layer).map_or(0, |keys| {
            keys.iter()
                .filter(|facets| !facets.is_empty() && !facets.iter().any(|f| f.testable()))
                .count()
        })
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
