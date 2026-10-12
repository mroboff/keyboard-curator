//! The key tester, on a board's page: press keys on the keyboard to see
//! them light up, layer by layer and function by function, and hear each
//! one as a note.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_boards::board::PointingKind;
use kc_boards::geometry::Key;
use kc_boards::Board;
use kc_model::keycap::{Keycap, KeycapKind};
use kc_model::tester::{grid, Direction, FacetState, Hit, ModClass, Mode, Mods, Outcome, Tester};
use kc_model::{LayerId, Project};
use kc_sound::{frequency, note_name, Player, Scale, Status, Waveform};

use crate::canvas::{self, Band, Device, Frame, Palette};
use crate::workspace::chip;

/// How often, and how many times, to look at whether the sound output has
/// opened before saying that it is not answering.
const SOUND_POLL: Duration = Duration::from_millis(100);
const SOUND_PATIENCE: usize = 50;

/// The furthest the notes can be moved, in semitones either way.
const TRANSPOSE_LIMIT: i32 = 24;

/// How long mouse functions stay lit after the pointer last moved.
const MOTION_HOLD: Duration = Duration::from_millis(160);
/// Pointer movement smaller than this, in pixels, is jitter.
const MOTION_LEAST: f32 = 2.;

pub struct KeyTester {
    board: Board,
    /// Which layout presses are matched against, for the screen to say.
    source: String,
    /// True while the tester is on screen and listening.
    active: bool,
    /// What has been seen, what is down, and which layer is shown.
    tester: Tester,
    /// The key sounding for each thing held on the computer's side, by
    /// the name the toolkit gives it.
    sounding: HashMap<String, usize>,
    /// The key being played with the mouse.
    mouse: Option<usize>,
    /// What the tester last has to say: what a press was taken to be, or
    /// that it could not be matched.
    message: Option<String>,
    /// The pointer moved or scrolled lately, so the drawn pointing devices
    /// are lit.
    moving: bool,
    /// Counts movements, so that the timer that ends one does not end a
    /// later one.
    motion: u64,
    last_pointer: Option<Point<Pixels>>,
    /// The sound output, open while the tester is on screen.
    sound: Option<Player>,
    /// The sound output has been asked for and has not answered in time.
    sound_slow: bool,
    sound_on: bool,
    waveform: Waveform,
    scale: Scale,
    /// Volume, as a percentage.
    volume: u8,
    /// How far the notes are moved, in semitones.
    transpose: i32,
    focus: FocusHandle,
    /// Asked to take the keyboard the next time it is drawn.
    wants_focus: bool,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

fn section_title(text: &'static str, cx: &App) -> Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

/// The modifiers as the tester understands them.
fn mods_of(modifiers: Modifiers) -> Mods {
    Mods {
        shift: modifiers.shift,
        control: modifiers.control,
        alt: modifiers.alt,
        gui: modifiers.platform,
    }
}

impl KeyTester {
    pub fn new(board: Board, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // A key let go while another app is in front is never reported, so
        // the tester lets go of everything when the window stops being used.
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.release_all();
                cx.notify();
            }
        })
        .detach();
        Self {
            tester: Tester::new(Project::new(String::new(), &board)),
            board,
            source: String::new(),
            active: false,
            sounding: HashMap::new(),
            mouse: None,
            message: None,
            moving: false,
            motion: 0,
            last_pointer: None,
            sound: None,
            sound_slow: false,
            sound_on: true,
            waveform: Waveform::Sine,
            scale: Scale::Major,
            volume: 60,
            transpose: 0,
            focus: cx.focus_handle(),
            wants_focus: false,
            bounds: Rc::new(Cell::new(Bounds::default())),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Starts testing against a layout: called each time the tester comes
    /// on screen, so that it follows the board's current layout. The same
    /// layout keeps its marks.
    pub fn enter(&mut self, project: Project, source: String, cx: &mut Context<Self>) {
        self.tester.set_project(project);
        self.source = source;
        self.active = true;
        self.wants_focus = true;
        self.message = None;
        // Opened on the way in, so the first key already sounds.
        self.open_sound(cx);
        cx.notify();
    }

    /// Stops listening and closes the sound output, as when another part
    /// of the page or the layout editor is shown.
    pub fn leave(&mut self, cx: &mut Context<Self>) {
        self.release_all();
        self.sound = None;
        self.active = false;
        cx.notify();
    }

    fn project(&self) -> &Project {
        self.tester.project()
    }

    fn keys(&self) -> &[Key] {
        self.board
            .layout(&self.project().layout)
            .map_or(&[], |l| l.keys.as_slice())
    }

    fn devices(&self) -> Vec<Device> {
        self.board
            .pointing
            .iter()
            .map(|d| Device {
                name: d.name.clone(),
                round: d.kind == PointingKind::Trackball,
                x: d.x as f32,
                y: d.y as f32,
                size: d.size as f32,
                active: self.moving,
            })
            .collect()
    }

    fn layer_name(&self, layer: LayerId) -> String {
        self.project()
            .layer(layer)
            .map_or_else(|| "?".to_string(), |l| l.name.clone())
    }

    /// The note a key plays, from where it sits on the board.
    fn note(&self, position: usize) -> Option<i32> {
        let (column, row) = *grid(self.keys()).get(position)?;
        Some(self.scale.note(column, row, self.transpose))
    }

    /// Opens the sound output, if sound is wanted and it is not open yet.
    /// Opening happens in the background; this watches for how it went, so
    /// the screen can say.
    fn open_sound(&mut self, cx: &mut Context<Self>) {
        if !self.sound_on || self.sound.is_some() {
            return;
        }
        let player = Player::new();
        player.set_waveform(self.waveform);
        player.set_volume(f32::from(self.volume) / 100.0);
        self.sound = Some(player);
        self.sound_slow = false;
        cx.spawn(async move |this, cx| {
            for _ in 0..SOUND_PATIENCE {
                cx.background_executor().timer(SOUND_POLL).await;
                let starting = this.update(cx, |this, cx| {
                    let starting = this
                        .sound
                        .as_ref()
                        .is_some_and(|player| player.status() == Status::Starting);
                    if !starting {
                        cx.notify();
                    }
                    starting
                });
                if !matches!(starting, Ok(true)) {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.sound_slow = true;
                cx.notify();
            });
        })
        .detach();
    }

    /// Starts a key's note.
    fn sound_on(&mut self, position: usize) {
        if !self.sound_on {
            return;
        }
        let Some(note) = self.note(position) else {
            return;
        };
        if let Some(player) = &self.sound {
            player.note_on(position as u32, frequency(note));
        }
    }

    fn sound_off(&self, position: usize) {
        if let Some(player) = &self.sound {
            player.note_off(position as u32);
        }
    }

    /// What there is to say about the sound, when it is not simply working.
    fn sound_note(&self) -> Option<String> {
        if !self.sound_on {
            return None;
        }
        match self.sound.as_ref()?.status() {
            Status::Playing => None,
            Status::Failed(error) => Some(format!("No sound: {error}. Keys still light up.")),
            Status::Starting if self.sound_slow => Some(
                "No sound: the computer's sound output is not answering. Keys still light up."
                    .to_string(),
            ),
            Status::Starting => Some("Starting sound…".to_string()),
        }
    }

    /// Lets go of everything.
    fn release_all(&mut self) {
        self.tester.release_all();
        self.sounding.clear();
        self.mouse = None;
        self.moving = false;
        if let Some(player) = &self.sound {
            player.all_off();
        }
    }

    /// How a lit function reads in a message: `A (tap) on Base`.
    fn describe(&self, hit: Hit) -> String {
        let facet = self.tester.facets(hit.layer, hit.position).get(hit.facet);
        let mut text = facet.map_or_else(|| "?".to_string(), |f| f.label.clone());
        if let Some(role) = facet.filter(|f| !f.role.is_empty()).map(|f| &f.role) {
            text.push_str(&format!(" ({role})"));
        }
        text
    }

    /// Says what a press was taken to be, and sounds it. `name` is what
    /// the toolkit calls the thing held, so its note can stop later;
    /// `shown` is how it reads in a message.
    fn report(&mut self, name: &str, shown: &str, outcome: Outcome, cx: &mut Context<Self>) {
        match outcome {
            Outcome::Keys {
                layer,
                hits,
                switched,
                completed,
            } => {
                let mut text = match hits.as_slice() {
                    [] => String::new(),
                    [one] => self.describe(*one),
                    [first, rest @ ..] => {
                        format!("{} and {} more", self.describe(*first), rest.len())
                    }
                };
                if switched {
                    text = format!("Moved to {}: {text}", self.layer_name(layer));
                }
                if let Some(hit) = completed {
                    text.push_str(&format!(". Macro {} completed", self.describe(hit)));
                }
                self.message = Some(format!("{text}."));
                if let Some(first) = hits.first() {
                    self.sounding.insert(name.to_string(), first.position);
                    self.sound_on(first.position);
                }
            }
            Outcome::Combo { combo } => {
                let (text, position) = self
                    .project()
                    .combos
                    .iter()
                    .find(|c| c.id == combo)
                    .map(|c| (c.name.clone(), c.key_positions.first().copied()))
                    .unwrap_or_default();
                self.message = Some(format!("Combo {text}."));
                if let Some(position) = position {
                    self.sounding.insert(name.to_string(), position);
                    self.sound_on(position);
                }
            }
            Outcome::Macro { hit } => {
                self.message = Some(format!("Macro {}.", self.describe(hit)));
                self.sounding.insert(name.to_string(), hit.position);
                self.sound_on(hit.position);
            }
            Outcome::Unmatched => {
                self.message = Some(format!(
                    "The computer saw {shown}, which nothing in this layout sends."
                ));
            }
            Outcome::Nothing => return,
        }
        cx.notify();
    }

    fn let_go(&mut self, name: &str, cx: &mut Context<Self>) {
        self.tester.key_up(name);
        if let Some(position) = self.sounding.remove(name) {
            self.sound_off(position);
        }
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let stroke = &event.keystroke;
        // Quitting and closing keep working; every other press is tested.
        if stroke.modifiers.platform && matches!(stroke.key.as_str(), "q" | "w") {
            return;
        }
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        let key = stroke.key.clone();
        let Some(name) = kc_zmk::keycodes::from_typed(&key) else {
            self.message = Some(format!(
                "The computer saw “{key}”, which this tester does not know how to match."
            ));
            cx.notify();
            return;
        };
        let outcome = self
            .tester
            .key_down(&key, name, mods_of(stroke.modifiers), Instant::now());
        self.report(&key, &format!("“{key}”"), outcome, cx);
    }

    fn key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.clone();
        self.let_go(&key, cx);
    }

    fn modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.tester.mods();
        let now = mods_of(event.modifiers);
        // macOS does not report keys let go while Command is held, so when
        // Command comes up, everything is let go.
        if before.gui && !now.gui {
            self.release_all();
        }
        let newly: Vec<ModClass> = ModClass::ALL
            .into_iter()
            .filter(|c| now.has(*c) && !self.tester.mods().has(*c))
            .collect();
        let gone: Vec<ModClass> = ModClass::ALL
            .into_iter()
            .filter(|c| !now.has(*c) && self.tester.mods().has(*c))
            .collect();
        let outcomes = self.tester.modifiers(now, Instant::now());
        for (class, outcome) in newly.into_iter().zip(outcomes) {
            self.report(class.name(), class.name(), outcome, cx);
        }
        for class in gone {
            if let Some(position) = self.sounding.remove(class.name()) {
                self.sound_off(position);
            }
        }
        cx.notify();
    }

    /// Something moved: lights what it matched for a moment.
    fn motion(&mut self, act: impl FnOnce(&mut Tester) -> Outcome, cx: &mut Context<Self>) {
        let outcome = act(&mut self.tester);
        self.moving = true;
        if let Outcome::Keys { hits, .. } = &outcome {
            if let Some(first) = hits.first() {
                self.message = Some(format!("{}.", self.describe(*first)));
            }
        }
        self.motion += 1;
        let motion = self.motion;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(MOTION_HOLD).await;
            let _ = this.update(cx, |this, cx| {
                if this.motion == motion {
                    this.moving = false;
                    this.tester.pointer_stop();
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn pointer_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(last) = self.last_pointer.replace(position) else {
            return;
        };
        let (dx, dy) = (
            f32::from(position.x - last.x),
            f32::from(position.y - last.y),
        );
        if dx.abs() < MOTION_LEAST && dy.abs() < MOTION_LEAST {
            return;
        }
        let direction = match (dx.abs() >= dy.abs(), dx > 0., dy > 0.) {
            (true, true, _) => Direction::Right,
            (true, false, _) => Direction::Left,
            (false, _, true) => Direction::Down,
            (false, _, false) => Direction::Up,
        };
        self.motion(|tester| tester.pointer(direction), cx);
    }

    fn scrolled(&mut self, delta: ScrollDelta, cx: &mut Context<Self>) {
        let (dx, dy) = match delta {
            ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
            ScrollDelta::Lines(p) => (p.x, p.y),
        };
        if dx == 0. && dy == 0. {
            return;
        }
        // Reported as the content moves: a positive y is the page coming
        // down, which is scrolling up.
        let direction = match (dx.abs() >= dy.abs(), dx > 0., dy > 0.) {
            (true, true, _) => Direction::Left,
            (true, false, _) => Direction::Right,
            (false, _, true) => Direction::Up,
            (false, _, false) => Direction::Down,
        };
        self.motion(|tester| tester.scroll(direction), cx);
    }

    fn button(&mut self, button: u8, down: bool, cx: &mut Context<Self>) {
        let outcome = self.tester.button(button, down);
        if down {
            let shown = match button {
                1 => "a left click",
                2 => "a right click",
                _ => "a middle click",
            };
            self.report(&format!("button{button}"), shown, outcome, cx);
        } else if let Some(position) = self.sounding.remove(&format!("button{button}")) {
            self.sound_off(position);
        }
        cx.notify();
    }

    /// Clicking a key plays its note, so the board can be tried as an
    /// instrument with the mouse too. It does not count as testing the key.
    /// A click beside the keys is a left mouse button, as a mouse key of
    /// the keyboard would send.
    fn mouse_down(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        let key = canvas::key_at(self.keys(), &self.devices(), self.bounds.get(), position);
        match key {
            Some(position) => {
                self.mouse = Some(position);
                self.sound_on(position);
                cx.notify();
            }
            None => self.button(1, true, cx),
        }
    }

    fn mouse_up(&mut self, cx: &mut Context<Self>) {
        match self.mouse.take() {
            Some(position) => {
                self.sound_off(position);
                cx.notify();
            }
            None => self.button(1, false, cx),
        }
    }

    fn set_waveform(&mut self, waveform: Waveform, cx: &mut Context<Self>) {
        self.waveform = waveform;
        if let Some(player) = &self.sound {
            player.set_waveform(waveform);
        }
        cx.notify();
    }

    fn set_volume(&mut self, volume: u8, cx: &mut Context<Self>) {
        self.volume = volume.min(100);
        if let Some(player) = &self.sound {
            player.set_volume(f32::from(self.volume) / 100.0);
        }
        cx.notify();
    }

    fn toggle_sound(&mut self, cx: &mut Context<Self>) {
        self.sound_on = !self.sound_on;
        if !self.sound_on {
            if let Some(player) = &self.sound {
                player.all_off();
            }
        }
        self.open_sound(cx);
        self.message = None;
        cx.notify();
    }

    fn show_layer(&mut self, layer: LayerId, cx: &mut Context<Self>) {
        self.tester.show(layer);
        self.message = Some(format!("Showing {}.", self.layer_name(layer)));
        cx.notify();
    }

    fn toggle_mode(&mut self, cx: &mut Context<Self>) {
        let mode = match self.tester.mode() {
            Mode::Keys => Mode::CombosAndMacros,
            Mode::CombosAndMacros => Mode::Keys,
        };
        self.tester.set_mode(mode);
        self.message = Some(
            match mode {
                Mode::Keys => {
                    "Keys first: a combo or macro is matched only when no key sends what arrived."
                }
                Mode::CombosAndMacros => {
                    "Combos and macros first: what they send is matched to them before plain keys."
                }
            }
            .to_string(),
        );
        cx.notify();
    }

    /// The line that says how far the test has come.
    fn progress_text(&self) -> String {
        let shown = self.tester.shown();
        let layer = self.tester.layer_progress(shown);
        let all = self.tester.total_progress();
        let mut text = format!(
            "{}: {} of {} keys, {} of {} functions seen. All layers: {} of {} functions.",
            self.layer_name(shown),
            layer.keys_seen,
            layer.keys,
            layer.functions_seen,
            layer.functions,
            all.functions_seen,
            all.functions
        );
        let (combos_seen, combos) = self.tester.combo_progress();
        if combos > 0 {
            text.push_str(&format!(" Combos: {combos_seen} of {combos}."));
        }
        let (macros_seen, macros) = self.tester.macro_progress();
        if macros > 0 {
            text.push_str(&format!(" Macros: {macros_seen} of {macros}."));
        }
        text
    }
}

impl Render for KeyTester {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.wants_focus {
            self.wants_focus = false;
            self.focus.focus(window, cx);
        }
        let theme = cx.theme();
        let (muted, border, accent, success, background, warning) = (
            theme.muted_foreground,
            theme.border,
            theme.primary,
            theme.success,
            theme.background,
            theme.warning,
        );
        let palette = Palette::plain(cx);
        let keys = self.keys().to_vec();
        let shown = self.tester.shown();
        let blank = Keycap {
            legend: String::new(),
            hold: None,
            kind: KeycapKind::None,
            marks: Vec::new(),
        };
        let plain = Keycap {
            kind: KeycapKind::Key,
            ..blank.clone()
        };
        // A transparent key is drawn ghosted, as the editor draws it: its
        // functions are the layer below's.
        let inherited = Keycap {
            kind: KeycapKind::Transparent,
            ..blank.clone()
        };
        // Each key of the shown layer in slices, one per function: down
        // now, seen before, never seen, or not able to be seen.
        let mut keycaps = Vec::with_capacity(keys.len());
        let mut bands = Vec::with_capacity(keys.len());
        let mut outlined = Vec::new();
        for position in 0..keys.len() {
            let view = self.tester.key_view(position);
            keycaps.push(if view.facets.is_empty() {
                blank.clone()
            } else if view.inherited {
                inherited.clone()
            } else {
                plain.clone()
            });
            if view.outlined {
                outlined.push(position);
            }
            let played = self.mouse == Some(position);
            bands.push(
                view.facets
                    .into_iter()
                    .map(|facet| Band {
                        label: facet.label,
                        fill: match facet.state {
                            _ if played => Some(accent),
                            FacetState::Pressed => Some(accent),
                            FacetState::Seen => Some(success.opacity(0.55)),
                            FacetState::Untestable => Some(background),
                            FacetState::Unseen => None,
                        },
                    })
                    .collect(),
            );
        }
        let links = self
            .tester
            .combo_keys()
            .map(|(_, positions)| vec![(positions, true)])
            .unwrap_or_default();
        let frame = Frame {
            keycaps,
            keys,
            devices: self.devices(),
            selected: outlined,
            hovered: None,
            drag: None,
            band: None,
            palette,
            tint: None,
            links,
            colors: Vec::new(),
            bands,
        };
        let bounds = self.bounds.clone();

        let untestable = self.tester.untestable_keys(shown);
        let progress = self.progress_text();
        // The range of notes, which is what choosing a scale changes.
        let range = {
            let notes: Vec<i32> = (0..self.keys().len())
                .filter_map(|p| self.note(p))
                .collect();
            match (notes.iter().min(), notes.iter().max()) {
                (Some(low), Some(high)) => format!("{} to {}", note_name(*low), note_name(*high)),
                _ => String::new(),
            }
        };

        let layers = self
            .project()
            .layers
            .iter()
            .enumerate()
            .map(|(index, layer)| {
                let progress = self.tester.layer_progress(layer.id);
                let label = format!("{} {}/{}", layer.name, progress.keys_seen, progress.keys);
                let id = layer.id;
                chip(("layer", index), label, shown == id, cx).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.show_layer(id, cx);
                        this.focus.focus(window, cx);
                    },
                ))
            })
            .collect::<Vec<_>>();
        let mods = self.tester.mods();
        let modifiers = ModClass::ALL
            .into_iter()
            .enumerate()
            .map(|(index, class)| {
                chip(
                    ("modifier", index),
                    format!("{} {}", class.symbol(), class.name()),
                    mods.has(class),
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let mode = chip(
            "tester-mode",
            "Combos & Macros first",
            self.tester.mode() == Mode::CombosAndMacros,
            cx,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.toggle_mode(cx);
            this.focus.focus(window, cx);
        }));

        let waves = Waveform::ALL
            .into_iter()
            .enumerate()
            .map(|(index, waveform)| {
                chip(
                    ("wave", index),
                    waveform.name(),
                    self.waveform == waveform,
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.set_waveform(waveform, cx);
                    this.focus.focus(window, cx);
                }))
            })
            .collect::<Vec<_>>();
        let scales = Scale::ALL
            .into_iter()
            .enumerate()
            .map(|(index, scale)| {
                chip(("scale", index), scale.name(), self.scale == scale, cx).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.scale = scale;
                        this.focus.focus(window, cx);
                        cx.notify();
                    },
                ))
            })
            .collect::<Vec<_>>();
        let stepper = |id: &'static str,
                       label: &'static str,
                       change: fn(&mut Self, &mut Context<Self>),
                       cx: &mut Context<Self>| {
            chip(id, label, false, cx).on_click(cx.listener(move |this, _, window, cx| {
                change(this, cx);
                this.focus.focus(window, cx);
            }))
        };
        let transpose = match self.transpose {
            0 => "0".to_string(),
            n => format!("{n:+}"),
        };
        let group = |title: &'static str, cx: &mut Context<Self>| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(section_title(title, cx))
        };
        let help = format!(
            "{} Press keys on the keyboard, or click them here to play. The computer only reports what a key sends, never which key or layer, so the tester shows the layer the keyboard must be on: a press that no key of the shown layer sends moves to the first layer that has one and outlines the keys that lead there. Choose a layer above to test it by hand. A key with more than one function is drawn in slices, one per function. Keep the mouse still while testing mouse keys; a click beside the keys counts as a left click.{} ⌘Q and ⌘W still work; every other press is tested.",
            self.source,
            match untestable {
                0 => String::new(),
                1 => " One key on this layer sends nothing to the computer and cannot be tested here; it is shown hollow.".to_string(),
                n => format!(" {n} keys on this layer, such as Bluetooth and lighting keys, send nothing to the computer and cannot be tested here; they are shown hollow."),
            }
        );

        div()
            .id("tester")
            .key_context("Tester")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed))
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_4()
                    .pt_3()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        group("LAYERS", cx)
                            .child(div().flex().flex_wrap().gap_1().children(layers)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_end()
                            .gap_6()
                            .child(
                                group("HELD", cx).child(div().flex().gap_1().children(modifiers)),
                            )
                            .child(group("MATCH", cx).child(mode)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(canvas::keyboard(frame, move |b| bounds.set(b)))
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        this.pointer_moved(event.position, cx);
                    }))
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                        this.scrolled(event.delta, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            this.mouse_down(event.position, window, cx);
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| this.mouse_up(cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| this.button(2, true, cx)),
                    )
                    .on_mouse_up(
                        MouseButton::Right,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| this.button(2, false, cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| this.button(3, true, cx)),
                    )
                    .on_mouse_up(
                        MouseButton::Middle,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| this.button(3, false, cx)),
                    ),
            )
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(border)
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().text_sm().child(progress))
                            .child(chip("tester-reset", "Start Over", false, cx).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.release_all();
                                    this.tester.reset();
                                    this.message = None;
                                    this.focus.focus(window, cx);
                                    cx.notify();
                                }),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(self.message.clone().unwrap_or_default()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_end()
                            .gap_6()
                            .child(
                                group("SOUND", cx).child(
                                    chip(
                                        "sound-toggle",
                                        if self.sound_on { "On" } else { "Off" },
                                        self.sound_on,
                                        cx,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, window, cx| {
                                            this.toggle_sound(cx);
                                            this.focus.focus(window, cx);
                                        },
                                    )),
                                ),
                            )
                            .child(group("WAVE", cx).child(div().flex().gap_1().children(waves)))
                            .child(
                                group("SCALE", cx)
                                    .child(div().flex().flex_wrap().gap_1().children(scales)),
                            )
                            .child(
                                group("VOLUME", cx).child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child(stepper(
                                            "volume-down",
                                            "−",
                                            |this, cx| {
                                                this.set_volume(this.volume.saturating_sub(10), cx)
                                            },
                                            cx,
                                        ))
                                        .child(
                                            div()
                                                .w_12()
                                                .text_sm()
                                                .child(format!("{}%", self.volume)),
                                        )
                                        .child(stepper(
                                            "volume-up",
                                            "+",
                                            |this, cx| {
                                                this.set_volume(this.volume.saturating_add(10), cx)
                                            },
                                            cx,
                                        )),
                                ),
                            )
                            .child(
                                group("TRANSPOSE", cx).child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child(stepper(
                                            "transpose-down",
                                            "−",
                                            |this, cx| {
                                                this.transpose =
                                                    (this.transpose - 1).max(-TRANSPOSE_LIMIT);
                                                cx.notify();
                                            },
                                            cx,
                                        ))
                                        .child(div().w_10().text_sm().child(transpose))
                                        .child(stepper(
                                            "transpose-up",
                                            "+",
                                            |this, cx| {
                                                this.transpose =
                                                    (this.transpose + 1).min(TRANSPOSE_LIMIT);
                                                cx.notify();
                                            },
                                            cx,
                                        ))
                                        .child(
                                            div().pl_2().text_xs().text_color(muted).child(range),
                                        ),
                                ),
                            ),
                    )
                    .child(div().text_xs().text_color(muted).child(help))
                    .when_some(self.sound_note(), |panel, note| {
                        panel.child(div().text_xs().text_color(warning).child(note))
                    }),
            )
    }
}
