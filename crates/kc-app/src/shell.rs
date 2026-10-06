//! The window's root view: My Boards, or a project open under one of the
//! user's keyboards, plus the file commands that move between them.

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
use kc_model::keyboards::{preview_retarget, Fit, Placement};
use kc_model::{file, Keyboard, KeyboardId, Project};

use crate::library::Library;
use crate::state::{default_project_dir, AppState, WindowFrame};
use crate::workspace::{chip, Workspace};
use crate::{CloseProject, ImportProject, NewProject, OpenProject, Redo, Save, SaveAs, Undo};

/// A keyboard being added or edited on the welcome screen.
struct KeyboardForm {
    /// The keyboard being edited; `None` while adding one.
    editing: Option<KeyboardId>,
    /// The boards to choose between, as indices into the board list.
    choices: Vec<usize>,
    board: usize,
    firmware: String,
    /// The connected device a new keyboard will be linked to.
    device: Option<UsbDevice>,
    /// Something the user should know about that device.
    note: Option<String>,
    /// The name last suggested, replaced when the board changes unless the
    /// user has typed their own.
    suggested: String,
}

/// A project on its way to being opened, while the keyboard it opens under
/// is settled.
struct Pending {
    project: Project,
    board: Board,
    path: Option<PathBuf>,
    /// A message for the workspace to show once open.
    notice: Option<String>,
}

pub struct Shell {
    boards: Rc<Vec<Board>>,
    state: AppState,
    library: Entity<Library>,
    /// The keyboard the welcome screen shows and new work happens under.
    selected: Option<KeyboardId>,
    workspace: Option<Entity<Workspace>>,
    form: Option<KeyboardForm>,
    name_input: Entity<InputState>,
    /// The last thing that went wrong, shown until the next action.
    error: Option<String>,
    /// A passing remark on the welcome screen.
    notice: Option<String>,
    focus: FocusHandle,
}

fn firmware_name(board: &Board, id: &str) -> String {
    board
        .profile(id)
        .map_or_else(|| id.to_string(), |p| p.name.clone())
}

/// Which half of each board to connect, for when none is found.
fn connection_hint(boards: &[Board]) -> String {
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
        let library = cx.new(Library::new);
        cx.observe(&library, |_, _, cx| cx.notify()).detach();
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Keyboard name"));
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
        // The keyboard last worked on, or else the first one.
        let selected = {
            let keyboards = library.read(cx).keyboards();
            state
                .keyboard
                .filter(|id| keyboards.get(*id).is_some())
                .or_else(|| keyboards.iter().next().map(|k| k.id))
        };
        Self {
            boards,
            state,
            library,
            selected,
            workspace: None,
            form: None,
            name_input,
            error: None,
            notice: None,
            focus,
        }
    }

    /// Whether the open project has changes that are not on disk.
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

    fn select(&mut self, id: Option<KeyboardId>, cx: &mut Context<Self>) {
        self.selected = id;
        self.form = None;
        if self.state.keyboard != id {
            self.state.keyboard = id;
            self.state.save();
        }
        cx.notify();
    }

    // Opening projects

    fn show(
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
        } = pending;
        // A keyboard without a repository folder adopts the one this
        // project built from before folders were kept per keyboard.
        let has_repo = self
            .keyboard(keyboard, cx)
            .is_some_and(|k| k.repo_dir.is_some());
        let earlier = (!has_repo)
            .then(|| self.state.earlier_repo(path.as_deref(), &board.id))
            .flatten();
        self.library.update(cx, |library, cx| {
            let _ = library.change(cx, |k| {
                if let Some(dir) = earlier {
                    k.set_repo_dir(keyboard, Some(dir))?;
                }
                if let Some(path) = &path {
                    k.note_recent(keyboard, path.clone())?;
                }
                Ok(())
            });
        });
        if let Some(path) = &path {
            self.state.forget_recent(path);
            self.state.save();
        }
        self.select(Some(keyboard), cx);

        let library = self.library.clone();
        let workspace =
            cx.new(|cx| Workspace::new(project, board, library, keyboard, path, window, cx));
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        workspace.update(cx, |w, cx| {
            w.focus_canvas(window, cx);
            if let Some(notice) = notice {
                w.set_notice(notice, cx);
            }
        });
        self.workspace = Some(workspace);
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    /// Settles which keyboard a project opens under, asking where there is
    /// a choice to make, and opens it.
    fn place(
        &mut self,
        project: Project,
        path: Option<PathBuf>,
        notice: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(board) = self.boards.iter().find(|b| b.id == project.board).cloned() else {
            self.error = Some(format!(
                "This project is for an unknown board, `{}`.",
                project.board
            ));
            cx.notify();
            return;
        };
        let placement = self
            .library
            .read(cx)
            .keyboards()
            .place(&project, self.selected);
        let pending = Pending {
            project,
            board,
            path,
            notice,
        };
        match placement {
            Placement::Open(id) => self.show(pending, id, window, cx),
            Placement::Retarget(id) => self.confirm_retarget(pending, id, window, cx),
            Placement::Choose(choices) => self.choose_keyboard(pending, choices, window, cx),
            Placement::NoKeyboard => self.offer_keyboard(pending, window, cx),
        }
    }

    /// Asks before opening a project made for another firmware of the
    /// keyboard's board.
    fn confirm_retarget(
        &mut self,
        pending: Pending,
        id: KeyboardId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(keyboard) = self.keyboard(id, cx) else {
            return;
        };
        let preview = match preview_retarget(&pending.project, &pending.board, &keyboard.firmware) {
            Ok(preview) => preview,
            Err(error) => {
                self.error = Some(format!("{error}."));
                cx.notify();
                return;
            }
        };
        let answer = window.prompt(
            PromptLevel::Info,
            &format!(
                "Open “{}” under “{}”?",
                pending.project.name, keyboard.name
            ),
            Some(&format!(
                "The project was made for {}. “{}” runs {}, so the project will be switched to it. {}",
                firmware_name(&pending.board, &pending.project.firmware),
                keyboard.name,
                firmware_name(&pending.board, &keyboard.firmware),
                preview.summary()
            )),
            &["Cancel", "Open"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update_in(cx, |this, window, cx| this.show(pending, id, window, cx));
            }
        })
        .detach();
    }

    /// Asks which of several fitting keyboards a project is for.
    fn choose_keyboard(
        &mut self,
        pending: Pending,
        choices: Vec<(KeyboardId, Fit)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let names: Vec<String> = choices
            .iter()
            .filter_map(|(id, _)| self.keyboard(*id, cx).map(|k| k.name))
            .collect();
        let mut buttons = vec!["Cancel"];
        buttons.extend(names.iter().map(String::as_str));
        let answer = window.prompt(
            PromptLevel::Info,
            &format!("Which keyboard is “{}” for?", pending.project.name),
            Some("Several of your keyboards could use this project."),
            &buttons,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(index) = answer.await else { return };
            let Some((id, fit)) = index.checked_sub(1).and_then(|i| choices.get(i)).copied() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| match fit {
                Fit::Exact => this.show(pending, id, window, cx),
                Fit::OtherFirmware => this.confirm_retarget(pending, id, window, cx),
            });
        })
        .detach();
    }

    /// Offers to save a keyboard for a project that none of the saved ones
    /// suits, since projects are always opened under one.
    fn offer_keyboard(&mut self, pending: Pending, window: &mut Window, cx: &mut Context<Self>) {
        let board = &pending.board;
        let name = self.library.read(cx).keyboards().suggest_name(board);
        // A firmware this version no longer knows falls back to the first.
        let firmware = if board.profile(&pending.project.firmware).is_some() {
            pending.project.firmware.clone()
        } else {
            board.firmware[0].id.clone()
        };
        let answer = window.prompt(
            PromptLevel::Info,
            &format!("Add “{name}” to My Boards?"),
            Some(&format!(
                "“{}” is for a {} {} with {}, and none of your saved keyboards is one. Projects are opened under a saved keyboard; you can rename it or change its firmware afterward.",
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

    fn new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(keyboard) = self.selected.and_then(|id| self.keyboard(id, cx)) else {
            self.start_add(window, cx);
            return;
        };
        let Some(board) = self.boards.iter().find(|b| b.id == keyboard.board).cloned() else {
            return;
        };
        // The workspace switches the template to the keyboard's firmware.
        let project = Project::from_template(format!("{} Layout", board.name), &board);
        let pending = Pending {
            project,
            board,
            path: None,
            notice: None,
        };
        self.show(pending, keyboard.id, window, cx);
    }

    pub fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match file::load(&path) {
            Ok(project) => self.place(project, Some(path), None, window, cx),
            Err(error) => {
                self.forget(&path, cx);
                self.error = Some(format!("Could not open {}: {error}", path.display()));
                cx.notify();
            }
        }
    }

    /// Drops a project that can no longer be opened from every recent list.
    fn forget(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.state.forget_recent(path);
        self.state.save();
        self.library.update(cx, |library, cx| {
            let ids: Vec<KeyboardId> = library.keyboards().iter().map(|k| k.id).collect();
            let _ = library.change(cx, |k| {
                ids.into_iter().try_for_each(|id| k.forget_recent(id, path))
            });
        });
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
    /// unsaved project. The source file is never changed.
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
        let imported = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                // A keymap's settings live in a `.conf` file beside it.
                let conf = std::fs::read_to_string(path.with_extension("conf")).ok();
                kc_import::import_file(
                    &path.display().to_string(),
                    &text,
                    conf.as_deref(),
                    &self.boards,
                )
                .map_err(|e| e.to_string())
            });
        match imported {
            Ok((project, _, report)) => {
                self.place(project, None, Some(report.summary()), window, cx);
            }
            Err(message) => {
                self.error = Some(format!("Could not import {}: {message}", path.display()));
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
                return;
            };
            if path.extension().is_none() {
                path.set_extension(file::EXTENSION);
            }
            let _ = this.update(cx, |this, cx| this.write(&workspace, path, cx));
        })
        .detach();
    }

    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        match workspace.read(cx).path().map(PathBuf::from) {
            Some(path) => self.write(&workspace, path, cx),
            None => self.save_as(&SaveAs, window, cx),
        }
    }

    fn write(&mut self, workspace: &Entity<Workspace>, path: PathBuf, cx: &mut Context<Self>) {
        let result = workspace.update(cx, |w, cx| {
            cx.notify();
            w.save_to(path.clone())
        });
        match result {
            Ok(()) => {
                let keyboard = workspace.read(cx).keyboard();
                self.library.update(cx, |library, cx| {
                    let _ = library.change(cx, |k| k.note_recent(keyboard, path));
                });
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Could not save {}: {e}", path.display())),
        }
        cx.notify();
    }

    /// Starts a project under the selected keyboard. With a project open,
    /// returns to My Boards first, where the keyboard is chosen.
    fn new_project_action(&mut self, _: &NewProject, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_some() {
            self.close_project(&CloseProject, window, cx);
        } else {
            self.new_project(window, cx);
        }
    }

    fn close_project(&mut self, _: &CloseProject, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_unsaved_changes(cx) {
            self.workspace = None;
            self.focus.focus(window, cx);
            cx.notify();
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            "Close this project without saving?",
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

    // Saved keyboards

    fn open_form(
        &mut self,
        form: KeyboardForm,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.name_input.update(cx, |input, cx| {
            input.set_value(name.to_string(), window, cx)
        });
        self.form = Some(form);
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    /// Starts adding a keyboard that need not be connected, or even owned.
    fn start_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let board = self
            .selected
            .and_then(|id| self.keyboard(id, cx))
            .and_then(|k| self.board_index(&k.board))
            .unwrap_or(0);
        let suggested = self
            .library
            .read(cx)
            .keyboards()
            .suggest_name(&self.boards[board]);
        let form = KeyboardForm {
            editing: None,
            choices: (0..self.boards.len()).collect(),
            board,
            firmware: self.boards[board].firmware[0].id.clone(),
            device: None,
            note: None,
            suggested: suggested.clone(),
        };
        self.open_form(form, &suggested, window, cx);
    }

    /// Looks for a keyboard on USB and starts adding it, linked to that
    /// device. One that is saved already is shown instead.
    fn add_connected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let detected = kc_device::detect(&self.boards);
        let keyboards = self.library.read(cx).keyboards();
        let saved = |usb: &UsbDevice| usb.link().and_then(|link| keyboards.linked_to(&link));
        let fresh = detected.iter().find(|d| saved(&d.usb).is_none());
        let already = detected.iter().find_map(|d| saved(&d.usb));
        match (fresh, already) {
            (Some(found), _) => {
                let board = found.boards[0];
                let suggested = keyboards.suggest_name(&self.boards[board]);
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
                    editing: None,
                    choices: found.boards.clone(),
                    board,
                    firmware: self.boards[board].firmware[0].id.clone(),
                    device: Some(found.usb.clone()),
                    note: (!notes.is_empty()).then(|| notes.join(" ")),
                    suggested: suggested.clone(),
                };
                self.open_form(form, &suggested, window, cx);
            }
            (None, Some(keyboard)) => {
                let (id, name) = (keyboard.id, keyboard.name.clone());
                self.select(Some(id), cx);
                self.notice = Some(format!(
                    "The connected keyboard is already saved as “{name}”."
                ));
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

    fn start_edit(&mut self, id: KeyboardId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(keyboard) = self.keyboard(id, cx) else {
            return;
        };
        let Some(board) = self.board_index(&keyboard.board) else {
            return;
        };
        let form = KeyboardForm {
            editing: Some(id),
            choices: vec![board],
            board,
            firmware: keyboard.firmware.clone(),
            device: None,
            note: None,
            suggested: String::new(),
        };
        self.open_form(form, &keyboard.name, window, cx);
    }

    /// Changes the board a new keyboard is of, with that board's first
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

    fn submit_form(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        let name = self.name_input.read(cx).value().to_string();
        let board = self.boards[form.board].clone();
        let firmware = form.firmware.clone();
        let link = form.device.as_ref().and_then(UsbDevice::link);
        let editing = form.editing;
        let result = self.library.update(cx, |library, cx| {
            library.change(cx, |k| match editing {
                Some(id) => {
                    k.rename(id, &name)?;
                    k.set_firmware(id, &board, &firmware)?;
                    Ok(id)
                }
                None => {
                    let id = k.add(&name, &board, &firmware)?;
                    // The device was free when the form opened; if it has
                    // been taken since, the keyboard is added unlinked.
                    if let Some(link) = link {
                        let _ = k.link(id, link);
                    }
                    Ok(id)
                }
            })
        });
        match result {
            Ok(id) => {
                self.error = None;
                self.select(Some(id), cx);
            }
            Err(error) => {
                self.error = Some(format!("{error}."));
                cx.notify();
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
            Some("Its projects stay where they are. The keyboard itself is not changed."),
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
                let next = this
                    .library
                    .read(cx)
                    .keyboards()
                    .iter()
                    .next()
                    .map(|k| k.id);
                this.select(next, cx);
            });
        })
        .detach();
    }

    /// Links the keyboard to a connected device of its board. A device
    /// linked to another keyboard moves only once the user agrees, so that
    /// no device is ever two keyboards.
    fn link_connected(&mut self, id: KeyboardId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(keyboard) = self.keyboard(id, cx) else {
            return;
        };
        let Some(board) = self.board_index(&keyboard.board) else {
            return;
        };
        let keyboards = self.library.read(cx).keyboards();
        let mut devices: Vec<_> = kc_device::detect(&self.boards)
            .into_iter()
            .filter(|d| d.boards.contains(&board))
            .filter_map(|d| d.usb.link())
            .collect();
        // A device no keyboard has yet is preferred.
        devices.sort_by_key(|device| keyboards.linked_to(device).is_some());
        let Some(device) = devices.into_iter().next() else {
            self.notice = Some(format!(
                "No {} that reports a serial number was found on USB. {}",
                self.boards[board].name,
                connection_hint(&self.boards[board..=board])
            ));
            cx.notify();
            return;
        };
        let holder = keyboards
            .linked_to(&device)
            .filter(|k| k.id != id)
            .map(|k| k.name.clone());
        let Some(holder) = holder else {
            self.library.update(cx, |library, cx| {
                let _ = library.change(cx, |k| k.link(id, device));
            });
            self.notice = None;
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Move the connected keyboard to “{}”?", keyboard.name),
            Some(&format!(
                "It is linked to “{holder}”. A keyboard can be linked to one saved keyboard at a time, so “{holder}” will be left without a device."
            )),
            &["Cancel", "Move Link"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(1) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.library.update(cx, |library, cx| {
                    let _ = library.change(cx, |k| k.relink(id, device).map(|_| ()));
                });
            });
        })
        .detach();
    }

    fn unlink(&mut self, id: KeyboardId, cx: &mut Context<Self>) {
        self.library.update(cx, |library, cx| {
            let _ = library.change(cx, |k| k.unlink(id));
        });
    }

    // Welcome screen

    fn render_recent(&self, title: &'static str, paths: &[PathBuf], cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (muted, hover) = (theme.muted_foreground, theme.secondary);
        let rows = paths
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let target = path.clone();
                let name = path
                    .file_stem()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                div()
                    .id((title, index))
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
            .child(div().px_3().pb_1().text_xs().text_color(muted).child(title))
            .children(rows)
    }

    /// The list of saved keyboards, and the ways to add one.
    fn render_boards(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, hover, accent, accent_text, success) = (
            theme.muted_foreground,
            theme.secondary,
            theme.primary,
            theme.primary_foreground,
            theme.success,
        );
        let library = self.library.read(cx);
        let rows = library
            .keyboards()
            .iter()
            .map(|keyboard| {
                let id = keyboard.id;
                let active = self.selected == Some(id) && self.form.is_none();
                let board = self.boards.iter().find(|b| b.id == keyboard.board);
                let detail = match board {
                    Some(board) => format!(
                        "{} · {}",
                        board.name,
                        firmware_name(board, &keyboard.firmware)
                    ),
                    None => format!("Unknown board `{}`", keyboard.board),
                };
                let connected = library.is_connected(keyboard);
                div()
                    .id(("keyboard", id.0 as usize))
                    .px_3()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .when(active, |row| row.bg(accent).text_color(accent_text))
                    .when(!active, |row| row.hover(|row| row.bg(hover)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .child(keyboard.name.clone()),
                            )
                            .when(connected, |row| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .when(!active, |label| label.text_color(success))
                                        .child("Connected"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .when(!active, |line| line.text_color(muted))
                            .child(detail),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.notice = None;
                        this.error = None;
                        this.select(Some(id), cx);
                    }))
            })
            .collect::<Vec<_>>();
        div()
            .w_72()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().px_3().text_xs().text_color(muted).child("MY BOARDS"))
            .child(
                div()
                    .id("keyboard-list")
                    .max_h_80()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .children(rows),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_2()
                    .px_3()
                    .child(
                        Button::new("add-keyboard").label("Add Keyboard…").on_click(
                            cx.listener(|this, _, window, cx| this.start_add(window, cx)),
                        ),
                    )
                    .child(
                        Button::new("add-connected")
                            .label("Add Connected Keyboard")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_connected(window, cx);
                            })),
                    ),
            )
    }

    /// The form for adding or editing a keyboard.
    fn render_form(&self, form: &KeyboardForm, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let label = |text: &'static str| div().text_xs().text_color(muted).child(text);
        let board = &self.boards[form.board];
        let title = match (form.editing, &form.device) {
            (Some(_), _) => "Edit Keyboard",
            (None, Some(_)) => "Add the Connected Keyboard",
            (None, None) => "Add a Keyboard",
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
                "Found {} on USB. It will be linked to this keyboard (serial {}).",
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
            .when(form.editing.is_none() && form.device.is_none(), |page| {
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
                "The firmware the keyboard runs, or will be built with. The editor shows only what it supports. A keyboard cannot report this itself.",
            ))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .pt_2()
                    .child(
                        Button::new("form-submit")
                            .primary()
                            .label(if form.editing.is_some() {
                                "Save"
                            } else {
                                "Add Keyboard"
                            })
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

    /// The selected keyboard: what it is, its device, and its projects.
    fn render_keyboard(&self, keyboard: &Keyboard, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (muted, success) = (theme.muted_foreground, theme.success);
        let id = keyboard.id;
        let board = self.boards.iter().find(|b| b.id == keyboard.board);
        let detail = match board {
            Some(board) => format!(
                "{} {} · {}",
                board.vendor,
                board.name,
                firmware_name(board, &keyboard.firmware)
            ),
            None => format!(
                "This version of Keyboard Curator does not know the board `{}`.",
                keyboard.board
            ),
        };
        let connected = self.library.read(cx).is_connected(keyboard);
        let device = match (&keyboard.device, connected) {
            (Some(_), true) => div().text_sm().text_color(success).child("Connected by USB."),
            (Some(device), false) => div().text_sm().text_color(muted).child(format!(
                "Linked to a device that is not connected by USB (serial {}).",
                device.serial
            )),
            (None, _) => div().text_sm().text_color(muted).child(
                "Not linked to a device. Linking lets the app recognize this keyboard when it is connected.",
            ),
        };
        let link = if keyboard.device.is_some() {
            Button::new("unlink")
                .ghost()
                .label("Unlink Device")
                .on_click(cx.listener(move |this, _, _, cx| this.unlink(id, cx)))
        } else {
            Button::new("link")
                .ghost()
                .label("Link Connected Keyboard")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.link_connected(id, window, cx);
                }))
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_xl().child(keyboard.name.clone()))
                    .child(div().text_sm().text_color(muted).child(detail)),
            )
            .child(device)
            .when(board.is_some(), |page| {
                page.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new("new-project")
                                .primary()
                                .label("New Project")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.new_project(window, cx);
                                })),
                        )
                        .child(Button::new("open-project").label("Open Project…").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.open(&OpenProject, window, cx);
                            }),
                        ))
                        .child(
                            Button::new("import-project")
                                .label("Import a Keymap…")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.import(&ImportProject, window, cx);
                                })),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .when(board.is_some(), |row| {
                        row.child(
                            Button::new("edit-keyboard")
                                .ghost()
                                .label("Edit…")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.start_edit(id, window, cx);
                                })),
                        )
                    })
                    .when(board.is_some(), |row| row.child(link))
                    .child(
                        Button::new("remove-keyboard")
                            .ghost()
                            .label("Remove…")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_keyboard(id, window, cx);
                            })),
                    ),
            )
            .when(!keyboard.recent.is_empty(), |page| {
                page.child(
                    self.render_recent("RECENT PROJECTS", &keyboard.recent, cx)
                        .pt_2(),
                )
            })
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let selected = self.selected.and_then(|id| self.keyboard(id, cx));
        let detail = match (&self.form, &selected) {
            (Some(form), _) => self.render_form(form, cx),
            (None, Some(keyboard)) => self.render_keyboard(keyboard, cx),
            (None, None) => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_xl().child("Add your first keyboard"))
                .child(div().max_w_96().text_sm().text_color(muted).child(
                    "Projects are opened under a keyboard, which decides the firmware they are built for and what the editor shows. It does not need to be connected, or even one you own.",
                ))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(Button::new("first-open").label("Open Project…").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.open(&OpenProject, window, cx);
                            }),
                        ))
                        .child(
                            Button::new("first-import")
                                .label("Import a Keymap…")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.import(&ImportProject, window, cx);
                                })),
                        ),
                ),
        };
        // Projects from before keyboards were saved, until each is opened.
        let earlier = (self.form.is_none() && !self.state.recent.is_empty())
            .then(|| self.render_recent("OPENED BEFORE MY BOARDS", &self.state.recent, cx));

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
                            .id("keyboard-detail")
                            .w(px(480.))
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
        let title = match &self.workspace {
            Some(workspace) => workspace.read(cx).title(cx),
            None => "Keyboard Curator".to_string(),
        };
        window.set_window_title(&title);

        let theme = cx.theme();
        let (background, foreground, danger) = (theme.background, theme.foreground, theme.danger);
        let error = self
            .error
            .clone()
            .or_else(|| self.library.read(cx).problem().map(str::to_string));
        let content = match &self.workspace {
            Some(workspace) => workspace.clone().into_any_element(),
            None => self.render_welcome(cx).into_any_element(),
        };
        div()
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::import))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::save_as))
            .on_action(cx.listener(Self::close_project))
            .on_action(cx.listener(Self::new_project_action))
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
