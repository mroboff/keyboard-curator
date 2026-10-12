//! The key setup panel: the selected key described in slots (tap, hold,
//! double tap, tap then hold, more taps, with ⇧ held, sticky) with its
//! feel, in the spirit of Oryx. Every slot is filled from the picker;
//! nothing is typed. The model's `KeySetup` does the storage.

use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_model::behavior::{
    BehaviorKind, Flavor, HoldTapSettings, MacroSettings, ModMorphSettings, Settings,
    StickyKeySettings, TapDanceSettings,
};
use kc_model::keycap::keycap;
use kc_model::setup::{Choice, KeySetup, Part, SetupError};
use kc_model::{edit, words, BehaviorId, Binding, LayerId, ModelError};
use kc_zmk::Modifier;

use super::{
    badge, chip, display, field, group, help, plinth, section_title, Mode, Workspace, MODIFIERS,
};
use crate::canvas::{self, Frame, Palette};

/// One slot of a key, as the picker can be aimed at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeySlot {
    Tap,
    Hold,
    DoubleTap,
    TapHold,
    DoubleTapHold,
    /// The third tap onward, counted from zero.
    MoreTap(usize),
    Shifted,
}

impl KeySlot {
    fn label(self) -> String {
        match self {
            KeySlot::Tap => "Tap".into(),
            KeySlot::Hold => "Hold".into(),
            KeySlot::DoubleTap => "Double tap".into(),
            KeySlot::TapHold => "Tap, then hold".into(),
            KeySlot::DoubleTapHold => "Double tap, then hold".into(),
            KeySlot::MoreTap(n) => format!("{} taps", n + 3),
            KeySlot::Shifted => "With ⇧ held".into(),
        }
    }

    /// A number that tells the slots apart in element IDs.
    fn index(self) -> usize {
        match self {
            KeySlot::Tap => 0,
            KeySlot::Hold => 1,
            KeySlot::DoubleTap => 2,
            KeySlot::TapHold => 3,
            KeySlot::Shifted => 4,
            KeySlot::DoubleTapHold => 5,
            KeySlot::MoreTap(n) => 6 + n,
        }
    }

    fn id(self) -> ElementId {
        ("slot", self.index()).into()
    }

    fn get(self, setup: &KeySetup) -> Option<&Binding> {
        match self {
            KeySlot::Tap => Some(&setup.tap),
            KeySlot::Hold => setup.hold.as_ref(),
            KeySlot::DoubleTap => setup.double_tap.as_ref(),
            KeySlot::TapHold => setup.tap_hold.as_ref(),
            KeySlot::DoubleTapHold => setup.double_tap_hold.as_ref(),
            KeySlot::MoreTap(n) => setup.more_taps.get(n),
            KeySlot::Shifted => setup.shifted.as_ref(),
        }
    }

    fn set(self, setup: &mut KeySetup, binding: Binding) {
        match self {
            KeySlot::Tap => setup.tap = binding,
            KeySlot::Hold => setup.hold = Some(binding),
            KeySlot::DoubleTap => setup.double_tap = Some(binding),
            KeySlot::TapHold => setup.tap_hold = Some(binding),
            KeySlot::DoubleTapHold => setup.double_tap_hold = Some(binding),
            KeySlot::MoreTap(n) => {
                while setup.more_taps.len() <= n {
                    setup.more_taps.push(Binding::none());
                }
                setup.more_taps[n] = binding;
            }
            KeySlot::Shifted => setup.shifted = Some(binding),
        }
    }

    fn clear(self, setup: &mut KeySetup) {
        match self {
            KeySlot::Tap => {}
            KeySlot::Hold => setup.hold = None,
            KeySlot::DoubleTap => setup.double_tap = None,
            KeySlot::TapHold => setup.tap_hold = None,
            KeySlot::DoubleTapHold => setup.double_tap_hold = None,
            KeySlot::MoreTap(n) => {
                if n < setup.more_taps.len() {
                    setup.more_taps.remove(n);
                }
            }
            KeySlot::Shifted => setup.shifted = None,
        }
    }
}

/// What choosing from a menu does.
type Pick = Rc<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// Where a feel form's changes go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FeelTarget {
    /// The selected keys' own settings for one part of their feel.
    Keys(Part),
    /// A template, and every key that follows it.
    Template(BehaviorId),
}

/// Whether a binding is a whole key on its own, which replaces the key
/// rather than filling its tap.
fn whole_key(binding: &Binding) -> bool {
    match binding {
        Binding::Raw { .. } => true,
        Binding::Behavior { behavior, .. } => {
            *behavior == kc_model::BehaviorRef::built_in("trans")
                || *behavior == kc_model::BehaviorRef::built_in("none")
        }
    }
}

fn is_momentary_layer(binding: &Binding) -> Option<LayerId> {
    match binding {
        Binding::Behavior { behavior, params }
            if *behavior == kc_model::BehaviorRef::built_in("mo") =>
        {
            match params.as_slice() {
                [kc_model::Param::Layer(id)] => Some(*id),
                _ => None,
            }
        }
        _ => None,
    }
}

impl Workspace {
    /// The slot the picker fills on the selected key, if one was chosen.
    fn aimed_slot(&self) -> Option<KeySlot> {
        let primary = self.primary()?;
        self.key_slot
            .filter(|(position, _)| *position == primary)
            .map(|(_, slot)| slot)
    }

    /// A slot added to the selected key but not filled yet.
    fn pending_slot(&self) -> Option<KeySlot> {
        let primary = self.primary()?;
        self.pending_slot
            .filter(|(position, _)| *position == primary)
            .map(|(_, slot)| slot)
    }

    fn aim_slot(&mut self, slot: Option<KeySlot>, cx: &mut Context<Self>) {
        self.key_slot = self.primary().zip(slot);
        self.slot = None;
        cx.notify();
    }

    /// The selected key as slots.
    fn setup(&self) -> Option<KeySetup> {
        KeySetup::of(self.project(), self.layer, self.primary()?)
    }

    /// Changes every selected key's slots the same way, as one undo step.
    pub(super) fn edit_setups(
        &mut self,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl Fn(&mut KeySetup),
    ) {
        let (layer, targets) = (self.layer, self.selection.clone());
        let mut failed: Option<SetupError> = None;
        let result = self.editor.edit(label, |p| {
            for position in &targets {
                let Some(mut setup) = KeySetup::of(p, layer, *position) else {
                    continue;
                };
                change(&mut setup);
                if let Err(error) = p.apply_setup(layer, *position, &setup) {
                    failed = Some(error);
                    return Err(ModelError::NoSuchSlot);
                }
            }
            Ok(())
        });
        match (failed, result) {
            (Some(error), _) => self.notice = Some(format!("{error}.")),
            (None, Err(error)) => self.notice = Some(format!("{error}.")),
            _ => {}
        }
        self.after_change(window, cx);
    }

    /// Puts a picker entry into the aimed slot of every selected key, or
    /// into the tap when no slot is aimed at.
    pub(super) fn pick_into_slots(
        &mut self,
        picked: Binding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let aimed = self.aimed_slot().or_else(|| self.pending_slot());
        let slot = aimed.unwrap_or(KeySlot::Tap);
        // A whole key chosen with nothing aimed at replaces the key, extras
        // and all; so does anything chosen for a key not shown as slots.
        let replace = aimed.is_none() && (whole_key(&picked) || self.setup().is_none());
        self.key_slot = None;
        self.pending_slot = None;
        if replace {
            self.apply_binding(picked, self.auto_advance, window, cx);
            return;
        }
        let advance = aimed.is_none() && self.auto_advance;
        self.edit_setups("Change Key", window, cx, |setup| {
            slot.set(setup, picked.clone());
        });
        if let (true, [position]) = (advance, self.selection.as_slice()) {
            self.selection = vec![(position + 1) % self.project().key_count.max(1)];
            self.after_change(window, cx);
        }
    }

    fn clear_slot(&mut self, slot: KeySlot, window: &mut Window, cx: &mut Context<Self>) {
        self.key_slot = None;
        self.pending_slot = None;
        self.edit_setups("Remove From Key", window, cx, |setup| slot.clear(setup));
    }

    /// Adds a slot to the key: it shows empty, aimed at, until something
    /// is chosen for it.
    fn add_slot(&mut self, slot: KeySlot, cx: &mut Context<Self>) {
        self.pending_slot = self.primary().map(|p| (p, slot));
        self.key_slot = self.pending_slot;
        cx.notify();
    }

    /// Saves the named part of the selected key's feel as a template and
    /// makes every selected key use it.
    pub(super) fn save_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.template_name.read(cx).value().trim().to_string();
        if let Some(id) = self.naming_macro {
            if name.is_empty() {
                self.notice = Some("Give the template a name.".into());
                cx.notify();
                return;
            }
            if let Err(error) = self
                .editor
                .edit("Save Template", |p| p.make_template(id, name))
            {
                self.notice = Some(format!("{error}."));
            }
            self.naming_macro = None;
            self.template_name
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.after_change(window, cx);
            return;
        }
        let Some(part) = self.naming else {
            return;
        };
        let Some(position) = self.primary() else {
            return;
        };
        if name.is_empty() {
            self.notice = Some("Give the template a name.".into());
            cx.notify();
            return;
        }
        let layer = self.layer;
        let mut failed: Option<SetupError> = None;
        let saved = self.editor.edit("Save Template", |p| {
            p.save_as_template(layer, position, part, name)
                .map_err(|e| {
                    failed = Some(e);
                    ModelError::NoSuchSlot
                })
        });
        match (saved, failed) {
            (Ok(id), _) => {
                self.naming = None;
                self.template_name
                    .update(cx, |input, cx| input.set_value("", window, cx));
                if self.selection.len() > 1 {
                    self.edit_setups("Use Template", window, cx, |setup| {
                        set_choice(setup, part, Choice::Template(id));
                    });
                }
            }
            (Err(_), Some(error)) => self.notice = Some(format!("{error}.")),
            (Err(error), None) => self.notice = Some(format!("{error}.")),
        }
        self.after_change(window, cx);
    }

    /// The panel for the selected key.
    pub(super) fn render_key_setup(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let panel = div()
            .id("key-setup")
            .w_96()
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .border_l_1()
            .border_color(border)
            .overflow_y_scrollbar();
        let (Some(position), Some(binding)) = (self.primary(), self.selected_binding()) else {
            return panel.child(section_title("NO KEY SELECTED", cx)).child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("Click a key to set it up."),
            );
        };

        let mut panel = panel.child(self.render_specimen(position, cx));
        let Some(setup) = self.setup() else {
            // Raw and hand-written bindings have no slots.
            return panel
                .child(section_title("SET BY HAND", cx))
                .child(div().text_sm().text_color(muted).child(
                    "This key is written as keymap text, so it has no slots. Choose a key below to start over, or edit the text.",
                ))
                .child(self.render_key_advanced(binding, true, cx));
        };

        panel = panel.child(self.render_slots(&setup, cx));
        if let Some(tap) = edit::tap_key(&setup.tap) {
            let modifiers = MODIFIERS.map(|(modifier, symbol)| {
                let active = tap.mods.contains(&modifier);
                chip(("tap-modifier", modifier as usize), symbol, active, cx).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.edit_setups("Change Modifiers", window, cx, |setup| {
                            if let Some(toggled) = edit::toggle_modifier(&setup.tap, modifier) {
                                setup.tap = toggled;
                            }
                        });
                    },
                ))
            });
            panel = panel.child(field(
                "With modifiers",
                "",
                div().flex().gap_1().children(modifiers),
                cx,
            ));
        }
        if setup.has_hold() {
            panel = panel.child(self.render_hold_feel(&setup, cx));
        }
        if setup.has_taps() {
            panel = panel.child(self.render_taps_feel(&setup, cx));
        }
        if setup.shifted.is_some() {
            panel = panel.child(self.render_shifted_feel(&setup, cx));
        }
        if setup.sticky {
            panel = panel.child(self.render_sticky_feel(&setup, cx));
        }
        if let Some(section) = self.render_macro_section(cx) {
            panel = panel.child(section);
        }
        if !setup.note.is_empty() {
            panel = panel.child(
                div()
                    .pt_3()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("Note: {}", setup.note)),
            );
        }
        panel.child(self.render_key_advanced(binding, false, cx))
    }

    /// The key drawn as a keycap, with its number.
    fn render_specimen(&self, position: usize, cx: &mut Context<Self>) -> Div {
        let colors = crate::theme::look(cx).colors;
        let muted = cx.theme().muted_foreground;
        let shown = keycap(self.project(), self.layer, position);
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .size_12()
                    .flex_shrink_0()
                    .rounded_lg()
                    .bg(colors.key_lip)
                    .border_1()
                    .border_color(colors.key_border)
                    .child(
                        div()
                            .h_10()
                            .rounded_lg()
                            .bg(colors.key)
                            .text_color(colors.key_text)
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .when_some(shown, |face, cap| {
                                face.child(display(cap.legend, 14., cx)).when_some(
                                    cap.hold,
                                    |face, hold| {
                                        face.child(
                                            div()
                                                .text_size(px(9.))
                                                .text_color(colors.key_text.opacity(0.7))
                                                .child(hold),
                                        )
                                    },
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(display(
                        match self.selection.len() {
                            1 => format!("Key {position}"),
                            n => format!("{n} keys"),
                        },
                        20.,
                        cx,
                    ))
                    .when(self.selection.len() > 1, |column| {
                        column.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child("Changes apply to all of them."),
                        )
                    }),
            )
    }

    /// A row for one slot: its name and what it holds. Clicking the value
    /// aims the picker at the slot.
    fn slot_row(
        &self,
        slot: KeySlot,
        binding: Option<&Binding>,
        removable: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let aimed = self.aimed_slot() == Some(slot);
        let text = match binding {
            Some(binding) => words::describe(self.project(), binding),
            None => "Choose below…".to_string(),
        };
        let value =
            chip(slot.id(), text, aimed, cx).on_click(cx.listener(move |this, _, _, cx| {
                let next = if this.aimed_slot() == Some(slot) {
                    None
                } else {
                    Some(slot)
                };
                this.aim_slot(next, cx);
            }));
        let control =
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(value)
                .when(removable, |row| {
                    row.child(chip(("slot-clear", slot.index()), "×", false, cx).on_click(
                        cx.listener(move |this, _, window, cx| {
                            this.clear_slot(slot, window, cx);
                        }),
                    ))
                });
        field(slot.label(), "", control, cx)
    }

    fn render_slots(&self, setup: &KeySetup, cx: &mut Context<Self>) -> Div {
        let pending = self.pending_slot();
        let mut rows = div().flex().flex_col();
        rows = rows.child(self.slot_row(KeySlot::Tap, Some(&setup.tap), false, cx));
        // The sticky toggle sits with the tap: a modifier or a layer can
        // stay on for the next key.
        let can_stick = setup.sticky
            || edit::modifier_set(&setup.tap).is_some()
            || is_momentary_layer(&setup.tap).is_some();
        if can_stick {
            let on = setup.sticky;
            rows = rows.child(field(
                "Sticky",
                "Stays active for the next key.",
                chip("slot-sticky", if on { "On" } else { "Off" }, on, cx).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.edit_setups("Change Sticky", window, cx, |setup| {
                            setup.sticky = !on;
                        });
                    },
                )),
                cx,
            ));
        }
        let mut shown: Vec<KeySlot> = Vec::new();
        for slot in [
            KeySlot::Hold,
            KeySlot::DoubleTap,
            KeySlot::TapHold,
            KeySlot::DoubleTapHold,
        ] {
            if slot.get(setup).is_some() || pending == Some(slot) {
                shown.push(slot);
            }
        }
        for n in 0..setup.more_taps.len() {
            shown.push(KeySlot::MoreTap(n));
        }
        if let Some(KeySlot::MoreTap(n)) = pending {
            if n >= setup.more_taps.len() {
                shown.push(KeySlot::MoreTap(n));
            }
        }
        if setup.shifted.is_some() || pending == Some(KeySlot::Shifted) {
            shown.push(KeySlot::Shifted);
        }
        for slot in &shown {
            rows = rows.child(self.slot_row(*slot, slot.get(setup), true, cx));
            if *slot == KeySlot::Hold && self.aimed_slot() == Some(KeySlot::Hold) {
                rows = rows.child(self.render_hold_choices(setup.hold.as_ref(), cx));
            }
        }

        // What can still be added.
        let mut additions: Vec<(&'static str, KeySlot)> = Vec::new();
        if !shown.contains(&KeySlot::Hold) {
            additions.push(("Hold", KeySlot::Hold));
        }
        if !shown.contains(&KeySlot::DoubleTap) {
            additions.push(("Double tap", KeySlot::DoubleTap));
        }
        if !shown.contains(&KeySlot::TapHold) {
            additions.push(("Tap, then hold", KeySlot::TapHold));
        }
        if !shown.contains(&KeySlot::DoubleTapHold) {
            additions.push(("Double tap, then hold", KeySlot::DoubleTapHold));
        }
        if setup.has_taps() {
            additions.push(("Another tap", KeySlot::MoreTap(setup.more_taps.len())));
        }
        if !shown.contains(&KeySlot::Shifted) {
            additions.push(("With ⇧ held", KeySlot::Shifted));
        }
        let workspace = cx.weak_entity();
        let has_macro = self.key_macro().is_some();
        let add = Button::new("slot-add")
            .label("Add…")
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for (label, slot) in &additions {
                    let (workspace, slot) = (workspace.clone(), *slot);
                    menu = menu.item(PopupMenuItem::new(*label).on_click(move |_, _, cx| {
                        let _ = workspace.update(cx, |this, cx| this.add_slot(slot, cx));
                    }));
                }
                if !has_macro {
                    let workspace = workspace.clone();
                    menu = menu.item(PopupMenuItem::new("Macro").on_click(move |_, window, cx| {
                        let _ = workspace.update(cx, |this, cx| this.make_key_macro(window, cx));
                    }));
                }
                menu
            });
        rows.child(div().flex().pt_2().child(add))
    }

    /// Quick choices for the hold: modifiers to combine, and layers.
    fn render_hold_choices(&self, hold: Option<&Binding>, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let held: Vec<Modifier> = hold.and_then(edit::modifier_set).unwrap_or_default();
        let right = self.right_mods;
        let modifiers = MODIFIERS.map(|(modifier, symbol)| {
            let modifier = if right {
                match modifier {
                    Modifier::LCtrl => Modifier::RCtrl,
                    Modifier::LAlt => Modifier::RAlt,
                    Modifier::LShift => Modifier::RShift,
                    Modifier::LGui => Modifier::RGui,
                    other => other,
                }
            } else {
                modifier
            };
            let active = held.contains(&modifier);
            chip(("hold-modifier", modifier as usize), symbol, active, cx).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.edit_setups("Change Hold", window, cx, |setup| {
                        let mut set = setup
                            .hold
                            .as_ref()
                            .and_then(edit::modifier_set)
                            .unwrap_or_default();
                        match set.iter().position(|m| *m == modifier) {
                            Some(at) => {
                                set.remove(at);
                            }
                            None => set.push(modifier),
                        }
                        setup.hold = edit::modifiers_binding(&set);
                    });
                    this.pending_slot = None;
                },
            ))
        });
        let side = chip(
            "hold-right",
            if right { "Right side" } else { "Left side" },
            false,
            cx,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.right_mods = !this.right_mods;
            cx.notify();
        }));
        let current_layer = hold.and_then(is_momentary_layer);
        let layers = self
            .project()
            .layers
            .iter()
            .filter(|l| l.id != self.layer)
            .map(|l| (l.id, l.name.clone()))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(id, name)| {
                chip(
                    ("hold-layer", id.0 as usize),
                    name,
                    current_layer == Some(id),
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.edit_setups("Change Hold", window, cx, |setup| {
                        setup.hold = Some(Binding::layer("mo", id));
                    });
                    this.pending_slot = None;
                }))
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .py_2()
            .child(
                div().text_xs().text_color(muted).child(
                    "Modifiers combine; a layer stands alone. Anything else: choose it below.",
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .children(modifiers)
                    .child(side),
            )
            .child(div().flex().flex_wrap().gap_1().children(layers))
    }

    /// The menu choosing where a part's feel comes from.
    fn feel_menu(
        &self,
        id: &'static str,
        part: Part,
        choice_name: String,
        templates: Vec<(BehaviorId, String)>,
        custom: impl Fn(&KeySetup) -> Settings + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let workspace = cx.weak_entity();
        let custom = Rc::new(custom);
        let mut options: Vec<(String, Pick)> = Vec::new();
        // A hold or a sticky key has a built-in with the keyboard's own
        // feel; extra taps and overrides always need settings of their own.
        if matches!(part, Part::Hold | Part::Sticky) {
            options.push((
                "Keyboard default".into(),
                Rc::new(move |this, window, cx| {
                    this.edit_setups("Change Feel", window, cx, |setup| {
                        set_choice_default(setup, part);
                    });
                }),
            ));
        }
        for (template, name) in templates {
            options.push((
                name,
                Rc::new(move |this, window, cx| {
                    this.edit_setups("Use Template", window, cx, |setup| {
                        set_choice(setup, part, Choice::Template(template));
                    });
                }),
            ));
        }
        options.push((
            "Custom for this key".into(),
            Rc::new(move |this, window, cx| {
                let custom = custom.clone();
                this.edit_setups("Change Feel", window, cx, move |setup| {
                    let settings = custom(setup);
                    set_custom(setup, part, settings);
                });
            }),
        ));
        Button::new(id)
            .label(choice_name)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for (label, pick) in &options {
                    let (workspace, pick) = (workspace.clone(), pick.clone());
                    menu = menu.item(PopupMenuItem::new(label.clone()).on_click(
                        move |_, window, cx| {
                            let _ = workspace.update(cx, |this, cx| pick(this, window, cx));
                        },
                    ));
                }
                menu
            })
    }

    /// The row under a feel: using a template, saving one, detaching.
    fn template_row(
        &self,
        part: Part,
        template: Option<BehaviorId>,
        cx: &mut Context<Self>,
    ) -> Div {
        let muted = cx.theme().muted_foreground;
        let mut row = div().flex().flex_wrap().items_center().gap_2().py_2();
        match template {
            Some(id) => {
                let name = self
                    .project()
                    .behavior(id)
                    .map(|d| d.name.clone())
                    .unwrap_or_default();
                let settings = self
                    .project()
                    .behavior(id)
                    .map(|d| d.kind.settings().summary())
                    .unwrap_or_default();
                row = row
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("Uses the template “{name}”: {settings}.")),
                    )
                    .child(
                        chip(("detach", part as usize), "Detach", false, cx).on_click(cx.listener(
                            move |this, _, window, cx| {
                                let settings =
                                    this.project().behavior(id).map(|d| d.kind.settings());
                                this.edit_setups("Detach From Template", window, cx, |setup| {
                                    if let Some(settings) = &settings {
                                        set_custom(setup, part, settings.clone());
                                    }
                                });
                            },
                        )),
                    );
            }
            None if self.naming == Some(part) => {
                row = row
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.template_name)),
                    )
                    .child(chip("template-save", "Save", false, cx).on_click(
                        cx.listener(|this, _, window, cx| this.save_template(window, cx)),
                    ))
                    .child(
                        chip("template-cancel", "Cancel", false, cx).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.naming = None;
                                cx.notify();
                            },
                        )),
                    );
            }
            None => {
                row = row.child(
                    chip(
                        ("save-template", part as usize),
                        "Save as template…",
                        false,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.naming = Some(part);
                        this.template_name
                            .read(cx)
                            .focus_handle(cx)
                            .focus(window, cx);
                        cx.notify();
                    })),
                );
            }
        }
        row
    }

    fn templates_of(&self, matches: fn(&BehaviorKind) -> bool) -> Vec<(BehaviorId, String)> {
        self.project()
            .behavior_templates()
            .filter(|d| matches(&d.kind))
            .map(|d| (d.id, d.name.clone()))
            .collect()
    }

    fn choice_name<S>(&self, choice: &Choice<S>) -> String {
        match choice {
            Choice::Default => "Keyboard default".into(),
            Choice::Template(id) => self
                .project()
                .behavior(*id)
                .map_or_else(|| "A missing template".into(), |d| d.name.clone()),
            Choice::Custom(_) => "Custom for this key".into(),
        }
    }

    /// A setting that is on or off, changed where the feel lives.
    fn setup_toggle(
        &self,
        id: &'static str,
        label: &'static str,
        on: bool,
        target: FeelTarget,
        cx: &mut Context<Self>,
        flip: fn(&mut Settings),
    ) -> Div {
        let control = chip(id, if on { "On" } else { "Off" }, on, cx).on_click(cx.listener(
            move |this, _, window, cx| {
                this.edit_feel(target, "Change Feel", window, cx, flip);
            },
        ));
        field(label, "", control, cx)
    }

    /// Changes a feel where it lives: the selected keys' own settings, or
    /// a template and every key that follows it.
    pub(super) fn edit_feel(
        &mut self,
        target: FeelTarget,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl Fn(&mut Settings),
    ) {
        match target {
            FeelTarget::Keys(part) => self.edit_setups(label, window, cx, |setup| {
                if let Some(mut settings) = custom_settings(setup, part) {
                    change(&mut settings);
                    set_custom(setup, part, settings);
                }
            }),
            FeelTarget::Template(id) => self.change(label, window, cx, |p| {
                let mut settings = p
                    .behavior(id)
                    .ok_or(ModelError::NoSuchBehavior(id))?
                    .kind
                    .settings();
                change(&mut settings);
                p.set_template_settings(id, &settings)
            }),
        }
    }

    /// The form for one kind of settings, wherever they live.
    pub(super) fn render_settings(
        &self,
        target: FeelTarget,
        settings: &Settings,
        cx: &mut Context<Self>,
    ) -> Div {
        match settings {
            Settings::HoldTap(h) => self.render_hold_tap_settings(target, h, cx),
            Settings::TapDance(t) => self.render_tap_dance_settings(target, t, cx),
            Settings::ModMorph(m) => self.render_mod_morph_settings(target, m, cx),
            Settings::StickyKey(s) => self.render_sticky_settings(target, s, cx),
            Settings::Macro(m) => self.render_macro_settings(target, m, cx),
        }
    }

    fn render_hold_feel(&self, setup: &KeySetup, cx: &mut Context<Self>) -> Div {
        let templates = self.templates_of(|k| matches!(k, BehaviorKind::HoldTap(_)));
        let menu = self.feel_menu(
            "hold-feel",
            Part::Hold,
            self.choice_name(&setup.feel.hold),
            templates,
            |setup| match &setup.feel.hold {
                Choice::Custom(s) => Settings::HoldTap(s.clone()),
                _ => Settings::HoldTap(HoldTapSettings::default()),
            },
            cx,
        );
        let section = group("HOLD AND TAP", cx)
            .child(field("How it feels", "", menu, cx))
            .child(self.template_row(Part::Hold, setup.feel.hold.template(), cx));
        match &setup.feel.hold {
            Choice::Custom(h) => {
                section.child(self.render_hold_tap_settings(FeelTarget::Keys(Part::Hold), h, cx))
            }
            _ => section,
        }
    }

    fn render_hold_tap_settings(
        &self,
        target: FeelTarget,
        h: &HoldTapSettings,
        cx: &mut Context<Self>,
    ) -> Div {
        let flavor_options = Flavor::ALL
            .into_iter()
            .map(|flavor| {
                let pick: Pick = Rc::new(move |this, window, cx| {
                    this.edit_feel(target, "Change Feel", window, cx, move |s| {
                        if let Settings::HoldTap(h) = s {
                            h.flavor = flavor;
                        }
                    });
                });
                (flavor.name(), flavor == h.flavor, pick)
            })
            .collect();
        let flavor = self.select("hold-flavor", h.flavor.name(), flavor_options, cx);
        let tuned = [
            h.quick_tap_ms.is_some(),
            h.require_prior_idle_ms.is_some(),
            h.retro_tap,
            h.hold_while_undecided,
            h.opposite_hand_hold,
            !h.hold_trigger_key_positions.is_empty(),
        ]
        .into_iter()
        .filter(|changed| *changed)
        .count();
        let summary = match tuned {
            0 => "All as they come".to_string(),
            n => format!("{n} changed"),
        };
        let (quick, idle) = (h.quick_tap_ms, h.require_prior_idle_ms);
        let (retro, undecided, other_hand, on_release) = (
            h.retro_tap,
            h.hold_while_undecided,
            h.opposite_hand_hold,
            h.hold_trigger_on_release,
        );
        let positions = h.hold_trigger_key_positions.clone();
        div()
            .flex()
            .flex_col()
            .child(field("Decide by", h.flavor.detail(), flavor, cx))
            .child(self.stepper(
                "hold-term",
                "Counts as a hold after",
                format!("{} ms", h.tapping_term_ms),
                cx,
                move |this, d, window, cx| {
                    this.edit_feel(target, "Change Hold Time", window, cx, move |s| {
                        if let Settings::HoldTap(h) = s {
                            h.tapping_term_ms = h
                                .tapping_term_ms
                                .saturating_add_signed(d as i32 * 10)
                                .max(10);
                        }
                    });
                },
            ))
            .child(
                self.fold("hold-more", "MORE", summary, cx, move |column, cx| {
                    column
                        .child(self.stepper(
                            "hold-quick-tap",
                            "Tap twice quickly to repeat",
                            optional_ms(quick, "off"),
                            cx,
                            move |this, d, window, cx| {
                                this.edit_feel(target, "Change Feel", window, cx, move |s| {
                                    if let Settings::HoldTap(h) = s {
                                        step_optional_ms(&mut h.quick_tap_ms, d, 25, 150);
                                    }
                                });
                            },
                        ))
                        .child(self.stepper(
                            "hold-prior-idle",
                            "No hold right after typing",
                            optional_ms(idle, "off"),
                            cx,
                            move |this, d, window, cx| {
                                this.edit_feel(target, "Change Feel", window, cx, move |s| {
                                    if let Settings::HoldTap(h) = s {
                                        step_optional_ms(&mut h.require_prior_idle_ms, d, 25, 125);
                                    }
                                });
                            },
                        ))
                        .child(self.setup_toggle(
                            "hold-retro",
                            "Tap if released with nothing else pressed",
                            retro,
                            target,
                            cx,
                            |s| {
                                if let Settings::HoldTap(h) = s {
                                    h.retro_tap = !h.retro_tap;
                                }
                            },
                        ))
                        .child(self.setup_toggle(
                            "hold-undecided",
                            "Hold starts right away",
                            undecided,
                            target,
                            cx,
                            |s| {
                                if let Settings::HoldTap(h) = s {
                                    h.hold_while_undecided = !h.hold_while_undecided;
                                }
                            },
                        ))
                        .child(self.setup_toggle(
                            "hold-other-hand",
                            "Hold only with keys on the other hand",
                            other_hand,
                            target,
                            cx,
                            |s| {
                                if let Settings::HoldTap(h) = s {
                                    h.opposite_hand_hold = !h.opposite_hand_hold;
                                }
                            },
                        ))
                        .child(self.setup_toggle(
                            "hold-on-release",
                            "Decide when the other key is released",
                            on_release,
                            target,
                            cx,
                            |s| {
                                if let Settings::HoldTap(h) = s {
                                    h.hold_trigger_on_release = !h.hold_trigger_on_release;
                                }
                            },
                        ))
                        .child(self.render_trigger_keys(target, positions, cx))
                }),
            )
    }

    /// The keys that let the hold happen, chosen on a small keyboard.
    fn render_trigger_keys(
        &self,
        target: FeelTarget,
        positions: Vec<usize>,
        cx: &mut Context<Self>,
    ) -> Div {
        let summary = match positions.len() {
            0 => "Any key".to_string(),
            1 => "1 key".to_string(),
            n => format!("{n} keys"),
        };
        let palette = Palette::themed(cx);
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
            palette,
            tint: None,
            links: Vec::new(),
            colors: Vec::new(),
            bands: Vec::new(),
            glow: Vec::new(),
            hovered_glow: None,
        };
        let bounds = self.mini_bounds.clone();
        let hands =
            |id: &'static str, label: &'static str, right: Option<bool>, cx: &mut Context<Self>| {
                chip(id, label, false, cx).on_click(cx.listener(move |this, _, window, cx| {
                    let positions = right.map_or_else(Vec::new, |right| this.hand_positions(right));
                    this.edit_feel(target, "Change Hold Keys", window, cx, move |s| {
                        if let Settings::HoldTap(h) = s {
                            h.hold_trigger_key_positions = positions.clone();
                        }
                    });
                }))
            };
        self.fold(
            "hold-trigger-keys",
            "HOLD ONLY WITH THESE KEYS",
            summary,
            cx,
            |column, cx| {
                column
                    .child(
                        help(
                            "With none chosen, any key can. Click keys to choose, or pick a hand.",
                            cx,
                        )
                        .pt_1()
                        .pb_3(),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .pb_2()
                            .child(hands("trigger-any", "Any key", None, cx))
                            .child(hands("trigger-left", "Left hand", Some(false), cx))
                            .child(hands("trigger-right", "Right hand", Some(true), cx)),
                    )
                    .child(
                        plinth(cx)
                            .mt_2()
                            .h_48()
                            .child(canvas::keyboard(frame, move |b| bounds.set(b)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    let hit = canvas::key_at(
                                        this.layout_keys(),
                                        &[],
                                        this.mini_bounds.get(),
                                        event.position,
                                    );
                                    let Some(key) = hit else { return };
                                    this.edit_feel(
                                        target,
                                        "Change Hold Keys",
                                        window,
                                        cx,
                                        move |s| {
                                            if let Settings::HoldTap(h) = s {
                                                match h
                                                    .hold_trigger_key_positions
                                                    .iter()
                                                    .position(|p| *p == key)
                                                {
                                                    Some(index) => {
                                                        h.hold_trigger_key_positions.remove(index);
                                                    }
                                                    None => h.hold_trigger_key_positions.push(key),
                                                }
                                                h.hold_trigger_key_positions.sort_unstable();
                                            }
                                        },
                                    );
                                }),
                            ),
                    )
            },
        )
    }

    fn render_taps_feel(&self, setup: &KeySetup, cx: &mut Context<Self>) -> Div {
        let templates = self.templates_of(|k| matches!(k, BehaviorKind::TapDance(_)));
        let menu = self.feel_menu(
            "taps-feel",
            Part::Taps,
            self.choice_name(&setup.feel.taps),
            templates,
            |setup| match &setup.feel.taps {
                Choice::Custom(s) => Settings::TapDance(s.clone()),
                _ => Settings::TapDance(TapDanceSettings::default()),
            },
            cx,
        );
        let section = group("MORE THAN ONE TAP", cx)
            .child(field("How it feels", "", menu, cx))
            .child(self.template_row(Part::Taps, setup.feel.taps.template(), cx));
        match &setup.feel.taps {
            Choice::Custom(t) => {
                section.child(self.render_tap_dance_settings(FeelTarget::Keys(Part::Taps), t, cx))
            }
            _ => section,
        }
    }

    fn render_tap_dance_settings(
        &self,
        target: FeelTarget,
        t: &TapDanceSettings,
        cx: &mut Context<Self>,
    ) -> Div {
        div().flex().flex_col().child(self.stepper(
            "taps-term",
            "Time allowed between taps",
            format!("{} ms", t.tapping_term_ms),
            cx,
            move |this, d, window, cx| {
                this.edit_feel(target, "Change Tap Time", window, cx, move |s| {
                    if let Settings::TapDance(t) = s {
                        t.tapping_term_ms = t
                            .tapping_term_ms
                            .saturating_add_signed(d as i32 * 10)
                            .max(10);
                    }
                });
            },
        ))
    }

    /// Chips for choosing modifiers, left and right apart.
    fn modifier_set_chips(
        &self,
        id: &'static str,
        current: &[Modifier],
        target: FeelTarget,
        keep: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let chips = Modifier::ALL
            .into_iter()
            .enumerate()
            .map(|(index, modifier)| {
                let name = words::modifier_name(modifier);
                chip((id, index), name, current.contains(&modifier), cx).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.edit_feel(target, "Change Modifiers", window, cx, move |s| {
                            if let Settings::ModMorph(m) = s {
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

    fn render_shifted_feel(&self, setup: &KeySetup, cx: &mut Context<Self>) -> Div {
        let templates = self.templates_of(|k| matches!(k, BehaviorKind::ModMorph(_)));
        let menu = self.feel_menu(
            "shifted-feel",
            Part::Shifted,
            self.choice_name(&setup.feel.shifted),
            templates,
            |setup| match &setup.feel.shifted {
                Choice::Custom(s) => Settings::ModMorph(s.clone()),
                _ => Settings::ModMorph(ModMorphSettings::default()),
            },
            cx,
        );
        let section = group("OVERRIDE", cx)
            .child(field("Which modifiers", "", menu, cx))
            .child(self.template_row(Part::Shifted, setup.feel.shifted.template(), cx));
        match &setup.feel.shifted {
            Choice::Custom(m) => section.child(self.render_mod_morph_settings(
                FeelTarget::Keys(Part::Shifted),
                m,
                cx,
            )),
            _ => section,
        }
    }

    fn render_mod_morph_settings(
        &self,
        target: FeelTarget,
        m: &ModMorphSettings,
        cx: &mut Context<Self>,
    ) -> Div {
        let kept = match m.keep_mods.len() {
            0 => "None".to_string(),
            1 => "1 modifier".to_string(),
            n => format!("{n} modifiers"),
        };
        div()
            .flex()
            .flex_col()
            .child(help("The override is sent while any of these is held.", cx).pt_2())
            .child(
                self.modifier_set_chips("shifted-mods", &m.mods, target, false, cx)
                    .pt_2(),
            )
            .child(self.fold(
                "shifted-keep",
                "MODIFIERS STILL HELD FOR IT",
                kept,
                cx,
                |column, cx| {
                    column.child(
                        self.modifier_set_chips("shifted-keep", &m.keep_mods, target, true, cx)
                            .pt_3(),
                    )
                },
            ))
    }

    fn render_sticky_feel(&self, setup: &KeySetup, cx: &mut Context<Self>) -> Div {
        let templates = self.templates_of(|k| matches!(k, BehaviorKind::StickyKey(_)));
        let menu = self.feel_menu(
            "sticky-feel",
            Part::Sticky,
            self.choice_name(&setup.feel.sticky),
            templates,
            |setup| match &setup.feel.sticky {
                Choice::Custom(s) => Settings::StickyKey(s.clone()),
                _ => Settings::StickyKey(StickyKeySettings::default()),
            },
            cx,
        );
        let section = group("STICKY", cx)
            .child(field("How it lets go", "", menu, cx))
            .child(self.template_row(Part::Sticky, setup.feel.sticky.template(), cx));
        match &setup.feel.sticky {
            Choice::Custom(s) => {
                section.child(self.render_sticky_settings(FeelTarget::Keys(Part::Sticky), s, cx))
            }
            _ => section,
        }
    }

    fn render_sticky_settings(
        &self,
        target: FeelTarget,
        s: &StickyKeySettings,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .child(self.stepper(
                "sticky-release",
                "Lets go after",
                format!("{} ms", s.release_after_ms),
                cx,
                move |this, d, window, cx| {
                    this.edit_feel(target, "Change Feel", window, cx, move |s| {
                        if let Settings::StickyKey(s) = s {
                            s.release_after_ms = s
                                .release_after_ms
                                .saturating_add_signed(d as i32 * 100)
                                .max(100);
                        }
                    });
                },
            ))
            .child(self.setup_toggle(
                "sticky-quick",
                "Or as soon as the next key is pressed",
                s.quick_release,
                target,
                cx,
                |s| {
                    if let Settings::StickyKey(s) = s {
                        s.quick_release = !s.quick_release;
                    }
                },
            ))
            .child(self.setup_toggle(
                "sticky-lazy",
                "Press only when the next key is",
                s.lazy,
                target,
                cx,
                |s| {
                    if let Settings::StickyKey(s) = s {
                        s.lazy = !s.lazy;
                    }
                },
            ))
            .child(self.setup_toggle(
                "sticky-ignore",
                "Let modifiers through",
                s.ignore_modifiers,
                target,
                cx,
                |s| {
                    if let Settings::StickyKey(s) = s {
                        s.ignore_modifiers = !s.ignore_modifiers;
                    }
                },
            ))
    }

    /// A macro's timing, the only settings it has apart from its steps.
    fn render_macro_settings(
        &self,
        target: FeelTarget,
        m: &MacroSettings,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .child(self.stepper(
                "macro-wait",
                "Wait between steps",
                optional_ms(m.wait_ms, "default"),
                cx,
                move |this, d, window, cx| {
                    this.edit_feel(target, "Change Macro Timing", window, cx, move |s| {
                        if let Settings::Macro(m) = s {
                            step_optional_ms(&mut m.wait_ms, d, 5, 15);
                        }
                    });
                },
            ))
            .child(self.stepper(
                "macro-tap",
                "Hold each tap for",
                optional_ms(m.tap_ms, "default"),
                cx,
                move |this, d, window, cx| {
                    this.edit_feel(target, "Change Macro Timing", window, cx, move |s| {
                        if let Settings::Macro(m) = s {
                            step_optional_ms(&mut m.tap_ms, d, 5, 30);
                        }
                    });
                },
            ))
    }

    /// The macro reached from the key's slots, edited in place when it is
    /// the key's own, or named when it is a template.
    fn render_macro_section(&self, cx: &mut Context<Self>) -> Option<Div> {
        let muted = cx.theme().muted_foreground;
        let id = self.key_macro()?;
        let def = self.project().behavior(id)?;
        let BehaviorKind::Macro(m) = &def.kind else {
            return None;
        };
        let (layer, position) = (self.layer, self.primary()?);
        if def.is_template() {
            let name = def.name.clone();
            return Some(
                group("MACRO", cx).child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .py_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("Runs the macro template “{name}”.")),
                        )
                        .child(chip("macro-open", "Open", false, cx).on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.behavior = Some(id);
                                this.set_mode(Mode::Templates, window, cx);
                            },
                        )))
                        .child(
                            chip("macro-detach", "Detach", false, cx).on_click(cx.listener(
                                move |this, _, window, cx| {
                                    let mut failed: Option<SetupError> = None;
                                    let _ = this.editor.edit("Detach Macro", |p| {
                                        p.detach_from_template(layer, position, id).map_err(|e| {
                                            failed = Some(e);
                                            ModelError::NoSuchSlot
                                        })
                                    });
                                    if let Some(error) = failed {
                                        this.notice = Some(format!("{error}."));
                                    }
                                    this.after_change(window, cx);
                                },
                            )),
                        ),
                ),
            );
        }
        let naming = self.naming_macro == Some(id);
        let mut row = div().flex().flex_wrap().items_center().gap_2().py_2();
        if naming {
            row = row
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&self.template_name)),
                )
                .child(
                    chip("macro-template-save", "Save", false, cx).on_click(
                        cx.listener(|this, _, window, cx| this.save_template(window, cx)),
                    ),
                )
                .child(
                    chip("macro-template-cancel", "Cancel", false, cx).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.naming_macro = None;
                            cx.notify();
                        },
                    )),
                );
        } else {
            row = row.child(
                chip("macro-save-template", "Save as template…", false, cx).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.naming_macro = Some(id);
                        this.naming = None;
                        this.template_name
                            .read(cx)
                            .focus_handle(cx)
                            .focus(window, cx);
                        cx.notify();
                    },
                )),
            );
        }
        Some(
            div()
                .flex()
                .flex_col()
                .child(group("MACRO", cx).child(row))
                .child(self.render_macro_steps(id, m, cx)),
        )
    }

    /// Makes the selected keys run a new macro of their own, built below.
    fn make_key_macro(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (layer, targets) = (self.layer, self.selection.clone());
        let mut failed: Option<SetupError> = None;
        let result = self.editor.edit("Add Macro", |p| {
            let label = p.free_label("macro");
            let id = p.add_behavior(
                label,
                "",
                BehaviorKind::Macro(kc_model::behavior::Macro {
                    wait_ms: None,
                    tap_ms: None,
                    params: 0,
                    steps: Vec::new(),
                }),
            )?;
            p.behavior_mut(id)?.scope = kc_model::behavior::Scope::Key;
            for position in &targets {
                let mut setup = KeySetup::of(p, layer, *position)
                    .unwrap_or_else(|| KeySetup::plain(Binding::none()));
                setup.tap = Binding::user(id, vec![]);
                if let Err(error) = p.apply_setup(layer, *position, &setup) {
                    failed = Some(error);
                    return Err(ModelError::NoSuchSlot);
                }
            }
            Ok(())
        });
        match (failed, result) {
            (Some(error), _) => self.notice = Some(format!("{error}.")),
            (None, Err(error)) => self.notice = Some(format!("{error}.")),
            _ => {}
        }
        self.key_slot = None;
        self.pending_slot = None;
        self.after_change(window, cx);
    }

    /// The key as keymap text, and the definition behind it, for people
    /// who want them. Folded away unless the key cannot be shown as slots.
    fn render_key_advanced(&self, binding: &Binding, open: bool, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let open = open || self.advanced_open;
        let header = div()
            .id("key-advanced")
            .flex()
            .items_center()
            .gap_3()
            .mt_6()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .cursor_pointer()
            .child(section_title("ADVANCED", cx))
            .child(div().flex_1())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(if open { "Hide" } else { "Show" }),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.advanced_open = !this.advanced_open;
                cx.notify();
            }));
        let mut column = div().flex().flex_col().child(header);
        if !open {
            return column;
        }
        column = column
            .child(div().pt_3().child(Input::new(&self.binding_input)))
            .child(
                div()
                    .pt_1()
                    .text_xs()
                    .text_color(muted)
                    .child("The key as written in the keymap. Press Return to apply."),
            );
        let own = binding
            .user_behavior()
            .and_then(|id| self.project().behavior(id));
        if let Some(def) = own {
            let id = def.id;
            column =
                column.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .pt_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("Definition: {}", def.display_name())),
                        )
                        .child(badge(def.kind.name(), cx.theme().foreground))
                        .when(Mode::Templates.available(&self.features()), |row| {
                            row.child(chip("edit-definition", "Open", false, cx).on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.behavior = Some(id);
                                    this.set_mode(Mode::Templates, window, cx);
                                }),
                            ))
                        }),
                );
        }
        column
    }
}

/// The selected key's own settings for one part, if it has its own.
fn custom_settings(setup: &KeySetup, part: Part) -> Option<Settings> {
    match part {
        Part::Hold => match &setup.feel.hold {
            Choice::Custom(h) => Some(Settings::HoldTap(h.clone())),
            _ => None,
        },
        Part::Taps => match &setup.feel.taps {
            Choice::Custom(t) => Some(Settings::TapDance(t.clone())),
            _ => None,
        },
        Part::Shifted => match &setup.feel.shifted {
            Choice::Custom(m) => Some(Settings::ModMorph(m.clone())),
            _ => None,
        },
        Part::Sticky => match &setup.feel.sticky {
            Choice::Custom(s) => Some(Settings::StickyKey(s.clone())),
            _ => None,
        },
    }
}

fn optional_ms(value: Option<u32>, none: &str) -> String {
    value.map_or_else(|| none.to_string(), |ms| format!("{ms} ms"))
}

/// Steps an optional time: off goes to `start`, and stepping below the
/// smallest step turns it off.
fn step_optional_ms(value: &mut Option<u32>, delta: i64, step: u32, start: u32) {
    *value = match (*value, delta.signum()) {
        (None, 1) => Some(start),
        (None, _) => None,
        (Some(ms), 1) => Some(ms + step),
        (Some(ms), _) if ms > step => Some(ms - step),
        (Some(_), _) => None,
    };
}

fn set_choice_default(setup: &mut KeySetup, part: Part) {
    match part {
        Part::Hold => setup.feel.hold = Choice::Default,
        Part::Taps => setup.feel.taps = Choice::Default,
        Part::Shifted => setup.feel.shifted = Choice::Default,
        Part::Sticky => setup.feel.sticky = Choice::Default,
    }
}

fn set_choice(setup: &mut KeySetup, part: Part, choice: Choice<()>) {
    let template = choice.template();
    match (part, template) {
        (Part::Hold, Some(id)) => setup.feel.hold = Choice::Template(id),
        (Part::Taps, Some(id)) => setup.feel.taps = Choice::Template(id),
        (Part::Shifted, Some(id)) => setup.feel.shifted = Choice::Template(id),
        (Part::Sticky, Some(id)) => setup.feel.sticky = Choice::Template(id),
        (part, None) => set_choice_default(setup, part),
    }
}

fn set_custom(setup: &mut KeySetup, part: Part, settings: Settings) {
    match (part, settings) {
        (Part::Hold, Settings::HoldTap(s)) => setup.feel.hold = Choice::Custom(s),
        (Part::Taps, Settings::TapDance(s)) => setup.feel.taps = Choice::Custom(s),
        (Part::Shifted, Settings::ModMorph(s)) => setup.feel.shifted = Choice::Custom(s),
        (Part::Sticky, Settings::StickyKey(s)) => setup.feel.sticky = Choice::Custom(s),
        _ => {}
    }
}
