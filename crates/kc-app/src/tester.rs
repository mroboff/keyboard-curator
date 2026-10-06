//! The key tester, on a board's page: press keys on the keyboard to see
//! them light up, and hear each one as a note.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_boards::geometry::Key;
use kc_boards::Board;
use kc_model::keycap::{keycap, Keycap, KeycapKind};
use kc_model::tester::{grid, positions_sending, testable, Tester};
use kc_model::Project;
use kc_sound::{frequency, note_name, Player, Scale, Status, Waveform};

use crate::canvas::{self, Frame, Palette};
use crate::workspace::chip;

/// The modifiers the computer reports, with the keys that send each. It
/// does not say which side was pressed, so both are matched.
const MODIFIER_KEYS: [(&str, [&str; 2]); 4] = [
    ("shift", ["LSHFT", "RSHFT"]),
    ("control", ["LCTRL", "RCTRL"]),
    ("alt", ["LALT", "RALT"]),
    ("platform", ["LGUI", "RGUI"]),
];

/// How often, and how many times, to look at whether the sound output has
/// opened before saying that it is not answering.
const SOUND_POLL: Duration = Duration::from_millis(100);
const SOUND_PATIENCE: usize = 50;

/// The furthest the notes can be moved, in semitones either way.
const TRANSPOSE_LIMIT: i32 = 24;

pub struct KeyTester {
    board: Board,
    /// The layout presses are matched against: the board's current one.
    project: Project,
    /// Which layout that is, for the screen to say.
    source: String,
    /// True while the tester is on screen and listening.
    active: bool,
    /// Which keys are down and which have been seen.
    tester: Tester,
    /// The keys of the layout each key held on the computer's keyboard
    /// lit, by the name the toolkit gives that key.
    held: HashMap<String, Vec<usize>>,
    /// The key being played with the mouse.
    mouse: Option<usize>,
    /// What the tester last has to say, such as a key it could not match.
    message: Option<String>,
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
            project: Project::new(String::new(), &board),
            board,
            source: String::new(),
            active: false,
            tester: Tester::default(),
            held: HashMap::new(),
            mouse: None,
            message: None,
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
    /// on screen, so that it follows the board's current layout.
    pub fn enter(&mut self, project: Project, source: String, cx: &mut Context<Self>) {
        // Which keys have been seen means nothing against another layout.
        if project != self.project {
            self.tester.reset();
        }
        self.project = project;
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

    fn keys(&self) -> &[Key] {
        self.board
            .layout(&self.project.layout)
            .map_or(&[], |l| l.keys.as_slice())
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
        self.held.clear();
        self.mouse = None;
        if let Some(player) = &self.sound {
            player.all_off();
        }
    }

    /// A key, named as the toolkit names it, went down on the computer's
    /// keyboard: light every key of the layout that can send it.
    fn press(&mut self, key: &str, names: &[&str], cx: &mut Context<Self>) {
        if self.held.contains_key(key) {
            return;
        }
        let mut positions: Vec<usize> = Vec::new();
        for name in names {
            for position in positions_sending(&self.project, name) {
                if !positions.contains(&position) {
                    positions.push(position);
                }
            }
        }
        if positions.is_empty() {
            self.message = Some(format!(
                "The computer saw “{key}”, which no key of this layout sends."
            ));
        } else {
            self.message = None;
            self.tester.press(&positions);
            // One note per press, from the first key that could have sent it.
            self.sound_on(positions[0]);
        }
        self.held.insert(key.to_string(), positions);
        cx.notify();
    }

    fn let_go(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(positions) = self.held.remove(key) else {
            return;
        };
        // A key another held press also lights stays lit.
        let still: Vec<usize> = self.held.values().flatten().copied().collect();
        let released: Vec<usize> = positions
            .iter()
            .copied()
            .filter(|p| !still.contains(p))
            .collect();
        self.tester.release(&released);
        if let Some(first) = positions.first() {
            self.sound_off(*first);
        }
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let stroke = &event.keystroke;
        // With Command held, shortcuts and menus work as usual.
        if stroke.modifiers.platform {
            return;
        }
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        let key = stroke.key.clone();
        match kc_zmk::keycodes::from_typed(&key) {
            Some(name) => self.press(&key, &[name], cx),
            None => {
                self.message = Some(format!(
                    "The computer saw “{key}”, which this tester does not know how to match."
                ));
                cx.notify();
            }
        }
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
        let now = event.modifiers;
        // macOS does not report keys let go while Command is held, so when
        // Command comes up, anything else still marked as held is let go.
        if !now.platform && self.held.contains_key("platform") {
            let stuck: Vec<String> = self
                .held
                .keys()
                .filter(|key| MODIFIER_KEYS.iter().all(|(name, _)| name != key))
                .cloned()
                .collect();
            for key in stuck {
                self.let_go(&key, cx);
            }
        }
        let down = [now.shift, now.control, now.alt, now.platform];
        for ((key, names), down) in MODIFIER_KEYS.iter().zip(down) {
            if down {
                self.press(key, names, cx);
            } else {
                self.let_go(key, cx);
            }
        }
    }

    /// Clicking a key plays its note, so the board can be tried as an
    /// instrument with the mouse too. It does not count as testing the key.
    fn mouse_down(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        let key = canvas::key_at(self.keys(), &[], self.bounds.get(), position);
        if let Some(position) = key {
            self.mouse = Some(position);
            self.sound_on(position);
            cx.notify();
        }
    }

    fn mouse_up(&mut self, cx: &mut Context<Self>) {
        if let Some(position) = self.mouse.take() {
            self.sound_off(position);
            cx.notify();
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
        let palette = Palette {
            key: theme.secondary,
            key_border: theme.border,
            text: theme.foreground,
            muted_text: theme.muted_foreground,
            accent: theme.primary,
            layer_key: theme.secondary,
        };
        let keys = self.keys().to_vec();
        let can = testable(&self.project);
        let blank = Keycap {
            legend: String::new(),
            hold: None,
            kind: KeycapKind::None,
        };
        // The legends are the first layer's, as the keyboard starts out.
        let layer = self.project.layers.first().map(|l| l.id);
        // Down now, seen before, never seen, or not able to be seen.
        let colors = (0..keys.len())
            .map(|position| {
                if self.tester.is_pressed(position) || self.mouse == Some(position) {
                    Some(accent)
                } else if self.tester.is_seen(position) {
                    Some(success.opacity(0.55))
                } else if !can.get(position).copied().unwrap_or(false) {
                    Some(background)
                } else {
                    None
                }
            })
            .collect();
        let frame = Frame {
            keycaps: (0..keys.len())
                .map(|p| {
                    layer
                        .and_then(|layer| keycap(&self.project, layer, p))
                        .unwrap_or(blank.clone())
                })
                .collect(),
            keys,
            devices: Vec::new(),
            selected: Vec::new(),
            hovered: None,
            drag: None,
            band: None,
            palette,
            tint: None,
            links: Vec::new(),
            colors,
        };
        let bounds = self.bounds.clone();

        let (seen, total) = self.tester.progress(&can);
        let untestable = can.iter().filter(|t| !**t).count();
        let progress = if total > 0 && seen == total {
            format!("All {total} keys seen.")
        } else {
            format!("{seen} of {total} keys seen.")
        };
        // The range of notes, which is what choosing a scale changes.
        let range = {
            let notes: Vec<i32> = (0..can.len()).filter_map(|p| self.note(p)).collect();
            match (notes.iter().min(), notes.iter().max()) {
                (Some(low), Some(high)) => format!("{} to {}", note_name(*low), note_name(*high)),
                _ => String::new(),
            }
        };

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
            "{} Press keys on the keyboard, or click them here to play. The computer only reports what a key sends, so a press lights every key of the layout that can send it, on any layer.{} Hold ⌘ to use shortcuts.",
            self.source,
            match untestable {
                0 => String::new(),
                1 => " One key sends nothing to the computer and cannot be tested here; it is shown hollow.".to_string(),
                n => format!(" {n} keys, such as layer keys, send nothing to the computer and cannot be tested here; they are shown hollow."),
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
                    .flex_1()
                    .min_h_0()
                    .child(canvas::keyboard(frame, move |b| bounds.set(b)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            this.mouse_down(event.position, window, cx);
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| this.mouse_up(cx)),
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
