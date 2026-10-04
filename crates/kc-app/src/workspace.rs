//! The editing window for one project: layer list, keyboard canvas and the
//! selected key's details.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_boards::Board;
use kc_model::keycap::{keycap, Keycap, KeycapKind};
use kc_model::{file, validate, Editor, LayerId, Project, Severity};

use crate::canvas::{self, Frame, Palette};

pub struct Workspace {
    editor: Editor,
    board: Board,
    path: Option<PathBuf>,
    layer: LayerId,
    selected: Option<usize>,
    hovered: Option<usize>,
    canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl Workspace {
    pub fn new(project: Project, board: Board, path: Option<PathBuf>) -> Self {
        let layer = project.layers[0].id;
        let mut editor = Editor::new(project);
        if path.is_some() {
            editor.mark_saved();
        }
        Self {
            editor,
            board,
            path,
            layer,
            selected: None,
            hovered: None,
            canvas_bounds: Rc::new(Cell::new(Bounds::default())),
        }
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

    pub fn undo(&mut self) {
        self.editor.undo();
        self.after_history_step();
    }

    pub fn redo(&mut self) {
        self.editor.redo();
        self.after_history_step();
    }

    /// Undo and redo can remove the layer being shown.
    fn after_history_step(&mut self) {
        if self.project().layer(self.layer).is_none() {
            self.layer = self.project().layers[0].id;
        }
    }

    fn layout_keys(&self) -> &[kc_boards::geometry::Key] {
        self.board
            .layout(&self.project().layout)
            .map_or(&[], |l| l.keys.as_slice())
    }

    fn key_at(&self, position: Point<Pixels>) -> Option<usize> {
        canvas::key_at(self.layout_keys(), self.canvas_bounds.get(), position)
    }

    fn add_layer(&mut self, cx: &mut Context<Self>) {
        let name = format!("Layer {}", self.project().layers.len());
        if let Ok(id) = self.editor.edit("Add Layer", |p| p.add_layer(name)) {
            self.layer = id;
            self.selected = None;
        }
        cx.notify();
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
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.layer = id;
                        this.selected = None;
                        cx.notify();
                    }))
            })
            .collect::<Vec<_>>();

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
                    .text_xs()
                    .text_color(muted)
                    .child("LAYERS"),
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
                div().p_2().child(
                    Button::new("add-layer")
                        .ghost()
                        .label("Add Layer")
                        .on_click(cx.listener(|this, _, _, cx| this.add_layer(cx))),
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
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.selected = this.key_at(event.position);
                    cx.notify();
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

    fn render_inspector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let detail = self.selected.and_then(|position| {
            let cap = keycap(self.project(), self.layer, position)?;
            let what = match cap.kind {
                KeycapKind::Transparent if cap.legend.is_empty() => "Transparent".to_string(),
                KeycapKind::Transparent => format!("Transparent, inherits {}", cap.legend),
                KeycapKind::None => "No action".to_string(),
                _ => match &cap.hold {
                    Some(hold) => format!("{}  ·  {hold}", cap.legend),
                    None => cap.legend.clone(),
                },
            };
            Some((format!("Key {position}"), what))
        });
        let (title, body) = detail.unwrap_or_else(|| {
            (
                "No key selected".to_string(),
                "Click a key to see what it does.".to_string(),
            )
        });
        div()
            .h_24()
            .flex()
            .flex_col()
            .gap_1()
            .px_4()
            .py_3()
            .border_t_1()
            .border_color(border)
            .child(div().text_xs().text_color(muted).child(title))
            .child(div().text_sm().child(body))
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
                    .child(self.render_inspector(cx))
                    .child(self.render_status(cx)),
            )
    }
}
