//! The Curator firmware on a Defy's board page: getting a release of the
//! fork, keeping it for Bazecor's custom firmware folder, and installing
//! the Neuron image from here.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_firmware::dygma_fork::{self, Progress};
use kc_model::Keyboard;

use super::{BoardPage, BuildStatus};
use crate::workspace::{card, help, subheading};

/// Where fetched releases are kept, as folders Bazecor's custom firmware
/// option can be pointed at.
fn firmware_dir() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("Keyboard Curator").join("firmware"))
}

fn progress_words(progress: &Progress) -> String {
    match progress {
        Progress::Restarting => {
            "Asking the keyboard to restart its Neuron into the bootloader…".into()
        }
        Progress::Erasing => "The Neuron is in its bootloader. Erasing…".into(),
        Progress::Writing { done, total } => {
            format!(
                "Writing the firmware: {} of {} KB…",
                done / 1024,
                total.div_ceil(1024)
            )
        }
        Progress::Starting => {
            "Starting the new firmware; waiting for the keyboard to come back…".into()
        }
        Progress::Done { version } => format!("Done. The keyboard reports {version}."),
    }
}

impl BoardPage {
    /// Fetches the fork's release named by the board's firmware, checks it,
    /// and keeps it on disk.
    pub(super) fn fetch_fork(&mut self, cx: &mut Context<Self>) {
        let (board, config) = (self.board.clone(), self.config(cx));
        let Some(fork) = dygma_fork::fork(&board, &config).cloned() else {
            return;
        };
        self.fork =
            BuildStatus::Working(format!("Fetching {} from {}…", fork.tag, fork.repository));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let fetched = cx
                .background_executor()
                .spawn(async move {
                    let token = kc_build::github::find_token();
                    let firmware = dygma_fork::fetch(&board, &config, token.as_deref())?;
                    let dir = firmware_dir().ok_or_else(|| "no configuration folder".to_string())?;
                    let folder = dygma_fork::save(&firmware, &dir)?;
                    Ok::<_, String>((firmware, folder))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match fetched {
                    Ok((firmware, folder)) => {
                        let commit = &firmware.manifest.source.commit;
                        this.fork = BuildStatus::Succeeded(format!(
                            "Fetched and checked {} (built from {}). The files are in {}, ready for Bazecor's custom firmware folder, or install the Neuron image below.",
                            firmware.tag,
                            &commit[..commit.len().min(9)],
                            folder.display()
                        ));
                        this.fork_firmware = Some(Arc::new(firmware));
                    }
                    Err(message) => {
                        this.fork = BuildStatus::Failed { message, url: None };
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Installs the fetched Neuron image on the connected keyboard, after
    /// the user has confirmed.
    pub(super) fn install_fork(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(firmware) = self.fork_firmware.clone() else {
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Install the {} on the Neuron?", firmware.fork_name),
            Some(&format!(
                "This writes {} to the keyboard's Neuron. The keyboard restarts into its bootloader (its light pulses blue), the image is written, and the keyboard comes back on the new firmware. Your keys, superkeys, macros, colors and settings stay as they are. Do not unplug the keyboard until it says it is done. If something goes wrong, the Neuron stays in its bootloader and Bazecor's firmware update, or this button, flashes it again. This has not been run on a keyboard before.",
                firmware.tag
            )),
            &["Cancel", "Install"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(1) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.live_busy = true;
                this.install = BuildStatus::Working("Starting…".into());
                cx.notify();
            });
            let Ok((board, device)) = this.update(cx, |this, cx| {
                let device = this.saved(cx).and_then(|k| k.device.clone());
                (this.board.clone(), device)
            }) else {
                return;
            };
            let progress: Arc<Mutex<Option<Progress>>> = Arc::new(Mutex::new(None));
            let result: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));
            {
                let (progress, result, firmware) = (progress.clone(), result.clone(), firmware.clone());
                std::thread::spawn(move || {
                    let mut report = |p: Progress| {
                        *progress.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
                    };
                    let outcome = dygma_fork::install(&board, device.as_ref(), &firmware, &mut report);
                    *result.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
                });
            }
            let mut last = String::new();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let done = result.lock().unwrap_or_else(|e| e.into_inner()).take();
                let words = progress
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_ref()
                    .map(progress_words);
                if let Some(outcome) = done {
                    let _ = this.update(cx, |this, cx| {
                        this.live_busy = false;
                        this.install = match outcome {
                            Ok(version) => BuildStatus::Succeeded(format!(
                                "Installed. The keyboard reports {version}. Read From Keyboard to see what it holds now."
                            )),
                            Err(message) => BuildStatus::Failed { message, url: None },
                        };
                        cx.notify();
                    });
                    break;
                }
                if let Some(words) = words.filter(|w| *w != last) {
                    last = words.clone();
                    let _ = this.update(cx, |this, cx| {
                        this.install = BuildStatus::Working(words);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// The panel for a Dygma profile taken from the fork's releases:
    /// getting the files and installing the Neuron image.
    pub(super) fn render_fork_section(&self, saved: &Keyboard, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (muted, danger, success) = (theme.muted_foreground, theme.danger, theme.success);
        let config = saved.firmware.clone();
        let Some(fork) = dygma_fork::fork(&self.board, &config).cloned() else {
            return div();
        };
        let connected = self.library.read(cx).is_connected(saved);
        let fetching = matches!(self.fork, BuildStatus::Working(_));
        let installing = matches!(self.install, BuildStatus::Working(_));
        let url = format!(
            "https://github.com/{}/releases/tag/{}",
            fork.repository, fork.tag
        );
        let status = |panel: Div, status: &BuildStatus| match status {
            BuildStatus::Idle => panel,
            BuildStatus::Working(text) => panel.child(div().text_sm().child(text.clone())),
            BuildStatus::Succeeded(text) => {
                panel.child(div().text_sm().text_color(success).child(text.clone()))
            }
            BuildStatus::Failed { message, .. } => {
                panel.child(div().text_sm().text_color(danger).child(message.clone()))
            }
        };
        let mut get = card(false, cx)
            .w(px(640.))
            .flex()
            .flex_col()
            .gap_3()
            .child(subheading("Get the firmware", cx))
            .child(div().text_sm().text_color(muted).child(format!(
                "The {} comes ready-made from release {} of {} on GitHub: the Neuron image and Dygma's own image for the sides. It is checked against the release's checksums and against the sources this app expects before it is offered, and kept in a folder Bazecor's custom firmware option can be pointed at.",
                fork.name, fork.tag, fork.repository
            )))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .when(!fetching && !installing, |row| {
                        row.child(
                            Button::new("fetch-fork")
                                .primary()
                                .label(format!("Fetch Firmware {}", fork.tag))
                                .on_click(cx.listener(|this, _, _, cx| this.fetch_fork(cx))),
                        )
                    })
                    .child(
                        Button::new("open-fork-release")
                            .ghost()
                            .label("Open the Release")
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    ),
            );
        get = status(get, &self.fork);

        let mut install = card(false, cx)
            .w(px(640.))
            .flex()
            .flex_col()
            .gap_3()
            .child(subheading("Install on the Neuron", cx))
            .child(div().text_sm().text_color(muted).child(
                "Writes the fetched Neuron image to the connected keyboard the way Bazecor's firmware update does: the keyboard restarts into its bootloader, the image is written, and the keyboard comes back on the new firmware. The sides keep Dygma's firmware. Going back is Bazecor's firmware update with Dygma's own release. Experimental: this installer has been checked against a simulated bootloader only.",
            ));
        if self.fork_firmware.is_some() && connected && !self.live_busy && !fetching {
            install = install.child(
                div().flex().child(
                    Button::new("install-fork")
                        .label("Install on the Neuron…")
                        .on_click(cx.listener(|this, _, window, cx| this.install_fork(window, cx))),
                ),
            );
        } else if self.fork_firmware.is_none() {
            install = install.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Fetch the firmware first."),
            );
        } else if !connected {
            install = install.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Connect the keyboard by USB to install."),
            );
        }
        install = status(install, &self.install);

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(help(
                format!("This board runs the {}, a fork of Dygma's firmware with combos and per-key timing. The keyboard must be running it for those to reach it; Read From Keyboard says which firmware the keyboard reports.", fork.name),
                cx,
            ))
            .child(get)
            .child(install)
    }
}
