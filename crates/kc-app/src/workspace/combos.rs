//! The Combos mode: keys pressed together that do something else.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_model::features::Combo;
use kc_model::keycap::{keycap, Keycap, KeycapKind};
use kc_model::{Binding, ComboId, Slot};

use super::{chip, section_title, Workspace};
use crate::canvas::{self, Frame, Palette};

pub(super) fn subscribe(
    name: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    cx.subscribe_in(name, window, |this, _, event: &InputEvent, window, cx| {
        if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            this.after_change(window, cx);
        }
    })
    .detach();
}

impl Workspace {
    fn edit_combo(
        &mut self,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Combo),
    ) {
        if let Some(id) = self.combo {
            self.change(label, window, cx, |p| {
                edit(p.combo_mut(id)?);
                Ok(())
            });
        }
    }

    fn select_combo(&mut self, id: ComboId, window: &mut Window, cx: &mut Context<Self>) {
        self.combo = Some(id);
        self.slot = Some(Slot::Combo(id));
        self.after_change(window, cx);
    }

    fn add_combo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = format!("Combo {}", self.project().combos.len() + 1);
        let added = self.editor.edit("Add Combo", |p| {
            Ok(p.add_combo(name, Vec::new(), Binding::none()))
        });
        if let Ok(id) = added {
            self.combo = Some(id);
            self.slot = Some(Slot::Combo(id));
        }
        self.after_change(window, cx);
    }

    fn render_combo_form(&self, combo: &Combo, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let text = kc_model::text::format_binding(
            self.project(),
            &combo.binding,
            kc_model::text::LayerStyle::Index,
        );
        let id = combo.id;
        let layers = self
            .project()
            .layers
            .iter()
            .map(|layer| {
                let layer_id = layer.id;
                chip(
                    ("combo-layer", layer_id.0 as usize),
                    layer.name.clone(),
                    combo.layers.contains(&layer_id),
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.edit_combo("Change Combo Layers", window, cx, |combo| {
                        match combo.layers.iter().position(|l| *l == layer_id) {
                            Some(at) => {
                                combo.layers.remove(at);
                            }
                            None => combo.layers.push(layer_id),
                        }
                    });
                }))
            })
            .collect::<Vec<_>>();
        let keys = match combo.key_positions.len() {
            0 => "No keys yet. Click the keys on the keyboard that make up this combo.".to_string(),
            1 => "One key so far. A combo needs at least two.".to_string(),
            n => format!("{n} keys. Click a key to add or remove it."),
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("NAME", cx))
            .child(Input::new(&self.combo_name))
            .child(div().text_xs().text_color(muted).child(keys))
            .child(section_title("PRESSING THEM TOGETHER DOES", cx))
            .child(
                chip(
                    "combo-binding",
                    text,
                    self.slot == Some(Slot::Combo(id)),
                    cx,
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.select_combo(id, window, cx)),
                ),
            )
            .child(self.stepper(
                "combo-timeout",
                "Keys must land within",
                Self::optional_ms_default(combo.timeout_ms, "50 ms (default)"),
                cx,
                |this, d, window, cx| {
                    this.edit_combo("Change Combo Timing", window, cx, |combo| {
                        Self::step_optional_ms(&mut combo.timeout_ms, d, 5, 50);
                    });
                },
            ))
            .child(self.stepper(
                "combo-idle",
                "Only after a pause in typing",
                Self::optional_ms_default(combo.require_prior_idle_ms, "off"),
                cx,
                |this, d, window, cx| {
                    this.edit_combo("Change Combo Timing", window, cx, |combo| {
                        Self::step_optional_ms(&mut combo.require_prior_idle_ms, d, 25, 100);
                    });
                },
            ))
            .child(
                chip(
                    "combo-slow",
                    "Stay active until every key is released",
                    combo.slow_release,
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.edit_combo("Change Combo", window, cx, |combo| {
                        combo.slow_release = !combo.slow_release;
                    });
                })),
            )
            .child(section_title("ACTIVE ON", cx))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(if combo.layers.is_empty() {
                        "Every layer. Choose layers to limit it."
                    } else {
                        "Only the chosen layers."
                    }),
            )
            .child(div().flex().flex_wrap().gap_1().children(layers))
            .child(
                chip("combo-delete", "Delete Combo", false, cx).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.change("Delete Combo", window, cx, |p| p.remove_combo(id))
                    },
                )),
            )
    }

    pub(super) fn optional_ms_default(value: Option<u32>, unset: &str) -> String {
        value.map_or_else(|| unset.to_string(), |ms| format!("{ms} ms"))
    }

    pub(super) fn step_optional_ms(value: &mut Option<u32>, direction: i64, step: u32, start: u32) {
        *value = match (*value, direction) {
            (None, 1) => Some(start),
            (None, _) => None,
            (Some(ms), 1) => Some(ms + step),
            (Some(ms), _) if ms <= step => None,
            (Some(ms), _) => Some(ms - step),
        };
    }

    pub(super) fn render_combos(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted, accent, accent_text, hover) = (
            theme.border,
            theme.muted_foreground,
            theme.primary,
            theme.primary_foreground,
            theme.secondary,
        );
        let rows = self
            .project()
            .combos
            .iter()
            .map(|combo| {
                let (id, active) = (combo.id, Some(combo.id) == self.combo);
                div()
                    .id(("combo", id.0 as usize))
                    .px_3()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .when(active, |row| row.bg(accent).text_color(accent_text))
                    .when(!active, |row| row.hover(|row| row.bg(hover)))
                    .child(combo.name.clone())
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.select_combo(id, window, cx)),
                    )
            })
            .collect::<Vec<_>>();
        let selected = self
            .combo
            .and_then(|id| self.project().combos.iter().find(|c| c.id == id));

        let palette = Palette::themed(cx);
        let keys = self.layout_keys().to_vec();
        let blank = Keycap {
            legend: String::new(),
            hold: None,
            kind: KeycapKind::None,
        };
        let frame = Frame {
            keycaps: (0..keys.len())
                .map(|p| keycap(self.project(), self.layer, p).unwrap_or(blank.clone()))
                .collect(),
            keys,
            devices: Vec::new(),
            selected: selected
                .map(|c| c.key_positions.clone())
                .unwrap_or_default(),
            hovered: self.hovered,
            drag: None,
            band: None,
            palette,
            tint: None,
            colors: Vec::new(),
            links: self
                .project()
                .combos
                .iter()
                .map(|c| (c.key_positions.clone(), Some(c.id) == self.combo))
                .collect(),
        };
        let bounds = self.canvas_bounds.clone();

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(
                div()
                    .id("combo-panel")
                    .w_80()
                    .h_full()
                    .overflow_y_scroll()
                    .border_r_1()
                    .border_color(border)
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(section_title("COMBOS", cx))
                    .when(rows.is_empty(), |panel| {
                        panel.child(div().text_sm().text_color(muted).child(
                            "A combo is two or more keys pressed together, such as J and K for Escape.",
                        ))
                    })
                    .children(rows)
                    .child(chip("combo-add", "New Combo", false, cx).on_click(cx.listener(
                        |this, _, window, cx| this.add_combo(window, cx),
                    )))
                    .when_some(selected, |panel, combo| {
                        panel.child(div().h_px().bg(border)).child(self.render_combo_form(combo, cx))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(canvas::keyboard(frame, move |b| bounds.set(b)))
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        let hovered = canvas::key_at(this.layout_keys(), &[], this.canvas_bounds.get(), event.position);
                        if hovered != this.hovered {
                            this.hovered = hovered;
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            let hit = canvas::key_at(this.layout_keys(), &[], this.canvas_bounds.get(), event.position);
                            let Some(key) = hit else { return };
                            this.edit_combo("Change Combo Keys", window, cx, |combo| {
                                match combo.key_positions.iter().position(|p| *p == key) {
                                    Some(at) => {
                                        combo.key_positions.remove(at);
                                    }
                                    None => combo.key_positions.push(key),
                                }
                                combo.key_positions.sort_unstable();
                            });
                        }),
                    ),
            )
    }
}
