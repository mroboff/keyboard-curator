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
use kc_boards::board::PointingKind;
use kc_boards::geometry::{self, Direction};
use kc_boards::Board;
use kc_model::clipboard;
use kc_model::edit::{self, Hold};
use kc_model::features::Rgb;
use kc_model::keycap::{keycap, Keycap, KeycapKind};
use kc_model::picker::{picker_items, PickerGroup};
use kc_model::text::{format_binding, parse_binding, LayerStyle};
use kc_model::{file, validate, Binding, Editor, KeyExpr, LayerId, ModelError, Project, Severity};
use kc_zmk::{Feature, Modifier};

use crate::canvas::{self, Device, Frame, Palette};
use crate::{
    Copy, Layer1, Layer2, Layer3, Layer4, Layer5, Layer6, Layer7, Layer8, Layer9, NextLayer, Paste,
    PreviousLayer, ToggleAutoAdvance, ToggleTypeToAssign,
};

/// The modifiers offered as toggles, with their keycap symbols.
const MODIFIERS: [(Modifier, &str); 4] = [
    (Modifier::LCtrl, "⌃"),
    (Modifier::LAlt, "⌥"),
    (Modifier::LShift, "⇧"),
    (Modifier::LGui, "⌘"),
];

/// Colour tags a layer can be given in the layer list.
const LAYER_TAGS: [Rgb; 6] = [
    Rgb(0xE5, 0x48, 0x4D),
    Rgb(0xF7, 0x9A, 0x3E),
    Rgb(0xE9, 0xC4, 0x3F),
    Rgb(0x46, 0xA7, 0x58),
    Rgb(0x3E, 0x8E, 0xD0),
    Rgb(0x8E, 0x4E, 0xC6),
];

fn tag_color(tag: Rgb) -> Hsla {
    rgb(u32::from(tag.0) << 16 | u32::from(tag.1) << 8 | u32::from(tag.2)).into()
}

/// A key being dragged on the canvas.
#[derive(Debug, Clone, Copy)]
struct KeyDrag {
    from: usize,
    origin: Point<Pixels>,
    position: Point<Pixels>,
    /// Becomes true once the pointer has moved far enough to mean it.
    active: bool,
}

/// A layer row being dragged to a new place in the list.
#[derive(Clone)]
struct DraggedLayer {
    id: LayerId,
    name: SharedString,
}

impl Render for DraggedLayer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .bg(theme.primary)
            .text_color(theme.primary_foreground)
            .text_sm()
            .child(self.name.clone())
    }
}

pub struct Workspace {
    editor: Editor,
    board: Board,
    path: Option<PathBuf>,
    layer: LayerId,
    /// Selected key positions; the last one is the key the inspector shows.
    selection: Vec<usize>,
    hovered: Option<usize>,
    drag: Option<KeyDrag>,
    /// A selection rectangle being dragged out, with the selection it adds to.
    band: Option<(Point<Pixels>, Point<Pixels>, Vec<usize>)>,
    canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
    canvas_focus: FocusHandle,
    /// Assign keys by typing them while the keyboard has focus.
    type_to_assign: bool,
    /// A short message about the last copy, paste or failed action.
    notice: Option<String>,
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
            selection: Vec::new(),
            hovered: None,
            drag: None,
            band: None,
            canvas_bounds: Rc::new(Cell::new(Bounds::default())),
            canvas_focus: cx.focus_handle(),
            type_to_assign: false,
            notice: None,
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
            self.selection.clear();
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
            })
            .collect()
    }

    fn key_at(&self, position: Point<Pixels>) -> Option<usize> {
        canvas::key_at(
            self.layout_keys(),
            &self.devices(),
            self.canvas_bounds.get(),
            position,
        )
    }

    /// The key the inspector shows: the one selected last.
    fn primary(&self) -> Option<usize> {
        self.selection.last().copied()
    }

    fn selected_binding(&self) -> Option<&Binding> {
        self.project().binding(self.layer, self.primary()?)
    }

    /// Puts keyboard focus on the canvas, so shortcuts reach the workspace.
    pub fn focus_canvas(&self, window: &mut Window, cx: &mut App) {
        self.canvas_focus.focus(window, cx);
    }

    fn pointer_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.canvas_focus.focus(window, cx);
        self.notice = None;
        match self.key_at(event.position) {
            Some(key) if event.modifiers.shift => {
                match self.selection.iter().position(|k| *k == key) {
                    Some(index) => {
                        self.selection.remove(index);
                    }
                    None => self.selection.push(key),
                }
            }
            Some(key) => {
                self.selection = vec![key];
                self.drag = Some(KeyDrag {
                    from: key,
                    origin: event.position,
                    position: event.position,
                    active: false,
                });
            }
            None => {
                if !event.modifiers.shift {
                    self.selection.clear();
                }
                self.band = Some((event.position, event.position, self.selection.clone()));
            }
        }
        self.after_change(window, cx);
    }

    fn pointer_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let hovered = self.key_at(event.position);
        let mut changed = hovered != self.hovered;
        self.hovered = hovered;
        if let Some(drag) = &mut self.drag {
            drag.position = event.position;
            let moved = event.position - drag.origin;
            drag.active |= f32::from(moved.x).abs() + f32::from(moved.y).abs() > 6.;
            changed |= drag.active;
        }
        if let Some((start, end, base)) = &mut self.band {
            *end = event.position;
            let (band, mut selection) = ((*start, *end), base.clone());
            let inside = canvas::keys_in_band(
                self.layout_keys(),
                &self.devices(),
                self.canvas_bounds.get(),
                band,
            );
            for key in inside {
                if !selection.contains(&key) {
                    selection.push(key);
                }
            }
            self.selection = selection;
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    /// Ends a drag: dropping a key on another swaps them, or copies with ⌥.
    fn pointer_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.band = None;
        if let Some(drag) = self.drag.take().filter(|d| d.active) {
            if let Some(target) = self.key_at(event.position).filter(|t| *t != drag.from) {
                let (layer, copy) = (self.layer, event.modifiers.alt);
                let label = if copy { "Copy Key" } else { "Swap Keys" };
                let _ = self.editor.edit(label, |p| {
                    if copy {
                        let binding = p.binding(layer, drag.from).cloned();
                        binding.map_or(Ok(()), |b| p.set_binding(layer, target, b))
                    } else {
                        p.swap_bindings(layer, drag.from, target)
                    }
                });
                self.selection = vec![target];
            }
        }
        self.after_change(window, cx);
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let stroke = &event.keystroke;
        if stroke.modifiers.platform || stroke.modifiers.control {
            return;
        }
        let direction = match stroke.key.as_str() {
            "left" => Some(Direction::Left),
            "right" => Some(Direction::Right),
            "up" => Some(Direction::Up),
            "down" => Some(Direction::Down),
            _ => None,
        };
        if let Some(direction) = direction {
            let next = match self.primary() {
                Some(from) => geometry::neighbor(self.layout_keys(), from, direction),
                None => Some(0),
            };
            if let Some(next) = next {
                if !stroke.modifiers.shift {
                    self.selection.clear();
                }
                self.selection.retain(|k| *k != next);
                self.selection.push(next);
                self.after_change(window, cx);
            }
            return;
        }
        if stroke.key == "escape" && !self.type_to_assign {
            self.selection.clear();
            self.after_change(window, cx);
        } else if self.type_to_assign {
            if let Some(code) = kc_zmk::keycodes::from_typed(&stroke.key) {
                self.pick(Binding::kp(KeyExpr::new(code)), window, cx);
            }
        } else if matches!(stroke.key.as_str(), "backspace" | "delete") {
            self.apply_binding(Binding::trans(), false, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let (text, what) = if self.selection.is_empty() {
            (
                clipboard::copy_layer(self.project(), self.layer),
                "layer".to_string(),
            )
        } else {
            (
                clipboard::copy_keys(self.project(), self.layer, &self.selection),
                format!("{} key(s)", self.selection.len()),
            )
        };
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.notice = Some(format!("Copied {what}."));
            cx.notify();
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let text = cx.read_from_clipboard().and_then(|item| item.text());
        let (layer, targets) = (self.layer, self.selection.clone());
        let result = self.editor.edit("Paste", |p| {
            Ok(clipboard::paste(
                p,
                layer,
                &targets,
                text.as_deref().unwrap_or(""),
            ))
        });
        self.notice = Some(match result {
            Ok(Ok(count)) => format!("Pasted {count} key(s)."),
            Ok(Err(error)) => format!("Could not paste: {error}."),
            Err(error) => format!("Could not paste: {error}."),
        });
        self.after_change(window, cx);
    }

    /// Shows the layer `offset` places after the current one, wrapping.
    fn step_layer(&mut self, offset: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.project().layers.len() as isize;
        let index = self.project().layer_index(self.layer).unwrap_or(0) as isize;
        self.show_layer_at((index + offset).rem_euclid(count) as usize, window, cx);
    }

    fn show_layer_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(layer) = self.project().layers.get(index) {
            self.select_layer(layer.id, window, cx);
        }
    }

    fn select_layer(&mut self, layer: LayerId, window: &mut Window, cx: &mut Context<Self>) {
        self.layer = layer;
        self.after_change(window, cx);
    }

    /// Sets the binding of every selected key. With a single key selected,
    /// `advance` moves on to the next key afterwards.
    fn apply_binding(
        &mut self,
        binding: Binding,
        advance: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection.is_empty() {
            return;
        }
        let (layer, targets) = (self.layer, self.selection.clone());
        let _ = self.editor.edit("Change Key", |p| {
            targets
                .iter()
                .try_for_each(|position| p.set_binding(layer, *position, binding.clone()))
        });
        if let (true, [position]) = (advance, targets.as_slice()) {
            self.selection = vec![(position + 1) % self.project().key_count.max(1)];
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
            self.selection.clear();
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

    /// Moves a dragged layer to the position of the row it was dropped on.
    fn move_layer_to(
        &mut self,
        layer: LayerId,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_layers("Move Layer", window, cx, |p| {
            p.move_layer(layer, index).map(|()| None)
        });
    }

    fn tag_layer(&mut self, tag: Option<Rgb>, window: &mut Window, cx: &mut Context<Self>) {
        let layer = self.layer;
        self.edit_layers("Tag Layer", window, cx, |p| {
            p.layer_mut(layer)?.color = tag;
            Ok(None)
        });
    }

    fn change_reserved(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.project().reserved_layers.saturating_add_signed(delta);
        self.edit_layers("Change Reserved Layers", window, cx, |p| {
            p.set_reserved_layers(count).map(|()| None)
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
                    .child(div().flex_1().text_sm().child(layer.name.clone()))
                    .when_some(layer.color, |row, tag| {
                        row.child(div().size_2().rounded_full().bg(tag_color(tag)))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_layer(id, window, cx);
                    }))
                    .on_drag(
                        DraggedLayer {
                            id,
                            name: layer.name.clone().into(),
                        },
                        |dragged, _, _, cx| cx.new(|_| dragged.clone()),
                    )
                    .on_drop(
                        cx.listener(move |this, dragged: &DraggedLayer, window, cx| {
                            this.move_layer_to(dragged.id, index, window, cx);
                        }),
                    )
            })
            .collect::<Vec<_>>();

        let can_delete = self.project().layers.len() > 1;
        let current_tag = self.project().layer(self.layer).and_then(|l| l.color);
        let tags = LAYER_TAGS
            .into_iter()
            .enumerate()
            .map(|(index, tag)| {
                let active = current_tag == Some(tag);
                div()
                    .id(("layer-tag", index))
                    .size_5()
                    .rounded_full()
                    .cursor_pointer()
                    .bg(tag_color(tag))
                    .border_2()
                    .border_color(if active {
                        cx.theme().foreground
                    } else {
                        tag_color(tag)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.tag_layer((!active).then_some(tag), window, cx);
                    }))
            })
            .collect::<Vec<_>>();
        let reserved = self.project().reserved_layers;
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
                    .child(div().flex().items_center().gap_1p5().children(tags))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("Spare layers for ZMK Studio: {reserved}")),
                            )
                            .child(chip("reserved-fewer", "−", false, cx).on_click(cx.listener(
                                |this, _, window, cx| this.change_reserved(-1, window, cx),
                            )))
                            .child(chip("reserved-more", "+", false, cx).on_click(cx.listener(
                                |this, _, window, cx| this.change_reserved(1, window, cx),
                            ))),
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
            devices: self.devices(),
            selected: self.selection.clone(),
            hovered: self.hovered,
            drag: self.drag.filter(|d| d.active).map(|d| (d.from, d.position)),
            band: self.band.as_ref().map(|(start, end, _)| (*start, *end)),
            palette,
            tint: self
                .project()
                .layer(self.layer)
                .and_then(|l| l.color)
                .map(tag_color),
        };
        let bounds = self.canvas_bounds.clone();

        div()
            .id("canvas")
            .key_context("Canvas")
            .track_focus(&self.canvas_focus)
            .flex_1()
            .min_h_0()
            .child(canvas::keyboard(frame, move |b| bounds.set(b)))
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.pointer_down(event, window, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.pointer_move(event, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.pointer_up(event, window, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.drag = None;
                    this.band = None;
                    cx.notify();
                }),
            )
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

        let enabled = !self.selection.is_empty();
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

        let hint = match (&self.hint, &self.notice, self.selection.len()) {
            (Some(hint), _, _) => hint.clone(),
            (None, Some(notice), _) => notice.clone(),
            (None, None, 0) => {
                "Select a key on the keyboard, then choose what it does.".to_string()
            }
            (None, None, 1) => "Choose what the selected key does.".to_string(),
            (None, None, n) => format!("Choose what the {n} selected keys do."),
        };
        let (advance, typing) = (self.auto_advance, self.type_to_assign);
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
                        chip("type-to-assign", "Type to assign", typing, cx).on_click(cx.listener(
                            |this, _, window, cx| {
                                this.type_to_assign = !this.type_to_assign;
                                this.canvas_focus.focus(window, cx);
                                cx.notify();
                            },
                        )),
                    )
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
        let (Some(position), Some(binding)) = (self.primary(), self.selected_binding()) else {
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
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(match self.selection.len() {
                        1 => format!("Key {position}. Type any ZMK binding and press Return."),
                        n => format!("{n} keys selected. Changes apply to all of them."),
                    }),
            );

        let arguments = edit::command_args(binding);
        if !arguments.is_empty() {
            panel = panel.child(section_title("VALUES", cx));
        }
        for (index, (arg, value)) in arguments.into_iter().enumerate() {
            let step = if arg.max - arg.min > 50 { 10 } else { 1 };
            let nudge = move |delta: i64| {
                move |this: &mut Self,
                      _: &ClickEvent,
                      window: &mut Window,
                      cx: &mut Context<Self>| {
                    let changed = this
                        .selected_binding()
                        .and_then(|b| edit::with_command_arg(b, index, i64::from(value) + delta));
                    if let Some(binding) = changed {
                        this.apply_binding(binding, false, window, cx);
                    }
                }
            };
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .child(format!("{}: {value}", arg.name)),
                    )
                    .child(
                        chip(("arg-fewer", index), "−", false, cx)
                            .on_click(cx.listener(nudge(-i64::from(step)))),
                    )
                    .child(
                        chip(("arg-more", index), "+", false, cx)
                            .on_click(cx.listener(nudge(i64::from(step)))),
                    ),
            );
        }

        // Layer parameters, other than a layer-tap's (the hold row covers it).
        let layer_params: Vec<(usize, LayerId)> = match binding {
            Binding::Behavior { params, .. } if edit::hold(binding) == Hold::None => params
                .iter()
                .enumerate()
                .filter_map(|(index, p)| match p {
                    kc_model::Param::Layer(id) => Some((index, *id)),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        for (index, current) in layer_params {
            let choices = self
                .project()
                .layers
                .iter()
                .map(|l| {
                    let id = l.id;
                    chip(
                        ("param-layer", (index << 16) | id.0 as usize),
                        l.name.clone(),
                        id == current,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let changed = this
                            .selected_binding()
                            .and_then(|b| edit::with_layer(b, index, id));
                        if let Some(binding) = changed {
                            this.apply_binding(binding, false, window, cx);
                        }
                    }))
                })
                .collect::<Vec<_>>();
            panel = panel
                .child(section_title("LAYER", cx))
                .child(div().flex().flex_wrap().gap_1().children(choices));
        }

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
            .key_context("Workspace")
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(|this, _: &NextLayer, window, cx| {
                this.step_layer(1, window, cx);
            }))
            .on_action(cx.listener(|this, _: &PreviousLayer, window, cx| {
                this.step_layer(-1, window, cx);
            }))
            .on_action(
                cx.listener(|this, _: &Layer1, window, cx| this.show_layer_at(0, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Layer2, window, cx| this.show_layer_at(1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Layer3, window, cx| this.show_layer_at(2, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Layer4, window, cx| this.show_layer_at(3, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Layer5, window, cx| this.show_layer_at(4, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Layer6, window, cx| this.show_layer_at(5, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Layer7, window, cx| this.show_layer_at(6, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Layer8, window, cx| this.show_layer_at(7, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Layer9, window, cx| this.show_layer_at(8, window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleAutoAdvance, _, cx| {
                this.auto_advance = !this.auto_advance;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleTypeToAssign, _, cx| {
                this.type_to_assign = !this.type_to_assign;
                cx.notify();
            }))
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
