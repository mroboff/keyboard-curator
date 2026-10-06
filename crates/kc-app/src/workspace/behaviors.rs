//! The Behaviors mode: hold-taps, tap-dances, mod-morphs, sticky keys and
//! macros.

use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_model::behavior::{
    BehaviorKind, Flavor, HoldTap, Macro, MacroStep, ModMorph, StickyKey, TapDance,
};
use kc_model::text::{format_binding, LayerStyle};
use kc_model::{BehaviorId, BehaviorRef, Binding, KeyExpr, ModelError, Slot};
use kc_zmk::Modifier;

use super::{badge, chip, display, field, group, heading, help, plinth, section_title, Workspace};
use crate::canvas::{self, Frame, Palette};

/// The built-in behaviors a hold-tap or sticky key can wrap.
const WRAPPABLE: [(&str, &str); 5] = [
    ("kp", "Key"),
    ("mo", "Layer while held"),
    ("to", "Go to layer"),
    ("sl", "Sticky layer"),
    ("sk", "Sticky key"),
];

/// What choosing from a menu does.
type Pick = Rc<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// The kinds of behavior that can be made from scratch.
#[derive(Debug, Clone, Copy)]
enum New {
    HoldTap,
    TapDance,
    ModMorph,
    StickyKey,
    Macro,
}

const NEW: [(&str, New); 5] = [
    ("Hold-tap", New::HoldTap),
    ("Tap-dance", New::TapDance),
    ("Mod-morph", New::ModMorph),
    ("Sticky key", New::StickyKey),
    ("Macro", New::Macro),
];

const FLAVORS: [(Flavor, &str, &str); 4] = [
    (
        Flavor::HoldPreferred,
        "Hold-preferred",
        "Holds as soon as another key is pressed.",
    ),
    (
        Flavor::Balanced,
        "Balanced",
        "Holds if another key is pressed and released while this one is down.",
    ),
    (
        Flavor::TapPreferred,
        "Tap-preferred",
        "Holds only after the tapping term.",
    ),
    (
        Flavor::TapUnlessInterrupted,
        "Tap-unless-interrupted",
        "Taps unless another key is pressed first.",
    ),
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
            if !keys.is_empty() {
                this.edit_behavior("Add Typed Text", window, cx, |kind| {
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
    /// Changes the behavior being edited.
    pub(super) fn edit_behavior(
        &mut self,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut BehaviorKind),
    ) {
        if let Some(id) = self.behavior {
            self.change(label, window, cx, |p| {
                edit(&mut p.behavior_mut(id)?.kind);
                Ok(())
            });
        }
    }

    /// A label that is not taken yet, from `base`.
    fn free_label(&self, base: &str) -> String {
        let taken = |label: &str| {
            kc_zmk::behaviors::built_in(label).is_some()
                || self.project().behaviors.iter().any(|b| b.label == label)
        };
        (1..)
            .map(|n| {
                if n == 1 {
                    base.to_string()
                } else {
                    format!("{base}{n}")
                }
            })
            .find(|label| !taken(label))
            .unwrap_or_else(|| base.to_string())
    }

    fn add_behavior(
        &mut self,
        base: &str,
        name: &str,
        kind: BehaviorKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = self.free_label(base);
        let added: Result<BehaviorId, ModelError> = self
            .editor
            .edit("Add Behavior", |p| p.add_behavior(label, name, kind));
        if let Ok(id) = added {
            self.behavior = Some(id);
            self.slot = None;
        }
        self.after_change(window, cx);
    }

    /// Key positions on one hand, judged by the layout's center line.
    fn hand_positions(&self, right: bool) -> Vec<usize> {
        let keys = self.layout_keys();
        let Some(bounds) = kc_boards::geometry::bounds(keys) else {
            return Vec::new();
        };
        let middle = (bounds.min.x + bounds.max.x) / 2.;
        keys.iter()
            .enumerate()
            .filter(|(_, key)| (key.center().x > middle) == right)
            .map(|(index, _)| index)
            .collect()
    }

    /// A home-row mod for one hand: it holds only when the next key is on
    /// the other hand, which is what stops rolls from misfiring.
    fn add_home_row_mod(&mut self, right: bool, window: &mut Window, cx: &mut Context<Self>) {
        let kind = BehaviorKind::HoldTap(HoldTap {
            flavor: Flavor::Balanced,
            tapping_term_ms: 280,
            quick_tap_ms: Some(175),
            require_prior_idle_ms: Some(150),
            hold_trigger_key_positions: self.hand_positions(!right),
            hold_trigger_on_release: true,
            ..HoldTap::new(BehaviorRef::built_in("kp"), BehaviorRef::built_in("kp"))
        });
        let (base, name) = if right {
            ("hmr", "Home-row mod (right hand)")
        } else {
            ("hml", "Home-row mod (left hand)")
        };
        self.add_behavior(base, name, kind, window, cx);
    }

    fn delete_behavior(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.behavior else { return };
        let uses = self.project().behavior_references(id).len();
        if uses > 0 {
            self.notice = Some(format!(
                "This behavior is used in {uses} place(s). Reassign those keys before deleting it."
            ));
            cx.notify();
            return;
        }
        self.change("Delete Behavior", window, cx, |p| p.remove_behavior(id));
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
    fn slot_chip(&self, slot: Slot, id: ElementId, cx: &mut Context<Self>) -> Stateful<Div> {
        let text = self
            .project()
            .slot(slot)
            .map(|b| format_binding(self.project(), b, LayerStyle::Index))
            .unwrap_or_default();
        chip(id, text, self.slot == Some(slot), cx).on_click(cx.listener(
            move |this, _, window, cx| {
                this.slot = Some(slot);
                this.after_change(window, cx);
            },
        ))
    }

    fn render_behavior_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted, accent, hover) = (
            theme.border,
            theme.muted_foreground,
            theme.primary,
            theme.secondary,
        );
        let rows = self
            .project()
            .behaviors
            .iter()
            .map(|def| {
                let (id, active) = (def.id, Some(def.id) == self.behavior);
                div()
                    .id(("behavior", id.0 as usize))
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap_2()
                    // Like the layer list: the one shown is set apart by
                    // weight, not by a block of color.
                    .when(active, |row| row.bg(hover))
                    .when(!active, |row| {
                        row.text_color(muted).hover(|row| row.bg(hover))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(display(def.name.clone(), 16., cx))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("&{}", def.label)),
                            ),
                    )
                    // What kind of behavior it is, set apart from its name.
                    .child(badge(def.kind.name(), if active { accent } else { muted }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.behavior = Some(id);
                        this.slot = None;
                        this.after_change(window, cx);
                    }))
            })
            .collect::<Vec<_>>();

        // Everything that can be made, in one menu beside the list's title.
        let workspace = cx.weak_entity();
        let new = Button::new("new-behavior")
            .label("New")
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for (label, kind) in NEW {
                    let workspace = workspace.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        let _ = workspace.update(cx, |this, cx| this.add_new(kind, window, cx));
                    }));
                }
                menu = menu.separator().label("Home-row mods");
                for (label, right) in [("For the left hand", false), ("For the right hand", true)] {
                    let workspace = workspace.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        let _ = workspace
                            .update(cx, |this, cx| this.add_home_row_mod(right, window, cx));
                    }));
                }
                menu
            });
        div()
            .w_72()
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
                    .child(section_title("BEHAVIORS", cx))
                    .child(div().flex_1())
                    .child(new),
            )
            .child(
                div()
                    .id("behavior-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .children(rows),
            )
    }

    /// Makes a new behavior of a kind, set up the way one usually starts.
    fn add_new(&mut self, kind: New, window: &mut Window, cx: &mut Context<Self>) {
        let kp = || BehaviorRef::built_in("kp");
        let (base, name, kind) = match kind {
            New::HoldTap => (
                "ht",
                "Hold-tap",
                BehaviorKind::HoldTap(HoldTap::new(kp(), kp())),
            ),
            New::TapDance => (
                "td",
                "Tap-dance",
                BehaviorKind::TapDance(TapDance {
                    tapping_term_ms: 200,
                    bindings: vec![Binding::none(), Binding::none()],
                }),
            ),
            New::ModMorph => (
                "mm",
                "Mod-morph",
                BehaviorKind::ModMorph(ModMorph {
                    normal: Binding::none(),
                    morphed: Binding::none(),
                    mods: vec![Modifier::LShift, Modifier::RShift],
                    keep_mods: vec![],
                }),
            ),
            New::StickyKey => (
                "sticky",
                "Sticky key",
                BehaviorKind::StickyKey(StickyKey {
                    behavior: kp(),
                    release_after_ms: 1000,
                    quick_release: false,
                    lazy: false,
                    ignore_modifiers: true,
                }),
            ),
            New::Macro => (
                "macro",
                "Macro",
                BehaviorKind::Macro(Macro {
                    wait_ms: None,
                    tap_ms: None,
                    params: 0,
                    steps: vec![],
                }),
            ),
        };
        self.add_behavior(base, name, kind, window, cx);
    }

    /// A button showing the choice made, which opens a menu of the choices.
    fn select(
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

    /// A menu choosing which built-in a hold-tap side or sticky key wraps.
    fn wrap_chips(
        &self,
        id: &'static str,
        current: &BehaviorRef,
        cx: &mut Context<Self>,
        set: fn(&mut BehaviorKind, BehaviorRef),
    ) -> impl IntoElement {
        let is = |label: &str| *current == BehaviorRef::built_in(label);
        let shown = WRAPPABLE
            .iter()
            .find(|(label, _)| is(label))
            .map_or("Something else", |(_, name)| *name);
        let options = WRAPPABLE
            .iter()
            .map(|(label, name)| {
                let label = *label;
                let pick: Pick = Rc::new(move |this, window, cx| {
                    this.edit_behavior("Change Behavior", window, cx, |kind| {
                        set(kind, BehaviorRef::built_in(label));
                    });
                });
                (*name, is(label), pick)
            })
            .collect();
        self.select(id, shown, options, cx)
    }

    /// A part of a form that starts folded away under its title, with a
    /// line on what it holds; clicking the title opens it.
    fn fold(
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

    /// A row for something that is on or off.
    fn toggle(
        &self,
        id: &'static str,
        label: &'static str,
        on: bool,
        cx: &mut Context<Self>,
        flip: fn(&mut BehaviorKind),
    ) -> Div {
        let control = chip(id, if on { "On" } else { "Off" }, on, cx).on_click(cx.listener(
            move |this, _, window, cx| {
                this.edit_behavior("Change Behavior", window, cx, flip);
            },
        ));
        field(label, "", control, cx)
    }

    fn render_hold_tap(&self, h: &HoldTap, cx: &mut Context<Self>) -> Div {
        let flavor_name = FLAVORS.iter().find(|f| f.0 == h.flavor).map_or("", |f| f.1);
        let flavors = FLAVORS
            .iter()
            .map(|(flavor, name, _)| {
                let flavor = *flavor;
                let pick: Pick = Rc::new(move |this, window, cx| {
                    this.edit_behavior("Change Flavor", window, cx, |kind| {
                        if let BehaviorKind::HoldTap(h) = kind {
                            h.flavor = flavor;
                        }
                    });
                });
                (*name, *name == flavor_name, pick)
            })
            .collect();
        let flavor = self.select("flavor", flavor_name, flavors, cx);
        let flavor_help = FLAVORS.iter().find(|f| f.0 == h.flavor).map_or("", |f| f.2);

        let palette = Palette::themed(cx);
        let keys = self.layout_keys().to_vec();
        let blank = kc_model::keycap::Keycap {
            legend: String::new(),
            hold: None,
            kind: kc_model::keycap::KeycapKind::Key,
        };
        let frame = Frame {
            keycaps: vec![blank; keys.len()],
            keys,
            devices: Vec::new(),
            selected: h.hold_trigger_key_positions.clone(),
            hovered: None,
            drag: None,
            band: None,
            palette,
            tint: None,
            links: Vec::new(),
            colors: Vec::new(),
        };
        let bounds = self.mini_bounds.clone();
        let hands =
            |id: &'static str, label: &'static str, right: Option<bool>, cx: &mut Context<Self>| {
                chip(id, label, false, cx).on_click(cx.listener(move |this, _, window, cx| {
                    let positions = right.map_or_else(Vec::new, |right| this.hand_positions(right));
                    this.edit_behavior("Change Hold Trigger Keys", window, cx, |kind| {
                        if let BehaviorKind::HoldTap(h) = kind {
                            h.hold_trigger_key_positions = positions;
                        }
                    });
                }))
            };

        let tuned = [
            h.quick_tap_ms.is_some(),
            h.require_prior_idle_ms.is_some(),
            h.retro_tap,
            h.hold_while_undecided,
        ]
        .into_iter()
        .filter(|changed| *changed)
        .count();
        let tuned = match tuned {
            0 => "All as they come".to_string(),
            n => format!("{n} changed"),
        };
        let triggers = match h.hold_trigger_key_positions.len() {
            0 => "Any key".to_string(),
            1 => "1 key".to_string(),
            n => format!("{n} keys"),
        };
        let tap = self.wrap_chips("tap-wrap", &h.tap, cx, |kind, behavior| {
            if let BehaviorKind::HoldTap(h) = kind {
                h.tap = behavior;
            }
        });
        let hold = self.wrap_chips("hold-wrap", &h.hold, cx, |kind, behavior| {
            if let BehaviorKind::HoldTap(h) = kind {
                h.hold = behavior;
            }
        });
        div()
            .flex()
            .flex_col()
            .child(
                group("WHAT IT DOES", cx)
                    .child(field("When tapped", "", tap, cx))
                    .child(field("When held", "", hold, cx)),
            )
            .child(
                group("DECIDING BETWEEN HOLD AND TAP", cx)
                    .child(field("How it decides", flavor_help, flavor, cx))
                    .child(self.stepper("tapping-term", "Tapping term", format!("{} ms", h.tapping_term_ms), cx, |this, d, window, cx| {
                this.edit_behavior("Change Tapping Term", window, cx, |kind| {
                    if let BehaviorKind::HoldTap(h) = kind {
                        h.tapping_term_ms = h.tapping_term_ms.saturating_add_signed(d as i32 * 10).max(10);
                    }
                });
            })),
            )
            .child(self.fold("ht-tuning", "FINE-TUNING", tuned, cx, |column, cx| {
                column
                    .child(self.stepper("quick-tap", "Quick tap (tap again to repeat the tap)", Self::optional_ms_default(h.quick_tap_ms, "off"), cx, |this, d, window, cx| {
                this.edit_behavior("Change Quick Tap", window, cx, |kind| {
                    if let BehaviorKind::HoldTap(h) = kind {
                        Self::step_optional_ms(&mut h.quick_tap_ms, d, 25, 150);
                    }
                });
            }))
                    .child(self.stepper("prior-idle", "Only hold after a pause in typing", Self::optional_ms_default(h.require_prior_idle_ms, "off"), cx, |this, d, window, cx| {
                this.edit_behavior("Change Prior Idle", window, cx, |kind| {
                    if let BehaviorKind::HoldTap(h) = kind {
                        Self::step_optional_ms(&mut h.require_prior_idle_ms, d, 25, 125);
                    }
                });
            }))
                    .child(self.toggle("retro-tap", "Tap if held alone and released", h.retro_tap, cx, |kind| {
                        if let BehaviorKind::HoldTap(h) = kind {
                            h.retro_tap = !h.retro_tap;
                        }
                    }))
                    .child(self.toggle("hold-undecided", "Hold while undecided", h.hold_while_undecided, cx, |kind| {
                        if let BehaviorKind::HoldTap(h) = kind {
                            h.hold_while_undecided = !h.hold_while_undecided;
                        }
                    }))
            }))
            .child(self.fold(
                "ht-trigger",
                "KEYS THAT CAN TRIGGER THE HOLD",
                triggers,
                cx,
                |column, cx| {
                    column
                        .child(help("With none chosen, any key can. Click keys to choose, or pick a hand: home-row mods usually hold only for keys on the other hand.", cx).pt_1().pb_3())
                        .child(div()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .pb_2()
                            .child(hands("trigger-any", "Any key", None, cx))
                            .child(hands("trigger-left", "Left hand", Some(false), cx))
                            .child(hands("trigger-right", "Right hand", Some(true), cx)))
                        .child(self.toggle("trigger-release", "Decide on release", h.hold_trigger_on_release, cx, |kind| {
                        if let BehaviorKind::HoldTap(h) = kind {
                            h.hold_trigger_on_release = !h.hold_trigger_on_release;
                        }
                    }))
                        .child(plinth(cx)
                            .mt_4()
                            .h_64()
                            .child(canvas::keyboard(frame, move |b| bounds.set(b)))
                            .on_mouse_down(MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            let hit = canvas::key_at(this.layout_keys(), &[], this.mini_bounds.get(), event.position);
                            let Some(key) = hit else { return };
                            this.edit_behavior("Change Hold Trigger Keys", window, cx, |kind| {
                                if let BehaviorKind::HoldTap(h) = kind {
                                    match h.hold_trigger_key_positions.iter().position(|p| *p == key) {
                                        Some(index) => {
                                            h.hold_trigger_key_positions.remove(index);
                                        }
                                        None => h.hold_trigger_key_positions.push(key),
                                    }
                                    h.hold_trigger_key_positions.sort_unstable();
                                }
                            });
                        })))
                },
            ))
    }

    fn render_tap_dance(&self, id: BehaviorId, t: &TapDance, cx: &mut Context<Self>) -> Div {
        let slots = (0..t.bindings.len())
            .map(|index| {
                let slot = self.slot_chip(
                    Slot::TapDance {
                        behavior: id,
                        index,
                    },
                    ("td-slot", index).into(),
                    cx,
                );
                field(
                    format!("{} tap{}", index + 1, if index == 0 { "" } else { "s" }),
                    "",
                    slot,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .child(
                group("WHAT EACH NUMBER OF TAPS DOES", cx)
                    .children(slots)
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(chip("td-add", "Add a tap", false, cx).on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.edit_behavior("Add Tap", window, cx, |kind| {
                                        if let BehaviorKind::TapDance(t) = kind {
                                            t.bindings.push(Binding::none());
                                        }
                                    });
                                },
                            )))
                            .when(t.bindings.len() > 1, |row| {
                                row.child(chip("td-remove", "Remove the last", false, cx).on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.slot = None;
                                        this.edit_behavior("Remove Tap", window, cx, |kind| {
                                            if let BehaviorKind::TapDance(t) = kind {
                                                t.bindings.pop();
                                            }
                                        });
                                    }),
                                ))
                            })
                            .pt_3(),
                    ),
            )
            .child(group("TIMING", cx).child(self.stepper(
                "td-term",
                "Time allowed between taps",
                format!("{} ms", t.tapping_term_ms),
                cx,
                |this, d, window, cx| {
                    this.edit_behavior("Change Tapping Term", window, cx, |kind| {
                        if let BehaviorKind::TapDance(t) = kind {
                            t.tapping_term_ms = t
                                .tapping_term_ms
                                .saturating_add_signed(d as i32 * 10)
                                .max(10);
                        }
                    });
                },
            )))
    }

    fn modifier_chips(
        &self,
        id: &'static str,
        current: &[Modifier],
        cx: &mut Context<Self>,
        keep: bool,
    ) -> Div {
        let chips = Modifier::ALL
            .into_iter()
            .enumerate()
            .map(|(index, modifier)| {
                let name = modifier.keycode();
                chip((id, index), name, current.contains(&modifier), cx).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.edit_behavior("Change Modifiers", window, cx, |kind| {
                            if let BehaviorKind::ModMorph(m) = kind {
                                let list = if keep { &mut m.keep_mods } else { &mut m.mods };
                                match list.iter().position(|x| *x == modifier) {
                                    Some(at) => {
                                        list.remove(at);
                                    }
                                    None => list.push(modifier),
                                }
                            }
                        });
                    },
                ))
            });
        div().flex().flex_wrap().gap_1().children(chips)
    }

    fn render_mod_morph(&self, id: BehaviorId, m: &ModMorph, cx: &mut Context<Self>) -> Div {
        let row = |label: &'static str, morphed: bool, this: &Self, cx: &mut Context<Self>| {
            let slot = this.slot_chip(
                Slot::ModMorph {
                    behavior: id,
                    morphed,
                },
                ("mm-slot", morphed as usize).into(),
                cx,
            );
            field(label, "", slot, cx)
        };
        let kept = match m.keep_mods.len() {
            0 => "None".to_string(),
            1 => "1 modifier".to_string(),
            n => format!("{n} modifiers"),
        };
        div()
            .flex()
            .flex_col()
            .child(
                group("WHAT IT SENDS", cx)
                    .child(row("Normally", false, self, cx))
                    .child(row("With the modifiers held", true, self, cx)),
            )
            .child(
                group("MODIFIERS THAT SWITCH IT", cx)
                    .child(self.modifier_chips("mm-mods", &m.mods, cx, false).pt_2()),
            )
            .child(self.fold(
                "mm-keep",
                "MODIFIERS PASSED ON TO THE SECOND BINDING",
                kept,
                cx,
                |column, cx| {
                    column.child(
                        self.modifier_chips("mm-keep", &m.keep_mods, cx, true)
                            .pt_3(),
                    )
                },
            ))
    }

    fn render_sticky(&self, s: &StickyKey, cx: &mut Context<Self>) -> Div {
        let tuned = [s.quick_release, s.lazy, !s.ignore_modifiers]
            .into_iter()
            .filter(|changed| *changed)
            .count();
        let tuned = match tuned {
            0 => "All as they come".to_string(),
            n => format!("{n} changed"),
        };
        let wraps = self.wrap_chips("sticky-wrap", &s.behavior, cx, |kind, behavior| {
            if let BehaviorKind::StickyKey(s) = kind {
                s.behavior = behavior;
            }
        });
        div()
            .flex()
            .flex_col()
            .child(
                group("WHAT IT DOES", cx)
                    .child(field("What is made sticky", "", wraps, cx))
                    .child(self.stepper(
                        "sticky-release",
                        "Release after",
                        format!("{} ms", s.release_after_ms),
                        cx,
                        |this, d, window, cx| {
                            this.edit_behavior("Change Release Time", window, cx, |kind| {
                                if let BehaviorKind::StickyKey(s) = kind {
                                    s.release_after_ms = s
                                        .release_after_ms
                                        .saturating_add_signed(d as i32 * 100)
                                        .max(100);
                                }
                            });
                        },
                    )),
            )
            .child(
                self.fold("sk-tuning", "FINE-TUNING", tuned, cx, |column, cx| {
                    column
                        .child(self.toggle(
                            "sticky-quick",
                            "Release as soon as the next key is pressed",
                            s.quick_release,
                            cx,
                            |kind| {
                                if let BehaviorKind::StickyKey(s) = kind {
                                    s.quick_release = !s.quick_release;
                                }
                            },
                        ))
                        .child(self.toggle(
                            "sticky-lazy",
                            "Press only when the next key is",
                            s.lazy,
                            cx,
                            |kind| {
                                if let BehaviorKind::StickyKey(s) = kind {
                                    s.lazy = !s.lazy;
                                }
                            },
                        ))
                        .child(self.toggle(
                            "sticky-ignore",
                            "Let modifiers through",
                            s.ignore_modifiers,
                            cx,
                            |kind| {
                                if let BehaviorKind::StickyKey(s) = kind {
                                    s.ignore_modifiers = !s.ignore_modifiers;
                                }
                            },
                        ))
                }),
            )
    }

    fn render_macro(&self, id: BehaviorId, m: &Macro, cx: &mut Context<Self>) -> Div {
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
                        format!("Pass parameter {from} to the next binding's parameter {to}"),
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
                let act = |id: &'static str,
                           label: &'static str,
                           cx: &mut Context<Self>,
                           edit: fn(&mut Vec<MacroStep>, usize)| {
                    chip((id, step), label, false, cx).on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.slot = None;
                            this.edit_behavior("Change Macro", window, cx, |kind| {
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

        let add = |id: &'static str,
                   label: &'static str,
                   cx: &mut Context<Self>,
                   make: fn() -> MacroStep| {
            chip(id, label, false, cx).on_click(cx.listener(move |this, _, window, cx| {
                this.edit_behavior("Add Macro Step", window, cx, |kind| {
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
                    this.edit_behavior("Change Macro Parameters", window, cx, |kind| {
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
                        .child(add("macro-add-param", "Pass parameter", cx, || {
                            MacroStep::Param { from: 1, to: 1 }
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
            .child(
                self.fold("macro-timing", "TIMING", timed, cx, |column, cx| {
                    column
                        .child(self.stepper(
                            "macro-wait",
                            "Wait between steps",
                            Self::optional_ms_default(m.wait_ms, "default"),
                            cx,
                            |this, d, window, cx| {
                                this.edit_behavior("Change Macro Timing", window, cx, |kind| {
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
                            |this, d, window, cx| {
                                this.edit_behavior("Change Macro Timing", window, cx, |kind| {
                                    if let BehaviorKind::Macro(m) = kind {
                                        Self::step_optional_ms(&mut m.tap_ms, d, 5, 30);
                                    }
                                });
                            },
                        ))
                }),
            )
            .child(self.fold(
                "macro-params",
                "PARAMETERS A KEY PASSES TO THIS MACRO",
                passed,
                cx,
                |column, _| column.child(div().flex().gap_1().children(params).pt_2()),
            ))
    }

    pub(super) fn render_behaviors(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let form = div()
            .id("behavior-form")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .px_8()
            .py_6();
        let column = div().w_full().max_w(px(760.)).flex().flex_col();
        let selected = self.behavior.and_then(|id| self.project().behavior(id));
        let column = match selected {
            None => column.child(help("Behaviors are keys with more than one job: a hold-tap does one thing when held and another when tapped, a tap-dance counts taps, a macro plays a sequence. Create one on the left, then assign it from the Custom tab of the key picker.", cx)),
            Some(def) => {
                let id = def.id;
                let uses = self.project().behavior_references(id).len();
                let body = match &def.kind {
                    BehaviorKind::HoldTap(h) => self.render_hold_tap(h, cx),
                    BehaviorKind::TapDance(t) => self.render_tap_dance(id, t, cx),
                    BehaviorKind::ModMorph(m) => self.render_mod_morph(id, m, cx),
                    BehaviorKind::StickyKey(s) => self.render_sticky(s, cx),
                    BehaviorKind::Macro(m) => self.render_macro(id, m, cx),
                };
                // First, what kind of behavior this is and what that means.
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
                                    .w_64()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(section_title("NAME", cx))
                                    .child(Input::new(&self.behavior_name)),
                            )
                            .child(
                                div()
                                    .w_48()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(section_title("LABEL IN THE KEYMAP", cx))
                                    .child(Input::new(&self.behavior_label)),
                            )
                            .child(div().flex_1())
                            .child(div().pb_1().text_sm().text_color(muted).child(match uses {
                                0 => "Not used yet".to_string(),
                                1 => "Used in 1 place".to_string(),
                                n => format!("Used in {n} places"),
                            }))
                            .child(chip("delete-behavior", "Delete", false, cx).on_click(
                                cx.listener(|this, _, window, cx| this.delete_behavior(window, cx)),
                            )),
                    )
                    .when(!def.description.is_empty(), |column| {
                        column.child(
                            group("DESCRIPTION", cx)
                                .child(help(def.description.clone(), cx).pt_1()),
                        )
                    })
                    .child(body)
            }
        };
        let form = form.child(column);
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(self.render_behavior_list(cx))
            .child(form)
    }
}
