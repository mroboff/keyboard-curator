//! The Keyboard Curator desktop application. This is the only crate allowed
//! to depend on the GUI framework; everything else stays UI-independent.

mod board_page;
mod canvas;
mod catalog;
mod flash_view;
mod library;
mod shell;
mod state;
mod tester;
mod theme;
mod workspace;

use std::rc::Rc;

use gpui_kit::*;

use crate::shell::Shell;
use crate::state::{AppState, Appearance, ThemeId};

actions!(
    keyboard_curator,
    [
        Quit,
        NewProject,
        OpenProject,
        ImportProject,
        CloseProject,
        Save,
        SaveAs,
        Undo,
        Redo,
        Copy,
        Paste,
        NextLayer,
        PreviousLayer,
        Layer1,
        Layer2,
        Layer3,
        Layer4,
        Layer5,
        Layer6,
        Layer7,
        Layer8,
        Layer9,
        ToggleAutoAdvance,
        ToggleTypeToAssign,
        ShowKeyboard,
        ShowFiles,
        ShowFlash,
        ExportConfig,
        OpenHelp,
        OpenZmkDocs,
        ShowShortcuts,
        CheckForUpdates,
        ToggleUpdateCheck,
        AppearanceSystem,
        AppearanceLight,
        AppearanceDark,
        ThemeGallery,
        ThemeGoldenGate,
        ThemeArena,
        ThemeCurator,
        ThemeBench
    ]
);

/// The GitHub project whose releases are the app's own.
pub const REPOSITORY: &str = "mroboff/keyboard-curator";

/// Every keyboard shortcut, for the reference under Help. Kept beside the
/// bindings below; the two must agree.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("⌘N", "New board or layout, for where you are"),
    ("⌘O", "Open a layout"),
    ("⌘I", "Import a keymap"),
    ("⌘S", "Save the layout"),
    ("⇧⌘S", "Save the layout as…"),
    ("⌘E", "Export the firmware config"),
    ("⌘W", "Close the layout or the board's page"),
    ("⌘Z", "Undo"),
    ("⇧⌘Z", "Redo"),
    ("⌘C", "Copy the selected keys"),
    ("⌘V", "Paste onto the selected keys"),
    ("⌘]", "Next layer"),
    ("⌘[", "Previous layer"),
    ("⌘1 to ⌘9", "Go to that layer"),
    ("Arrow keys", "Move the selection across the keyboard"),
    ("Backspace or Delete", "Clear the selected keys"),
    ("Escape", "Clear the selection"),
    ("⌘Q", "Quit"),
];

/// The menu bar. The theme and appearance in use are checked in the View
/// menu, and whether the app looks for a newer release at launch in Help.
fn menus(theme: ThemeId, appearance: Appearance, checks_for_updates: bool) -> Vec<Menu> {
    let themes = ThemeId::ALL.map(|id| {
        let item = match id {
            ThemeId::Gallery => MenuItem::action(id.name(), ThemeGallery),
            ThemeId::GoldenGate => MenuItem::action(id.name(), ThemeGoldenGate),
            ThemeId::Arena => MenuItem::action(id.name(), ThemeArena),
            ThemeId::Curator => MenuItem::action(id.name(), ThemeCurator),
            ThemeId::Bench => MenuItem::action(id.name(), ThemeBench),
        };
        item.checked(theme == id)
    });
    vec![
        Menu::new("Keyboard Curator").items([
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Quit Keyboard Curator", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New", NewProject),
            MenuItem::action("Open Layout…", OpenProject),
            MenuItem::action("Import Keymap…", ImportProject),
            MenuItem::separator(),
            MenuItem::action("Save", Save),
            MenuItem::action("Save As…", SaveAs),
            MenuItem::separator(),
            MenuItem::action("Export Firmware Config…", ExportConfig),
            MenuItem::separator(),
            MenuItem::action("Close", CloseProject),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Undo", Undo),
            MenuItem::action("Redo", Redo),
            MenuItem::separator(),
            MenuItem::action("Copy", Copy),
            MenuItem::action("Paste", Paste),
        ]),
        Menu::new("View").items([
            MenuItem::action("Keyboard", ShowKeyboard),
            MenuItem::action("Generated Files", ShowFiles),
            MenuItem::action("Apply to Board", ShowFlash),
            MenuItem::separator(),
            MenuItem::action("Next Layer", NextLayer),
            MenuItem::action("Previous Layer", PreviousLayer),
            MenuItem::separator(),
            MenuItem::action("Advance After Assigning", ToggleAutoAdvance),
            MenuItem::action("Type to Assign", ToggleTypeToAssign),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Theme").items(themes)),
            MenuItem::submenu(
                Menu::new("Appearance").items([
                    MenuItem::action("Match System", AppearanceSystem)
                        .checked(appearance == Appearance::System),
                    MenuItem::action("Light", AppearanceLight)
                        .checked(appearance == Appearance::Light),
                    MenuItem::action("Dark", AppearanceDark)
                        .checked(appearance == Appearance::Dark),
                ]),
            ),
        ]),
        Menu::new("Help").items([
            MenuItem::action("Keyboard Curator Help", OpenHelp),
            MenuItem::action("Keyboard Shortcuts", ShowShortcuts),
            MenuItem::action("ZMK Documentation", OpenZmkDocs),
            MenuItem::separator(),
            MenuItem::action("Check for Updates…", CheckForUpdates),
            MenuItem::action("Check for Updates at Launch", ToggleUpdateCheck)
                .checked(checks_for_updates),
        ]),
    ]
}

fn main() {
    let boards = Rc::new(kc_boards::built_in().expect("built-in board definitions are valid"));
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &OpenHelp, cx| {
            cx.open_url("https://github.com/mroboff/keyboard-curator#readme");
        });
        cx.on_action(|_: &OpenZmkDocs, cx| cx.open_url("https://zmk.dev/docs"));
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-n", NewProject, Some("Shell")),
            KeyBinding::new("cmd-o", OpenProject, Some("Shell")),
            KeyBinding::new("cmd-i", ImportProject, Some("Shell")),
            KeyBinding::new("cmd-e", ExportConfig, Some("Shell")),
            KeyBinding::new("cmd-c", Copy, Some("Workspace")),
            KeyBinding::new("cmd-v", Paste, Some("Workspace")),
            KeyBinding::new("cmd-]", NextLayer, Some("Workspace")),
            KeyBinding::new("cmd-[", PreviousLayer, Some("Workspace")),
            KeyBinding::new("cmd-1", Layer1, Some("Workspace")),
            KeyBinding::new("cmd-2", Layer2, Some("Workspace")),
            KeyBinding::new("cmd-3", Layer3, Some("Workspace")),
            KeyBinding::new("cmd-4", Layer4, Some("Workspace")),
            KeyBinding::new("cmd-5", Layer5, Some("Workspace")),
            KeyBinding::new("cmd-6", Layer6, Some("Workspace")),
            KeyBinding::new("cmd-7", Layer7, Some("Workspace")),
            KeyBinding::new("cmd-8", Layer8, Some("Workspace")),
            KeyBinding::new("cmd-9", Layer9, Some("Workspace")),
            KeyBinding::new("cmd-s", Save, Some("Shell")),
            KeyBinding::new("cmd-shift-s", SaveAs, Some("Shell")),
            KeyBinding::new("cmd-w", CloseProject, Some("Shell")),
            KeyBinding::new("cmd-z", Undo, Some("Shell")),
            KeyBinding::new("cmd-shift-z", Redo, Some("Shell")),
        ]);
        let state = AppState::load();
        theme::init(cx);
        // For checking a theme from the command line: KC_THEME=arena. It
        // is not remembered.
        let shown = std::env::var("KC_THEME")
            .ok()
            .and_then(|name| serde_json::from_value(serde_json::Value::String(name)).ok())
            .unwrap_or(state.theme);
        theme::apply(shown, state.appearance, cx);
        cx.set_menus(menus(shown, state.appearance, !state.updates_off));
        catalog::start(cx);

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

        let _ = window.update(cx, |_, window, cx| {
            // Once a day at most would be kinder to GitHub, but the check
            // is one small request, and only if it has not been turned off.
            shell.update(cx, |shell, cx| {
                shell.check_for_updates_at_launch(window, cx)
            });
            // A project given on the command line opens straight away; a
            // keymap or Layout Editor export is imported.
            if let Some(path) = std::env::args_os().nth(1) {
                let path = std::path::PathBuf::from(path);
                let is_project = path
                    .extension()
                    .is_some_and(|e| e == kc_model::file::EXTENSION);
                shell.update(cx, |shell, cx| {
                    if is_project {
                        shell.open_path(path, window, cx);
                    } else {
                        shell.import_path(path, window, cx);
                    }
                });
            }
            // Closing the window with unsaved changes asks first.
            window.on_window_should_close(cx, move |window, cx| {
                if !shell.read(cx).has_unsaved_changes(cx) {
                    return true;
                }
                let answer = window.prompt(
                    PromptLevel::Warning,
                    "Quit without saving?",
                    Some("Your changes to this layout will be lost."),
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
