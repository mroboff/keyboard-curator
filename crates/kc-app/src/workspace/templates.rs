//! The Templates mode: the feels and macros the user named for reuse,
//! each shown with a summary in words and where it is used. Key-owned
//! definitions never appear here; they are edited on their key.

use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_model::behavior::{
    BehaviorKind, HoldTap, HoldTapSettings, Macro, MacroStep, ModMorph, ModMorphSettings, Scope,
    StickyKey, StickyKeySettings, TapDance,
};
use kc_model::setup::KeySetup;
use kc_model::{BehaviorId, BehaviorRef, Binding, KeyExpr, Location, ModelError, Slot};

use super::setup::FeelTarget;
use super::{
    badge, chip, display, field, group, heading, help, plinth, section_title, Mode, Workspace,
};
use crate::canvas::{self, Frame, Palette};

/// What choosing from a menu does.
type Pick = Rc<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// The kinds of template that can be made from scratch.
#[derive(Debug, Clone, Copy)]
enum New {
    HoldTap,
    HomeRow,
    TapDance,
    ModMorph,
    StickyKey,
    Macro,
}

const NEW: [(&str, New); 6] = [
    ("Hold & tap", New::HoldTap),
    ("Home-row mod", New::HomeRow),
    ("Multi-tap", New::TapDance),
    ("Shift override", New::ModMorph),
    ("Sticky", New::StickyKey),
    ("Macro", New::Macro),
];

pub(super) fn subscribe(
    name: &Entity<InputState>,
    label: &Entity<InputState>,
    macro_text: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    // Names are committed when focus leaves the field or Return is pressed.
    for input in [name, label] {
        cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.after_change(window, cx);
            }
        })
        .detach();
    }
    cx.subscribe_in(
        macro_text,
        window,
        |this, input, event: &InputEvent, window, cx| {
            if !matches!(event, InputEvent::PressEnter { .. }) {
                return;
            }
            let text = input.read(cx).value().to_string();
            let keys: Vec<Binding> = text
                .chars()
                .filter_map(kc_zmk::keycodes::for_char)
                .filter_map(|expr| expr.parse::<KeyExpr>().ok())
                .map(Binding::kp)
                .collect();
            let Some(id) = this.editing_macro() else {
                return;
            };
            if !keys.is_empty() {
                this.edit_definition(id, "Add Typed Text", window, cx, |kind| {
                    if let BehaviorKind::Macro(m) = kind {
                        m.steps.push(MacroStep::Tap(keys));
                    }
                });
                input.update(cx, |input, cx| input.set_value("", window, cx));
            }
        },
    )
    .detach();
}

impl Workspace {
    /// Changes one definition, by ID.
    pub(super) fn edit_definition(
        &mut self,
        id: BehaviorId,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut BehaviorKind),
    ) {
        self.change(label, window, cx, |p| {
            edit(&mut p.behavior_mut(id)?.kind);
            Ok(())
        });
    }

    /// The macro reached from the selected key's slots, if any.
    pub(super) fn key_macro(&self) -> Option<BehaviorId> {
        let position = self.primary()?;
        let setup = KeySetup::of(self.project(), self.layer, position)?;
        let mut slots: Vec<&Binding> = vec![&setup.tap];
        slots.extend(setup.hold.iter());
        slots.extend(setup.double_tap.iter());
        slots.extend(setup.tap_hold.iter());
        slots.extend(setup.double_tap_hold.iter());
        slots.extend(setup.shifted.iter());
        slots.extend(setup.more_taps.iter());
        let ids: Vec<BehaviorId> = slots
            .into_iter()
            .filter_map(Binding::user_behavior)
            .collect();
        ids.into_iter().find(|id| {
            matches!(self.project().behavior(*id), Some(def) if matches!(def.kind, BehaviorKind::Macro(_)))
        })
    }

    /// The macro the macro editor and the typed-text field work on.
    pub(super) fn editing_macro(&self) -> Option<BehaviorId> {
        match self.mode {
            Mode::Templates => self.behavior.filter(|id| {
                matches!(self.project().behavior(*id), Some(def) if matches!(def.kind, BehaviorKind::Macro(_)))
            }),
            Mode::Keyboard => self.key_macro(),
            _ => None,
        }
    }

    /// The slot inside the selected key's macro the picker fills, if the
    /// picker is aimed at one.
    pub(super) fn key_macro_slot(&self) -> Option<Slot> {
        let id = self.key_macro()?;
        self.slot
            .filter(|slot| matches!(slot, Slot::MacroStep { behavior, .. } if *behavior == id))
    }

    /// Key positions on one hand, judged by the layout's center line.
    pub(super) fn hand_positions(&self, right: bool) -> Vec<usize> {
        kc_boards::geometry::hand_positions(self.layout_keys(), right)
    }

    fn add_template(&mut self, kind: New, window: &mut Window, cx: &mut Context<Self>) {
        let kp = || BehaviorRef::built_in("kp");
        let (name, kind) = match kind {
            New::HoldTap => (
                "Hold & tap",
                BehaviorKind::HoldTap(HoldTap::new(kp(), kp())),
            ),
            New::HomeRow => (
                "Home-row mod",
                BehaviorKind::HoldTap(HoldTap::with_settings(
                    kp(),
                    kp(),
                    &HoldTapSettings::home_row(),
                )),
            ),
            New::TapDance => (
                "Multi-tap",
                BehaviorKind::TapDance(TapDance {
                    tapping_term_ms: 200,
                    bindings: vec![Binding::none(), Binding::none()],
                }),
            ),
            New::ModMorph => (
                "Shift override",
                BehaviorKind::ModMorph(ModMorph {
                    normal: Binding::none(),
                    morphed: Binding::none(),
                    mods: ModMorphSettings::default().mods,
                    keep_mods: vec![],
                }),
            ),
            New::StickyKey => (
                "Sticky",
                BehaviorKind::StickyKey(StickyKey::with_settings(
                    kp(),
                    &StickyKeySettings::default(),
                )),
            ),
            New::Macro => (
                "Macro",
                BehaviorKind::Macro(Macro {
                    wait_ms: None,
                    tap_ms: None,
                    params: 0,
                    steps: vec![],
                }),
            ),
        };
        // Names stay unique by a number, like the labels made from them.
        let taken: Vec<String> = self
            .project()
            .behavior_templates()
            .map(|d| d.name.clone())
            .collect();
        let name = (1..)
            .map(|n| {
                if n == 1 {
                    name.to_string()
                } else {
                    format!("{name} {n}")
                }
            })
            .find(|candidate| !taken.contains(candidate))
            .unwrap_or_else(|| name.to_string());
        let added: Result<BehaviorId, ModelError> = self.editor.edit("New Template", |p| {
            let label = p.label_for(&name);
            p.add_behavior(label, name, kind)
        });
        if let Ok(id) = added {
            self.behavior = Some(id);
            self.slot = None;
        }
        self.after_change(window, cx);
    }

    fn delete_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.behavior else { return };
        self.behavior = None;
        self.slot = None;
        self.change("Delete Template", window, cx, |p| p.delete_template(id));
    }

    fn duplicate_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.behavior else { return };
        let name = self
            .project()
            .behavior(id)
            .map(|d| format!("{} copy", d.name))
            .unwrap_or_default();
        let copied = self
            .editor
            .edit("Duplicate Template", |p| p.duplicate_template(id, name));
        if let Ok(copy) = copied {
            self.behavior = Some(copy);
            self.slot = None;
        }
        self.after_change(window, cx);
    }

    /// A row with a label, the current value and buttons to change it.
    pub(super) fn stepper(
        &self,
        id: &'static str,
        label: &str,
        value: String,
        cx: &mut Context<Self>,
        on_step: impl Fn(&mut Self, i64, &mut Window, &mut Context<Self>) + 'static,
    ) -> Div {
        let on_step = Rc::new(on_step);
        let (fewer, more) = (on_step.clone(), on_step);
        let control = div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                chip((id, 0usize), "−", false, cx)
                    .on_click(cx.listener(move |this, _, window, cx| fewer(this, -1, window, cx))),
            )
            .child(div().w_20().flex().justify_center().text_sm().child(value))
            .child(
                chip((id, 1usize), "+", false, cx)
                    .on_click(cx.listener(move |this, _, window, cx| more(this, 1, window, cx))),
            );
        field(label.to_string(), "", control, cx)
    }

    /// A chip showing a slot's binding; clicking aims the picker at it.
    pub(super) fn slot_chip(
        &self,
        slot: Slot,
        id: ElementId,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let text = self
            .project()
            .slot(slot)
            .map(|b| kc_model::words::describe(self.project(), b))
            .unwrap_or_default();
        chip(id, text, self.slot == Some(slot), cx).on_click(cx.listener(
            move |this, _, window, cx| {
                this.slot = Some(slot);
                this.key_slot = None;
                this.pending_slot = None;
                this.after_change(window, cx);
            },
        ))
    }

    /// A button showing the choice made, which opens a menu of the choices.
    pub(super) fn select(
        &self,
        id: &'static str,
        shown: &'static str,
        options: Vec<(&'static str, bool, Pick)>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let workspace = cx.weak_entity();
        Button::new(id)
            .label(shown)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for (label, chosen, pick) in &options {
                    let (workspace, pick) = (workspace.clone(), pick.clone());
                    menu = menu.item(PopupMenuItem::new(*label).checked(*chosen).on_click(
                        move |_, window, cx| {
                            let _ = workspace.update(cx, |this, cx| pick(this, window, cx));
                        },
                    ));
                }
                menu
            })
    }

    /// A part of a form that starts folded away under its title, with a
    /// line on what it holds; clicking the title opens it.
    pub(super) fn fold(
        &self,
        id: &'static str,
        title: &'static str,
        summary: String,
        cx: &mut Context<Self>,
        body: impl FnOnce(Div, &mut Context<Self>) -> Div,
    ) -> Div {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let open = self.unfolded.contains(&id);
        let header = div()
            .id(id)
            .flex()
            .items_center()
            .gap_3()
            .mt_8()
            .py_2()
            .border_b_1()
            .border_color(border)
            .cursor_pointer()
            .child(section_title(title, cx))
            .child(div().flex_1())
            .when(!open, |header| {
                header.child(div().text_sm().text_color(muted).child(summary))
            })
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(if open { "Hide" } else { "Show" }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.unfolded.remove(&id) {
                    this.unfolded.insert(id);
                }
                cx.notify();
            }));
        let column = div().flex().flex_col().child(header);
        if open {
            body(column, cx)
        } else {
            column
        }
    }

    /// The steps of a macro as a timeline, with the typed-text field. The
    /// same editor serves a key's own macro and a macro template.
    pub(super) fn render_macro_steps(
        &self,
        id: BehaviorId,
        m: &Macro,
        cx: &mut Context<Self>,
    ) -> Div {
        let (muted, border) = (cx.theme().muted_foreground, cx.theme().border);
        let steps = m
            .steps
            .iter()
            .enumerate()
            .map(|(step, kind)| {
                let (title, bindings): (String, Option<usize>) = match kind {
                    MacroStep::Tap(b) => ("Tap".into(), Some(b.len())),
                    MacroStep::Press(b) => ("Press".into(), Some(b.len())),
                    MacroStep::Release(b) => ("Release".into(), Some(b.len())),
                    MacroStep::PauseForRelease => ("Wait until the key is released".into(), None),
                    MacroStep::WaitTime(ms) => {
                        (format!("From here, wait {ms} ms between steps"), None)
                    }
                    MacroStep::TapTime(ms) => (format!("From here, hold taps for {ms} ms"), None),
                    MacroStep::Param { from, to } => (
                        format!("Pass value {from} of the key on to the next step's value {to}"),
                        None,
                    ),
                };
                let slots = (0..bindings.unwrap_or(0))
                    .map(|index| {
                        self.slot_chip(
                            Slot::MacroStep {
                                behavior: id,
                                step,
                                index,
                            },
                            ("macro-slot", (step << 16) | index).into(),
                            cx,
                        )
                    })
                    .collect::<Vec<_>>();
                let timed = matches!(kind, MacroStep::WaitTime(_) | MacroStep::TapTime(_));
                let act = |chip_id: &'static str,
                           label: &'static str,
                           cx: &mut Context<Self>,
                           edit: fn(&mut Vec<MacroStep>, usize)| {
                    chip((chip_id, step), label, false, cx).on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.slot = None;
                            this.edit_definition(id, "Change Macro", window, cx, |kind| {
                                if let BehaviorKind::Macro(m) = kind {
                                    edit(&mut m.steps, step);
                                }
                            });
                        },
                    ))
                };
                div()
                    .flex()
                    .items_center()
                    .flex_wrap()
                    .gap_1p5()
                    .py_2()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        div()
                            .min_w_20()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(title),
                    )
                    .children(slots)
                    .when(bindings.is_some(), |row| {
                        row.child(act("macro-add-binding", "+", cx, |steps, at| {
                            if let Some(
                                MacroStep::Tap(b) | MacroStep::Press(b) | MacroStep::Release(b),
                            ) = steps.get_mut(at)
                            {
                                b.push(Binding::none());
                            }
                        }))
                    })
                    .when(timed, |row| {
                        row.child(act("macro-time-less", "−", cx, |steps, at| {
                            if let Some(MacroStep::WaitTime(ms) | MacroStep::TapTime(ms)) =
                                steps.get_mut(at)
                            {
                                *ms = ms.saturating_sub(10);
                            }
                        }))
                        .child(act(
                            "macro-time-more",
                            "+",
                            cx,
                            |steps, at| {
                                if let Some(MacroStep::WaitTime(ms) | MacroStep::TapTime(ms)) =
                                    steps.get_mut(at)
                                {
                                    *ms += 10;
                                }
                            },
                        ))
                    })
                    .child(div().flex_1())
                    .child(act("macro-up", "↑", cx, |steps, at| {
                        if at > 0 {
                            steps.swap(at, at - 1);
                        }
                    }))
                    .child(act("macro-down", "↓", cx, |steps, at| {
                        if at + 1 < steps.len() {
                            steps.swap(at, at + 1);
                        }
                    }))
                    .child(act("macro-remove", "Remove", cx, |steps, at| {
                        steps.remove(at);
                    }))
            })
            .collect::<Vec<_>>();

        let add = |chip_id: &'static str,
                   label: &'static str,
                   cx: &mut Context<Self>,
                   make: fn() -> MacroStep| {
            chip(chip_id, label, false, cx).on_click(cx.listener(move |this, _, window, cx| {
                this.edit_definition(id, "Add Macro Step", window, cx, |kind| {
                    if let BehaviorKind::Macro(m) = kind {
                        m.steps.push(make());
                    }
                });
            }))
        };
        let params = (0..=2u8)
            .map(|count| {
                chip(
                    ("macro-params", count as usize),
                    count.to_string(),
                    m.params == count,
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.edit_definition(id, "Change Macro Values", window, cx, |kind| {
                        if let BehaviorKind::Macro(m) = kind {
                            m.params = count;
                        }
                    });
                }))
            })
            .collect::<Vec<_>>();
        let timed = match (m.wait_ms, m.tap_ms) {
            (None, None) => "As it comes".to_string(),
            _ => "Changed".to_string(),
        };
        let passed = match m.params {
            0 => "None".to_string(),
            n => n.to_string(),
        };
        div()
            .flex()
            .flex_col()
            .child(
                group("STEPS", cx)
                    .when(m.steps.is_empty(), |page| {
                        page.child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child("No steps yet. Add one below, or type some text."),
                        )
                    })
                    .children(steps),
            )
            .child(
                group("ADD A STEP", cx).child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .child(add("macro-add-tap", "Tap", cx, || {
                            MacroStep::Tap(vec![Binding::none()])
                        }))
                        .child(add("macro-add-press", "Press", cx, || {
                            MacroStep::Press(vec![Binding::none()])
                        }))
                        .child(add("macro-add-release", "Release", cx, || {
                            MacroStep::Release(vec![Binding::none()])
                        }))
                        .child(add("macro-add-wait", "Change wait time", cx, || {
                            MacroStep::WaitTime(50)
                        }))
                        .child(add("macro-add-tap-time", "Change tap time", cx, || {
                            MacroStep::TapTime(30)
                        }))
                        .child(add("macro-add-pause", "Wait for key release", cx, || {
                            MacroStep::PauseForRelease
                        }))
                        .pt_2(),
                ),
            )
            .child(
                group("TYPE TEXT", cx).child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w_72().child(Input::new(&self.macro_text)))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child("Press Return to add it as taps."),
                        )
                        .pt_2(),
                ),
            )
            .child(self.fold("macro-timing", "TIMING", timed, cx, |column, cx| {
                column
                    .child(self.stepper(
                        "macro-wait",
                        "Wait between steps",
                        Self::optional_ms_default(m.wait_ms, "default"),
                        cx,
                        move |this, d, window, cx| {
                            this.edit_definition(id, "Change Macro Timing", window, cx, |kind| {
                                if let BehaviorKind::Macro(m) = kind {
                                    Self::step_optional_ms(&mut m.wait_ms, d, 5, 15);
                                }
                            });
                        },
                    ))
                    .child(self.stepper(
                        "macro-tap",
                        "Hold each tap for",
                        Self::optional_ms_default(m.tap_ms, "default"),
                        cx,
                        move |this, d, window, cx| {
                            this.edit_definition(id, "Change Macro Timing", window, cx, |kind| {
                                if let BehaviorKind::Macro(m) = kind {
                                    Self::step_optional_ms(&mut m.tap_ms, d, 5, 30);
                                }
                            });
                        },
                    ))
            }))
            // Values a key passes on are a thing imported macros do; they
            // are kept out of the way.
            .child(self.fold(
                "macro-params",
                "ADVANCED: VALUES THE KEY PASSES ON",
                passed,
                cx,
                |column, cx| {
                    column
                        .child(help("A macro can take one or two values from the key that runs it and pass them to a step. Keymaps written by hand use this; keys set up here do not.", cx).pt_1().pb_2())
                        .child(div().flex().gap_1().children(params).pt_2())
                        .child(div().flex().pt_2().child(add("macro-add-param", "Add a pass-on step", cx, || {
                            MacroStep::Param { from: 1, to: 1 }
                        })))
                },
            ))
    }

    fn render_template_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted, hover) = (theme.border, theme.muted_foreground, theme.secondary);
        let accent_ink = crate::theme::look(cx).colors.accent_text;
        let rows = self
            .project()
            .behavior_templates()
            .map(|def| {
                let (id, active) = (def.id, Some(def.id) == self.behavior);
                let uses = self.project().template_uses(id).len();
                let summary = match &def.kind {
                    BehaviorKind::Macro(m) => match m.steps.len() {
                        0 => "No steps yet".to_string(),
                        1 => "1 step".to_string(),
                        n => format!("{n} steps"),
                    },
                    other => other.settings().summary(),
                };
                div()
                    .id(("template", id.0 as usize))
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .when(active, |row| row.bg(hover))
                    .when(!active, |row| {
                        row.text_color(muted).hover(|row| row.bg(hover))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().flex_1().min_w_0().child(display(
                                def.name.clone(),
                                16.,
                                cx,
                            )))
                            .child(badge(def.kind.name(), accent_ink)),
                    )
                    .child(div().text_xs().text_color(muted).child(summary))
                    .child(div().text_xs().text_color(muted).child(match uses {
                        0 => "Not used yet".to_string(),
                        1 => "Used on 1 key".to_string(),
                        n => format!("Used on {n} keys"),
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.behavior = Some(id);
                        this.slot = None;
                        this.after_change(window, cx);
                    }))
            })
            .collect::<Vec<_>>();
        let workspace = cx.weak_entity();
        let new = Button::new("new-template")
            .label("New")
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for (label, kind) in NEW {
                    let workspace = workspace.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        let _ =
                            workspace.update(cx, |this, cx| this.add_template(kind, window, cx));
                    }));
                }
                menu
            });
        div()
            .w_80()
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .child(section_title("TEMPLATES", cx))
                    .child(div().flex_1())
                    .child(new),
            )
            .child(
                div()
                    .id("template-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px_2()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .children(rows),
            )
    }

    /// Where a template is used: the keys on a small keyboard, and a list
    /// by layer that jumps to the key.
    fn render_template_uses(&self, id: BehaviorId, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let uses = self.project().template_uses(id);
        let mut positions: Vec<usize> = uses
            .iter()
            .filter_map(|u| match u {
                Location::Key { position, .. } => Some(*position),
                _ => None,
            })
            .collect();
        positions.sort_unstable();
        positions.dedup();
        let chips = uses
            .iter()
            .filter_map(|u| match u {
                Location::Key { layer, position } => {
                    let name = self.project().layer(*layer)?.name.clone();
                    Some((*layer, *position, name))
                }
                _ => None,
            })
            .map(|(layer, position, name)| {
                chip(
                    ("template-use", (layer.0 as usize) << 16 | position),
                    format!("{name}: key {position}"),
                    false,
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.layer = layer;
                    this.selection = vec![position];
                    this.set_mode(Mode::Keyboard, window, cx);
                }))
            })
            .collect::<Vec<_>>();
        let keys = self.layout_keys().to_vec();
        let blank = kc_model::keycap::Keycap::new("", kc_model::keycap::KeycapKind::Key);
        let frame = Frame {
            keycaps: vec![blank; keys.len()],
            keys,
            devices: Vec::new(),
            selected: positions,
            hovered: None,
            drag: None,
            band: None,
            palette: Palette::themed(cx),
            tint: None,
            links: Vec::new(),
            colors: Vec::new(),
            bands: Vec::new(),
        };
        let bounds = self.mini_bounds.clone();
        group("USED ON", cx)
            .when(uses.is_empty(), |column| {
                column.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("No key yet. Choose it under a key's feel in the Keyboard tab."),
                )
            })
            .when(!uses.is_empty(), |column| {
                column
                    .child(div().flex().flex_wrap().gap_1().pt_1().children(chips))
                    .child(
                        plinth(cx)
                            .mt_4()
                            .h_48()
                            .child(canvas::keyboard(frame, move |b| bounds.set(b))),
                    )
            })
    }

    pub(super) fn render_templates(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let form = div()
            .id("template-form")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scrollbar()
            .px_8()
            .py_6();
        let column = div().w_full().max_w(px(760.)).flex().flex_col();
        let selected = self
            .behavior
            .and_then(|id| self.project().behavior(id))
            .filter(|def| def.scope == Scope::Template);
        let column = match selected {
            None => column.child(help("A template is a feel worth repeating: how a hold & tap decides, how long a second tap may wait, which modifiers switch an override, how a sticky key lets go. Set a key up in the Keyboard tab and save its feel as a template, or start one here and choose it under a key's feel. Macros saved as templates can be put on any key.", cx)),
            Some(def) => {
                let id = def.id;
                let body = match &def.kind {
                    BehaviorKind::Macro(m) => self.render_macro_steps(id, m, cx),
                    other => self.render_settings(FeelTarget::Template(id), &other.settings(), cx),
                };
                column
                    .child(heading(def.kind.name(), cx))
                    .child(help(def.kind.summary(), cx).pt_1())
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_4()
                            .pt_5()
                            .child(
                                div()
                                    .w_72()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(section_title("NAME", cx))
                                    .child(Input::new(&self.behavior_name)),
                            )
                            .child(div().flex_1())
                            .child(chip("duplicate-template", "Duplicate", false, cx).on_click(
                                cx.listener(|this, _, window, cx| this.duplicate_template(window, cx)),
                            ))
                            .child(chip("delete-template", "Delete", false, cx).on_click(
                                cx.listener(|this, _, window, cx| this.delete_template(window, cx)),
                            )),
                    )
                    .when(!def.description.is_empty(), |column| {
                        column.child(
                            group("DESCRIPTION", cx)
                                .child(help(def.description.clone(), cx).pt_1()),
                        )
                    })
                    .child(body)
                    .child(self.render_template_uses(id, cx))
                    .child(self.fold("template-advanced", "ADVANCED", def.label.clone(), cx, |column, cx| {
                        column
                            .child(help("The template's name in the generated keymap, for custom devicetree that refers to it.", cx).pt_1().pb_2())
                            .child(div().w_72().child(Input::new(&self.behavior_label)))
                            .child(div().pt_1().text_xs().text_color(muted).child("Letters, digits and underscores, not starting with a digit."))
                    }))
            }
        };
        let form = form.child(column);
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(self.render_template_list(cx))
            .child(form)
    }
}
