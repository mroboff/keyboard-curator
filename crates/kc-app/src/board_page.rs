//! A board's own page: its firmware and settings, the layouts used with
//! it, and building and flashing. Everything about the firmware is here
//! and nowhere in the layout editor.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_boards::board::Source;
use kc_boards::Board;
use kc_firmware::{Delivery, Family, FirmwareError, GeneratedFile};
use kc_model::features::SettingValue;
use kc_model::{file, FirmwareConfig, Keyboard, KeyboardId, Project};
use kc_zmk::settings::{Setting, SettingKind, BRIGHTNESS_SETTINGS, SETTINGS};

use crate::flash_view::FlashView;
use crate::library::Library;
use crate::shell::connection_hint;
use crate::state::AppState;
use crate::tester::KeyTester;
use crate::workspace::{badge, card, chip, display, heading, help, subheading, tab};

const KEYBOARD_NAME: &str = "CONFIG_ZMK_KEYBOARD_NAME";

/// The parts of a board's page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Which firmware the board runs. First, because it decides what the
    /// other sections offer.
    Firmware,
    Settings,
    Layouts,
    /// Building and flashing, or for firmware configured live, reading
    /// from and applying to the keyboard.
    Build,
    /// Pressing keys to see that they work, and to play them.
    Tester,
}

/// What the page asks of the window around it.
pub enum BoardEvent {
    /// Go back to My Boards.
    Back,
    NewLayout,
    /// Open this layout file in the editor.
    OpenLayout(PathBuf),
    /// Let the user pick a layout file to open.
    ChooseLayout,
    /// Let the user pick a keymap to import as a layout.
    Import,
    /// Remove this board, after asking.
    Remove,
    /// Open a layout that was just read from the keyboard, unsaved.
    Loaded(Box<Project>),
}

impl EventEmitter<BoardEvent> for BoardPage {}

/// Where a firmware build has got to.
#[derive(Debug, Clone, PartialEq)]
enum BuildStatus {
    Idle,
    /// A step is under way; the text says which.
    Working(String),
    Succeeded(String),
    Failed {
        message: String,
        /// The run's page on GitHub, when the failure was in the build.
        url: Option<String>,
    },
}

pub struct BoardPage {
    boards: Rc<Vec<Board>>,
    board: Board,
    library: Entity<Library>,
    keyboard: KeyboardId,
    section: Section,
    /// The board's own name in My Boards.
    name: Entity<InputState>,
    /// The name the keyboard announces itself by, a firmware setting.
    announced_name: Entity<InputState>,
    raw_conf: Entity<TextareaState>,
    /// A ZMK repository and revision to build from in place of the
    /// firmware's own.
    source_url: Entity<InputState>,
    source_revision: Entity<InputState>,
    flash: Entity<FlashView>,
    tester: Entity<KeyTester>,
    build: BuildStatus,
    /// What reading from or applying to a live-configured keyboard is
    /// doing, or how it went.
    live: Option<String>,
    /// True while the keyboard is being read or written.
    live_busy: bool,
    /// A short message about the last action.
    notice: Option<String>,
    /// The saved name, announced name and custom settings the text fields
    /// were last filled from, so that a change made elsewhere reaches the
    /// fields without typing in progress being overwritten.
    seen: Option<[String; 3]>,
}

fn section_title(text: &'static str, cx: &App) -> Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

impl BoardPage {
    pub fn new(
        boards: Rc<Vec<Board>>,
        board: Board,
        library: Entity<Library>,
        keyboard: KeyboardId,
        section: Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let line = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let name = line("Board name", window, cx);
        let announced_name = line("Board default", window, cx);
        let raw_conf = cx.new(|cx| TextareaState::new(window, cx));
        let source_url = line("https://github.com/someone/zmk", window, cx);
        let source_revision = line("branch, tag or commit", window, cx);
        // Text is applied when Return is pressed or focus leaves the field.
        for input in [&name, &announced_name] {
            cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    this.commit_inputs(cx);
                }
            })
            .detach();
        }
        cx.subscribe(&raw_conf, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Blur) {
                this.commit_inputs(cx);
            }
        })
        .detach();
        cx.observe_in(&library, window, |this, _, window, cx| {
            this.sync_inputs(window, cx);
            cx.notify();
        })
        .detach();
        let flash = cx.new(|cx| FlashView::new(board.clone(), cx));
        let tester = cx.new(|cx| KeyTester::new(board.clone(), window, cx));

        let mut page = Self {
            boards,
            board,
            library,
            keyboard,
            section,
            name,
            announced_name,
            raw_conf,
            source_url,
            source_revision,
            flash,
            tester,
            build: BuildStatus::Idle,
            live: None,
            live_busy: false,
            notice: None,
            seen: None,
        };
        page.sync_inputs(window, cx);
        if let Some(source) = page.saved(cx).and_then(|k| k.firmware.source.clone()) {
            page.source_url
                .update(cx, |input, cx| input.set_value(source.url, window, cx));
            page.source_revision
                .update(cx, |input, cx| input.set_value(source.revision, window, cx));
        }
        page
    }

    /// Fills the text fields from the saved board, for each value that has
    /// changed since they were last filled.
    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(saved) = self.saved(cx) else {
            return;
        };
        let announced = match saved.firmware.settings.get(KEYBOARD_NAME) {
            Some(SettingValue::Text(text)) => text.clone(),
            _ => String::new(),
        };
        let now = [
            saved.name.clone(),
            announced,
            saved.firmware.raw_conf.clone(),
        ];
        if self.seen.as_ref() == Some(&now) {
            return;
        }
        let changed = |index: usize| {
            self.seen
                .as_ref()
                .is_none_or(|seen| seen[index] != now[index])
        };
        for (index, input) in [&self.name, &self.announced_name].into_iter().enumerate() {
            if changed(index) && input.read(cx).value().trim() != now[index] {
                let text = now[index].clone();
                input.update(cx, |input, cx| input.set_value(text, window, cx));
            }
        }
        if changed(2) && self.raw_conf.read(cx).value() != now[2] {
            let text = now[2].clone();
            self.raw_conf
                .update(cx, |input, cx| input.set_value(text, window, cx));
        }
        self.seen = Some(now);
    }

    pub fn keyboard(&self) -> KeyboardId {
        self.keyboard
    }

    pub fn show_section(&mut self, section: Section, cx: &mut Context<Self>) {
        self.commit_inputs(cx);
        if self.section == Section::Tester && section != Section::Tester {
            self.tester.update(cx, |tester, cx| tester.leave(cx));
        }
        self.section = section;
        cx.notify();
    }

    /// The page is going out of sight behind the layout editor: nothing
    /// on it keeps listening or sounding.
    pub fn set_aside(&mut self, cx: &mut Context<Self>) {
        self.tester.update(cx, |tester, cx| {
            if tester.is_active() {
                tester.leave(cx);
            }
        });
    }

    /// The layout the key tester matches presses against, and a sentence
    /// saying which it is: the board's current layout, or the factory
    /// layout when there is none to use.
    fn tester_layout(&self, cx: &App) -> (Project, String) {
        let factory =
            || Project::from_template(format!("{} factory layout", self.board.name), &self.board);
        let Some(path) = self.saved(cx).and_then(|k| k.current.clone()) else {
            return (
                factory(),
                "This board has no current layout, so keys are matched against the factory layout."
                    .to_string(),
            );
        };
        match file::load(&path) {
            Ok(layout) if layout.board == self.board.id => {
                let source = format!(
                    "Keys are matched against the board's current layout, {}.",
                    layout.name
                );
                (layout, source)
            }
            _ => (
                factory(),
                "The board's current layout could not be used, so keys are matched against the factory layout."
                    .to_string(),
            ),
        }
    }

    pub fn set_notice(&mut self, notice: String, cx: &mut Context<Self>) {
        self.notice = Some(notice);
        cx.notify();
    }

    fn saved<'a>(&self, cx: &'a App) -> Option<&'a Keyboard> {
        self.library.read(cx).keyboards().get(self.keyboard)
    }

    fn config(&self, cx: &App) -> FirmwareConfig {
        self.saved(cx).map_or_else(
            || FirmwareConfig::stock(&self.board),
            |k| k.firmware.clone(),
        )
    }

    fn repo_dir(&self, cx: &App) -> Option<PathBuf> {
        self.saved(cx).and_then(|k| k.repo_dir.clone())
    }

    /// Changes the saved board, reporting anything that goes wrong.
    fn change(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut kc_model::Keyboards) -> Result<(), kc_model::keyboards::KeyboardError>,
    ) {
        let result = self
            .library
            .update(cx, |library, cx| library.change(cx, change));
        self.notice = result.err().map(|error| format!("{error}."));
        cx.notify();
    }

    /// Applies what has been typed into the name and custom settings
    /// fields.
    fn commit_inputs(&mut self, cx: &mut Context<Self>) {
        let Some(saved) = self.saved(cx).cloned() else {
            return;
        };
        let id = self.keyboard;
        let name = self.name.read(cx).value().trim().to_string();
        // An emptied name is not applied; the field keeps what was typed.
        if !name.is_empty() && name != saved.name {
            self.change(cx, |k| k.rename(id, &name));
        }
        let announced = self.announced_name.read(cx).value().trim().to_string();
        let stored = match saved.firmware.settings.get(KEYBOARD_NAME) {
            Some(SettingValue::Text(text)) => text.clone(),
            _ => String::new(),
        };
        if announced != stored {
            let value = (!announced.is_empty()).then_some(SettingValue::Text(announced));
            self.change(cx, |k| k.set_setting(id, KEYBOARD_NAME, value));
        }
        let conf = self.raw_conf.read(cx).value().to_string();
        if conf != saved.firmware.raw_conf {
            self.change(cx, |k| k.set_raw_conf(id, &conf));
        }
    }

    fn set_setting(
        &mut self,
        key: &'static str,
        value: Option<SettingValue>,
        cx: &mut Context<Self>,
    ) {
        let id = self.keyboard;
        self.change(cx, |k| k.set_setting(id, key, value));
    }

    fn set_firmware(&mut self, profile: String, cx: &mut Context<Self>) {
        let (id, board) = (self.keyboard, self.board.clone());
        self.change(cx, |k| k.set_firmware(id, &board, &profile));
        self.build = BuildStatus::Idle;
    }

    fn toggle_addon(&mut self, addon: String, cx: &mut Context<Self>) {
        let id = self.keyboard;
        let on = !self.config(cx).addons.contains(&addon);
        self.change(cx, |k| k.set_addon(id, &addon, on));
        self.build = BuildStatus::Idle;
    }

    /// Builds from the repository and revision typed into the custom
    /// source fields, or with `clear` from the firmware's own again.
    fn apply_source(&mut self, clear: bool, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.keyboard;
        let source = (!clear).then(|| Source {
            url: self.source_url.read(cx).value().trim().to_string(),
            revision: self.source_revision.read(cx).value().trim().to_string(),
        });
        if clear {
            for input in [&self.source_url, &self.source_revision] {
                input.update(cx, |input, cx| input.set_value(String::new(), window, cx));
            }
        }
        self.change(cx, |k| k.set_source(id, source));
        self.build = BuildStatus::Idle;
    }

    // Device

    /// Links the board to a connected device of its model. A device linked
    /// to another board moves only once the user agrees, so that no device
    /// is ever two boards.
    fn link_connected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.keyboard;
        let Some(saved) = self.saved(cx).cloned() else {
            return;
        };
        let Some(board) = self.boards.iter().position(|b| b.id == self.board.id) else {
            return;
        };
        let keyboards = self.library.read(cx).keyboards();
        let mut devices: Vec<_> = kc_device::detect(&self.boards)
            .into_iter()
            .filter(|d| d.boards.contains(&board))
            .filter_map(|d| d.usb.link())
            .collect();
        // A device no board has yet is preferred.
        devices.sort_by_key(|device| keyboards.linked_to(device).is_some());
        let Some(device) = devices.into_iter().next() else {
            self.notice = Some(format!(
                "No {} that reports a serial number was found on USB. {}",
                self.board.name,
                connection_hint(std::slice::from_ref(&self.board))
            ));
            cx.notify();
            return;
        };
        let holder = keyboards
            .linked_to(&device)
            .filter(|k| k.id != id)
            .map(|k| k.name.clone());
        let Some(holder) = holder else {
            self.change(cx, |k| k.link(id, device));
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Move the connected keyboard to “{}”?", saved.name),
            Some(&format!(
                "It is linked to “{holder}”. A keyboard can be linked to one board at a time, so “{holder}” will be left without a device."
            )),
            &["Cancel", "Move Link"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(1) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.change(cx, |k| k.relink(id, device).map(|_| ()));
            });
        })
        .detach();
    }

    // Live configuration

    /// Reads the connected keyboard's configuration and opens it as a new,
    /// unsaved layout. Nothing is written to the keyboard.
    fn read_keyboard(&mut self, cx: &mut Context<Self>) {
        let (board, config) = (self.board.clone(), self.config(cx));
        let device = self.saved(cx).and_then(|k| k.device.clone());
        self.live = Some("Reading the keyboard…".into());
        self.live_busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { kc_firmware::live::read(&board, &config, device.as_ref()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.live_busy = false;
                match read {
                    Ok(project) => {
                        this.live =
                            Some("Read the keyboard's configuration into a new layout.".into());
                        cx.emit(BoardEvent::Loaded(Box::new(project)));
                    }
                    Err(error) => this.live = Some(format!("{error}.")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Writes the board's current layout to the connected keyboard, after
    /// asking. What the keyboard held is saved to disk first.
    fn apply_to_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let layout = match self.layout(cx) {
            Ok(layout) => layout,
            Err(message) => {
                self.live = Some(message);
                cx.notify();
                return;
            }
        };
        let Some(saved) = self.saved(cx).cloned() else {
            return;
        };
        let Some(backups) = dirs::config_dir().map(|d| d.join("Keyboard Curator").join("backups"))
        else {
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Write “{}” to the keyboard?", layout.name),
            Some(&format!(
                "This changes “{}” at once: its keys and the colors under them on every layer. Superkeys, macros, underglow and settings are left as they are. What the keyboard holds now is saved first, in {}.",
                saved.name,
                backups.display()
            )),
            &["Cancel", "Write to Keyboard"],
            cx,
        );
        let (board, config) = (self.board.clone(), saved.firmware.clone());
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(1) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.live = Some("Writing to the keyboard…".into());
                this.live_busy = true;
                cx.notify();
            });
            let applied = cx
                .background_executor()
                .spawn(async move {
                    kc_firmware::live::apply(
                        &layout,
                        &board,
                        &config,
                        saved.device.as_ref(),
                        &backups,
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.live_busy = false;
                this.live = Some(match applied {
                    Ok(applied) if !applied.changed => {
                        "The keyboard already holds this layout. Nothing was written.".to_string()
                    }
                    Ok(applied) => format!(
                        "Written to the keyboard. Its earlier configuration is saved in {}.",
                        applied
                            .backup
                            .map_or_else(String::new, |path| path.display().to_string())
                    ),
                    Err(error) => format!("Nothing more was written: {error}."),
                });
                cx.notify();
            });
        })
        .detach();
    }

    // Building

    /// The layout the firmware is built with: the board's current one, or
    /// the factory layout when none has been applied.
    fn layout(&self, cx: &App) -> Result<Project, String> {
        let Some(path) = self.saved(cx).and_then(|k| k.current.clone()) else {
            if self.config(cx).family(&self.board).delivery() == Delivery::Live {
                return Err(
                    "This board has no current layout. Read the keyboard's own, or make one current under Layouts."
                        .into(),
                );
            }
            return Ok(Project::from_template(
                format!("{} factory layout", self.board.name),
                &self.board,
            ));
        };
        let layout = file::load(&path).map_err(|error| {
            format!(
                "The current layout, {}, could not be read: {error}. Choose another under Layouts.",
                path.display()
            )
        })?;
        if layout.board != self.board.id {
            return Err(format!(
                "The current layout, {}, is for another keyboard model. Choose another under Layouts.",
                path.display()
            ));
        }
        Ok(layout)
    }

    /// The zmk-config files for the board's settings and its layout, with
    /// that layout.
    fn generate(&self, cx: &App) -> Result<(Vec<GeneratedFile>, Project), String> {
        let layout = self.layout(cx)?;
        match kc_firmware::generate(&layout, &self.board, &self.config(cx)) {
            Ok(files) => Ok((files, layout)),
            Err(FirmwareError::Invalid(problems)) => {
                let first: Vec<&str> = problems
                    .iter()
                    .take(4)
                    .map(|p| p.message.as_str())
                    .collect();
                Err(format!(
                    "The layout “{}” has {} problem(s) to fix first: {}. Open it to see them all.",
                    layout.name,
                    problems.len(),
                    first.join("; ")
                ))
            }
            Err(error) => Err(format!("{error}.")),
        }
    }

    /// Writes the generated zmk-config files, and a copy of the layout,
    /// into a folder the user chooses.
    pub fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_inputs(cx);
        let (files, layout) = match self.generate(cx) {
            Ok(generated) => generated,
            Err(message) => {
                self.notice = Some(format!("Cannot export. {message}"));
                cx.notify();
                return;
            }
        };
        let project = file::to_json(&layout);
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Export Here".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(root) = paths.into_iter().next() else {
                return;
            };
            let write = |path: PathBuf, contents: &str| -> std::io::Result<()> {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(path, contents)
            };
            let result = files
                .iter()
                .try_for_each(|f| write(root.join(&f.path), &f.contents))
                .and_then(|()| {
                    write(
                        root.join(format!("keyboard-curator.{}", file::EXTENSION)),
                        &project,
                    )
                });
            let _ = this.update(cx, |this, cx| {
                this.notice = Some(match result {
                    Ok(()) => {
                        let mut state = AppState::load();
                        state.export_dir = Some(root.clone());
                        state.save();
                        format!("Exported {} files to {}.", files.len() + 1, root.display())
                    }
                    Err(error) => format!("Could not export: {error}."),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn choose_repo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Use This Folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let keyboard = this.keyboard;
                let _ = this.library.update(cx, |library, cx| {
                    library.change(cx, |k| k.set_repo_dir(keyboard, Some(dir)))
                });
                this.build = BuildStatus::Idle;
                cx.notify();
            });
        })
        .detach();
    }

    /// Generates the config, pushes it to the firmware repository, follows
    /// the GitHub build and hands the firmware to the flash view.
    fn build_firmware(&mut self, cx: &mut Context<Self>) {
        self.commit_inputs(cx);
        let fail = |message: String| BuildStatus::Failed { message, url: None };
        let Some(dir) = self.repo_dir(cx) else {
            self.build = fail("Choose the folder of your firmware repository first.".into());
            cx.notify();
            return;
        };
        let (generated, layout) = match self.generate(cx) {
            Ok(generated) => generated,
            Err(message) => {
                self.build = fail(message);
                cx.notify();
                return;
            }
        };
        let mut files: Vec<(String, String)> = generated
            .into_iter()
            .map(|f| (f.path, f.contents))
            .collect();
        files.push((
            format!("keyboard-curator.{}", file::EXTENSION),
            file::to_json(&layout),
        ));
        let Some(token) = kc_build::github::find_token() else {
            self.build = fail(
                "Not signed in to GitHub. Run `gh auth login` in a terminal, then try again."
                    .into(),
            );
            cx.notify();
            return;
        };
        let message = format!("Update {} from Keyboard Curator", layout.name);
        let repo_name = dir.file_name().map_or_else(
            || "zmk-config".to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        self.build = BuildStatus::Working("Pushing the config to GitHub…".into());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let status = |this: &WeakEntity<Self>, cx: &mut AsyncApp, status: BuildStatus| {
                let _ = this.update(cx, |this, cx| {
                    this.build = status;
                    cx.notify();
                });
            };
            let background = cx.background_executor().clone();

            // Commit and push, creating the repository on first use.
            let push_token = token.clone();
            let pushed = background
                .spawn(async move {
                    let github = kc_build::GitHub::new(push_token.clone());
                    let repo = match kc_build::Repo::open(&dir) {
                        Ok(repo) => repo,
                        Err(kc_build::BuildError::NotARepository(_)) => kc_build::Repo::init(&dir)?,
                        Err(other) => return Err(other),
                    };
                    if repo.github().is_err() {
                        let url = github.create_repo(&repo_name)?;
                        repo.set_origin(&url)?;
                    }
                    repo.write(&files)?;
                    repo.commit(&message)?;
                    let sha = repo.push(&push_token)?;
                    let (owner, name) = repo.github()?;
                    Ok::<_, kc_build::BuildError>((owner, name, sha))
                })
                .await;
            let (owner, name, sha) = match pushed {
                Ok(pushed) => pushed,
                Err(error) => {
                    status(&this, cx, BuildStatus::Failed { message: error.to_string(), url: None });
                    return;
                }
            };

            // Follow the run that the push started.
            let started = std::time::Instant::now();
            let run = loop {
                let actions_url = format!("https://github.com/{owner}/{name}/actions");
                let (token, owner, name, sha) = (token.clone(), owner.clone(), name.clone(), sha.clone());
                let found = background
                    .spawn(async move {
                        kc_build::GitHub::new(token).run_for_commit(&owner, &name, &sha)
                    })
                    .await;
                let elapsed = started.elapsed().as_secs();
                match found {
                    Ok(Some(run)) if run.state != kc_build::RunState::InProgress => break run,
                    Ok(Some(_)) => status(
                        &this,
                        cx,
                        BuildStatus::Working(format!(
                            "GitHub is building the firmware… {}:{:02}",
                            elapsed / 60,
                            elapsed % 60
                        )),
                    ),
                    Ok(None) if elapsed > 90 => {
                        status(&this, cx, BuildStatus::Failed {
                            message: "GitHub did not start a build. Check that Actions are enabled for the repository.".into(),
                            url: Some(actions_url),
                        });
                        return;
                    }
                    Ok(None) => status(
                        &this,
                        cx,
                        BuildStatus::Working("Waiting for GitHub to start the build…".into()),
                    ),
                    Err(error) => {
                        status(&this, cx, BuildStatus::Failed { message: error.to_string(), url: None });
                        return;
                    }
                }
                background.timer(std::time::Duration::from_secs(5)).await;
            };

            let (run_id, run_url) = (run.id, run.url.clone());
            if run.state == kc_build::RunState::Failed {
                let log = background
                    .spawn(async move { kc_build::GitHub::new(token).failure(&owner, &name, run_id) })
                    .await
                    .unwrap_or_default();
                status(&this, cx, BuildStatus::Failed {
                    message: format!("The firmware did not build.\n{log}"),
                    url: Some(run_url),
                });
                return;
            }

            status(&this, cx, BuildStatus::Working("Downloading the firmware…".into()));
            let downloaded = background
                .spawn(async move {
                    let archives = kc_build::GitHub::new(token).artifacts(&owner, &name, run_id)?;
                    let mut firmware = Vec::new();
                    for archive in archives {
                        firmware.extend(kc_build::extract_uf2(&archive)?);
                    }
                    Ok::<_, kc_build::BuildError>(firmware)
                })
                .await;
            match downloaded {
                Ok(firmware) if firmware.is_empty() => status(&this, cx, BuildStatus::Failed {
                    message: "The build finished but produced no firmware files.".into(),
                    url: Some(run_url),
                }),
                Ok(firmware) => {
                    let count = firmware.len();
                    let _ = this.update(cx, |this, cx| {
                        let files = firmware.into_iter().map(|f| (f.name, f.bytes)).collect();
                        this.flash.update(cx, |flash, cx| flash.set_firmware(files, cx));
                        this.build = BuildStatus::Succeeded(format!(
                            "Built {count} firmware file(s). Flash each half below."
                        ));
                        cx.notify();
                    });
                }
                Err(error) => {
                    status(&this, cx, BuildStatus::Failed { message: error.to_string(), url: Some(run_url) });
                }
            }
        })
        .detach();
    }

    // Rendering

    fn render_setting(
        &self,
        setting: &'static Setting,
        config: &FirmwareConfig,
        cx: &mut Context<Self>,
    ) -> Div {
        let muted = cx.theme().muted_foreground;
        let key = setting.key;
        let value = config.settings.get(key).cloned();
        let choose = |id: &'static str,
                      label: String,
                      active: bool,
                      value: Option<SettingValue>,
                      cx: &mut Context<Self>| {
            chip(
                (
                    id,
                    key.len() * 131 + key.bytes().map(usize::from).sum::<usize>(),
                ),
                label,
                active,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_setting(key, value.clone(), cx);
            }))
        };
        let controls = match setting.kind {
            SettingKind::Bool => div()
                .flex()
                .gap_1()
                .child(choose(
                    "setting-default",
                    "Board default".into(),
                    value.is_none(),
                    None,
                    cx,
                ))
                .child(choose(
                    "setting-on",
                    "On".into(),
                    value == Some(SettingValue::Bool(true)),
                    Some(SettingValue::Bool(true)),
                    cx,
                ))
                .child(choose(
                    "setting-off",
                    "Off".into(),
                    value == Some(SettingValue::Bool(false)),
                    Some(SettingValue::Bool(false)),
                    cx,
                )),
            SettingKind::Int { min, max, step } => {
                // Brightness settings stop at what the board allows.
                let max = if BRIGHTNESS_SETTINGS.contains(&key) {
                    max.min(i64::from(self.board.brightness_cap))
                } else {
                    max
                };
                let current = match value {
                    Some(SettingValue::Int(n)) => Some(n),
                    _ => None,
                };
                let shown = current.map_or_else(|| "Board default".to_string(), |n| n.to_string());
                let stepped = |delta: i64| {
                    Some(SettingValue::Int(match current {
                        Some(n) => (n + delta).clamp(min, max),
                        None if delta > 0 => min,
                        None => max,
                    }))
                };
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().w_32().text_sm().child(shown))
                    .child(choose(
                        "setting-less",
                        "−".into(),
                        false,
                        stepped(-step),
                        cx,
                    ))
                    .child(choose("setting-more", "+".into(), false, stepped(step), cx))
                    .when(current.is_some(), |row| {
                        row.child(choose(
                            "setting-default",
                            "Board default".into(),
                            false,
                            None,
                            cx,
                        ))
                    })
            }
            SettingKind::Text { .. } => div().w_64().child(Input::new(&self.announced_name)),
        };
        div()
            .flex()
            .items_center()
            .gap_8()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(div().font_weight(FontWeight::MEDIUM).child(setting.name))
                    .child(
                        div()
                            .text_sm()
                            .line_height(relative(1.4))
                            .text_color(muted)
                            .child(setting.description),
                    ),
            )
            .child(controls)
    }

    /// Which firmware the board runs, family by family.
    fn render_firmware(&self, config: &FirmwareConfig, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (muted, warning) = (theme.muted_foreground, theme.warning);
        let mut page = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(heading("Firmware", cx))
            .child(help(
                "The firmware this board runs, or will be built with. It decides which settings the board has and what the layout editor offers. Layouts are not changed by it: what a firmware lacks is kept out of sight, and settings a firmware lacks are kept the same way.", cx));
        for family in self.board.families() {
            page = page.child(
                div()
                    .pt_4()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(muted)
                    .child(family.name().to_uppercase()),
            );
            for (index, profile) in self
                .board
                .firmware
                .iter()
                .enumerate()
                .filter(|(_, p)| p.family == family)
            {
                let chosen = profile.id == config.profile;
                let id = profile.id.clone();
                let notes = profile
                    .notes
                    .iter()
                    .map(|note| div().text_sm().text_color(muted).child(note.clone()))
                    .collect::<Vec<_>>();
                page = page.child(
                    card(chosen, cx)
                        .id(("firmware", index))
                        .cursor_pointer()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(display(profile.name.clone(), 18., cx))
                                .when(chosen, |row| {
                                    row.child(badge(
                                        "Selected",
                                        crate::theme::look(cx).colors.accent_text,
                                    ))
                                })
                                .when(profile.experimental, |row| {
                                    row.child(badge("Experimental", warning))
                                }),
                        )
                        .children(notes)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_firmware(id.clone(), cx);
                        })),
                );
            }
        }
        if config.family(&self.board) == Family::Zmk {
            page = page
                .child(self.render_addons(config, cx))
                .child(self.render_source(config, cx));
        }
        page
    }

    /// The add-ons the chosen ZMK can take, each with what it is for.
    fn render_addons(&self, config: &FirmwareConfig, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (muted, warning) = (theme.muted_foreground, theme.warning);
        let catalog = kc_zmk::addons::catalog();
        let zephyr = self
            .board
            .profile(&config.profile)
            .map_or_else(String::new, |p| p.zephyr.clone());
        let checked = catalog.checked.as_ref().map_or_else(String::new, |date| {
            format!(" The list was last refreshed on {date}.")
        });
        let mut section = div()
            .pt_10()
            .flex()
            .flex_col()
            .gap_3()
            .child(subheading("Add-ons", cx))
            .child(help(format!(
                "Extras from the ZMK community that can be built into this firmware. Each is pinned to a version made for this ZMK. They are other people's code: read the notes, and expect to test.{checked}"
            ), cx));
        for (index, addon) in catalog
            .addons
            .iter()
            .enumerate()
            .filter(|(_, a)| a.fits(&zephyr, &self.board.id))
        {
            let on = config.addons.contains(&addon.id);
            let (id, url) = (addon.id.clone(), addon.url.clone());
            let mut facts = vec![format!("By {}", addon.author), addon.category.clone()];
            if let Some(stars) = addon.stars {
                facts.push(format!("{stars} stars"));
            }
            if let Some(pushed) = &addon.pushed {
                facts.push(format!("updated {pushed}"));
            }
            let notes = addon
                .notes
                .iter()
                .map(|note| div().text_sm().text_color(muted).child(format!("• {note}")))
                .collect::<Vec<_>>();
            section = section.child(
                card(on, cx)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(display(addon.name.clone(), 18., cx).flex_1().min_w_0())
                            .when(addon.archived, |row| {
                                row.child(badge("No longer maintained", warning))
                            })
                            .child(
                                chip(("addon-page", index), "Its Page", false, cx)
                                    .on_click(move |_, _, cx| cx.open_url(&url)),
                            )
                            .child(
                                chip(
                                    ("addon-toggle", index),
                                    if on { "Added" } else { "Add" },
                                    on,
                                    cx,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.toggle_addon(id.clone(), cx);
                                    },
                                )),
                            ),
                    )
                    .child(div().text_xs().text_color(muted).child(facts.join(" · ")))
                    .child(div().text_sm().child(addon.summary.clone()))
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(format!("People use it for: {}", addon.uses)),
                    )
                    .children(notes),
            );
        }
        // Chosen add-ons this firmware cannot take stay chosen, and say so.
        for (index, id) in config.inactive_addons(&self.board).into_iter().enumerate() {
            let remove = id.clone();
            section = section.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().text_xs().text_color(warning).child(format!(
                        "The add-on “{id}” does not fit this firmware and is left out of the build."
                    )))
                    .child(chip(("addon-remove", index), "Remove", false, cx).on_click(
                        cx.listener(move |this, _, _, cx| this.toggle_addon(remove.clone(), cx)),
                    )),
            );
        }
        section
    }

    /// Building from another ZMK repository, for trying a fork.
    fn render_source(&self, config: &FirmwareConfig, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (muted, warning) = (theme.muted_foreground, theme.warning);
        let current = match &config.source {
            Some(source) => div().text_xs().text_color(warning).child(format!(
                "Building from {} at {}, not from the firmware chosen above.",
                source.url, source.revision
            )),
            None => div()
                .text_xs()
                .text_color(muted)
                .child("Building from the chosen firmware's own source."),
        };
        div()
            .pt_10()
            .flex()
            .flex_col()
            .gap_3()
            .child(subheading("Custom source", cx))
            .child(help(
                "Builds from another ZMK repository and revision in place of the chosen firmware's own, for trying a fork or an unmerged feature. The app still assumes the firmware chosen above when deciding what to offer, so choose the closest one. ZMK takes one source: several unmerged features can only be combined in a fork that merges them.", cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w_80().child(Input::new(&self.source_url)))
                    .child(div().w_56().child(Input::new(&self.source_revision)))
                    .child(chip("source-apply", "Use This Source", false, cx).on_click(
                        cx.listener(|this, _, window, cx| this.apply_source(false, window, cx)),
                    ))
                    .when(config.source.is_some(), |row| {
                        row.child(chip("source-clear", "Use the Firmware's Own", false, cx).on_click(
                            cx.listener(|this, _, window, cx| this.apply_source(true, window, cx)),
                        ))
                    }),
            )
            .child(current)
    }

    /// Every setting of the board's firmware.
    fn render_settings(&self, config: &FirmwareConfig, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let features = config.features(&self.board);
        let firmware = self
            .board
            .profile(&config.profile)
            .map_or_else(|| config.profile.clone(), |p| p.name.clone());
        let mut page = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(heading("Settings", cx))
            .child(help(format!(
                "Settings of {firmware}, the firmware chosen under Firmware. They belong to the board, and apply whichever layout it is built with."
            ), cx));
        if config.family(&self.board) != Family::Zmk {
            return page.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("This firmware has no settings to adjust here yet."),
            );
        }
        let mut group = "";
        for setting in SETTINGS {
            if setting.requires.is_some_and(|f| !features.contains(&f)) {
                continue;
            }
            if setting.group != group {
                group = setting.group;
                page = page.child(subheading(group, cx).pt_8());
                if group == "Lighting" {
                    page = page.child(help(
                        format!(
                            "This keyboard's brightness is limited to {}% to protect it.",
                            self.board.brightness_cap
                        ),
                        cx,
                    ));
                }
            }
            page = page.child(self.render_setting(setting, config, cx));
        }
        page.child(subheading("Custom settings", cx).pt_8()).child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(section_title("EXTRA SETTINGS", cx))
                .child(help(
                    "Lines added to the .conf file as they are, such as CONFIG_ZMK_USB_LOGGING=y.",
                    cx,
                ))
                .child(Textarea::new(&self.raw_conf).h(px(140.))),
        )
    }

    /// The layouts used with the board, and which one it is built with.
    fn render_layouts(&self, saved: &Keyboard, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (muted, success, danger) = (theme.muted_foreground, theme.success, theme.danger);
        let id = self.keyboard;
        let row = |name: String, detail: String, current: bool, missing: bool| {
            card(current, cx).flex().items_center().gap_3().child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(display(name, 18., cx))
                            .when(current, |line| line.child(badge("Current", success)))
                            .when(missing, |line| line.child(badge("File not found", danger))),
                    )
                    .child(div().text_xs().text_color(muted).child(detail)),
            )
        };
        let factory = row(
            "Factory layout".into(),
            format!("What the {} ships with.", self.board.name),
            saved.current.is_none(),
            false,
        )
        .when(saved.current.is_some(), |row| {
            row.child(
                chip("factory-current", "Make Current", false, cx).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.change(cx, |k| k.set_current(id, None));
                    },
                )),
            )
        });
        let rows = saved
            .layouts
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let name = path
                    .file_stem()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                let current = saved.current.as_ref() == Some(path);
                let missing = !path.exists();
                let (open, make, forget) = (path.clone(), path.clone(), path.clone());
                row(name, path.display().to_string(), current, missing)
                    .when(!missing, |row| {
                        row.child(chip(("layout-open", index), "Open", false, cx).on_click(
                            cx.listener(move |_, _, _, cx| {
                                cx.emit(BoardEvent::OpenLayout(open.clone()));
                            }),
                        ))
                    })
                    .when(!missing && !current, |row| {
                        row.child(
                            chip(("layout-current", index), "Make Current", false, cx).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    let path = make.clone();
                                    this.change(cx, |k| k.set_current(id, Some(path)));
                                }),
                            ),
                        )
                    })
                    .child(
                        chip(("layout-forget", index), "Remove From List", false, cx).on_click(
                            cx.listener(move |this, _, _, cx| {
                                let path = forget.clone();
                                this.change(cx, |k| k.forget_layout(id, &path));
                            }),
                        ),
                    )
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(heading("Layouts", cx))
            .child(help(
                "A layout is what the keys do: layers, behaviors, combos, colors and pointing. This board can have any number of them. The current one goes into the firmware when it is built.", cx))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("new-layout")
                            .primary()
                            .label("New Layout")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(BoardEvent::NewLayout))),
                    )
                    .child(
                        Button::new("open-layout")
                            .label("Open Layout…")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(BoardEvent::ChooseLayout))),
                    )
                    .child(
                        Button::new("import-layout")
                            .label("Import a Keymap…")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(BoardEvent::Import))),
                    ),
            )
            .when(
                saved.firmware.family(&self.board).delivery() == Delivery::Build,
                |page| page.child(factory),
            )
            .children(rows)
    }

    fn render_build(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (danger, success) = (theme.danger, theme.success);
        let working = matches!(self.build, BuildStatus::Working(_));
        let repo = match self.repo_dir(cx) {
            Some(dir) => dir.display().to_string(),
            None => "No folder chosen".to_string(),
        };
        let panel = card(false, cx)
            .w(px(640.))
            .flex()
            .flex_col()
            .gap_3()
            .child(subheading("Build the firmware", cx))
            .child(help("Your layout is pushed to a firmware repository on GitHub, which builds it. Choose the folder of that repository, or an empty folder to have a private one created.", cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().text_sm().child(repo))
                    .child(
                        Button::new("choose-repo")
                            .label("Choose Folder…")
                            .on_click(cx.listener(|this, _, window, cx| this.choose_repo(window, cx))),
                    )
                    .when(!working, |row| {
                        row.child(
                            Button::new("build-firmware")
                                .primary()
                                .label("Build Firmware")
                                .on_click(cx.listener(|this, _, _, cx| this.build_firmware(cx))),
                        )
                    }),
            );
        let panel = match &self.build {
            BuildStatus::Idle => panel,
            BuildStatus::Working(text) => panel.child(div().text_sm().child(text.clone())),
            BuildStatus::Succeeded(text) => {
                panel.child(div().text_sm().text_color(success).child(text.clone()))
            }
            BuildStatus::Failed { message, url } => {
                let lines = message
                    .lines()
                    .map(|l| div().child(l.to_string()))
                    .collect::<Vec<_>>();
                panel
                    .child(
                        div()
                            .id("build-failure")
                            .max_h_48()
                            .overflow_y_scroll()
                            .text_xs()
                            .text_color(danger)
                            .children(lines),
                    )
                    .when_some(url.clone(), |panel, url| {
                        panel.child(
                            Button::new("open-run")
                                .ghost()
                                .label("Open on GitHub")
                                .on_click(move |_, _, cx| cx.open_url(&url)),
                        )
                    })
            }
        };
        div().w_full().flex().pt_6().pb_4().child(panel)
    }

    /// Reading from and writing to a keyboard whose firmware is configured
    /// live.
    fn render_keyboard_section(&self, saved: &Keyboard, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let current = match &saved.current {
            Some(path) => format!(
                "The current layout is “{}”.",
                path.file_stem()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            ),
            None => "This board has no current layout yet.".to_string(),
        };
        let panel = |title: &'static str, text: String| {
            card(false, cx)
                .w(px(640.))
                .flex()
                .flex_col()
                .gap_3()
                .child(subheading(title, cx))
                .child(div().text_sm().text_color(muted).child(text))
        };
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                panel(
                    "Read from the keyboard",
                    "Opens what the keyboard holds now as a new layout: every layer's keys and the colors under them. Nothing is written to the keyboard. Close Dygma's Bazecor first; only one program can talk to the keyboard at a time.".into(),
                )
                .when(!self.live_busy, |panel| {
                    panel.child(
                        div().flex().child(
                            Button::new("read-keyboard")
                                .primary()
                                .label("Read From Keyboard")
                                .on_click(cx.listener(|this, _, _, cx| this.read_keyboard(cx))),
                        ),
                    )
                }),
            )
            .child(
                panel(
                    "Apply to the keyboard",
                    format!(
                        "{current} Applying writes it to the keyboard at once, with no build. Only keys and the colors under them are written, and only if they differ; superkeys, macros, underglow and settings stay as they are. The keyboard's configuration is saved to disk first."
                    ),
                )
                .when(!self.live_busy && saved.current.is_some(), |panel| {
                    panel.child(
                        div().flex().child(
                            Button::new("apply-keyboard")
                                .label("Apply To Keyboard…")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.apply_to_keyboard(window, cx);
                                })),
                        ),
                    )
                }),
            )
            .when_some(self.live.clone(), |page, status| {
                page.child(div().w(px(640.)).text_sm().child(status))
            })
    }

    /// Building the firmware, exporting its config and flashing it.
    fn render_build_section(
        &self,
        saved: &Keyboard,
        config: &FirmwareConfig,
        cx: &mut Context<Self>,
    ) -> Div {
        let firmware = self
            .board
            .profile(&config.profile)
            .map_or_else(|| config.profile.clone(), |p| p.name.clone());
        let layout = match &saved.current {
            Some(path) => format!(
                "the layout “{}”",
                path.file_stem()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            ),
            None => "the factory layout".to_string(),
        };
        let changed = config
            .settings
            .keys()
            .filter(|key| config.offers(key, &self.board))
            .count();
        let settings = match changed {
            0 => "default settings".to_string(),
            1 => "1 changed setting".to_string(),
            n => format!("{n} changed settings"),
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .w(px(640.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_sm().child(format!(
                        "This builds {firmware} with {settings} and {layout}."
                    )))
                    .child(help("Change the firmware under Firmware, its settings under Settings, and which layout is built in under Layouts.", cx))
                    .child(
                        div().flex().child(
                            Button::new("export-config")
                                .ghost()
                                .label("Export Firmware Config…")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.export(window, cx);
                                })),
                        ),
                    ),
            )
            .child(self.render_build(cx))
            .child(self.flash.clone())
    }
}

impl Render for BoardPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border, success) = (theme.muted_foreground, theme.border, theme.success);
        let Some(saved) = self.saved(cx).cloned() else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child("This board is no longer saved.")
                .into_any_element();
        };
        let config = saved.firmware.clone();
        let firmware = self
            .board
            .profile(&config.profile)
            .map_or_else(|| config.profile.clone(), |p| p.name.clone());
        let connected = self.library.read(cx).is_connected(&saved);
        let device = match (&saved.device, connected) {
            (Some(_), true) => div().text_sm().text_color(success).child("Connected by USB."),
            (Some(device), false) => div().text_sm().text_color(muted).child(format!(
                "Linked to a device that is not connected by USB (serial {}).",
                device.serial
            )),
            (None, _) => help("Not linked to a device. Linking lets the app recognize this keyboard when it is connected.", cx),
        };
        let link = if saved.device.is_some() {
            let id = self.keyboard;
            chip("unlink", "Unlink Device", false, cx).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.change(cx, |k| k.unlink(id));
                },
            ))
        } else {
            chip("link", "Link Connected Keyboard", false, cx)
                .on_click(cx.listener(|this, _, window, cx| this.link_connected(window, cx)))
        };
        // Firmware configured live has no build; its last tab is the
        // keyboard itself.
        let build_label = match config.family(&self.board).delivery() {
            Delivery::Build => "Build & Flash",
            Delivery::Live => "Keyboard",
        };
        let tab =
            |id: &'static str, label: &'static str, section: Section, cx: &mut Context<Self>| {
                tab(id, label, self.section == section, cx)
                    .on_click(cx.listener(move |this, _, _, cx| this.show_section(section, cx)))
            };
        // The tester starts, on the board's current layout, each time it
        // comes into sight: on its tab being chosen, and on coming back
        // from the layout editor.
        if self.section == Section::Tester && !self.tester.read(cx).is_active() {
            let (layout, source) = self.tester_layout(cx);
            self.tester
                .update(cx, |tester, cx| tester.enter(layout, source, cx));
        }
        let content = match self.section {
            Section::Firmware => Some(self.render_firmware(&config, cx)),
            Section::Settings => Some(self.render_settings(&config, cx)),
            Section::Layouts => Some(self.render_layouts(&saved, cx)),
            Section::Build => Some(match config.family(&self.board).delivery() {
                Delivery::Build => self.render_build_section(&saved, &config, cx),
                Delivery::Live => self.render_keyboard_section(&saved, cx),
            }),
            // The tester fills the page instead of sitting in its column.
            Section::Tester => None,
        };
        let body = match content {
            Some(content) => div()
                .id("board-page")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_10()
                .py_8()
                .child(div().w_full().max_w(px(820.)).child(content))
                .into_any_element(),
            None => div()
                .flex_1()
                .min_h_0()
                .child(self.tester.clone())
                .into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_10()
                    .pt_5()
                    .child(chip("back", "‹ My Boards", false, cx).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.commit_inputs(cx);
                            cx.emit(BoardEvent::Back);
                        },
                    )))
                    .child(div().flex_1())
                    .child(link)
                    .child(
                        chip("remove", "Remove…", false, cx)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(BoardEvent::Remove))),
                    ),
            )
            // The board's name, large, with what it is under it; the field
            // that renames it sits to the side.
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap_8()
                    .px_10()
                    .pt_4()
                    .pb_5()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(display(saved.name.clone(), 40., cx))
                            .child(div().text_color(muted).child(format!(
                                "{} {} · {firmware}",
                                self.board.vendor, self.board.name
                            )))
                            .child(device),
                    )
                    .child(
                        div()
                            .w_64()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(section_title("NAME", cx))
                            .child(Input::new(&self.name)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_7()
                    .px_10()
                    .border_b_1()
                    .border_color(border)
                    .child(tab("section-firmware", "Firmware", Section::Firmware, cx))
                    .child(tab("section-settings", "Settings", Section::Settings, cx))
                    .child(tab("section-layouts", "Layouts", Section::Layouts, cx))
                    .child(tab("section-build", build_label, Section::Build, cx))
                    .child(tab("section-tester", "Key Tester", Section::Tester, cx))
                    .child(
                        div()
                            .flex_1()
                            .px_3()
                            .text_xs()
                            .text_color(muted)
                            .child(self.notice.clone().unwrap_or_default()),
                    ),
            )
            .child(body)
            .into_any_element()
    }
}
