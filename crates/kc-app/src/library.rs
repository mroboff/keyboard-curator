//! The user's saved keyboards as shared app state: loaded once, saved on
//! every change, and watched by the welcome screen and any open project.
//! It also keeps an eye on which USB devices are connected.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::*;
use kc_device::UsbDevice;
use kc_model::keyboards::{KeyboardError, KeyboardsFileError};
use kc_model::{Keyboard, Keyboards};

/// How often the connected USB devices are looked at again.
const USB_POLL: Duration = Duration::from_secs(2);

fn keyboards_path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("Keyboard Curator")
            .join("keyboards.json"),
    )
}

pub struct Library {
    keyboards: Keyboards,
    /// Why the keyboards file could not be read or written, if it could not.
    problem: Option<String>,
    connected: Vec<UsbDevice>,
}

impl Library {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (keyboards, problem) = match keyboards_path().map(|path| Keyboards::load(&path)) {
            Some(Ok(keyboards)) => (keyboards, None),
            Some(Err(error)) => (Keyboards::default(), Some(Self::set_aside(&error))),
            None => (Keyboards::default(), None),
        };
        cx.spawn(async move |this, cx| loop {
            let devices = cx
                .background_executor()
                .spawn(async { kc_device::connected() })
                .await;
            let alive = this.update(cx, |this, cx| {
                if this.connected != devices {
                    this.connected = devices;
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
            cx.background_executor().timer(USB_POLL).await;
        })
        .detach();
        Self {
            keyboards,
            problem,
            connected: Vec::new(),
        }
    }

    /// Moves a keyboards file that cannot be read out of the way, so that
    /// starting afresh does not destroy it, and says what happened.
    fn set_aside(error: &KeyboardsFileError) -> String {
        let kept = keyboards_path().and_then(|path| {
            let aside = path.with_extension("unreadable.json");
            std::fs::rename(&path, &aside).ok().map(|()| aside)
        });
        match kept {
            Some(aside) => format!(
                "Your saved keyboards could not be read ({error}). The file was kept as {}.",
                aside.display()
            ),
            None => format!("Your saved keyboards could not be read ({error})."),
        }
    }

    pub fn keyboards(&self) -> &Keyboards {
        &self.keyboards
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// Changes the saved keyboards and writes them to disk.
    pub fn change<T>(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Keyboards) -> Result<T, KeyboardError>,
    ) -> Result<T, KeyboardError> {
        let result = change(&mut self.keyboards);
        if let Some(path) = keyboards_path() {
            self.problem = self
                .keyboards
                .save(&path)
                .err()
                .map(|error| format!("Your keyboards could not be saved: {error}."));
        }
        cx.notify();
        result
    }

    /// Whether the keyboard's linked device is connected by USB.
    pub fn is_connected(&self, keyboard: &Keyboard) -> bool {
        keyboard
            .device
            .as_ref()
            .is_some_and(|device| kc_device::is_connected(device, &self.connected))
    }
}
