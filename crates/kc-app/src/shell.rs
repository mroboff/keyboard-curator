//! The window's root view: the welcome screen or an open project, plus the
//! file commands that move between them.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_boards::Board;
use kc_model::{file, Project};

use crate::state::{default_project_dir, AppState, WindowFrame};
use crate::workspace::Workspace;
use crate::{CloseProject, NewProject, OpenProject, Redo, Save, SaveAs, Undo};

pub struct Shell {
    boards: Rc<Vec<Board>>,
    state: AppState,
    workspace: Option<Entity<Workspace>>,
    /// The last thing that went wrong, shown until the next action.
    error: Option<String>,
    focus: FocusHandle,
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
        Self {
            boards,
            state,
            workspace: None,
            error: None,
            focus,
        }
    }

    /// Whether the open project has changes that are not on disk.
    pub fn has_unsaved_changes(&self, cx: &App) -> bool {
        self.workspace
            .as_ref()
            .is_some_and(|w| w.read(cx).is_dirty())
    }

    fn show(
        &mut self,
        project: Project,
        board: Board,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = cx.new(|cx| Workspace::new(project, board, path, window, cx));
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        workspace.update(cx, |w, cx| w.focus_canvas(window, cx));
        self.workspace = Some(workspace);
        self.error = None;
        cx.notify();
    }

    fn new_project(&mut self, board: &Board, window: &mut Window, cx: &mut Context<Self>) {
        let project = Project::new(format!("My {}", board.name), board);
        self.show(project, board.clone(), None, window, cx);
    }

    pub fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let opened = file::load(&path)
            .map_err(|e| e.to_string())
            .and_then(|project| {
                let board = self.boards.iter().find(|b| b.id == project.board);
                let board = board.ok_or(format!(
                    "This project is for an unknown board, `{}`.",
                    project.board
                ))?;
                Ok((project, board.clone()))
            });
        match opened {
            Ok((project, board)) => {
                self.state.note_recent(path.clone());
                self.state.save();
                self.show(project, board, Some(path), window, cx);
            }
            Err(message) => {
                self.state.forget_recent(&path);
                self.state.save();
                self.error = Some(format!("Could not open {}: {message}", path.display()));
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
                self.state.note_recent(path);
                self.state.save();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Could not save {}: {e}", path.display())),
        }
        cx.notify();
    }

    /// Returns to the welcome screen, where new projects are started.
    fn new_project_screen(&mut self, _: &NewProject, window: &mut Window, cx: &mut Context<Self>) {
        self.close_project(&CloseProject, window, cx);
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

    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, hover) = (theme.muted_foreground, theme.secondary);
        let new_buttons = self
            .boards
            .iter()
            .enumerate()
            .map(|(index, board)| {
                let boards = self.boards.clone();
                Button::new(("new-project", index))
                    .primary()
                    .label(format!("New {} {} Project", board.vendor, board.name))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.new_project(&boards[index], window, cx);
                    }))
            })
            .collect::<Vec<_>>();
        let recent = self
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
                    .id(("recent", index))
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
            .child(div().flex().gap_3().children(new_buttons))
            .child(
                Button::new("open-project")
                    .label("Open Project…")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open(&OpenProject, window, cx);
                    })),
            )
            .when(!recent.is_empty(), |page| {
                page.child(
                    div()
                        .w_96()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .px_3()
                                .pb_1()
                                .text_xs()
                                .text_color(muted)
                                .child("RECENT"),
                        )
                        .children(recent),
                )
            })
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match &self.workspace {
            Some(workspace) => workspace.read(cx).title(),
            None => "Keyboard Curator".to_string(),
        };
        window.set_window_title(&title);

        let theme = cx.theme();
        let (background, foreground, danger) = (theme.background, theme.foreground, theme.danger);
        let content = match &self.workspace {
            Some(workspace) => workspace.clone().into_any_element(),
            None => self.render_welcome(cx).into_any_element(),
        };
        div()
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::save_as))
            .on_action(cx.listener(Self::close_project))
            .on_action(cx.listener(Self::new_project_screen))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .size_full()
            .flex()
            .flex_col()
            .bg(background)
            .text_color(foreground)
            .when_some(self.error.clone(), |page, error| {
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
