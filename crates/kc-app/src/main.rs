//! The Keyboard Curator desktop application. This is the only crate allowed
//! to depend on the GUI framework; everything else stays UI-independent.

mod canvas;
mod shell;
mod state;
mod workspace;

use std::rc::Rc;

use gpui_kit::*;

use crate::shell::Shell;
use crate::state::AppState;

actions!(
    keyboard_curator,
    [Quit, OpenProject, CloseProject, Save, SaveAs, Undo, Redo]
);

fn menus() -> Vec<Menu> {
    vec![
        Menu::new("Keyboard Curator").items([
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Quit Keyboard Curator", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("Open…", OpenProject),
            MenuItem::separator(),
            MenuItem::action("Save", Save),
            MenuItem::action("Save As…", SaveAs),
            MenuItem::separator(),
            MenuItem::action("Close Project", CloseProject),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Undo", Undo),
            MenuItem::action("Redo", Redo),
        ]),
    ]
}

fn main() {
    let boards = Rc::new(kc_boards::built_in().expect("built-in board definitions are valid"));
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-o", OpenProject, Some("Shell")),
            KeyBinding::new("cmd-s", Save, Some("Shell")),
            KeyBinding::new("cmd-shift-s", SaveAs, Some("Shell")),
            KeyBinding::new("cmd-w", CloseProject, Some("Shell")),
            KeyBinding::new("cmd-z", Undo, Some("Shell")),
            KeyBinding::new("cmd-shift-z", Redo, Some("Shell")),
        ]);
        cx.set_menus(menus());

        let state = AppState::load();
        let bounds = match state.window {
            Some(frame) => Bounds {
                origin: point(px(frame.x), px(frame.y)),
                size: size(px(frame.width), px(frame.height)),
            },
            None => Bounds::centered(None, size(px(1180.), px(720.)), cx),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(760.), px(480.))),
            ..Default::default()
        };
        let (window, shell) = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Shell::new(boards, state, window, cx))
        })
        .expect("failed to open window");

        // Closing the window with unsaved changes asks first.
        let _ = window.update(cx, |_, window, cx| {
            window.on_window_should_close(cx, move |window, cx| {
                if !shell.read(cx).has_unsaved_changes(cx) {
                    return true;
                }
                let answer = window.prompt(
                    PromptLevel::Warning,
                    "Quit without saving?",
                    Some("Your changes to this project will be lost."),
                    &["Cancel", "Quit Without Saving"],
                    cx,
                );
                cx.spawn(async move |cx| {
                    if answer.await == Ok(1) {
                        cx.update(|cx| cx.quit());
                    }
                })
                .detach();
                false
            });
        });
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
