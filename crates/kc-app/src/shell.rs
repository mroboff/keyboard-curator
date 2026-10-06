//! The window's root view. It moves between three screens: My Boards, one
//! board's page, and the layout editor opened from it.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_boards::board::Side;
use kc_boards::Board;
use kc_device::UsbDevice;
use kc_model::keyboards::{hidden, Placement};
use kc_model::{file, Carried, Keyboard, KeyboardId, Project};

use crate::board_page::{BoardEvent, BoardPage, Section};
use crate::library::Library;
use crate::state::{default_project_dir, AppState, Appearance, WindowFrame};
use crate::workspace::{chip, Workspace, WorkspaceEvent};
use crate::{
    CloseProject, ExportConfig, ImportProject, NewProject, OpenProject, Redo, Save, SaveAs, Undo,
};

/// A board being added on the welcome screen.
struct KeyboardForm {
    /// The boards to choose between, as indices into the board list.
    choices: Vec<usize>,
    board: usize,
    firmware: String,
    /// The connected device the new board will be linked to.
    device: Option<UsbDevice>,
    /// Something the user should know about that device.
    note: Option<String>,
    /// The name last suggested, replaced when the model changes unless the
    /// user has typed their own.
    suggested: String,
}

/// A layout on its way to being opened, while the board it opens under is
/// settled.
struct Pending {
    project: Project,
    board: Board,
    path: Option<PathBuf>,
    /// A message for the editor to show once open.
    notice: Option<String>,
    /// Firmware settings that arrived with the layout, for the board.
    carried: Carried,
}

pub struct Shell {
    boards: Rc<Vec<Board>>,
    state: AppState,
    library: Entity<Library>,
    /// The page of the board being worked on. It stays underneath an open
    /// layout, which closes back to it.
    page: Option<Entity<BoardPage>>,
    workspace: Option<Entity<Workspace>>,
    form: Option<KeyboardForm>,
    name_input: Entity<InputState>,
    /// The last thing that went wrong, shown until the next action.
    error: Option<String>,
    /// A passing remark on the welcome screen.
    notice: Option<String>,
    /// True while a layout is being saved so that it can be applied.
    applying: bool,
    focus: FocusHandle,
}

fn firmware_name(board: &Board, id: &str) -> String {
    board
        .profile(id)
        .map_or_else(|| id.to_string(), |p| p.name.clone())
}

/// Which half of each board to connect, for when none is found.
pub(crate) fn connection_hint(boards: &[Board]) -> String {
    let halves = boards
        .iter()
        .filter_map(|board| {
            let central = board.halves.iter().find(|h| h.central)?;
            let side = match central.side {
                Side::Left => "left",
                Side::Right => "right",
            };
            Some(format!("the {side} half of the {}", board.name))
        })
        .collect::<Vec<_>>();
    format!(
        "Connect the main half with a USB data cable: {}.",
        halves.join(", ")
    )
}

impl Shell {
    pub fn new(
        boards: Rc<Vec<Board>>,
        state: AppState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        cx.observe_window_bounds(window, |this, window, _| {
            let bounds = window.bounds();
            this.state.window = Some(WindowFrame {
                x: bounds.origin.x.into(),
                y: bounds.origin.y.into(),
                width: bounds.size.width.into(),
                height: bounds.size.height.into(),
            });
            this.state.save();
        })
        .detach();
        cx.observe_window_appearance(window, |_, window, cx| {
            crate::appearance::follow(window, cx);
        })
        .detach();
        let library = cx.new(Library::new);
        cx.observe(&library, |_, _, cx| cx.notify()).detach();
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Board name"));
        cx.subscribe_in(
            &name_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_form(window, cx);
                }
            },
        )
        .detach();
        Self {
            boards,
            state,
            library,
            page: None,
            workspace: None,
            form: None,
            name_input,
            error: None,
            notice: None,
            applying: false,
            focus,
        }
    }

    /// Whether the open layout has changes that are not on disk.
    pub fn has_unsaved_changes(&self, cx: &App) -> bool {
        self.workspace
            .as_ref()
            .is_some_and(|w| w.read(cx).is_dirty())
    }

    fn board_index(&self, id: &str) -> Option<usize> {
        self.boards.iter().position(|b| b.id == id)
    }

    fn keyboard(&self, id: KeyboardId, cx: &App) -> Option<Keyboard> {
        self.library.read(cx).keyboards().get(id).cloned()
    }

    /// The board being worked on: the one whose page is showing, or else
    /// the one last opened.
    fn selected(&self, cx: &App) -> Option<KeyboardId> {
        self.page
            .as_ref()
            .map(|page| page.read(cx).keyboard())
            .or(self.state.keyboard)
    }

    // Boards

    /// Shows a board's page.
    fn open_board(
        &mut self,
        id: KeyboardId,
        section: Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(keyboard) = self.keyboard(id, cx) else {
            return;
        };
        let Some(board) = self.boards.iter().find(|b| b.id == keyboard.board).cloned() else {
            self.error = Some(format!(
                "This version of Keyboard Curator does not know the board `{}`.",
                keyboard.board
            ));
            cx.notify();
            return;
        };
        let (boards, library) = (self.boards.clone(), self.library.clone());
        let page = cx.new(|cx| BoardPage::new(boards, board, library, id, section, window, cx));
        cx.subscribe_in(&page, window, Self::on_board_event)
            .detach();
        self.page = Some(page);
        self.form = None;
        self.error = None;
        self.notice = None;
        if self.state.keyboard != Some(id) {
            self.state.keyboard = Some(id);
            self.state.save();
        }
        cx.notify();
    }

    fn on_board_event(
        &mut self,
        page: &Entity<BoardPage>,
        event: &BoardEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = page.read(cx).keyboard();
        match event {
            BoardEvent::Back => {
                self.page = None;
                self.focus.focus(window, cx);
                cx.notify();
            }
            BoardEvent::NewLayout => self.new_layout(id, window, cx),
            BoardEvent::OpenLayout(path) => self.open_path(path.clone(), window, cx),
            BoardEvent::ChooseLayout => self.open(&OpenProject, window, cx),
            BoardEvent::Import => self.import(&ImportProject, window, cx),
            BoardEvent::Remove => self.remove_keyboard(id, window, cx),
            BoardEvent::Loaded(project) => {
                let Some(board) = self.boards.iter().find(|b| b.id == project.board).cloned()
                else {
                    return;
                };
                let pending = Pending {
                    project: (**project).clone(),
                    board,
                    path: None,
                    notice: Some(
                        "Read from the keyboard. Save it to keep it as a layout file.".into(),
                    ),
                    carried: Carried::default(),
                };
                self.open_editor(pending, id, window, cx);
            }
        }
    }

    fn remove_keyboard(&mut self, id: KeyboardId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(keyboard) = self.keyboard(id, cx) else {
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Remove “{}” from My Boards?", keyboard.name),
            Some("Its firmware settings are forgotten. Its layout files stay where they are, and the keyboard itself is not changed."),
            &["Cancel", "Remove"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(1) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.library.update(cx, |library, cx| {
                    let _ = library.change(cx, |k| k.remove(id).map(|_| ()));
                });
                this.page = None;
                if this.state.keyboard == Some(id) {
                    this.state.keyboard = None;
                    this.state.save();
                }
                cx.notify();
            });
        })
        .detach();
    }

    // Opening layouts

    /// Opens a layout under a board, first offering the board any firmware
    /// settings that arrived with it.
    fn show(
        &mut self,
        mut pending: Pending,
        keyboard: KeyboardId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if pending.carried.is_empty() {
            self.open_editor(pending, keyboard, window, cx);
            return;
        }
        let name = self
            .keyboard(keyboard, cx)
            .map_or_else(String::new, |k| k.name);
        let carried = std::mem::take(&mut pending.carried);
        let answer = window.prompt(
            PromptLevel::Info,
            &format!("Apply this file's firmware settings to “{name}”?"),
            Some(&format!(
                "It carries firmware settings: {}. Those belong to the board, not to the layout. Applying them replaces the board's own values for the same settings; ignoring them leaves the board as it is.",
                carried.summary()
            )),
            &["Ignore", "Apply to Board"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let apply = answer.await == Ok(1);
            let _ = this.update_in(cx, |this, window, cx| {
                if apply {
                    this.library.update(cx, |library, cx| {
                        let _ = library.change(cx, |k| k.absorb(keyboard, carried));
                    });
                }
                this.open_editor(pending, keyboard, window, cx);
            });
        })
        .detach();
    }

    fn open_editor(
        &mut self,
        pending: Pending,
        keyboard: KeyboardId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Pending {
            project,
            board,
            path,
            notice,
            ..
        } = pending;
        let Some(saved) = self.keyboard(keyboard, cx) else {
            return;
        };
        // A board without a repository folder adopts the one this layout
        // built from before folders were kept per board.
        let earlier = saved
            .repo_dir
            .is_none()
            .then(|| self.state.earlier_repo(path.as_deref(), &board.id))
            .flatten();
        self.library.update(cx, |library, cx| {
            let _ = library.change(cx, |k| {
                if let Some(dir) = earlier {
                    k.set_repo_dir(keyboard, Some(dir))?;
                }
                if let Some(path) = &path {
                    k.note_layout(keyboard, path.clone())?;
                }
                Ok(())
            });
        });
        if let Some(path) = &path {
            self.state.forget_recent(path);
            self.state.save();
        }
        // The layout closes back to its board's page.
        let on_page = self
            .page
            .as_ref()
            .is_some_and(|page| page.read(cx).keyboard() == keyboard);
        if !on_page {
            self.open_board(keyboard, Section::Layouts, window, cx);
        }

        // Say what this board's firmware keeps out of sight.
        let unseen = hidden(&project, &saved.firmware.features(&board)).summary();
        let notice = match (notice, unseen) {
            (Some(notice), Some(unseen)) => Some(format!("{notice} {unseen}")),
            (notice, unseen) => notice.or(unseen),
        };
        let library = self.library.clone();
        let workspace =
            cx.new(|cx| Workspace::new(project, board, library, keyboard, path, window, cx));
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(
            &workspace,
            window,
            |this, workspace, event: &WorkspaceEvent, window, cx| match event {
                WorkspaceEvent::Apply => this.apply(workspace.clone(), window, cx),
                // Asks first if the layout has unsaved changes.
                WorkspaceEvent::Close => this.close(&CloseProject, window, cx),
            },
        )
        .detach();
        workspace.update(cx, |w, cx| {
            w.focus_canvas(window, cx);
            if let Some(notice) = notice {
                w.set_notice(notice, cx);
            }
        });
        self.workspace = Some(workspace);
        if let Some(page) = &self.page {
            page.update(cx, |page, cx| page.set_aside(cx));
        }
        self.applying = false;
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    /// Settles which board a layout opens under, asking where there is a
    /// choice to make, and opens it.
    fn place(
        &mut self,
        project: Project,
        path: Option<PathBuf>,
        notice: Option<String>,
        carried: Carried,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(board) = self.boards.iter().find(|b| b.id == project.board).cloned() else {
            self.error = Some(format!(
                "This layout is for an unknown board, `{}`.",
                project.board
            ));
            cx.notify();
            return;
        };
        let placement = self
            .library
            .read(cx)
            .keyboards()
            .place(&project, self.selected(cx));
        let pending = Pending {
            project,
            board,
            path,
            notice,
            carried,
        };
        match placement {
            Placement::Open(id) => self.show(pending, id, window, cx),
            Placement::Choose(choices) => self.choose_keyboard(pending, choices, window, cx),
            Placement::NoKeyboard => self.offer_keyboard(pending, window, cx),
        }
    }

    /// Asks which of several boards of the right model a layout is for.
    fn choose_keyboard(
        &mut self,
        pending: Pending,
        choices: Vec<KeyboardId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let names: Vec<String> = choices
            .iter()
            .filter_map(|id| self.keyboard(*id, cx).map(|k| k.name))
            .collect();
        let mut buttons = vec!["Cancel"];
        buttons.extend(names.iter().map(String::as_str));
        let answer = window.prompt(
            PromptLevel::Info,
            &format!("Which board is “{}” for?", pending.project.name),
            Some("Several of your boards could use this layout."),
            &buttons,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(index) = answer.await else { return };
            let Some(id) = index.checked_sub(1).and_then(|i| choices.get(i)).copied() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| this.show(pending, id, window, cx));
        })
        .detach();
    }

    /// Offers to add a board for a layout that none of the saved ones is
    /// the model for, since layouts are always opened under a board.
    fn offer_keyboard(&mut self, pending: Pending, window: &mut Window, cx: &mut Context<Self>) {
        let board = &pending.board;
        let name = self.library.read(cx).keyboards().suggest_name(board);
        let firmware = board.firmware[0].id.clone();
        let answer = window.prompt(
            PromptLevel::Info,
            &format!("Add “{name}” to My Boards?"),
            Some(&format!(
                "“{}” is a layout for a {} {}, and none of your boards is one. Layouts are opened under a board. It will start with {}; you can change the firmware and its settings on the board's page.",
                pending.project.name,
                board.vendor,
                board.name,
                firmware_name(board, &firmware)
            )),
            &["Cancel", "Add and Open"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(1) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                let added = this.library.update(cx, |library, cx| {
                    library.change(cx, |k| k.add(&name, &pending.board, &firmware))
                });
                match added {
                    Ok(id) => this.show(pending, id, window, cx),
                    Err(error) => {
                        this.error = Some(format!("{error}."));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Starts a layout from the board's factory layout.
    fn new_layout(&mut self, id: KeyboardId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(keyboard) = self.keyboard(id, cx) else {
            return;
        };
        let Some(board) = self.boards.iter().find(|b| b.id == keyboard.board).cloned() else {
            return;
        };
        let project = Project::from_template(format!("{} Layout", board.name), &board);
        let pending = Pending {
            project,
            board,
            path: None,
            notice: None,
            carried: Carried::default(),
        };
        self.open_editor(pending, id, window, cx);
    }

    pub fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // A file from before settings moved to the board still carries them.
        match file::load_carrying(&path) {
            Ok((project, carried)) => self.place(project, Some(path), None, carried, window, cx),
            Err(error) => {
                self.state.forget_recent(&path);
                self.state.save();
                self.error = Some(format!("Could not open {}: {error}", path.display()));
                cx.notify();
            }
        }
    }

    fn open(&mut self, _: &OpenProject, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| this.open_path(path, window, cx));
        })
        .detach();
    }

    /// Imports a `.keymap` file or a MoErgo Layout Editor export as a new,
    /// unsaved layout. The source file is never changed.
    fn import(&mut self, _: &ImportProject, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| this.import_path(path, window, cx));
        })
        .detach();
    }

    pub fn import_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // From a board's page the keymap is read as one for that board;
        // otherwise for whichever board it fits.
        let on_page = self
            .page
            .as_ref()
            .map(|page| page.read(cx).keyboard())
            .and_then(|id| self.keyboard(id, cx))
            .and_then(|k| self.boards.iter().find(|b| b.id == k.board).cloned());
        let boards: &[Board] = match &on_page {
            Some(board) => std::slice::from_ref(board),
            None => &self.boards,
        };
        let imported = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                // A keymap's settings live in a `.conf` file beside it.
                let conf = std::fs::read_to_string(path.with_extension("conf")).ok();
                kc_import::import_file(&path.display().to_string(), &text, conf.as_deref(), boards)
                    .map_err(|e| e.to_string())
            });
        match imported {
            Ok(imported) => {
                let notice = Some(imported.report.summary());
                self.place(imported.project, None, notice, imported.carried, window, cx);
            }
            Err(message) => {
                let whose =
                    on_page.map_or_else(String::new, |b| format!(" as a {} keymap", b.name));
                self.error = Some(format!(
                    "Could not import {}{whose}: {message}",
                    path.display()
                ));
                cx.notify();
            }
        }
    }

    fn save_as(&mut self, _: &SaveAs, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let (directory, name) = {
            let workspace = workspace.read(cx);
            let directory = workspace
                .path()
                .and_then(|p| p.parent())
                .map_or_else(default_project_dir, PathBuf::from);
            let name = format!("{}.{}", workspace.project().name, file::EXTENSION);
            (directory, name)
        };
        let _ = std::fs::create_dir_all(&directory);
        let chosen = cx.prompt_for_new_path(&directory, Some(&name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut path))) = chosen.await else {
                // Nothing was saved, so nothing is applied either.
                let _ = this.update(cx, |this, _| this.applying = false);
                return;
            };
            if path.extension().is_none() {
                path.set_extension(file::EXTENSION);
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.write(&workspace, path, window, cx);
            });
        })
        .detach();
    }

    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        match workspace.read(cx).path().map(PathBuf::from) {
            Some(path) => self.write(&workspace, path, window, cx),
            None => self.save_as(&SaveAs, window, cx),
        }
    }

    fn write(
        &mut self,
        workspace: &Entity<Workspace>,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = workspace.update(cx, |w, cx| {
            cx.notify();
            w.save_to(path.clone())
        });
        let applying = std::mem::take(&mut self.applying);
        match result {
            Ok(()) => {
                let keyboard = workspace.read(cx).keyboard();
                self.library.update(cx, |library, cx| {
                    let _ = library.change(cx, |k| k.note_layout(keyboard, path.clone()));
                });
                self.error = None;
                if applying {
                    self.finish_apply(keyboard, &path, window, cx);
                }
            }
            Err(e) => self.error = Some(format!("Could not save {}: {e}", path.display())),
        }
        cx.notify();
    }

    /// Makes the open layout its board's current one. The board builds
    /// from the file, so the layout is saved first.
    fn apply(&mut self, workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) {
        let (path, dirty, keyboard) = {
            let workspace = workspace.read(cx);
            (
                workspace.path().map(PathBuf::from),
                workspace.is_dirty(),
                workspace.keyboard(),
            )
        };
        match path {
            Some(path) if !dirty => self.finish_apply(keyboard, &path, window, cx),
            Some(path) => {
                self.applying = true;
                self.write(&workspace, path, window, cx);
            }
            None => {
                self.applying = true;
                self.save_as(&SaveAs, window, cx);
            }
        }
    }

    fn finish_apply(
        &mut self,
        keyboard: KeyboardId,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.library.update(cx, |library, cx| {
            let _ = library.change(cx, |k| k.set_current(keyboard, Some(path.to_path_buf())));
        });
        self.workspace = None;
        let on_page = self
            .page
            .as_ref()
            .is_some_and(|page| page.read(cx).keyboard() == keyboard);
        if !on_page {
            self.open_board(keyboard, Section::Build, window, cx);
        }
        let name = path
            .file_stem()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if let Some(page) = &self.page {
            page.update(cx, |page, cx| {
                page.show_section(Section::Build, cx);
                page.set_notice(
                    format!("“{name}” is now this board's layout. Build and flash to put it on the keyboard."),
                    cx,
                );
            });
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Starts something new for where the user is: a board on My Boards, a
    /// layout on a board's page. With a layout open, closes it first.
    fn new_action(&mut self, _: &NewProject, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_some() {
            self.close(&CloseProject, window, cx);
            return;
        }
        match self.page.as_ref().map(|page| page.read(cx).keyboard()) {
            Some(id) => self.new_layout(id, window, cx),
            None => self.start_add(window, cx),
        }
    }

    /// Closes the open layout back to its board, or a board's page back to
    /// My Boards.
    fn close(&mut self, _: &CloseProject, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_none() {
            self.page = None;
            self.focus.focus(window, cx);
            cx.notify();
            return;
        }
        if !self.has_unsaved_changes(cx) {
            self.workspace = None;
            self.focus.focus(window, cx);
            cx.notify();
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            "Close this layout without saving?",
            Some("Your changes will be lost."),
            &["Cancel", "Close Without Saving"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update(cx, |this, cx| {
                    this.workspace = None;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Exports the config of the board whose page is showing.
    fn export(&mut self, _: &ExportConfig, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_some() {
            return;
        }
        if let Some(page) = &self.page {
            page.update(cx, |page, cx| page.export(window, cx));
        }
    }

    /// Chooses light, dark or whatever the computer is set to, from the
    /// View menu, and remembers it.
    fn set_appearance(&mut self, appearance: Appearance, cx: &mut Context<Self>) {
        self.state.appearance = appearance;
        self.state.save();
        crate::appearance::apply(appearance, cx);
        cx.set_menus(crate::menus(appearance));
        cx.notify();
    }

    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(workspace) = &self.workspace {
            workspace.update(cx, |w, cx| {
                w.undo(window, cx);
            });
        }
    }

    fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(workspace) = &self.workspace {
            workspace.update(cx, |w, cx| {
                w.redo(window, cx);
            });
        }
    }

    // Adding boards

    fn open_form(&mut self, form: KeyboardForm, window: &mut Window, cx: &mut Context<Self>) {
        let name = form.suggested.clone();
        self.name_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.form = Some(form);
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    /// Starts adding a board that need not be connected, or even owned.
    fn start_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let board = self
            .state
            .keyboard
            .and_then(|id| self.keyboard(id, cx))
            .and_then(|k| self.board_index(&k.board))
            .unwrap_or(0);
        let suggested = self
            .library
            .read(cx)
            .keyboards()
            .suggest_name(&self.boards[board]);
        let form = KeyboardForm {
            choices: (0..self.boards.len()).collect(),
            board,
            firmware: self.boards[board].firmware[0].id.clone(),
            device: None,
            note: None,
            suggested,
        };
        self.open_form(form, window, cx);
    }

    /// Looks for a keyboard on USB and starts adding it, linked to that
    /// device. One that is saved already is opened instead.
    fn add_connected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let detected = kc_device::detect(&self.boards);
        let keyboards = self.library.read(cx).keyboards();
        let saved = |usb: &UsbDevice| usb.link().and_then(|link| keyboards.linked_to(&link));
        let fresh = detected.iter().find(|d| saved(&d.usb).is_none());
        let already = detected.iter().find_map(|d| saved(&d.usb));
        match (fresh, already) {
            (Some(found), _) => {
                let board = found.boards[0];
                let mut notes = Vec::new();
                if !found.certain {
                    notes.push(
                        "Its name does not say for certain which model it is, so check the model.",
                    );
                }
                if found.usb.link().is_none() {
                    notes.push("It reports no serial number, so it cannot be linked and will not be recognized later.");
                }
                let form = KeyboardForm {
                    choices: found.boards.clone(),
                    board,
                    firmware: self.boards[board].firmware[0].id.clone(),
                    device: Some(found.usb.clone()),
                    note: (!notes.is_empty()).then(|| notes.join(" ")),
                    suggested: keyboards.suggest_name(&self.boards[board]),
                };
                self.open_form(form, window, cx);
            }
            (None, Some(keyboard)) => {
                let (id, name) = (keyboard.id, keyboard.name.clone());
                self.open_board(id, Section::Firmware, window, cx);
                if let Some(page) = &self.page {
                    page.update(cx, |page, cx| {
                        page.set_notice(
                            format!("The connected keyboard is already saved as “{name}”."),
                            cx,
                        );
                    });
                }
            }
            (None, None) => {
                self.notice = Some(format!(
                    "No supported keyboard was found on USB. {}",
                    connection_hint(&self.boards)
                ));
                cx.notify();
            }
        }
    }

    /// Changes the model of the board being added, with that model's first
    /// firmware and, unless the user has typed a name, a fitting name.
    fn set_form_board(&mut self, board: usize, window: &mut Window, cx: &mut Context<Self>) {
        let suggested = self
            .library
            .read(cx)
            .keyboards()
            .suggest_name(&self.boards[board]);
        let typed = self.name_input.read(cx).value().to_string();
        let Some(form) = &mut self.form else { return };
        form.board = board;
        form.firmware = self.boards[board].firmware[0].id.clone();
        if typed.trim().is_empty() || typed == form.suggested {
            self.name_input.update(cx, |input, cx| {
                input.set_value(suggested.clone(), window, cx);
            });
        }
        form.suggested = suggested;
        cx.notify();
    }

    /// Adds the board and opens its page, where its firmware is set up.
    fn submit_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        let name = self.name_input.read(cx).value().to_string();
        let board = self.boards[form.board].clone();
        let firmware = form.firmware.clone();
        let link = form.device.as_ref().and_then(UsbDevice::link);
        let result = self.library.update(cx, |library, cx| {
            library.change(cx, |k| {
                let id = k.add(&name, &board, &firmware)?;
                // The device was free when the form opened; if it has been
                // taken since, the board is added unlinked.
                if let Some(link) = link {
                    let _ = k.link(id, link);
                }
                Ok(id)
            })
        });
        match result {
            Ok(id) => self.open_board(id, Section::Firmware, window, cx),
            Err(error) => {
                self.error = Some(format!("{error}."));
                cx.notify();
            }
        }
    }

    // Welcome screen

    /// Layouts opened before boards were saved, until each is opened again.
    fn render_earlier(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (muted, hover) = (theme.muted_foreground, theme.secondary);
        let rows = self
            .state
            .recent
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let target = path.clone();
                let name = path
                    .file_stem()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                div()
                    .id(("earlier", index))
                    .px_3()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|row| row.bg(hover))
                    .child(div().text_sm().child(name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(path.display().to_string()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_path(target.clone(), window, cx);
                    }))
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(
                div()
                    .px_3()
                    .pb_1()
                    .text_xs()
                    .text_color(muted)
                    .child("OPENED BEFORE MY BOARDS"),
            )
            .children(rows)
    }

    /// The list of boards, and the ways to add one.
    fn render_boards(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, hover, border, success) = (
            theme.muted_foreground,
            theme.secondary,
            theme.border,
            theme.success,
        );
        let library = self.library.read(cx);
        let rows = library
            .keyboards()
            .iter()
            .map(|keyboard| {
                let id = keyboard.id;
                let board = self.boards.iter().find(|b| b.id == keyboard.board);
                let detail = match board {
                    Some(board) => format!(
                        "{} {} · {}",
                        board.vendor,
                        board.name,
                        firmware_name(board, &keyboard.firmware.profile)
                    ),
                    None => format!("Unknown board `{}`", keyboard.board),
                };
                let layouts = match keyboard.layouts.len() {
                    0 => "No layouts yet".to_string(),
                    1 => "1 layout".to_string(),
                    n => format!("{n} layouts"),
                };
                let connected = library.is_connected(keyboard);
                div()
                    .id(("keyboard", id.0 as usize))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .cursor_pointer()
                    .hover(|row| row.bg(hover))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().flex_1().min_w_0().child(keyboard.name.clone()))
                            .when(connected, |row| {
                                row.child(div().text_xs().text_color(success).child("Connected"))
                            }),
                    )
                    .child(div().text_xs().text_color(muted).child(detail))
                    .child(div().text_xs().text_color(muted).child(layouts))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_board(id, Section::Firmware, window, cx);
                    }))
            })
            .collect::<Vec<_>>();
        div()
            .w_80()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().px_1().text_xs().text_color(muted).child("MY BOARDS"))
            .child(
                div()
                    .id("keyboard-list")
                    .max_h_96()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(rows),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("add-keyboard")
                            .primary()
                            .label("Add Board…")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_add(window, cx);
                            })),
                    )
                    .child(
                        Button::new("add-connected")
                            .label("Add Connected Board")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_connected(window, cx);
                            })),
                    ),
            )
    }

    /// The form for adding a board.
    fn render_form(&self, form: &KeyboardForm, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let label = |text: &'static str| div().text_xs().text_color(muted).child(text);
        let board = &self.boards[form.board];
        let title = if form.device.is_some() {
            "Add the Connected Board"
        } else {
            "Add a Board"
        };
        let models = form
            .choices
            .iter()
            .map(|&index| {
                let name = format!("{} {}", self.boards[index].vendor, self.boards[index].name);
                chip(("form-board", index), name, index == form.board, cx).on_click(
                    cx.listener(move |this, _, window, cx| this.set_form_board(index, window, cx)),
                )
            })
            .collect::<Vec<_>>();
        let firmwares = board
            .firmware
            .iter()
            .enumerate()
            .map(|(index, profile)| {
                let id = profile.id.clone();
                chip(
                    ("form-firmware", index),
                    profile.name.clone(),
                    profile.id == form.firmware,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(form) = &mut this.form {
                        form.firmware = id.clone();
                        cx.notify();
                    }
                }))
            })
            .collect::<Vec<_>>();
        let device = form.device.as_ref().map(|usb| match usb.link() {
            Some(link) => format!(
                "Found {} on USB. It will be linked to this board (serial {}).",
                usb.label(),
                link.serial
            ),
            None => format!("Found {} on USB.", usb.label()),
        });
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_xl().child(title))
            .when(form.device.is_none(), |page| {
                page.child(div().text_sm().text_color(muted).child(
                    "The keyboard does not need to be connected, or even one you own.",
                ))
            })
            .when_some(device, |page, device| {
                page.child(div().text_sm().child(device))
            })
            .when_some(form.note.clone(), |page, note| {
                page.child(div().text_sm().text_color(muted).child(note))
            })
            .child(label("NAME"))
            .child(div().w_72().child(Input::new(&self.name_input)))
            .child(label("MODEL"))
            .child(div().flex().flex_wrap().gap_1().children(models))
            .child(label("FIRMWARE"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_1()
                    .children(firmwares),
            )
            .child(div().text_xs().text_color(muted).child(
                "The firmware the keyboard runs, or will be built with. A keyboard cannot report this itself. You can change it, and adjust its settings, on the board's page.",
            ))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .pt_2()
                    .child(
                        Button::new("form-submit")
                            .primary()
                            .label("Add Board")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.submit_form(window, cx);
                            })),
                    )
                    .child(
                        Button::new("form-cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.form = None;
                                this.error = None;
                                cx.notify();
                            })),
                    ),
            )
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let empty = self.library.read(cx).keyboards().is_empty();
        let detail = match &self.form {
            Some(form) => self.render_form(form, cx),
            None => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_xl().child(if empty {
                    "Add your first board"
                } else {
                    "Choose a board"
                }))
                .child(div().max_w_96().text_sm().text_color(muted).child(
                    "A board is one of your keyboards: its firmware and that firmware's settings. Open one to set up its firmware, build and flash it, and create the layouts used with it. A board does not need to be connected, or even one you own.",
                )),
        };
        let earlier =
            (self.form.is_none() && !self.state.recent.is_empty()).then(|| self.render_earlier(cx));

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_6()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .child(div().text_2xl().child("Keyboard Curator"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child("Keymaps, layers and lighting for ZMK keyboards"),
                    ),
            )
            .when_some(self.notice.clone(), |page, notice| {
                page.child(div().max_w(px(760.)).text_sm().child(notice))
            })
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_6()
                    .child(self.render_boards(cx))
                    .child(
                        div()
                            .id("welcome-detail")
                            .w(px(460.))
                            .max_h(px(520.))
                            .overflow_y_scroll()
                            .pl_6()
                            .border_l_1()
                            .border_color(border)
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(detail)
                            .when_some(earlier, |pane, earlier| pane.child(earlier)),
                    ),
            )
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let board_name = self
            .page
            .as_ref()
            .map(|page| page.read(cx).keyboard())
            .and_then(|id| self.keyboard(id, cx))
            .map(|k| k.name);
        let title = match (&self.workspace, board_name) {
            (Some(workspace), _) => workspace.read(cx).title(cx),
            (None, Some(name)) => format!("{name} — Keyboard Curator"),
            (None, None) => "Keyboard Curator".to_string(),
        };
        window.set_window_title(&title);

        let theme = cx.theme();
        let (background, foreground, danger) = (theme.background, theme.foreground, theme.danger);
        let error = self
            .error
            .clone()
            .or_else(|| self.library.read(cx).problem().map(str::to_string));
        let content = match (&self.workspace, &self.page) {
            (Some(workspace), _) => workspace.clone().into_any_element(),
            (None, Some(page)) => page.clone().into_any_element(),
            (None, None) => self.render_welcome(cx).into_any_element(),
        };
        div()
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::import))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::save_as))
            .on_action(cx.listener(Self::close))
            .on_action(cx.listener(Self::new_action))
            .on_action(cx.listener(Self::export))
            .on_action(cx.listener(|this, _: &crate::AppearanceSystem, _, cx| {
                this.set_appearance(Appearance::System, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::AppearanceLight, _, cx| {
                this.set_appearance(Appearance::Light, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::AppearanceDark, _, cx| {
                this.set_appearance(Appearance::Dark, cx);
            }))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .size_full()
            .flex()
            .flex_col()
            .bg(background)
            .text_color(foreground)
            .when_some(error, |page, error| {
                page.child(
                    div()
                        .px_4()
                        .py_2()
                        .text_sm()
                        .text_color(danger)
                        .child(error),
                )
            })
            .child(div().flex_1().min_h_0().child(content))
    }
}
