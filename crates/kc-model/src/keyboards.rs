//! The user's keyboards: which boards they work on, the firmware each one
//! runs and how it is set up, which physical device each one is, and the
//! layouts used with it.
//!
//! A [`Keyboard`] owns everything about the firmware. A layout (a
//! [`Project`]) owns the keymap and nothing about the firmware, so one
//! keyboard can have many layouts, and a layout can be used with any
//! keyboard of its board.

use std::path::{Path, PathBuf};

use kc_boards::Board;
use kc_zmk::Feature;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::features::{KeyLight, SettingValue};
use crate::firmware::{Carried, FirmwareConfig};
use crate::ids::KeyboardId;
use crate::project::Project;

/// The keyboards file format version this build writes.
pub const FORMAT: u32 = 2;

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
    /// The board, by its ID in the board definitions.
    pub board: String,
    /// The firmware the keyboard runs and its settings.
    pub firmware: FirmwareConfig,
    /// The physical keyboard this is, once one has been linked.
    pub device: Option<Device>,
    /// The local clone of the firmware repository this keyboard builds from.
    pub repo_dir: Option<PathBuf>,
    /// The layout files used with this keyboard, last used first.
    #[serde(default)]
    pub layouts: Vec<PathBuf>,
    /// The layout that goes into the firmware when it is built. Without
    /// one, the firmware is built with the board's factory layout.
    #[serde(default)]
    pub current: Option<PathBuf>,
}

impl Keyboard {
    /// Whether `project` is a layout for this keyboard's board.
    pub fn suits(&self, project: &Project) -> bool {
        project.board == self.board
    }
}

/// Which keyboard a layout being opened belongs under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    Open(KeyboardId),
    /// Under one of these; the user chooses.
    Choose(Vec<KeyboardId>),
    /// No saved keyboard is of the layout's board.
    NoKeyboard,
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

    /// Adds a keyboard running `firmware` with every setting at its
    /// default.
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
            firmware: FirmwareConfig::new(firmware),
            device: None,
            repo_dir: None,
            layouts: Vec::new(),
            current: None,
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

    // Firmware

    /// Records that the keyboard now runs `firmware`. `board` must be the
    /// keyboard's own board. Settings the new firmware lacks are kept, and
    /// return if the keyboard goes back.
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
        keyboard.firmware.profile = firmware.to_string();
        Ok(())
    }

    /// Sets a firmware setting, or with `None` returns it to the board's
    /// default.
    pub fn set_setting(
        &mut self,
        id: KeyboardId,
        key: &str,
        value: Option<SettingValue>,
    ) -> Result<(), KeyboardError> {
        let settings = &mut self.get_mut(id)?.firmware.settings;
        match value {
            Some(value) => settings.insert(key.to_string(), value),
            None => settings.remove(key),
        };
        Ok(())
    }

    pub fn set_raw_conf(&mut self, id: KeyboardId, text: &str) -> Result<(), KeyboardError> {
        self.get_mut(id)?.firmware.raw_conf = text.to_string();
        Ok(())
    }

    /// Gives the keyboard the firmware settings that came with a layout.
    pub fn absorb(&mut self, id: KeyboardId, carried: Carried) -> Result<(), KeyboardError> {
        self.get_mut(id)?.firmware.absorb(carried);
        Ok(())
    }

    // Devices

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

    // Layouts

    /// Lists a layout file under the keyboard, or moves it to the front.
    pub fn note_layout(&mut self, id: KeyboardId, path: PathBuf) -> Result<(), KeyboardError> {
        let layouts = &mut self.get_mut(id)?.layouts;
        layouts.retain(|p| *p != path);
        layouts.insert(0, path);
        Ok(())
    }

    /// Takes a layout file off the keyboard's list. The file is untouched.
    /// If it was the current layout, the keyboard has none.
    pub fn forget_layout(&mut self, id: KeyboardId, path: &Path) -> Result<(), KeyboardError> {
        let keyboard = self.get_mut(id)?;
        keyboard.layouts.retain(|p| p != path);
        if keyboard.current.as_deref() == Some(path) {
            keyboard.current = None;
        }
        Ok(())
    }

    /// Chooses the layout the keyboard's firmware is built with; `None`
    /// goes back to the factory layout.
    pub fn set_current(
        &mut self,
        id: KeyboardId,
        path: Option<PathBuf>,
    ) -> Result<(), KeyboardError> {
        let keyboard = self.get_mut(id)?;
        if let Some(path) = &path {
            if !keyboard.layouts.contains(path) {
                keyboard.layouts.insert(0, path.clone());
            }
        }
        keyboard.current = path;
        Ok(())
    }

    /// Decides which keyboard a layout opens under: the selected one when
    /// it is of the layout's board, since that is the one being worked on,
    /// and otherwise whichever saved keyboard is.
    pub fn place(&self, project: &Project, selected: Option<KeyboardId>) -> Placement {
        let selected = selected
            .and_then(|id| self.get(id))
            .filter(|k| k.suits(project));
        if let Some(keyboard) = selected {
            return Placement::Open(keyboard.id);
        }
        let suiting: Vec<KeyboardId> = self
            .keyboards
            .iter()
            .filter(|k| k.suits(project))
            .map(|k| k.id)
            .collect();
        match suiting.as_slice() {
            [] => Placement::NoKeyboard,
            [only] => Placement::Open(*only),
            _ => Placement::Choose(suiting),
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
        let mut value: Value =
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
        if found == 1 {
            migrate_from_1(&mut value);
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

/// Format 1 held only the firmware's ID, and called the layout list
/// `recent`.
fn migrate_from_1(value: &mut Value) {
    let keyboards = value.get_mut("keyboards").and_then(Value::as_array_mut);
    for keyboard in keyboards.into_iter().flatten() {
        let Some(keyboard) = keyboard.as_object_mut() else {
            continue;
        };
        if let Some(Value::String(profile)) = keyboard.get("firmware").cloned() {
            keyboard.insert("firmware".into(), serde_json::json!({ "profile": profile }));
        }
        if let Some(recent) = keyboard.remove("recent") {
            keyboard.insert("layouts".into(), recent);
        }
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

/// What a layout holds that a firmware cannot use. It stays in the layout
/// file, out of sight and out of the generated config, and returns when the
/// layout is used with a firmware that has the feature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hidden {
    /// Per-key colors, when the firmware has no per-key lighting.
    pub lighting: bool,
    /// Pointing-device configuration, when the firmware has no pointing.
    pub pointing: bool,
}

impl Hidden {
    pub fn is_empty(&self) -> bool {
        !self.lighting && !self.pointing
    }

    /// A sentence for the user about what is hidden; `None` when nothing
    /// is.
    pub fn summary(&self) -> Option<String> {
        let what = match (self.lighting, self.pointing) {
            (false, false) => return None,
            (true, false) => "Per-key colors in this layout are",
            (false, true) => "Pointing configuration in this layout is",
            (true, true) => "Per-key colors and pointing configuration in this layout are",
        };
        Some(format!(
            "{what} hidden, because this board's firmware does not have the feature. Nothing is removed from the file."
        ))
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
    }
}
