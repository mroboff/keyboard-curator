//! The Key Tester mode: press keys on the keyboard to see them light up,
//! and hear each one as a note.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_model::keycap::{keycap, Keycap, KeycapKind};
use kc_model::tester::{grid, positions_sending, testable};
use kc_sound::{frequency, note_name, Player, Scale, Status, Waveform};

use super::{chip, section_title, Workspace};
use crate::canvas::{self, Frame, Palette};

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
const SOUND_POLL: std::time::Duration = std::time::Duration::from_millis(100);
const SOUND_PATIENCE: usize = 50;

/// The furthest the notes can be moved, in semitones either way.
const TRANSPOSE_LIMIT: i32 = 24;

impl Workspace {
    /// The note a key plays, from where it sits on the board.
    fn tester_note(&self, position: usize) -> Option<i32> {
        let (column, row) = *grid(self.layout_keys()).get(position)?;
        Some(self.scale.note(column, row, self.transpose))
    }

    /// Opens the sound output, if sound is wanted and it is not open yet.
    /// Opening happens in the background; this watches for how it went, so
    /// the screen can say.
    pub(super) fn tester_open_sound(&mut self, cx: &mut Context<Self>) {
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
    fn tester_sound_on(&mut self, position: usize) {
        if !self.sound_on {
            return;
        }
        let Some(note) = self.tester_note(position) else {
            return;
        };
        if let Some(player) = &self.sound {
            player.note_on(position as u32, frequency(note));
        }
    }

    /// What there is to say about the sound, when it is not simply working.
    fn tester_sound_note(&self) -> Option<String> {
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

    fn tester_sound_off(&self, position: usize) {
        if let Some(player) = &self.sound {
            player.note_off(position as u32);
        }
    }

    /// Lets go of everything, as when leaving the tester.
    pub(super) fn tester_release_all(&mut self) {
        self.tester.release_all();
        self.tester_held.clear();
        self.tester_mouse = None;
        if let Some(player) = &self.sound {
            player.all_off();
        }
    }

    /// A key, named as the toolkit names it, went down on the computer's
    /// keyboard: light every key of the layout that can send it.
    fn tester_press(&mut self, key: &str, names: &[&str], cx: &mut Context<Self>) {
        if self.tester_held.contains_key(key) {
            return;
        }
        let mut positions: Vec<usize> = Vec::new();
        for name in names {
            for position in positions_sending(self.project(), name) {
                if !positions.contains(&position) {
                    positions.push(position);
                }
            }
        }
        if positions.is_empty() {
            self.tester_message = Some(format!(
                "The computer saw “{key}”, which no key of this layout sends."
            ));
        } else {
            self.tester_message = None;
            self.tester.press(&positions);
            // One note per press, from the first key that could have sent it.
            self.tester_sound_on(positions[0]);
        }
        self.tester_held.insert(key.to_string(), positions);
        cx.notify();
    }

    fn tester_let_go(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(positions) = self.tester_held.remove(key) else {
            return;
        };
        // A key another held press also lights stays lit.
        let still: Vec<usize> = self.tester_held.values().flatten().copied().collect();
        let released: Vec<usize> = positions
            .iter()
            .copied()
            .filter(|p| !still.contains(p))
            .collect();
        self.tester.release(&released);
        if let Some(first) = positions.first() {
            self.tester_sound_off(*first);
        }
        cx.notify();
    }

    fn tester_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
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
            Some(name) => self.tester_press(&key, &[name], cx),
            None => {
                self.tester_message = Some(format!(
                    "The computer saw “{key}”, which this tester does not know how to match."
                ));
                cx.notify();
            }
        }
    }

    fn tester_key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.clone();
        self.tester_let_go(&key, cx);
    }

    fn tester_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = event.modifiers;
        // macOS does not report keys let go while Command is held, so when
        // Command comes up, anything else still marked as held is let go.
        if !now.platform && self.tester_held.contains_key("platform") {
            let stuck: Vec<String> = self
                .tester_held
                .keys()
                .filter(|key| MODIFIER_KEYS.iter().all(|(name, _)| name != key))
                .cloned()
                .collect();
            for key in stuck {
                self.tester_let_go(&key, cx);
            }
        }
        let down = [now.shift, now.control, now.alt, now.platform];
        for ((key, names), down) in MODIFIER_KEYS.iter().zip(down) {
            if down {
                self.tester_press(key, names, cx);
            } else {
                self.tester_let_go(key, cx);
            }
        }
    }

    /// Clicking a key plays its note, so the board can be tried as an
    /// instrument with the mouse too. It does not count as testing the key.
    fn tester_mouse_down(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.canvas_focus.focus(window, cx);
        let key = canvas::key_at(self.layout_keys(), &[], self.canvas_bounds.get(), position);
        if let Some(position) = key {
            self.tester_mouse = Some(position);
            self.tester_sound_on(position);
            cx.notify();
        }
    }

    fn tester_mouse_up(&mut self, cx: &mut Context<Self>) {
        if let Some(position) = self.tester_mouse.take() {
            self.tester_sound_off(position);
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
        self.tester_open_sound(cx);
        self.tester_message = None;
        cx.notify();
    }

    pub(super) fn render_tester(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
        let keys = self.layout_keys().to_vec();
        let can = testable(self.project());
        let blank = Keycap {
            legend: String::new(),
            hold: None,
            kind: KeycapKind::None,
        };
        // Down now, seen before, never seen, or not able to be seen.
        let colors = (0..keys.len())
            .map(|position| {
                if self.tester.is_pressed(position) || self.tester_mouse == Some(position) {
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
                .map(|p| keycap(self.project(), self.layer, p).unwrap_or(blank.clone()))
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
        let bounds = self.canvas_bounds.clone();

        let (seen, total) = self.tester.progress(&can);
        let untestable = can.iter().filter(|t| !**t).count();
        let progress = if total > 0 && seen == total {
            format!("All {total} keys seen.")
        } else {
            format!("{seen} of {total} keys seen.")
        };
        // The note under the pointer or the last key pressed is not shown:
        // the range is, which is what choosing a scale changes.
        let range = {
            let notes: Vec<i32> = (0..can.len()).filter_map(|p| self.tester_note(p)).collect();
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
                    this.canvas_focus.focus(window, cx);
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
                        this.canvas_focus.focus(window, cx);
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
                this.canvas_focus.focus(window, cx);
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

        div()
            .id("tester")
            .key_context("Tester")
            .track_focus(&self.canvas_focus)
            .on_key_down(cx.listener(Self::tester_key_down))
            .on_key_up(cx.listener(Self::tester_key_up))
            .on_modifiers_changed(cx.listener(Self::tester_modifiers_changed))
            .flex_1()
            .min_h_0()
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
                            this.tester_mouse_down(event.position, window, cx);
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| this.tester_mouse_up(cx)),
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
                                    this.tester_release_all();
                                    this.tester.reset();
                                    this.tester_message = None;
                                    this.canvas_focus.focus(window, cx);
                                    cx.notify();
                                }),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(self.tester_message.clone().unwrap_or_default()),
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
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_sound(cx);
                                        this.canvas_focus.focus(window, cx);
                                    })),
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
                                            |this, cx| this.set_volume(this.volume.saturating_sub(10), cx),
                                            cx,
                                        ))
                                        .child(div().w_12().text_sm().child(format!("{}%", self.volume)))
                                        .child(stepper(
                                            "volume-up",
                                            "+",
                                            |this, cx| this.set_volume(this.volume.saturating_add(10), cx),
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
                                                this.transpose = (this.transpose - 1).max(-TRANSPOSE_LIMIT);
                                                cx.notify();
                                            },
                                            cx,
                                        ))
                                        .child(div().w_10().text_sm().child(transpose))
                                        .child(stepper(
                                            "transpose-up",
                                            "+",
                                            |this, cx| {
                                                this.transpose = (this.transpose + 1).min(TRANSPOSE_LIMIT);
                                                cx.notify();
                                            },
                                            cx,
                                        ))
                                        .child(div().pl_2().text_xs().text_color(muted).child(range)),
                                ),
                            ),
                    )
                    .child(div().text_xs().text_color(muted).child(format!(
                        "Press keys on the keyboard, or click them here to play. The computer only reports what a key sends, so a press lights every key of the layout that can send it, on any layer.{} Hold ⌘ to use shortcuts.",
                        match untestable {
                            0 => String::new(),
                            1 => " One key sends nothing to the computer and cannot be tested here; it is shown hollow.".to_string(),
                            n => format!(" {n} keys, such as layer keys, send nothing to the computer and cannot be tested here; they are shown hollow."),
                        }
                    )))
                    .when_some(self.tester_sound_note(), |panel, note| {
                        panel.child(div().text_xs().text_color(warning).child(note))
                    }),
            )
    }
}
