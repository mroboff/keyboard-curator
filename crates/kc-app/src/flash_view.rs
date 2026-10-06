//! Guided flashing: choose firmware files, put each half into its
//! bootloader, and copy the right file to the right half.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_boards::board::Side;
use kc_boards::Board;
use kc_flash::{Bootloader, Half};

use crate::workspace::{help, subheading};

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Left => "left",
        Side::Right => "right",
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Progress {
    Waiting,
    Done,
    Failed(String),
}

struct Step {
    side: Side,
    volume: String,
    /// The firmware chosen for this half.
    firmware: Option<(PathBuf, Vec<u8>)>,
    progress: Progress,
}

pub struct FlashView {
    board: Board,
    steps: Vec<Step>,
    /// Bootloader drives currently mounted.
    drives: Vec<Bootloader>,
    message: Option<String>,
}

impl FlashView {
    pub fn new(board: Board, cx: &mut Context<Self>) -> Self {
        let steps = kc_flash::plan(&board)
            .into_iter()
            .map(|step| Step {
                side: step.side,
                volume: step.volume,
                firmware: None,
                progress: Progress::Waiting,
            })
            .collect();
        // Watch for bootloader drives for as long as this view exists.
        cx.spawn(async move |this, cx| loop {
            let drives = kc_flash::scan(Path::new(kc_flash::VOLUMES));
            let alive = this.update(cx, |this, cx| {
                if this.drives != drives {
                    this.drives = drives;
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_millis(700))
                .await;
        })
        .detach();
        Self {
            board,
            steps,
            drives: Vec::new(),
            message: None,
        }
    }

    /// Takes firmware from a build, replacing anything chosen before.
    pub fn set_firmware(&mut self, firmware: Vec<(String, Vec<u8>)>, cx: &mut Context<Self>) {
        for step in &mut self.steps {
            step.firmware = None;
            step.progress = Progress::Waiting;
        }
        self.assign(
            firmware
                .into_iter()
                .map(|(name, bytes)| (PathBuf::from(name), bytes)),
            cx,
        );
    }

    fn add_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let mut unreadable = Vec::new();
        let files: Vec<(PathBuf, Vec<u8>)> = paths
            .into_iter()
            .filter_map(|path| match std::fs::read(&path) {
                Ok(bytes) => Some((path, bytes)),
                Err(error) => {
                    unreadable.push(format!("{}: {error}", path.display()));
                    None
                }
            })
            .collect();
        self.assign(files.into_iter(), cx);
        if !unreadable.is_empty() {
            self.message = Some(unreadable.join(" "));
        }
    }

    /// Assigns firmware to halves: by UF2 family where the board has them,
    /// otherwise by `left` or `right` in the file name.
    fn assign(&mut self, files: impl Iterator<Item = (PathBuf, Vec<u8>)>, cx: &mut Context<Self>) {
        let mut problems = Vec::new();
        for (path, bytes) in files {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let mut used = false;
            let mut rejected = false;
            for step in &mut self.steps {
                let has_family = self
                    .board
                    .half(step.side)
                    .is_some_and(|h| h.uf2_family.is_some());
                let by_name = name.contains(side_name(step.side));
                let suits = match kc_flash::check(&self.board, step.side, &bytes) {
                    Ok(info) => (has_family && !info.families.is_empty()) || by_name,
                    Err(kc_flash::FlashError::WrongHalf) => false,
                    Err(error) => {
                        problems.push(format!("{name}: {error}"));
                        rejected = true;
                        break;
                    }
                };
                if suits {
                    step.firmware = Some((path.clone(), bytes.clone()));
                    step.progress = Progress::Waiting;
                    used = true;
                }
            }
            if !used && !rejected {
                problems.push(format!(
                    "{name}: could not tell which half this is for. Name the file with “left” or “right”."
                ));
            }
        }
        self.message = (!problems.is_empty()).then(|| problems.join(" "));
        cx.notify();
    }

    fn choose_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Choose Firmware".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await {
                let _ = this.update(cx, |this, cx| this.add_files(paths, cx));
            }
        })
        .detach();
    }

    /// The step that should be flashed next.
    fn current(&self) -> Option<usize> {
        self.steps.iter().position(|s| s.progress != Progress::Done)
    }

    /// The mounted drive a step can be flashed to. A drive name shared by
    /// both halves is only offered to the current step, because the user
    /// is told which half to plug in.
    fn drive_for(&self, index: usize) -> Option<&Bootloader> {
        let step = &self.steps[index];
        self.drives.iter().find(|drive| {
            drive.name == step.volume
                && match kc_flash::identify(&self.board, drive) {
                    Half::Known(side) => side == step.side,
                    Half::Either => self.current() == Some(index),
                    Half::NotThisBoard => false,
                }
        })
    }

    fn flash(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(drive) = self.drive_for(index).cloned() else {
            return;
        };
        let step = &mut self.steps[index];
        let Some((_, firmware)) = &step.firmware else {
            return;
        };
        step.progress = match kc_flash::flash(&drive, firmware) {
            Ok(()) => Progress::Done,
            Err(error) => Progress::Failed(error.to_string()),
        };
        cx.notify();
    }

    fn start_over(&mut self, cx: &mut Context<Self>) {
        for step in &mut self.steps {
            step.progress = Progress::Waiting;
        }
        cx.notify();
    }
}

impl Render for FlashView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted, danger, success, accent) = (
            theme.border,
            theme.muted_foreground,
            theme.danger,
            theme.success,
            theme.primary,
        );
        let current = self.current();
        let rows = (0..self.steps.len())
            .map(|index| {
                let step = &self.steps[index];
                let drive = self.drive_for(index).is_some();
                let file = step.firmware.as_ref().map(|(path, _)| {
                    path.file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                });
                let (status, color) = match (&step.progress, &file, drive) {
                    (Progress::Done, _, _) => ("Flashed.".to_string(), success),
                    (Progress::Failed(error), _, _) => (format!("Failed: {error}"), danger),
                    (_, None, _) => ("No firmware chosen for this half yet.".to_string(), muted),
                    (_, Some(_), true) => ("Bootloader found. Ready to flash.".to_string(), accent),
                    (_, Some(_), false) if current == Some(index) => (
                        format!(
                            "Connect the {} half by USB and put it into its bootloader. Waiting for {}…",
                            side_name(step.side),
                            step.volume
                        ),
                        muted,
                    ),
                    (_, Some(_), false) => ("Waiting for the previous half.".to_string(), muted),
                };
                let ready = drive && file.is_some() && step.progress != Progress::Done;
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .p_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(if current == Some(index) { accent } else { border })
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_sm().child(format!(
                                "{}. {} half",
                                index + 1,
                                if step.side == Side::Left { "Left" } else { "Right" }
                            )))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(file.unwrap_or_else(|| "No file".to_string())),
                            )
                            .child(div().text_xs().text_color(color).child(status)),
                    )
                    .when(ready, |row| {
                        row.child(
                            Button::new(("flash", index))
                                .primary()
                                .label(format!("Flash {} Half", if step.side == Side::Left { "Left" } else { "Right" }))
                                .on_click(cx.listener(move |this, _, _, cx| this.flash(index, cx))),
                        )
                    })
            })
            .collect::<Vec<_>>();

        let finished = current.is_none();
        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .px_6()
            .pb_6()
            .gap_4()
            .child(
                div()
                    .w(px(640.))
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(subheading(format!(
                        "Flash the {} {}",
                        self.board.vendor, self.board.name
                    ), cx))
                    .child(help("Firmware from a build appears here on its own. You can also choose .uf2 files yourself, one for each half. Each half is flashed separately, in the order shown.", cx))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("choose-firmware")
                                    .label("Choose Firmware Files…")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_files(window, cx);
                                    })),
                            )
                            .when(self.steps.iter().any(|s| s.progress != Progress::Waiting), |row| {
                                row.child(
                                    Button::new("flash-again")
                                        .ghost()
                                        .label("Start Over")
                                        .on_click(cx.listener(|this, _, _, cx| this.start_over(cx))),
                                )
                            }),
                    )
                    .when_some(self.message.clone(), |page, message| {
                        page.child(div().text_sm().text_color(danger).child(message))
                    })
                    .children(rows)
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("To enter the bootloader: {}", self.board.flash.bootloader_entry)),
                    )
                    .when(finished, |page| {
                        page.child(div().text_sm().text_color(success).child(
                            "Both halves are flashed. They restart on their own and reconnect to each other.",
                        ))
                    }),
            )
    }
}
