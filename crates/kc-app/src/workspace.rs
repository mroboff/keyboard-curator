//! The editing window for one project: layer list, keyboard canvas, key
//! picker and key inspector.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_boards::Board;
use kc_model::edit::{self, Hold};
use kc_model::keycap::{keycap, Keycap, KeycapKind};
use kc_model::picker::{picker_items, PickerGroup};
use kc_model::text::{format_binding, parse_binding, LayerStyle};
use kc_model::{file, validate, Binding, Editor, LayerId, ModelError, Project, Severity};
use kc_zmk::{Feature, Modifier};

use crate::canvas::{self, Frame, Palette};

/// The modifiers offered as toggles, with their keycap symbols.
const MODIFIERS: [(Modifier, &str); 4] = [
    (Modifier::LCtrl, "⌃"),
    (Modifier::LAlt, "⌥"),
    (Modifier::LShift, "⇧"),
    (Modifier::LGui, "⌘"),
];

pub struct Workspace {
    editor: Editor,
    board: Board,
    path: Option<PathBuf>,
    layer: LayerId,
    selected: Option<usize>,
    hovered: Option<usize>,
    canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
    group: PickerGroup,
    /// Move to the next key after assigning one from the picker.
    auto_advance: bool,
    /// Describes the picker entry under the pointer.
    hint: Option<String>,
    search: Entity<InputState>,
    binding_input: Entity<InputState>,
    layer_name: Entity<InputState>,
}

/// A small toggle button, filled when active.
fn chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (accent, accent_text, border, hover) = (
        theme.primary,
        theme.primary_foreground,
        theme.border,
        theme.secondary,
    );
    div()
        .id(id)
        .px_2()
        .h_7()
        .min_w_8()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .border_1()
        .border_color(border)
        .text_sm()
        .cursor_pointer()
        .when(active, |chip| {
            chip.bg(accent).text_color(accent_text).border_color(accent)
        })
        .when(!active, |chip| chip.hover(|chip| chip.bg(hover)))
        .child(label.into())
}

fn section_title(text: &'static str, cx: &App) -> Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

impl Workspace {
    pub fn new(
        project: Project,
        board: Board,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let layer = project.layers[0].id;
        let mut editor = Editor::new(project);
        if path.is_some() {
            editor.mark_saved();
        }
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search keys"));
        let binding_input = cx.new(|cx| InputState::new(window, cx).placeholder("&kp A"));
        let layer_name = cx.new(|cx| InputState::new(window, cx).placeholder("Layer name"));

        cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        cx.subscribe_in(
            &binding_input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let text = input.read(cx).value();
                    let binding = parse_binding(this.project(), &text);
                    this.apply_binding(binding, false, window, cx);
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &layer_name,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    let name = input.read(cx).value().trim().to_string();
                    this.rename_layer(name, window, cx);
                }
            },
        )
        .detach();

        let mut workspace = Self {
            editor,
            board,
            path,
            layer,
            selected: None,
            hovered: None,
            canvas_bounds: Rc::new(Cell::new(Bounds::default())),
            group: PickerGroup::Basic,
            auto_advance: true,
            hint: None,
            search,
            binding_input,
            layer_name,
        };
        workspace.sync_inputs(window, cx);
        workspace
    }

    pub fn project(&self) -> &Project {
        self.editor.project()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Whether there are changes that are not on disk. A project that has
    /// never been saved counts as changed.
    pub fn is_dirty(&self) -> bool {
        self.path.is_none() || self.editor.is_dirty()
    }

    pub fn title(&self) -> String {
        let mark = if self.is_dirty() { " — Edited" } else { "" };
        format!("{}{mark}", self.project().name)
    }

    pub fn save_to(&mut self, path: PathBuf) -> Result<(), file::FileError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        file::save(self.project(), &path)?;
        self.path = Some(path);
        self.editor.mark_saved();
        Ok(())
    }

    pub fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.undo();
        self.after_change(window, cx);
    }

    pub fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.redo();
        self.after_change(window, cx);
    }

    /// Brings the view back in line with the project after any change.
    fn after_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project().layer(self.layer).is_none() {
            self.layer = self.project().layers[0].id;
            self.selected = None;
        }
        self.sync_inputs(window, cx);
        cx.notify();
    }

    /// Shows the selected key's binding and the layer's name in their fields.
    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let binding = self
            .selected_binding()
            .map(|b| format_binding(self.project(), b, LayerStyle::Index))
            .unwrap_or_default();
        self.binding_input
            .update(cx, |input, cx| input.set_value(binding, window, cx));
        let name = self
            .project()
            .layer(self.layer)
            .map(|l| l.name.clone())
            .unwrap_or_default();
        self.layer_name
            .update(cx, |input, cx| input.set_value(name, window, cx));
    }

    fn features(&self) -> Vec<Feature> {
        self.board
            .profile(&self.project().firmware)
            .map(|p| p.capabilities.clone())
            .unwrap_or_default()
    }

    fn layout_keys(&self) -> &[kc_boards::geometry::Key] {
        self.board
            .layout(&self.project().layout)
            .map_or(&[], |l| l.keys.as_slice())
    }

    fn key_at(&self, position: Point<Pixels>) -> Option<usize> {
        canvas::key_at(self.layout_keys(), self.canvas_bounds.get(), position)
    }

    fn selected_binding(&self) -> Option<&Binding> {
        self.project().binding(self.layer, self.selected?)
    }

    fn select(&mut self, position: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = position;
        self.after_change(window, cx);
    }

    fn select_layer(&mut self, layer: LayerId, window: &mut Window, cx: &mut Context<Self>) {
        self.layer = layer;
        self.after_change(window, cx);
    }

    /// Sets the selected key's binding, optionally moving on to the next key.
    fn apply_binding(
        &mut self,
        binding: Binding,
        advance: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(position), layer) = (self.selected, self.layer) else {
            return;
        };
        let _ = self
            .editor
            .edit("Change Key", |p| p.set_binding(layer, position, binding));
        if advance {
            self.selected = Some((position + 1) % self.project().key_count.max(1));
        }
        self.after_change(window, cx);
    }

    /// Assigns a picker entry to the selected key.
    fn pick(&mut self, picked: Binding, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.selected_binding() else {
            return;
        };
        let binding = edit::assign(current, &picked);
        self.apply_binding(binding, self.auto_advance, window, cx);
    }

    fn edit_layers(
        &mut self,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Project) -> Result<Option<LayerId>, ModelError>,
    ) {
        if let Ok(Some(layer)) = self.editor.edit(label, change) {
            self.layer = layer;
            self.selected = None;
        }
        self.after_change(window, cx);
    }

    fn add_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = format!("Layer {}", self.project().layers.len());
        self.edit_layers("Add Layer", window, cx, |p| p.add_layer(name).map(Some));
    }

    fn duplicate_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let layer = self.layer;
        self.edit_layers("Duplicate Layer", window, cx, |p| {
            p.duplicate_layer(layer).map(Some)
        });
    }

    fn move_layer(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let layer = self.layer;
        let Some(index) = self.project().layer_index(layer) else {
            return;
        };
        let target = if down {
            index + 1
        } else {
            index.saturating_sub(1)
        };
        self.edit_layers("Move Layer", window, cx, |p| {
            p.move_layer(layer, target).map(|()| None)
        });
    }

    fn rename_layer(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let layer = self.layer;
        if !name.is_empty() {
            self.edit_layers("Rename Layer", window, cx, |p| {
                p.layer_mut(layer)?.name = name;
                Ok(None)
            });
        }
    }

    fn delete_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let layer = self.layer;
        let references = self.project().layer_references(layer).len();
        if references == 0 {
            self.edit_layers("Delete Layer", window, cx, |p| {
                p.remove_layer(layer).map(|()| None)
            });
            return;
        }
        let name = self
            .project()
            .layer(layer)
            .map(|l| l.name.clone())
            .unwrap_or_default();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Delete the layer “{name}”?"),
            Some(&format!(
                "It is used in {references} place(s). Keys that switch to it will do nothing, and rules that mention it will be removed."
            )),
            &["Cancel", "Delete Layer"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.edit_layers("Delete Layer", window, cx, |p| {
                        p.remove_layer_and_references(layer).map(|()| None)
                    });
                });
            }
        })
        .detach();
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            .layers
            .iter()
            .enumerate()
            .map(|(index, layer)| {
                let (id, active) = (layer.id, layer.id == self.layer);
                div()
                    .id(("layer", layer.id.0 as usize))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .when(active, |row| row.bg(accent).text_color(accent_text))
                    .when(!active, |row| row.hover(|row| row.bg(hover)))
                    .child(
                        div()
                            .w_5()
                            .text_xs()
                            .when(!active, |d| d.text_color(muted))
                            .child(index.to_string()),
                    )
                    .child(div().text_sm().child(layer.name.clone()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_layer(id, window, cx);
                    }))
            })
            .collect::<Vec<_>>();

        let can_delete = self.project().layers.len() > 1;
        div()
            .w_56()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(border)
            .child(
                div()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .child(section_title("LAYERS", cx)),
            )
            .child(
                div()
                    .id("layer-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .px_2()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .children(rows),
            )
            .child(
                div()
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_t_1()
                    .border_color(border)
                    .child(section_title("SELECTED LAYER", cx))
                    .child(Input::new(&self.layer_name))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .child(chip("layer-up", "↑", false, cx).on_click(cx.listener(
                                |this, _, window, cx| this.move_layer(false, window, cx),
                            )))
                            .child(chip("layer-down", "↓", false, cx).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.move_layer(true, window, cx)
                                }),
                            ))
                            .child(chip("layer-duplicate", "Duplicate", false, cx).on_click(
                                cx.listener(|this, _, window, cx| this.duplicate_layer(window, cx)),
                            ))
                            .when(can_delete, |row| {
                                row.child(chip("layer-delete", "Delete", false, cx).on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.delete_layer(window, cx)
                                    }),
                                ))
                            }),
                    )
                    .child(
                        Button::new("add-layer")
                            .ghost()
                            .label("Add Layer")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_layer(window, cx)),
                            ),
                    ),
            )
    }

    fn render_canvas(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let palette = Palette {
            key: theme.secondary,
            key_border: theme.border,
            text: theme.foreground,
            muted_text: theme.muted_foreground,
            accent: theme.primary,
            layer_key: theme.primary.opacity(0.22),
        };
        let keys = self.layout_keys().to_vec();
        let blank = Keycap {
            legend: String::new(),
            hold: None,
            kind: KeycapKind::None,
        };
        let keycaps = (0..keys.len())
            .map(|position| keycap(self.project(), self.layer, position).unwrap_or(blank.clone()))
            .collect();
        let frame = Frame {
            keys,
            keycaps,
            selected: self.selected,
            hovered: self.hovered,
            palette,
        };
        let bounds = self.canvas_bounds.clone();

        div()
            .flex_1()
            .min_h_0()
            .child(canvas::keyboard(frame, move |b| bounds.set(b)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    let position = this.key_at(event.position);
                    this.select(position, window, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                let hovered = this.key_at(event.position);
                if hovered != this.hovered {
                    this.hovered = hovered;
                    cx.notify();
                }
            }))
    }

    fn render_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted, hover) = (theme.border, theme.muted_foreground, theme.secondary);
        let items = picker_items(self.project(), &self.features());
        let query = self.search.read(cx).value().trim().to_string();
        let searching = !query.is_empty();

        let tabs = PickerGroup::ALL
            .into_iter()
            .filter(|group| items.iter().any(|i| i.group == *group))
            .map(|group| {
                let active = !searching && group == self.group;
                chip(("picker-tab", group as usize), group.title(), active, cx).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.group = group;
                        cx.notify();
                    }),
                )
            })
            .collect::<Vec<_>>();

        let enabled = self.selected.is_some();
        let cells = items
            .into_iter()
            .filter(|item| {
                if searching {
                    item.matches(&query)
                } else {
                    item.group == self.group
                }
            })
            .enumerate()
            .map(|(index, item)| {
                let (binding, description) = (item.binding, item.description);
                div()
                    .id(("pick", index))
                    .h_9()
                    .min_w_10()
                    .px_2()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .text_sm()
                    .when(enabled, |cell| {
                        cell.cursor_pointer().hover(|cell| cell.bg(hover))
                    })
                    .when(!enabled, |cell| cell.text_color(muted))
                    .child(item.label)
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        this.hint = hovered.then(|| description.clone());
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick(binding.clone(), window, cx);
                    }))
            })
            .collect::<Vec<_>>();

        let hint = match (&self.hint, enabled) {
            (Some(hint), _) => hint.clone(),
            (None, true) => "Choose what the selected key does.".to_string(),
            (None, false) => "Select a key on the keyboard, then choose what it does.".to_string(),
        };
        let advance = self.auto_advance;
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().w_56().child(Input::new(&self.search)))
                    .child(div().flex_1().text_xs().text_color(muted).child(hint))
                    .child(
                        chip("auto-advance", "Advance after assigning", advance, cx).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.auto_advance = !this.auto_advance;
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(div().flex().flex_wrap().gap_1().children(tabs))
            .child(
                div()
                    .id("picker-grid")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(div().flex().flex_wrap().gap_1().children(cells)),
            )
    }

    fn render_inspector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let panel = div()
            .w_80()
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .border_l_1()
            .border_color(border);
        let (Some(position), Some(binding)) = (self.selected, self.selected_binding()) else {
            return panel.child(section_title("NO KEY SELECTED", cx)).child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("Click a key to edit it."),
            );
        };

        let mut panel = panel
            .child(section_title("BINDING", cx))
            .child(Input::new(&self.binding_input))
            .child(div().text_xs().text_color(muted).child(format!(
                "Key {position}. Type any ZMK binding and press Return."
            )));

        if let Some(tap) = edit::tap_key(binding) {
            let modifiers = MODIFIERS.map(|(modifier, symbol)| {
                let active = tap.mods.contains(&modifier);
                chip(("modifier", modifier as usize), symbol, active, cx).on_click(cx.listener(
                    move |this, _, window, cx| {
                        let toggled = this
                            .selected_binding()
                            .and_then(|b| edit::toggle_modifier(b, modifier));
                        if let Some(binding) = toggled {
                            this.apply_binding(binding, false, window, cx);
                        }
                    },
                ))
            });
            let held = edit::hold(binding);
            let hold_chip = |id: ElementId, label: String, hold: Hold, cx: &mut Context<Self>| {
                chip(id, label, held == hold, cx).on_click(cx.listener(
                    move |this, _, window, cx| {
                        let changed = this
                            .selected_binding()
                            .and_then(|b| edit::with_hold(b, hold));
                        if let Some(binding) = changed {
                            this.apply_binding(binding, false, window, cx);
                        }
                    },
                ))
            };
            let mut holds = vec![hold_chip("hold-none".into(), "None".into(), Hold::None, cx)];
            for (modifier, symbol) in MODIFIERS {
                holds.push(hold_chip(
                    ("hold-modifier", modifier as usize).into(),
                    symbol.to_string(),
                    Hold::Modifier(modifier),
                    cx,
                ));
            }
            let layers: Vec<(LayerId, String)> = self
                .project()
                .layers
                .iter()
                .filter(|l| l.id != self.layer)
                .map(|l| (l.id, l.name.clone()))
                .collect();
            for (id, name) in layers {
                holds.push(hold_chip(
                    ("hold-layer", id.0 as usize).into(),
                    name,
                    Hold::Layer(id),
                    cx,
                ));
            }
            panel = panel
                .child(section_title("WITH MODIFIERS", cx))
                .child(div().flex().gap_1().children(modifiers))
                .child(section_title("WHEN HELD", cx))
                .child(div().flex().flex_wrap().gap_1().children(holds));
        }
        panel
    }

    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let problems = validate(self.project(), &self.board);
        let errors = problems
            .iter()
            .filter(|p| p.severity == Severity::Error)
            .count();
        let warnings = problems.len() - errors;
        let summary = match (errors, warnings) {
            (0, 0) => "No problems".to_string(),
            (e, w) => format!("{e} error(s), {w} warning(s)"),
        };
        let location = match &self.path {
            Some(path) => path.display().to_string(),
            None => "Not saved yet".to_string(),
        };
        div()
            .flex()
            .justify_between()
            .px_4()
            .py_1()
            .border_t_1()
            .border_color(border)
            .text_xs()
            .text_color(muted)
            .child(format!(
                "{} {}  ·  {summary}",
                self.board.vendor, self.board.name
            ))
            .child(location)
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        div()
            .size_full()
            .flex()
            .child(self.render_sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.render_canvas(cx))
                    .child(
                        div()
                            .h_72()
                            .flex()
                            .border_t_1()
                            .border_color(border)
                            .child(self.render_picker(cx))
                            .child(self.render_inspector(cx)),
                    )
                    .child(self.render_status(cx)),
            )
    }
}
