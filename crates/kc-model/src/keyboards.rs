//! The user's keyboards: which boards they work on, the firmware each one
//! runs, and which physical device each one is.
//!
//! A [`Keyboard`] is separate from any project. Projects are opened under a
//! keyboard, which decides the firmware they are built for; a project made
//! for another firmware of the same board is switched over with
//! [`retarget`], keeping whatever the new firmware cannot show.

use std::path::{Path, PathBuf};

use kc_boards::Board;
use kc_zmk::settings::setting_for;
use kc_zmk::Feature;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::features::KeyLight;
use crate::ids::KeyboardId;
use crate::project::Project;
use crate::validate::{validate, Severity};

/// The keyboards file format version this build writes.
pub const FORMAT: u32 = 1;

/// How many recent projects each keyboard remembers.
const RECENT_LIMIT: usize = 8;

/// A physical keyboard, as its USB connection identifies it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Device {
    pub vendor: u16,
    pub product: u16,
    pub serial: String,
}

/// One of the user's keyboards. It need not be connected, or even owned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keyboard {
    pub id: KeyboardId,
    pub name: String,
    /// Board and firmware profile, by their IDs in the board definition.
    pub board: String,
    pub firmware: String,
    /// The physical keyboard this is, once one has been linked.
    pub device: Option<Device>,
    /// The local clone of the firmware repository this keyboard builds from.
    pub repo_dir: Option<PathBuf>,
    /// Projects last opened under this keyboard, newest first.
    #[serde(default)]
    pub recent: Vec<PathBuf>,
}

/// How well a project suits a keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Made for this board and firmware.
    Exact,
    /// Made for this board with another firmware; it can be retargeted.
    OtherFirmware,
}

/// Where a project being opened belongs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// Under this keyboard, as it is.
    Open(KeyboardId),
    /// Under this keyboard, once switched to its firmware.
    Retarget(KeyboardId),
    /// Under one of these; the user chooses.
    Choose(Vec<(KeyboardId, Fit)>),
    /// No saved keyboard is of the project's board.
    NoKeyboard,
}

impl Keyboard {
    /// Whether `project` can be opened under this keyboard, and how.
    /// `None` when it is for a different board.
    pub fn fit(&self, project: &Project) -> Option<Fit> {
        if project.board != self.board {
            None
        } else if project.firmware == self.firmware {
            Some(Fit::Exact)
        } else {
            Some(Fit::OtherFirmware)
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum KeyboardError {
    #[error("no such keyboard")]
    NoSuchKeyboard,
    #[error("a keyboard needs a name")]
    EmptyName,
    #[error("the board `{board}` has no firmware `{firmware}`")]
    NoSuchFirmware { board: String, firmware: String },
    #[error("that device is already linked to another keyboard")]
    DeviceTaken { by: KeyboardId },
}

#[derive(Debug, thiserror::Error)]
pub enum KeyboardsFileError {
    #[error("the keyboards file is damaged: {0}")]
    Invalid(String),
    #[error("the keyboards file was saved by a newer version of Keyboard Curator (format {found}; this version reads up to {supported})")]
    Newer { found: u64, supported: u32 },
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Every keyboard the user has saved. No device is ever linked to more than
/// one of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keyboards {
    format: u32,
    keyboards: Vec<Keyboard>,
    next_id: u32,
}

impl Default for Keyboards {
    fn default() -> Self {
        Self {
            format: FORMAT,
            keyboards: Vec::new(),
            next_id: 1,
        }
    }
}

impl Keyboards {
    pub fn iter(&self) -> impl Iterator<Item = &Keyboard> {
        self.keyboards.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.keyboards.is_empty()
    }

    pub fn get(&self, id: KeyboardId) -> Option<&Keyboard> {
        self.keyboards.iter().find(|k| k.id == id)
    }

    fn get_mut(&mut self, id: KeyboardId) -> Result<&mut Keyboard, KeyboardError> {
        self.keyboards
            .iter_mut()
            .find(|k| k.id == id)
            .ok_or(KeyboardError::NoSuchKeyboard)
    }

    /// A name for a new keyboard of this board that no saved one has yet:
    /// `My Go60`, then `My Go60 2`, and so on.
    pub fn suggest_name(&self, board: &Board) -> String {
        let base = format!("My {}", board.name);
        let taken = |name: &str| self.keyboards.iter().any(|k| k.name == name);
        if !taken(&base) {
            return base;
        }
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|name| !taken(name))
            .expect("some number is always free")
    }

    pub fn add(
        &mut self,
        name: &str,
        board: &Board,
        firmware: &str,
    ) -> Result<KeyboardId, KeyboardError> {
        let name = checked_name(name)?;
        check_firmware(board, firmware)?;
        let id = KeyboardId(self.next_id);
        self.next_id += 1;
        self.keyboards.push(Keyboard {
            id,
            name,
            board: board.id.clone(),
            firmware: firmware.to_string(),
            device: None,
            repo_dir: None,
            recent: Vec::new(),
        });
        Ok(id)
    }

    pub fn remove(&mut self, id: KeyboardId) -> Result<Keyboard, KeyboardError> {
        let index = self
            .keyboards
            .iter()
            .position(|k| k.id == id)
            .ok_or(KeyboardError::NoSuchKeyboard)?;
        Ok(self.keyboards.remove(index))
    }

    pub fn rename(&mut self, id: KeyboardId, name: &str) -> Result<(), KeyboardError> {
        let name = checked_name(name)?;
        self.get_mut(id)?.name = name;
        Ok(())
    }

    /// Records that the keyboard now runs `firmware`. `board` must be the
    /// keyboard's own board.
    pub fn set_firmware(
        &mut self,
        id: KeyboardId,
        board: &Board,
        firmware: &str,
    ) -> Result<(), KeyboardError> {
        let keyboard = self.get_mut(id)?;
        if keyboard.board != board.id {
            return Err(KeyboardError::NoSuchFirmware {
                board: keyboard.board.clone(),
                firmware: firmware.to_string(),
            });
        }
        check_firmware(board, firmware)?;
        keyboard.firmware = firmware.to_string();
        Ok(())
    }

    /// The keyboard a device is linked to, if any.
    pub fn linked_to(&self, device: &Device) -> Option<&Keyboard> {
        self.keyboards
            .iter()
            .find(|k| k.device.as_ref() == Some(device))
    }

    /// Links a device to a keyboard. Fails when another keyboard has it;
    /// [`Keyboards::relink`] takes it over instead.
    pub fn link(&mut self, id: KeyboardId, device: Device) -> Result<(), KeyboardError> {
        if let Some(other) = self.linked_to(&device).filter(|k| k.id != id) {
            return Err(KeyboardError::DeviceTaken { by: other.id });
        }
        self.get_mut(id)?.device = Some(device);
        Ok(())
    }

    /// Links a device to a keyboard, unlinking it from whichever keyboard
    /// had it. Returns that keyboard's ID.
    pub fn relink(
        &mut self,
        id: KeyboardId,
        device: Device,
    ) -> Result<Option<KeyboardId>, KeyboardError> {
        self.get_mut(id)?;
        let previous = self.linked_to(&device).map(|k| k.id).filter(|k| *k != id);
        if let Some(previous) = previous {
            self.get_mut(previous)?.device = None;
        }
        self.get_mut(id)?.device = Some(device);
        Ok(previous)
    }

    pub fn unlink(&mut self, id: KeyboardId) -> Result<(), KeyboardError> {
        self.get_mut(id)?.device = None;
        Ok(())
    }

    pub fn set_repo_dir(
        &mut self,
        id: KeyboardId,
        dir: Option<PathBuf>,
    ) -> Result<(), KeyboardError> {
        self.get_mut(id)?.repo_dir = dir;
        Ok(())
    }

    /// Moves `path` to the front of the keyboard's recent projects.
    pub fn note_recent(&mut self, id: KeyboardId, path: PathBuf) -> Result<(), KeyboardError> {
        let recent = &mut self.get_mut(id)?.recent;
        recent.retain(|p| *p != path);
        recent.insert(0, path);
        recent.truncate(RECENT_LIMIT);
        Ok(())
    }

    pub fn forget_recent(&mut self, id: KeyboardId, path: &Path) -> Result<(), KeyboardError> {
        self.get_mut(id)?.recent.retain(|p| p != path);
        Ok(())
    }

    /// The keyboards `project` can be opened under, exact fits first.
    pub fn fitting(&self, project: &Project) -> Vec<(KeyboardId, Fit)> {
        let mut fits: Vec<_> = self
            .keyboards
            .iter()
            .filter_map(|k| Some((k.id, k.fit(project)?)))
            .collect();
        fits.sort_by_key(|(_, fit)| *fit != Fit::Exact);
        fits
    }

    /// Decides which keyboard `project` opens under. The selected keyboard
    /// is used whenever the project suits it, since that is the one being
    /// worked on; otherwise the saved keyboards of the project's board are
    /// considered, exact fits first.
    pub fn place(&self, project: &Project, selected: Option<KeyboardId>) -> Placement {
        let direct = |(id, fit)| match fit {
            Fit::Exact => Placement::Open(id),
            Fit::OtherFirmware => Placement::Retarget(id),
        };
        let selected = selected
            .and_then(|id| self.get(id))
            .and_then(|k| Some((k.id, k.fit(project)?)));
        if let Some(fit) = selected {
            return direct(fit);
        }
        let fits = self.fitting(project);
        let exact: Vec<_> = fits
            .iter()
            .copied()
            .filter(|(_, fit)| *fit == Fit::Exact)
            .collect();
        let choices = if exact.is_empty() { fits } else { exact };
        match choices.as_slice() {
            [] => Placement::NoKeyboard,
            [only] => direct(*only),
            _ => Placement::Choose(choices),
        }
    }

    /// Restores what a hand-edited or damaged file may have broken: unique
    /// IDs, and no device linked twice. The first keyboard keeps a
    /// contested device.
    fn repair(&mut self) {
        let mut ids = std::collections::HashSet::new();
        self.keyboards.retain(|k| ids.insert(k.id));
        let mut devices = std::collections::HashSet::new();
        for keyboard in &mut self.keyboards {
            if let Some(device) = &keyboard.device {
                if !devices.insert(device.clone()) {
                    keyboard.device = None;
                }
            }
        }
        let highest = self.keyboards.iter().map(|k| k.id.0).max().unwrap_or(0);
        self.next_id = self.next_id.max(highest + 1);
    }

    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("keyboards always serialize");
        text.push('\n');
        text
    }

    pub fn from_json(text: &str) -> Result<Self, KeyboardsFileError> {
        let value: Value =
            serde_json::from_str(text).map_err(|e| KeyboardsFileError::Invalid(e.to_string()))?;
        let found = value
            .get("format")
            .and_then(Value::as_u64)
            .ok_or_else(|| KeyboardsFileError::Invalid("it has no format version".into()))?;
        if found > u64::from(FORMAT) {
            return Err(KeyboardsFileError::Newer {
                found,
                supported: FORMAT,
            });
        }
        let mut keyboards: Self = serde_json::from_value(value)
            .map_err(|e| KeyboardsFileError::Invalid(e.to_string()))?;
        keyboards.format = FORMAT;
        keyboards.repair();
        Ok(keyboards)
    }

    /// Loads the keyboards file; a missing file means no keyboards yet.
    pub fn load(path: &Path) -> Result<Self, KeyboardsFileError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_json(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Writes the file, replacing any existing one only once the new
    /// contents are safely on disk.
    pub fn save(&self, path: &Path) -> Result<(), KeyboardsFileError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut temp = path.as_os_str().to_owned();
        temp.push(".tmp");
        std::fs::write(&temp, self.to_json())?;
        std::fs::rename(&temp, path)?;
        Ok(())
    }
}

fn checked_name(name: &str) -> Result<String, KeyboardError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(KeyboardError::EmptyName);
    }
    Ok(name.to_string())
}

fn check_firmware(board: &Board, firmware: &str) -> Result<(), KeyboardError> {
    if board.profile(firmware).is_none() {
        return Err(KeyboardError::NoSuchFirmware {
            board: board.id.clone(),
            firmware: firmware.to_string(),
        });
    }
    Ok(())
}

/// What a project holds that a firmware cannot use. It stays in the project
/// file, out of sight and out of the generated config, and returns when the
/// project is opened with a firmware that has the feature.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hidden {
    /// Per-key colors, when the firmware has no per-key lighting.
    pub lighting: bool,
    /// Pointing-device configuration, when the firmware has no pointing.
    pub pointing: bool,
    /// The names of settings the firmware does not have.
    pub settings: Vec<&'static str>,
}

impl Hidden {
    pub fn is_empty(&self) -> bool {
        !self.lighting && !self.pointing && self.settings.is_empty()
    }
}

/// What of `project` a firmware with `features` leaves unused.
pub fn hidden(project: &Project, features: &[Feature]) -> Hidden {
    let lacks = |feature| !features.contains(&feature);
    Hidden {
        lighting: lacks(Feature::PerKeyLighting)
            && project
                .lighting
                .iter()
                .any(|l| l.keys.iter().any(|k| *k != KeyLight::Inherit)),
        pointing: lacks(Feature::Pointing) && !project.pointing.is_empty(),
        settings: project
            .settings
            .keys()
            .filter_map(|key| setting_for(key))
            .filter(|setting| setting.requires.is_some_and(lacks))
            .map(|setting| setting.name)
            .collect(),
    }
}

/// What switching a project to another firmware of its board would do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Retarget {
    /// What the new firmware hides that the current one shows.
    pub hidden: Hidden,
    /// How many new problems the switch causes: keys and behaviors that
    /// use something the new firmware lacks. These are flagged, not hidden,
    /// because a key cannot be left out of a keymap.
    pub flagged: usize,
}

impl Retarget {
    /// What the switch means, in sentences for the user.
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.hidden.lighting {
            parts.push("Per-key colors will be hidden.".into());
        }
        if self.hidden.pointing {
            parts.push("Pointing device configuration will be hidden.".into());
        }
        match self.hidden.settings.as_slice() {
            [] => {}
            [one] => parts.push(format!("The setting \u{201c}{one}\u{201d} will be hidden.")),
            many => parts.push(format!(
                "{} settings will be hidden: {}.",
                many.len(),
                many.join(", ")
            )),
        }
        if !self.hidden.is_empty() {
            parts.push(
                "Hidden parts stay in the project and return with a firmware that has them.".into(),
            );
        }
        match self.flagged {
            0 => {}
            1 => parts.push(
                "One key or behavior uses a feature this firmware lacks and will be flagged."
                    .into(),
            ),
            n => parts.push(format!(
                "{n} keys or behaviors use features this firmware lacks and will be flagged."
            )),
        }
        if parts.is_empty() {
            parts.push("Everything in the project works with this firmware.".into());
        }
        parts.join(" ")
    }
}

/// Previews switching `project` to `firmware`, a profile of `board`.
pub fn preview_retarget(
    project: &Project,
    board: &Board,
    firmware: &str,
) -> Result<Retarget, KeyboardError> {
    check_firmware(board, firmware)?;
    let features = &board
        .profile(firmware)
        .expect("checked just above")
        .capabilities;
    let errors = |project: &Project| {
        validate(project, board)
            .into_iter()
            .filter(|p| p.severity == Severity::Error)
            .collect::<Vec<_>>()
    };
    let before = errors(project);
    let mut switched = project.clone();
    switched.firmware = firmware.to_string();
    let flagged = errors(&switched)
        .iter()
        .filter(|p| !before.contains(p))
        .count();
    Ok(Retarget {
        hidden: hidden(project, features),
        flagged,
    })
}

/// Switches `project` to `firmware`, a profile of `board`. Nothing else in
/// the project changes.
pub fn retarget(project: &mut Project, board: &Board, firmware: &str) -> Result<(), KeyboardError> {
    check_firmware(board, firmware)?;
    project.firmware = firmware.to_string();
    Ok(())
}
