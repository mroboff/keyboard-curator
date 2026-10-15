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

use gpui_kit::component::TitleBar;
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
        ThemeArena,
        ThemeCurator,
        ThemeBench
    ]
);

/// The GitHub project whose releases are the app's own.
pub const REPOSITORY: &str = "mroboff/keyboard-curator";

/// Every keyboard shortcut, for the reference under Help, written as a
/// Mac writes it; [`shortcut`] spells one for the computer the app runs
/// on. Kept beside the bindings below; the two must agree.
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

/// A shortcut as this computer writes it: `⇧⌘S` on a Mac, and
/// `Ctrl+Shift+S` elsewhere, where Control stands in for Command (the
/// bindings below say `secondary`, which GPUI reads the same way).
pub fn shortcut(keys: &str) -> String {
    if cfg!(target_os = "macos") {
        keys.to_string()
    } else {
        spelled_out(keys)
    }
}

/// What showing a folder in the system's file browser is called here:
/// Finder on a Mac, the file manager elsewhere.
pub fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else {
        "Show in file manager"
    }
}

/// `⇧⌘S` as `Ctrl+Shift+S`. Each word holding the Command glyph is
/// rewritten; the rest ("Arrow keys", "to") is left alone.
fn spelled_out(keys: &str) -> String {
    keys.split(' ')
        .map(|word| {
            if !word.contains('⌘') {
                return word.to_string();
            }
            let (mut alt, mut shift) = (false, false);
            let mut key = String::new();
            for c in word.chars() {
                match c {
                    '⌘' => {}
                    '⌥' => alt = true,
                    '⇧' => shift = true,
                    other => key.push(other),
                }
            }
            let mut parts = vec!["Ctrl"];
            if alt {
                parts.push("Alt");
            }
            if shift {
                parts.push("Shift");
            }
            parts.push(&key);
            parts.join("+")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The menu bar. The theme and appearance in use are checked in the View
/// menu, and whether the app looks for a newer release at launch in Help.
/// A Mac has an application menu as well; elsewhere Quit closes the File
/// menu.
fn menus(theme: ThemeId, appearance: Appearance, checks_for_updates: bool) -> Vec<Menu> {
    let themes = ThemeId::ALL.map(|id| {
        let item = match id {
            ThemeId::Gallery => MenuItem::action(id.name(), ThemeGallery),
            ThemeId::Arena => MenuItem::action(id.name(), ThemeArena),
            ThemeId::Curator => MenuItem::action(id.name(), ThemeCurator),
            ThemeId::Bench => MenuItem::action(id.name(), ThemeBench),
        };
        item.checked(theme == id)
    });
    let mut file = vec![
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
    ];
    let mut bar = Vec::new();
    if cfg!(target_os = "macos") {
        bar.push(Menu::new("Keyboard Curator").items([
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Quit Keyboard Curator", Quit),
        ]));
    } else {
        file.push(MenuItem::separator());
        file.push(MenuItem::action("Quit", Quit));
    }
    bar.extend([
        Menu::new("File").items(file),
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
    ]);
    bar
}

/// Puts the menus where this computer shows them: in the menu bar at the
/// top of the screen on a Mac, and otherwise in the window's own title
/// bar, which the [`Shell`] draws from the same list.
pub fn install_menus(
    theme: ThemeId,
    appearance: Appearance,
    checks_for_updates: bool,
    cx: &mut App,
) {
    let menus = menus(theme, appearance, checks_for_updates);
    if cfg!(target_os = "macos") {
        cx.set_menus(menus);
    } else {
        let owned = menus.into_iter().map(Menu::owned).collect();
        gpui_kit::component::GlobalState::global_mut(cx).set_app_menus(owned);
    }
}

/// How the window is opened. On a Mac the system draws the title bar. On
/// Linux the window draws its own frame and title bar, with the menus in
/// it: Wayland compositors do not all decorate windows, and the title bar
/// component owns moving, resizing and closing the window.
fn window_options(bounds: Bounds<Pixels>) -> WindowOptions {
    let base = if cfg!(target_os = "macos") {
        WindowOptions::default()
    } else {
        TitleBar::window_options()
    };
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(760.), px(480.))),
        window_decorations: (!cfg!(target_os = "macos")).then_some(WindowDecorations::Client),
        // Matches the desktop entry, so Linux desktops can find the icon.
        app_id: Some("keyboard-curator".to_string()),
        ..base
    }
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
        // `secondary` is Command on a Mac and Control elsewhere.
        cx.bind_keys([
            KeyBinding::new("secondary-q", Quit, None),
            KeyBinding::new("secondary-n", NewProject, Some("Shell")),
            KeyBinding::new("secondary-o", OpenProject, Some("Shell")),
            KeyBinding::new("secondary-i", ImportProject, Some("Shell")),
            KeyBinding::new("secondary-e", ExportConfig, Some("Shell")),
            KeyBinding::new("secondary-c", Copy, Some("Workspace")),
            KeyBinding::new("secondary-v", Paste, Some("Workspace")),
            KeyBinding::new("secondary-]", NextLayer, Some("Workspace")),
            KeyBinding::new("secondary-[", PreviousLayer, Some("Workspace")),
            KeyBinding::new("secondary-1", Layer1, Some("Workspace")),
            KeyBinding::new("secondary-2", Layer2, Some("Workspace")),
            KeyBinding::new("secondary-3", Layer3, Some("Workspace")),
            KeyBinding::new("secondary-4", Layer4, Some("Workspace")),
            KeyBinding::new("secondary-5", Layer5, Some("Workspace")),
            KeyBinding::new("secondary-6", Layer6, Some("Workspace")),
            KeyBinding::new("secondary-7", Layer7, Some("Workspace")),
            KeyBinding::new("secondary-8", Layer8, Some("Workspace")),
            KeyBinding::new("secondary-9", Layer9, Some("Workspace")),
            KeyBinding::new("secondary-s", Save, Some("Shell")),
            KeyBinding::new("secondary-shift-s", SaveAs, Some("Shell")),
            KeyBinding::new("secondary-w", CloseProject, Some("Shell")),
            KeyBinding::new("secondary-z", Undo, Some("Shell")),
            KeyBinding::new("secondary-shift-z", Redo, Some("Shell")),
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
        install_menus(shown, state.appearance, !state.updates_off, cx);
        catalog::start(cx);

        let bounds = match state.window {
            Some(frame) => Bounds {
                origin: point(px(frame.x), px(frame.y)),
                size: size(px(frame.width), px(frame.height)),
            },
            None => Bounds::centered(None, size(px(1180.), px(720.)), cx),
        };
        let (window, shell) = gpui_kit::open_window(window_options(bounds), cx, |window, cx| {
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
                shell.update(cx, |shell, cx| shell.may_close(window, cx))
            });
        });
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    // Named, not globbed: the GUI crate's glob carries a `test` macro of
    // its own that would shadow Rust's.
    use super::{spelled_out, SHORTCUTS};

    #[test]
    fn shortcuts_are_spelled_out_with_control_off_a_mac() {
        assert_eq!(spelled_out("⌘N"), "Ctrl+N");
        assert_eq!(spelled_out("⇧⌘S"), "Ctrl+Shift+S");
        assert_eq!(spelled_out("⌥⌘I"), "Ctrl+Alt+I");
        assert_eq!(spelled_out("⌘1 to ⌘9"), "Ctrl+1 to Ctrl+9");
        assert_eq!(spelled_out("⌘["), "Ctrl+[");
        assert_eq!(spelled_out("Arrow keys"), "Arrow keys");
        assert_eq!(spelled_out("Backspace or Delete"), "Backspace or Delete");
    }

    #[test]
    fn every_shortcut_in_the_list_can_be_spelled_out() {
        for (keys, _) in SHORTCUTS {
            let spelled = spelled_out(keys);
            assert!(!spelled.contains('⌘'), "{keys} keeps its Command glyph");
            assert!(!spelled.contains("++"), "{keys} spells out as {spelled}");
        }
    }
}
