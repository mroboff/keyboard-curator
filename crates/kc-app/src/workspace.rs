//! The editing window for one project: layer list, keyboard canvas, key
//! picker and key inspector.

mod advanced;
mod behaviors;
mod combos;
mod lighting;
mod pointing;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::color_picker::ColorPickerState;
use gpui_kit::component::input::{Input, InputEvent, InputState, TextareaState};
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
use kc_model::{
    file, BehaviorId, Binding, ComboId, Editor, FirmwareConfig, KeyExpr, Keyboard, KeyboardId,
    LayerId, ModelError, Project, Severity, Slot,
};
use kc_zmk::{Feature, Modifier};

use crate::canvas::{self, Device, Frame, Palette};
use crate::library::Library;
use crate::{
    Copy, Layer1, Layer2, Layer3, Layer4, Layer5, Layer6, Layer7, Layer8, Layer9, NextLayer, Paste,
    PreviousLayer, ShowFiles, ShowFlash, ShowKeyboard, ToggleAutoAdvance, ToggleTypeToAssign,
};

/// The modifiers offered as toggles, with their keycap symbols.
const MODIFIERS: [(Modifier, &str); 4] = [
    (Modifier::LCtrl, "⌃"),
    (Modifier::LAlt, "⌥"),
    (Modifier::LShift, "⇧"),
    (Modifier::LGui, "⌘"),
];

/// Color tags a layer can be given in the layer list.
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

/// What the main area of the workspace shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Keyboard,
    Lighting,
    Behaviors,
    Combos,
    Pointing,
    /// Layer rules and custom devicetree.
    Advanced,
    Files,
    /// Putting the layout on the board.
    Apply,
}

impl Mode {
    /// The modes in the order of their tabs, with their labels.
    const TABS: [(Mode, &'static str, &'static str); 8] = [
        (Mode::Keyboard, "mode-keyboard", "Keyboard"),
        (Mode::Lighting, "mode-lighting", "Lighting"),
        (Mode::Behaviors, "mode-behaviors", "Behaviors"),
        (Mode::Combos, "mode-combos", "Combos"),
        (Mode::Pointing, "mode-pointing", "Pointing"),
        (Mode::Advanced, "mode-advanced", "Advanced"),
        (Mode::Files, "mode-files", "Generated Files"),
        (Mode::Apply, "mode-apply", "Apply"),
    ];

    /// What a firmware must have for the mode to be offered at all. A
    /// mode the keyboard's firmware cannot use is left out, not disabled.
    fn requires(self) -> Option<Feature> {
        match self {
            Mode::Lighting => Some(Feature::PerKeyLighting),
            Mode::Pointing => Some(Feature::Pointing),
            Mode::Combos => Some(Feature::Combos),
            // Layer rules and custom devicetree both need a firmware built
            // from devicetree.
            Mode::Advanced => Some(Feature::Devicetree),
            // Firmware configured on the keyboard has no files to show.
            Mode::Files => Some(Feature::Build),
            _ => None,
        }
    }

    fn available(self, features: &[Feature]) -> bool {
        // The behavior editors are for behaviors the user defines; a
        // firmware with none of those kinds has nothing to edit there.
        if self == Mode::Behaviors {
            return [
                Feature::Macros,
                Feature::TapDance,
                Feature::ModMorph,
                Feature::HoldTaps,
                Feature::StickyKeys,
            ]
            .iter()
            .any(|f| features.contains(f));
        }
        self.requires().is_none_or(|f| features.contains(&f))
    }
}

/// What is known about a keyboard connected for direct updates.
#[derive(Debug, Clone, PartialEq)]
enum LiveStatus {
    Idle,
    Working(String),
    Found(kc_studio::session::Found),
    Message(String),
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

/// Which layer, behavior and combo the name fields currently show, so
/// that text typed into them is applied to the right thing even when the
/// selection has just moved on.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Shown {
    layer: Option<LayerId>,
    behavior: Option<BehaviorId>,
    combo: Option<ComboId>,
}

/// What the workspace asks of the window around it.
pub enum WorkspaceEvent {
    /// Make this layout the board's current one.
    Apply,
    /// Leave the editor for the board's page.
    Close,
}

impl EventEmitter<WorkspaceEvent> for Workspace {}

pub struct Workspace {
    shown: Shown,
    editor: Editor,
    board: Board,
    path: Option<PathBuf>,
    layer: LayerId,
    mode: Mode,
    /// Which generated file the files view shows.
    file_index: usize,
    /// The saved keyboards, and the one this layout is open under.
    library: Entity<Library>,
    keyboard: KeyboardId,
    /// That keyboard's firmware and settings, which decide what the editor
    /// offers and what the layout is checked against. Kept in step with
    /// the library.
    config: FirmwareConfig,
    live: LiveStatus,
    /// The binding inside a behavior or combo that the picker assigns to,
    /// in the Behaviors and Combos modes.
    slot: Option<Slot>,
    /// The behavior and combo being edited.
    behavior: Option<BehaviorId>,
    combo: Option<ComboId>,
    /// Where the small keyboard for choosing key positions was laid out.
    mini_bounds: Rc<Cell<Bounds<Pixels>>>,
    behavior_name: Entity<InputState>,
    behavior_label: Entity<InputState>,
    macro_text: Entity<InputState>,
    combo_name: Entity<InputState>,
    raw_behaviors: Entity<TextareaState>,
    raw_devicetree: Entity<TextareaState>,
    /// What painting does in the Lighting mode, and with which color.
    brush: lighting::Brush,
    paint_color: kc_model::features::Rgb,
    /// True while the pointer is held down painting.
    painting: bool,
    color_picker: Entity<ColorPickerState>,
    /// Selected key positions; the last one is the key the inspector shows.
    selection: Vec<usize>,
    hovered: Option<usize>,
    drag: Option<KeyDrag>,
    /// The layer being dragged in the layer list, and the place in the
    /// list it would take if let go of now.
    layer_drop: Option<(LayerId, usize)>,
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
pub(crate) fn chip(
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
        .px_2p5()
        .h_7()
        .min_w_8()
        .flex()
        .items_center()
        .justify_center()
        .rounded(crate::theme::look(cx).control_radius)
        .border_1()
        .border_color(border)
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .when(active, |chip| {
            chip.bg(accent).text_color(accent_text).border_color(accent)
        })
        .when(!active, |chip| chip.hover(|chip| chip.bg(hover)))
        .child(label.into())
}

/// A text tab, underlined when it is the one shown.
pub(crate) fn tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (accent, text, muted) = (theme.primary, theme.foreground, theme.muted_foreground);
    div()
        .id(id)
        .h_9()
        .flex()
        .items_center()
        .border_b_2()
        .text_sm()
        .cursor_pointer()
        .when(active, |tab| {
            tab.border_color(accent)
                .text_color(text)
                .font_weight(FontWeight::SEMIBOLD)
        })
        .when(!active, |tab| {
            tab.border_color(transparent_black())
                .text_color(muted)
                .font_weight(FontWeight::MEDIUM)
                .hover(|tab| tab.text_color(text))
        })
        .child(label.into())
}

/// Text in the theme's display face, for headings and figures.
pub(crate) fn display(text: impl Into<SharedString>, size: f32, cx: &App) -> Div {
    div()
        .font_family(crate::theme::look(cx).display_font.clone())
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(size))
        .line_height(relative(1.05))
        .child(text.into())
}

/// The surface a keyboard is shown on.
pub(crate) fn plinth(cx: &App) -> Div {
    let colors = crate::theme::look(cx).colors;
    div()
        .rounded(px(24.))
        .bg(colors.plinth)
        .border_1()
        .border_color(colors.plinth_border)
        .shadow(vec![BoxShadow {
            color: black().opacity(0.2),
            offset: point(px(0.), px(26.)),
            blur_radius: px(40.),
            spread_radius: px(-22.),
            inset: false,
        }])
}

/// A small outlined tag, such as the kind of a behavior.
pub(crate) fn badge(label: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .px_1p5()
        .rounded_md()
        .border_1()
        .border_color(color.opacity(0.4))
        .text_xs()
        .text_color(color)
        .flex_shrink_0()
        .child(label.into())
}

fn section_title(text: &'static str, cx: &App) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

impl Workspace {
    pub fn new(
        project: Project,
        board: Board,
        library: Entity<Library>,
        keyboard: KeyboardId,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let layer = project.layers[0].id;
        let mut editor = Editor::new(project);
        if path.is_some() {
            editor.mark_saved();
        }
        let firmware_of = move |library: &Entity<Library>, cx: &App| {
            library
                .read(cx)
                .keyboards()
                .get(keyboard)
                .map(|k| k.firmware.clone())
        };
        let config = firmware_of(&library, cx).unwrap_or_else(|| FirmwareConfig::stock(&board));
        cx.observe_in(&library, window, move |this, library, window, cx| {
            let Some(config) = firmware_of(&library, cx) else {
                return;
            };
            if this.config != config {
                this.config = config;
                // What the firmware offers may have changed under the mode.
                if !this.mode.available(&this.features()) {
                    this.mode = Mode::Keyboard;
                }
                this.after_change(window, cx);
            }
            cx.notify();
        })
        .detach();
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
                    this.assign_to_target(binding, false, window, cx);
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &layer_name,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    this.after_change(window, cx);
                }
            },
        )
        .detach();

        let line = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let behavior_name = line("Name", window, cx);
        let behavior_label = line("label", window, cx);
        let macro_text = line("Text to type", window, cx);
        let combo_name = line("Name", window, cx);
        let raw_behaviors = cx.new(|cx| TextareaState::new(window, cx));
        let raw_devicetree = cx.new(|cx| TextareaState::new(window, cx));
        advanced::subscribe(&raw_behaviors, &raw_devicetree, window, cx);
        behaviors::subscribe(&behavior_name, &behavior_label, &macro_text, window, cx);
        combos::subscribe(&combo_name, window, cx);
        let color_picker = cx.new(|cx| ColorPickerState::new(window, cx));
        lighting::subscribe(&color_picker, cx);
        let mut workspace = Self {
            shown: Shown::default(),
            editor,
            board,
            path,
            layer,
            mode: Mode::Keyboard,
            file_index: 0,
            library,
            keyboard,
            config,
            live: LiveStatus::Idle,
            slot: None,
            behavior: None,
            combo: None,
            mini_bounds: Rc::new(Cell::new(Bounds::default())),
            behavior_name,
            behavior_label,
            macro_text,
            combo_name,
            raw_behaviors,
            raw_devicetree,
            brush: lighting::Brush::Color,
            paint_color: kc_model::features::Rgb(0x00, 0xC0, 0xFF),
            painting: false,
            color_picker,
            selection: Vec::new(),
            hovered: None,
            drag: None,
            layer_drop: None,
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
        // For checking a screen from the command line: KC_MODE=advanced.
        let start = match std::env::var("KC_MODE").as_deref() {
            Ok("lighting") => Some(Mode::Lighting),
            Ok("behaviors") => Some(Mode::Behaviors),
            Ok("combos") => Some(Mode::Combos),
            Ok("pointing") => Some(Mode::Pointing),
            Ok("advanced") => Some(Mode::Advanced),
            Ok("files") => Some(Mode::Files),
            Ok("apply") => Some(Mode::Apply),
            _ => None,
        };
        if let Some(mode) = start {
            workspace.set_mode(mode, window, cx);
        }
        workspace
    }

    /// The saved keyboard this project is open under.
    pub fn keyboard(&self) -> KeyboardId {
        self.keyboard
    }

    fn saved_keyboard<'a>(&self, cx: &'a App) -> Option<&'a Keyboard> {
        self.library.read(cx).keyboards().get(self.keyboard)
    }

    fn keyboard_name(&self, cx: &App) -> String {
        self.saved_keyboard(cx)
            .map_or_else(String::new, |k| k.name.clone())
    }

    pub fn project(&self) -> &Project {
        self.editor.project()
    }

    /// Shows a message in the bar above the keyboard.
    pub fn set_notice(&mut self, notice: String, cx: &mut Context<Self>) {
        self.notice = Some(notice);
        cx.notify();
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Whether there are changes that are not on disk. A project that has
    /// never been saved counts as changed.
    pub fn is_dirty(&self) -> bool {
        self.path.is_none() || self.editor.is_dirty()
    }

    /// What the layout is called: its file's name once it has one.
    fn layout_name(&self) -> String {
        self.path.as_deref().and_then(Path::file_stem).map_or_else(
            || self.project().name.clone(),
            |stem| stem.to_string_lossy().into_owned(),
        )
    }

    pub fn title(&self, cx: &App) -> String {
        let mark = if self.is_dirty() { " — Edited" } else { "" };
        format!("{} › {}{mark}", self.keyboard_name(cx), self.layout_name())
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
        self.refresh(window, cx);
    }

    pub fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.redo();
        self.refresh(window, cx);
    }

    /// Applies text typed into the name fields to what they were showing.
    fn commit_inputs(&mut self, cx: &mut Context<Self>) {
        let shown = self.shown;
        let layer = self.layer_name.read(cx).value().trim().to_string();
        let name = self.behavior_name.read(cx).value().trim().to_string();
        let label = self.behavior_label.read(cx).value().trim().to_string();
        let combo = self.combo_name.read(cx).value().trim().to_string();
        let project = self.project();
        let layer_edit = shown
            .layer
            .filter(|id| !layer.is_empty() && project.layer(*id).is_some_and(|l| l.name != layer));
        let behavior = shown.behavior.and_then(|id| project.behavior(id));
        let name_edit = behavior
            .filter(|b| !name.is_empty() && b.name != name)
            .map(|b| b.id);
        let label_edit = behavior
            .filter(|b| !label.is_empty() && b.label != label)
            .map(|b| b.id);
        let combo_edit = shown.combo.filter(|id| {
            !combo.is_empty()
                && project
                    .combos
                    .iter()
                    .any(|c| c.id == *id && c.name != combo)
        });
        if layer_edit.is_some() || name_edit.is_some() || combo_edit.is_some() {
            let _ = self.editor.edit("Rename", |p| {
                if let Some(id) = layer_edit {
                    p.layer_mut(id)?.name = layer;
                }
                if let Some(id) = name_edit {
                    p.behavior_mut(id)?.name = name;
                }
                if let Some(id) = combo_edit {
                    p.combo_mut(id)?.name = combo;
                }
                Ok(())
            });
        }
        if let Some(id) = label_edit {
            if let Err(error) = self.editor.edit("Change Behavior Label", |p| {
                p.rename_behavior_label(id, label)
            }) {
                self.notice = Some(format!("{error}."));
            }
        }
        self.commit_advanced_inputs(cx);
    }

    /// Shows the edited behavior's and combo's names in their fields.
    fn sync_editor_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (name, label) = self
            .behavior
            .and_then(|id| self.project().behavior(id))
            .map(|b| (b.name.clone(), b.label.clone()))
            .unwrap_or_default();
        let combo = self
            .combo
            .and_then(|id| self.project().combos.iter().find(|c| c.id == id))
            .map(|c| c.name.clone())
            .unwrap_or_default();
        for (input, text) in [
            (&self.behavior_name, name),
            (&self.behavior_label, label),
            (&self.combo_name, combo),
        ] {
            if input.read(cx).value() != text {
                input.update(cx, |input, cx| input.set_value(text, window, cx));
            }
        }
        self.sync_advanced_inputs(window, cx);
        self.shown = Shown {
            layer: Some(self.layer),
            behavior: self.behavior,
            combo: self.combo,
        };
    }

    /// Brings the view back in line with the project after any change,
    /// first applying anything typed into a text field.
    fn after_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_inputs(cx);
        self.refresh(window, cx);
    }

    /// Brings the view back in line with the project, discarding anything
    /// typed but not yet applied. Used after undo and redo.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project().layer(self.layer).is_none() {
            self.layer = self.project().layers[0].id;
            self.selection.clear();
        }
        if self
            .behavior
            .is_some_and(|id| self.project().behavior(id).is_none())
        {
            self.behavior = None;
        }
        if self
            .combo
            .is_some_and(|id| !self.project().combos.iter().any(|c| c.id == id))
        {
            self.combo = None;
        }
        if self
            .slot
            .is_some_and(|slot| self.project().slot(slot).is_none())
        {
            self.slot = None;
        }
        self.sync_inputs(window, cx);
        cx.notify();
    }

    /// Shows the selected key's binding and the layer's name in their fields.
    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let binding = self
            .target_binding()
            .map(|b| format_binding(self.project(), b, LayerStyle::Index))
            .unwrap_or_default();
        self.sync_editor_inputs(window, cx);
        self.binding_input
            .update(cx, |input, cx| input.set_value(binding, window, cx));
        let name = self
            .project()
            .layer(self.layer)
            .map(|l| l.name.clone())
            .unwrap_or_default();
        if self.layer_name.read(cx).value() != name {
            self.layer_name
                .update(cx, |input, cx| input.set_value(name, window, cx));
        }
    }

    fn features(&self) -> Vec<Feature> {
        self.config.features(&self.board)
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

    /// Whether the picker currently assigns to a slot rather than to keys.
    fn targets_slot(&self) -> bool {
        matches!(self.mode, Mode::Behaviors | Mode::Combos)
    }

    /// The binding the picker and the binding field act on.
    fn target_binding(&self) -> Option<&Binding> {
        if self.targets_slot() {
            self.project().slot(self.slot?)
        } else {
            self.selected_binding()
        }
    }

    /// Assigns a binding to whatever the picker is aimed at.
    fn assign_to_target(
        &mut self,
        binding: Binding,
        advance: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.targets_slot() {
            if let Some(slot) = self.slot {
                self.change("Change Binding", window, cx, |p| p.set_slot(slot, binding));
            }
        } else {
            self.apply_binding(binding, advance, window, cx);
        }
    }

    /// Applies an edit as one undo step and refreshes the view.
    fn change(
        &mut self,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Project) -> Result<(), ModelError>,
    ) {
        if let Err(error) = self.editor.edit(label, edit) {
            self.notice = Some(format!("{error}."));
        }
        self.after_change(window, cx);
    }

    fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_inputs(cx);
        // A mode the firmware cannot use has no tab; fall back if asked.
        let mode = if mode.available(&self.features()) {
            mode
        } else {
            Mode::Keyboard
        };
        self.mode = mode;
        self.hint = None;
        // Open the editors on something, so they are not blank.
        match mode {
            Mode::Behaviors if self.behavior.is_none() => {
                self.behavior = self.project().behaviors.first().map(|b| b.id);
            }
            Mode::Combos if self.combo.is_none() => {
                self.combo = self.project().combos.first().map(|c| c.id);
                self.slot = self.combo.map(Slot::Combo);
            }
            _ => {}
        }
        self.after_change(window, cx);
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
    /// `advance` moves on to the next key afterward.
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

    /// Assigns a picker entry to the selected keys, or to the slot being
    /// edited.
    fn pick(&mut self, picked: Binding, window: &mut Window, cx: &mut Context<Self>) {
        if self.targets_slot() {
            self.assign_to_target(picked, false, window, cx);
            return;
        }
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

    /// Notes where a dragged layer would land, so the list can show it.
    fn aim_layer(&mut self, layer: LayerId, index: usize, cx: &mut Context<Self>) {
        if self.layer_drop != Some((layer, index)) {
            self.layer_drop = Some((layer, index));
            cx.notify();
        }
    }

    /// Puts a dragged layer where the list was showing it would go.
    fn drop_layer(&mut self, layer: LayerId, window: &mut Window, cx: &mut Context<Self>) {
        match self.layer_drop.take() {
            Some((aimed, index)) if aimed == layer => {
                self.move_layer_to(layer, index, window, cx);
            }
            _ => cx.notify(),
        }
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

    /// Leaves the editor for the board's page.
    fn leave(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.commit_inputs(cx);
        cx.emit(WorkspaceEvent::Close);
    }

    /// Where the user is: the board, then this layout. The back button and
    /// the board's name both lead out of the editor to the board's page.
    fn render_breadcrumb(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted, hover) = (theme.border, theme.muted_foreground, theme.secondary);
        let firmware = self
            .board
            .profile(&self.config.profile)
            .map_or_else(String::new, |p| p.name.clone());
        let mark = if self.is_dirty() { " — Edited" } else { "" };
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(border)
            .child(chip("back", "‹ Back", false, cx).on_click(cx.listener(Self::leave)))
            .child(
                div()
                    .id("crumb-board")
                    .px_2()
                    .h_7()
                    .flex()
                    .items_center()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|crumb| crumb.bg(hover))
                    .text_sm()
                    .child(self.keyboard_name(cx))
                    .on_click(cx.listener(Self::leave)),
            )
            .child(div().text_sm().text_color(muted).child("›"))
            .child(div().px_1().child(display(self.layout_name(), 17., cx)))
            .child(div().text_xs().text_color(muted).child(mark))
            .child(div().flex_1())
            .child(div().text_xs().text_color(muted).child(format!(
                "{} {} · {firmware}",
                self.board.vendor, self.board.name
            )))
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted, accent, hover) = (
            theme.border,
            theme.muted_foreground,
            theme.primary,
            theme.secondary,
        );
        // While a layer is dragged, the list is drawn in the order a drop
        // would give: the other layers have moved out of the way, and a
        // shaded slot shows where the dragged layer would land. A drag let
        // go of elsewhere leaves the aim behind, so it only counts while a
        // drag is going on.
        let aim = self.layer_drop.filter(|_| cx.has_active_drag());
        let mut order: Vec<&kc_model::Layer> = self.project().layers.iter().collect();
        if let Some((dragged, index)) = aim {
            if let Some(from) = order.iter().position(|l| l.id == dragged) {
                let layer = order.remove(from);
                order.insert(index.min(order.len()), layer);
            }
        }
        let workspace = cx.weak_entity();
        let rows = order
            .into_iter()
            .enumerate()
            .map(|(index, layer)| {
                let id = layer.id;
                let slot = aim.is_some_and(|(dragged, _)| dragged == id);
                let active = id == self.layer && !slot;
                let workspace = workspace.clone();
                div()
                    .id(("layer", layer.id.0 as usize))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .h_10()
                    .rounded_lg()
                    .border_1()
                    .border_color(transparent_black())
                    .cursor_pointer()
                    .when(slot, |row| {
                        row.bg(hover)
                            .border_dashed()
                            .border_color(accent)
                            .text_color(muted)
                    })
                    // The layer shown is set apart by weight and its
                    // figure in the accent color, not by a block of color.
                    .when(active, |row| row.bg(hover))
                    .when(!active && !slot, |row| row.text_color(muted))
                    .when(!active && !slot && aim.is_none(), |row| {
                        row.hover(|row| row.bg(hover))
                    })
                    .child(
                        display(index.to_string(), 13., cx)
                            .w_5()
                            .when(active, |d| d.text_color(accent)),
                    )
                    .child(display(layer.name.clone(), 17., cx).flex_1().min_w_0())
                    .when_some(layer.color, |row, tag| {
                        row.child(div().size_2p5().rounded_full().bg(tag_color(tag)))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_layer(id, window, cx);
                    }))
                    .on_drag(
                        DraggedLayer {
                            id,
                            name: layer.name.clone().into(),
                        },
                        move |dragged, _, _, cx| {
                            // The slot shows from the first moment, where
                            // the layer already is.
                            let _ = workspace.update(cx, |this, cx| {
                                this.aim_layer(id, index, cx);
                            });
                            cx.new(|_| dragged.clone())
                        },
                    )
                    // Every row hears every move of a drag, so each
                    // answers only for the pointer being over it. Rows are
                    // all one height, so the slot settles under the pointer.
                    .on_drag_move(cx.listener(
                        move |this, event: &DragMoveEvent<DraggedLayer>, _, cx| {
                            if event.bounds.contains(&event.event.position) {
                                let dragged = event.drag(cx).id;
                                this.aim_layer(dragged, index, cx);
                            }
                        },
                    ))
                    .on_drop(
                        cx.listener(move |this, dragged: &DraggedLayer, window, cx| {
                            this.aim_layer(dragged.id, index, cx);
                            this.drop_layer(dragged.id, window, cx);
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
                    .children(rows)
                    // Let go between rows or below the last one: the layer
                    // goes where the slot was last shown.
                    .on_drop(cx.listener(|this, dragged: &DraggedLayer, window, cx| {
                        this.drop_layer(dragged.id, window, cx);
                    })),
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
        let palette = Palette::themed(cx);
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
            links: Vec::new(),
            colors: Vec::new(),
        };
        let bounds = self.canvas_bounds.clone();

        let look = crate::theme::look(cx);
        let (wash, figures) = (look.colors.wash, look.figures);
        let muted = cx.theme().muted_foreground;
        let index = self.project().layer_index(self.layer).unwrap_or_default();
        let name = self
            .project()
            .layer(self.layer)
            .map(|l| l.name.clone())
            .unwrap_or_default();
        let held = (0..self.layout_keys().len())
            .filter(|p| {
                keycap(self.project(), self.layer, *p).is_some_and(|cap| cap.hold.is_some())
            })
            .count();
        let count = match held {
            0 => format!("{} keys", self.layout_keys().len()),
            n => format!(
                "{} keys · {n} with a second job when held",
                self.layout_keys().len()
            ),
        };

        div()
            .id("canvas")
            .key_context("Canvas")
            .track_focus(&self.canvas_focus)
            .relative()
            .overflow_hidden()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .px_5()
            .pt_3()
            .pb_5()
            .gap_3()
            // A theme may set the layer's number, very large and barely
            // there, behind everything.
            .when(figures, |area| {
                area.child(
                    display(index.to_string(), 260., cx)
                        .absolute()
                        .top(px(-56.))
                        .left(px(4.))
                        .text_color(wash),
                )
            })
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap_3()
                    .child(
                        div()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(muted)
                                    .child(format!("LAYER {index}")),
                            )
                            .child(display(name, 34., cx)),
                    )
                    .child(div().flex_1())
                    .child(div().pb_1().text_sm().text_color(muted).child(count)),
            )
            .child(
                plinth(cx)
                    .flex_1()
                    .min_h_0()
                    .child(canvas::keyboard(frame, move |b| bounds.set(b))),
            )
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
        let (muted, hover) = (theme.muted_foreground, theme.secondary);
        // Only what the board's firmware has something for is offered.
        let items: Vec<_> = picker_items(self.project(), &self.features())
            .into_iter()
            .filter(|item| {
                kc_firmware::expressible(&item.binding, self.project(), &self.board, &self.config)
            })
            .collect();
        let query = self.search.read(cx).value().trim().to_string();
        let searching = !query.is_empty();

        let tabs = PickerGroup::ALL
            .into_iter()
            .filter(|group| items.iter().any(|i| i.group == *group))
            .map(|group| {
                let active = !searching && group == self.group;
                tab(("picker-tab", group as usize), group.title(), active, cx).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.group = group;
                        cx.notify();
                    }),
                )
            })
            .collect::<Vec<_>>();

        let enabled = self.target_binding().is_some();
        // The keys on offer are drawn as caps, like the ones on the board.
        let look = crate::theme::look(cx);
        let (cap, display_font) = (look.colors, look.display_font.clone());
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
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .rounded_lg()
                    .border_1()
                    .border_color(cap.key_border)
                    .bg(cap.key)
                    .text_color(cap.key_text)
                    .font_family(display_font.clone())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_sm()
                    .when(enabled, |cell| {
                        cell.cursor_pointer().hover(|cell| cell.bg(hover))
                    })
                    .when(!enabled, |cell| cell.text_color(muted))
                    .child(item.label)
                    // A behavior the user named says what kind it is.
                    .when_some(item.kind, |cell, kind| {
                        cell.h_12()
                            .child(div().text_xs().text_color(muted).child(kind))
                    })
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

    /// The picker with a binding field beside it, for the modes where it
    /// assigns to a slot inside a behavior or combo.
    fn render_slot_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (border, muted) = (cx.theme().border, cx.theme().muted_foreground);
        div()
            .h_64()
            .flex()
            .border_t_1()
            .border_color(border)
            .child(self.render_picker(cx))
            .child(
                div()
                    .w_80()
                    .h_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .border_l_1()
                    .border_color(border)
                    .child(section_title("SELECTED BINDING", cx))
                    .child(Input::new(&self.binding_input))
                    .child(div().text_xs().text_color(muted).child(
                        "Click a binding above to select it. Choose from the list, or type any ZMK binding and press Return.",
                    )),
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

        let colors = crate::theme::look(cx).colors;
        let shown = keycap(self.project(), self.layer, position);
        let specimen = div()
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
                                            div().text_size(px(9.)).text_color(muted).child(hold),
                                        )
                                    },
                                )
                            }),
                    ),
            )
            .child(display(
                match self.selection.len() {
                    1 => format!("Key {position}"),
                    n => format!("{n} keys"),
                },
                20.,
                cx,
            ));
        let mut panel = panel
            .child(specimen)
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

        // A key that uses one of the user's own behaviors says which kind
        // it is, since the name alone does not.
        let own = match binding {
            Binding::Behavior {
                behavior: kc_model::BehaviorRef::User { user },
                ..
            } => self.project().behavior(*user),
            _ => None,
        };
        if let Some(def) = own {
            let id = def.id;
            panel = panel
                .child(section_title("BEHAVIOR", cx))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().min_w_0().text_sm().child(def.name.clone()))
                        .child(badge(def.kind.name(), cx.theme().foreground)),
                )
                .child(div().text_xs().text_color(muted).child(def.kind.summary()))
                .when(Mode::Behaviors.available(&self.features()), |panel| {
                    panel.child(div().flex().child(
                        chip("edit-behavior", "Edit Behavior", false, cx).on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.behavior = Some(id);
                                this.set_mode(Mode::Behaviors, window, cx);
                            },
                        )),
                    ))
                });
        }

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

    /// Why a keyboard that answered must not be updated from this project:
    /// it is saved as a different keyboard, or is not the device this
    /// keyboard is linked to. `None` when nothing speaks against it.
    fn wrong_keyboard(&self, found: &kc_studio::session::Found, cx: &App) -> Option<String> {
        let device = found.device.as_ref()?;
        let ours = self.saved_keyboard(cx)?;
        let keyboards = self.library.read(cx).keyboards();
        match keyboards.linked_to(device) {
            Some(linked) if linked.id == ours.id => None,
            Some(linked) => Some(format!(
                "The connected keyboard is saved as “{}”, but this layout is open under “{}”. Open the layout under “{}”, or connect “{}”.",
                linked.name, ours.name, linked.name, ours.name
            )),
            None if ours.device.is_some() => Some(format!(
                "The connected keyboard is not the one linked to “{}”. Connect that keyboard, or change its link in My Boards.",
                ours.name
            )),
            None => None,
        }
    }

    /// Making this layout the one the board's firmware is built with.
    fn render_apply(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let name = self.keyboard_name(cx);
        let panel = div()
            .w(px(640.))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .child(div().text_lg().child(format!("Apply to “{name}”")))
            .child(div().text_sm().text_color(muted).child(if self.features().contains(&Feature::Build) {
                format!("Makes this the layout “{name}” is built with, saving it first, and takes you to the board's Build & Flash. The board's firmware and settings are not changed.")
            } else {
                format!("Makes this the layout of “{name}”, saving it first, and takes you to the board's Keyboard tab, where it is written to the keyboard.")
            }))
            .child(
                div().flex().child(
                    Button::new("apply-layout")
                        .primary()
                        .label("Apply to Board")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.commit_inputs(cx);
                            cx.emit(WorkspaceEvent::Apply);
                        })),
                ),
            );
        div()
            .w_full()
            .flex()
            .justify_center()
            .pt_6()
            .pb_4()
            .child(panel)
    }

    /// Looks for a connected keyboard and compares it with the project.
    fn find_keyboard(&mut self, cx: &mut Context<Self>) {
        let project = self.project().clone();
        self.live = LiveStatus::Working("Looking for a keyboard…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { kc_studio::session::find(&project) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.live = match found {
                    Ok(Some(found)) => match this.wrong_keyboard(&found, cx) {
                        Some(message) => LiveStatus::Message(message),
                        None => LiveStatus::Found(found),
                    },
                    Ok(None) => LiveStatus::Message(
                        "No keyboard answered. Connect the main half by USB; its firmware must be built with ZMK Studio.".into(),
                    ),
                    Err(error) => LiveStatus::Message(format!("{error}.")),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Sends the keys that can be changed directly, then looks again.
    fn send_to_keyboard(&mut self, port: String, cx: &mut Context<Self>) {
        let project = self.project().clone();
        self.live = LiveStatus::Working("Updating the keyboard…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let sent = cx
                .background_executor()
                .spawn(async move { kc_studio::session::send(&project, &port) })
                .await;
            let _ = this.update(cx, |this, cx| match sent {
                Ok(count) => {
                    this.notice = Some(format!(
                        "Updated {count} key(s) on the keyboard and saved them there."
                    ));
                    this.find_keyboard(cx);
                }
                Err(error) => {
                    this.live = LiveStatus::Message(format!("{error}."));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Brings the keyboard's keys into the project, as one undo step.
    fn read_from_keyboard(&mut self, port: String, window: &mut Window, cx: &mut Context<Self>) {
        let project = self.project().clone();
        self.live = LiveStatus::Working("Reading the keyboard…".into());
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { kc_studio::session::read(&project, &port) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match read {
                Ok(differences) => {
                    let count = differences.len();
                    this.change("Read From Keyboard", window, cx, |p| {
                        for (layer, position, binding) in differences {
                            let id = p.layers[layer].id;
                            p.set_binding(id, position, binding)?;
                        }
                        Ok(())
                    });
                    this.notice = Some(format!(
                        "Read {count} key(s) from the keyboard into the layout."
                    ));
                    this.find_keyboard(cx);
                }
                Err(error) => {
                    this.live = LiveStatus::Message(format!("{error}."));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn render_live(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted, success) = (theme.border, theme.muted_foreground, theme.success);
        let working = matches!(self.live, LiveStatus::Working(_));
        let panel = div()
            .w(px(640.))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .child(div().text_lg().child("Update the keyboard directly"))
            .child(div().text_sm().text_color(muted).child(
                "With firmware built for ZMK Studio, changes to what keys do can be sent over USB in a moment, without a build. New layers, behaviors, combos, lighting and settings still need a build.",
            ))
            .when(!working, |panel| {
                panel.child(
                    div().flex().child(
                        Button::new("find-keyboard")
                            .label("Find Keyboard")
                            .on_click(cx.listener(|this, _, _, cx| this.find_keyboard(cx))),
                    ),
                )
            });
        let panel = match &self.live {
            LiveStatus::Idle => panel,
            LiveStatus::Working(text) | LiveStatus::Message(text) => {
                panel.child(div().text_sm().child(text.clone()))
            }
            LiveStatus::Found(found) => {
                let panel = panel.child(
                    div()
                        .text_sm()
                        .text_color(success)
                        .child(format!("Connected to {}.", found.name)),
                );
                match &found.comparison {
                    None => panel.child(div().text_sm().child(format!(
                        "It is locked. {} Then press Find Keyboard again.",
                        self.board
                            .flash
                            .studio_unlock
                            .as_deref()
                            .unwrap_or("Press the key assigned to Studio Unlock.")
                    ))),
                    Some(comparison) => match &comparison.mismatch {
                        Some(mismatch) => panel.child(div().text_sm().child(mismatch.clone())),
                        None => {
                            let (send_port, read_port) = (found.port.clone(), found.port.clone());
                            let count = comparison.changes.len();
                            let summary = match (count, comparison.needs_build) {
                                (0, 0) => "The keyboard matches the layout.".to_string(),
                                (0, n) => format!("{n} key(s) differ in ways that need a firmware build."),
                                (c, 0) => format!("{c} key(s) differ and can be sent now."),
                                (c, n) => format!("{c} key(s) differ and can be sent now; {n} more need a firmware build."),
                            };
                            panel.child(div().text_sm().child(summary)).child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .when(count > 0, |row| {
                                        row.child(
                                            Button::new("send-live")
                                                .primary()
                                                .label("Send to Keyboard")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.send_to_keyboard(send_port.clone(), cx);
                                                })),
                                        )
                                    })
                                    .when(count > 0, |row| {
                                        row.child(
                                            Button::new("read-live")
                                                .ghost()
                                                .label("Use the Keyboard's Keys Instead")
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.read_from_keyboard(
                                                            read_port.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                        )
                                    }),
                            )
                        }
                    },
                }
            }
        };
        div().w_full().flex().justify_center().pb_4().child(panel)
    }

    fn render_mode_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let features = self.features();
        let tabs = Mode::TABS
            .into_iter()
            .filter(|(mode, _, _)| mode.available(&features))
            .map(|(mode, id, label)| {
                tab(id, label, self.mode == mode, cx).on_click(
                    cx.listener(move |this, _, window, cx| this.set_mode(mode, window, cx)),
                )
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .items_center()
            .gap_5()
            .px_4()
            .pt_1()
            .border_b_1()
            .border_color(border)
            .children(tabs)
            .child(
                div()
                    .flex_1()
                    .px_3()
                    .text_xs()
                    .text_color(muted)
                    .child(self.notice.clone().unwrap_or_default()),
            )
    }

    /// Shows the key a problem is about, when it is about one.
    fn show_problem(
        &mut self,
        location: &kc_model::Location,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let kc_model::Location::Key { layer, position } = location {
            self.layer = *layer;
            self.selection = vec![*position];
            self.mode = Mode::Keyboard;
            self.after_change(window, cx);
        }
    }

    /// The generated zmk-config files, read-only, or what stops them from
    /// being generated.
    fn render_files(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger, hover, warning) = (
            theme.muted_foreground,
            theme.danger,
            theme.secondary,
            theme.warning,
        );
        let page = div().flex_1().min_h_0().flex().flex_col().gap_2().p_3();

        let problems = kc_firmware::check(self.project(), &self.board, &self.config);
        let problem_rows = problems
            .iter()
            .enumerate()
            .map(|(index, problem)| {
                let location = problem.location.clone();
                let color = if problem.severity == Severity::Error {
                    danger
                } else {
                    warning
                };
                let place = match &problem.location {
                    kc_model::Location::Key { layer, position } => format!(
                        "{}, key {position}: ",
                        self.project()
                            .layer(*layer)
                            .map_or("?", |l| l.name.as_str())
                    ),
                    _ => String::new(),
                };
                div()
                    .id(("problem", index))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_sm()
                    .text_color(color)
                    .cursor_pointer()
                    .hover(|row| row.bg(hover))
                    .child(format!("{place}{}", problem.message))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show_problem(&location, window, cx);
                    }))
            })
            .collect::<Vec<_>>();
        let page = page.when(!problem_rows.is_empty(), |page| {
            page.child(section_title("PROBLEMS", cx))
                .child(div().flex().flex_col().children(problem_rows))
        });

        let files = match kc_firmware::generate(self.project(), &self.board, &self.config) {
            Ok(files) => files,
            Err(_) => {
                return page.child(div().text_sm().text_color(muted).child(
                    "The files cannot be generated until the errors above are fixed. Click one to go to it.",
                ));
            }
        };
        let index = self.file_index.min(files.len().saturating_sub(1));
        let tabs = files
            .iter()
            .enumerate()
            .map(|(i, f)| {
                chip(("file-tab", i), f.path.clone(), i == index, cx).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.file_index = i;
                        cx.notify();
                    },
                ))
            })
            .collect::<Vec<_>>();
        let lines = files[index]
            .contents
            .lines()
            .map(|line| {
                // Keep blank lines from collapsing.
                div().child(if line.is_empty() {
                    " ".to_string()
                } else {
                    line.to_string()
                })
            })
            .collect::<Vec<_>>();
        page.child(div().flex().flex_wrap().gap_1().children(tabs))
            .child(
                div()
                    .id("file-contents")
                    .flex_1()
                    .min_h_0()
                    .overflow_scroll()
                    .p_2()
                    .rounded_md()
                    .bg(hover.opacity(0.4))
                    .font_family("Menlo")
                    .text_xs()
                    .whitespace_nowrap()
                    .children(lines),
            )
    }

    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let problems = kc_firmware::check(self.project(), &self.board, &self.config);
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
            .on_action(cx.listener(|this, _: &ShowKeyboard, _, cx| {
                this.mode = Mode::Keyboard;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowFiles, _, cx| {
                this.mode = Mode::Files;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowFlash, _, cx| {
                this.mode = Mode::Apply;
                cx.notify();
            }))
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
            .flex_col()
            .child(self.render_breadcrumb(cx))
            .child(self.render_editor(cx))
    }
}

impl Workspace {
    /// Everything under the breadcrumb: the layer list and the main area.
    fn render_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(self.render_sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.render_mode_bar(cx))
                    .map(|main| match self.mode {
                        Mode::Keyboard => main.child(self.render_canvas(cx)).child(
                            div()
                                .h_72()
                                .flex()
                                .border_t_1()
                                .border_color(border)
                                .child(self.render_picker(cx))
                                .child(self.render_inspector(cx)),
                        ),
                        Mode::Lighting => main.child(self.render_lighting(cx)),
                        Mode::Behaviors => main
                            .child(self.render_behaviors(cx))
                            .child(self.render_slot_picker(cx)),
                        Mode::Combos => main
                            .child(self.render_combos(cx))
                            .child(self.render_slot_picker(cx)),
                        Mode::Pointing => main.child(self.render_pointing(cx)),
                        Mode::Advanced => main.child(self.render_advanced(cx)),
                        Mode::Files => main.child(self.render_files(cx)),
                        Mode::Apply => main.child(
                            div()
                                .id("apply")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .child(self.render_apply(cx))
                                .when(self.features().contains(&Feature::Studio), |page| {
                                    page.child(self.render_live(cx))
                                }),
                        ),
                    })
                    .child(self.render_status(cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: the GUI toolkit's own `test` macro would replace the
    // standard one.
    use super::Mode;

    /// The tabs a firmware profile of a built-in board gets.
    fn tabs(board: &str, firmware: &str) -> Vec<&'static str> {
        let boards = kc_boards::built_in().unwrap();
        let board = boards.iter().find(|b| b.id == board).unwrap();
        let features = &board.profile(firmware).unwrap().features();
        Mode::TABS
            .into_iter()
            .filter(|(mode, _, _)| mode.available(features))
            .map(|(_, _, label)| label)
            .collect()
    }

    #[test]
    fn tabs_follow_what_the_firmware_can_do() {
        let without_lighting = [
            "Keyboard",
            "Behaviors",
            "Combos",
            "Pointing",
            "Advanced",
            "Generated Files",
            "Apply",
        ];
        assert_eq!(tabs("moergo-go60", "moergo-zmk-26.09"), without_lighting);
        assert_eq!(tabs("cyboard-imprint", "cyboard-zmk-0.3"), without_lighting);

        for (board, firmware) in [
            ("moergo-go60", "moergo-zmk-perkey"),
            ("cyboard-imprint", "kc-zmk-0.3-perkey"),
        ] {
            let tabs = tabs(board, firmware);
            assert_eq!(tabs.len(), 8);
            assert_eq!(tabs[1], "Lighting");
        }

        // A firmware with nothing optional keeps the modes every ZMK has.
        assert!(!Mode::Lighting.available(&[]));
        assert!(!Mode::Pointing.available(&[]));
        assert!(!Mode::Combos.available(&[]));
        assert!(!Mode::Advanced.available(&[]));
        assert!(!Mode::Behaviors.available(&[]));
        assert!(!Mode::Files.available(&[]));

        // RMK builds from generated files, and has combos; behaviors
        // defined in a layout are not translated to it yet.
        assert_eq!(
            tabs("cyboard-imprint", "rmk-0.9"),
            ["Keyboard", "Combos", "Generated Files", "Apply"]
        );
        // Dygma's firmware is configured live: keys and colors, no files.
        assert_eq!(
            tabs("dygma-defy", "dygma-defy"),
            ["Keyboard", "Lighting", "Apply"]
        );
        assert!(Mode::Keyboard.available(&[]));
        assert!(Mode::Apply.available(&[]));
    }
}
