//! Dygma keyboards, whose firmware is configured live.
//!
//! There is nothing to build: a layout is turned into the keyboard's own
//! stored form (a grid of key codes per layer, and a color per light chosen
//! from a small palette) and written over USB with Dygma's text-based Focus
//! protocol. Reading works the same way in reverse.
//!
//! Every write goes straight to the keyboard's flash, so this crate is
//! strict about what it sends: only the commands in [`WRITES`], only with
//! payloads of exactly the right size, and only what has changed. The
//! keyboard's own list of commands, from `help`, gates everything else, so
//! a firmware that lacks a command is never sent it.

pub mod chords;
pub mod codec;
pub mod flash;
pub mod macros;
pub mod settings;
pub mod superkeys;
pub mod timing;

use std::io::{Read, Write};
use std::time::Duration;

use kc_boards::board::{Capability, DygmaFork, DygmaProfile};
use kc_boards::Board;
use kc_model::behavior::{BehaviorKind, Macro, MacroStep, Scope};
use kc_model::features::{KeyLight, Rgb};
use kc_model::setup::KeySetup;
use kc_model::{
    BehaviorId, BehaviorRef, Binding, Device, LayerId, Location, Param, Problem, Project, Severity,
};
use serde::{Deserialize, Serialize};

/// The commands this crate reads with. None takes an argument, so none can
/// change anything. The `chords.*` and `timing.*` commands are the
/// Curator firmware's; stock firmware does not list them.
const READS: [&str; 19] = [
    "help",
    "version",
    "hardware.version",
    "hardware.chip_id",
    "keymap.custom",
    "palette",
    "colormap.map",
    "superkeys.map",
    "macros.map",
    "macros.memory",
    "chords.map",
    "chords.memory",
    "timing.map",
    "timing.capacity",
    "timing.defaults",
    "wireless.battery.left.level",
    "wireless.battery.right.level",
    "wireless.battery.left.status",
    "wireless.battery.right.status",
];

/// The only commands this crate writes with, besides the settings in
/// [`settings::MAPPINGS`].
pub const WRITES: [&str; 7] = [
    "keymap.custom",
    "palette",
    "colormap.map",
    "superkeys.map",
    "macros.map",
    "chords.map",
    "timing.map",
];

/// Whether a command may ever be written.
pub fn writable(command: &str) -> bool {
    WRITES.contains(&command) || settings::is_command(command) || restorable(command)
}

/// Parts of command names that a backup leaves out, as Bazecor's backup
/// does (src/api/backup/Backup.ts, `Backup.Commands`): actions, the
/// factory keymap, hardware probes, and values that are not
/// configuration. A command containing any of these is neither read for
/// the backup nor written by a restore.
pub const BACKUP_SKIP: [&str; 28] = [
    "eeprom",
    "hardware",
    "settings.valid?",
    "settings.version",
    "settings.crc",
    "keymap.default",
    "layer",
    "help",
    "version",
    "led.mode",
    "led.theme",
    "led.at",
    "led.setMultiple",
    "led.getMultiple",
    "led.setAll",
    "macros.trigger",
    "macros.memory",
    "upgrade",
    "settings.printConfig",
    "settings.aliveInterval",
    "settings.spiSpeed",
    "settings.cpuSpeed",
    "settings.ledDriverPullUp",
    "settings.underGlow",
    "settings.ledDriver",
    "wireless.battery.left",
    "wireless.battery.right",
    "wireless.battery",
];

/// Whether a command the keyboard lists goes into a backup: anything but
/// the skip list and what the image already holds in a field of its own.
pub fn backup_extra(command: &str) -> bool {
    !BACKUP_SKIP.iter().any(|skip| command.contains(skip))
        && !READS.contains(&command)
        && !settings::is_command(command)
}

/// Whether a backup's command is written back by a restore. Bazecor's
/// restore leaves the lights and the wireless side alone (`led.*` and
/// `wireless.*`), and so does this one.
pub fn restorable(command: &str) -> bool {
    (backup_extra(command) || settings::is_command(command))
        && !command.starts_with("led.")
        && !command.starts_with("wireless.")
}

/// Whether a command may ever be read.
pub fn readable(command: &str) -> bool {
    READS.contains(&command) || settings::is_command(command) || backup_extra(command)
}

/// Commands every Focus firmware has, sent whether or not `help` lists
/// them: the list itself, and what identifies the keyboard.
const ALWAYS: [&str; 3] = ["help", "version", "hardware.chip_id"];

#[derive(Debug, thiserror::Error)]
pub enum FocusError {
    #[error("the keyboard did not answer `{0}` in time. Close Dygma's Bazecor if it is open: only one program can talk to the keyboard at a time")]
    Timeout(String),
    #[error("the keyboard's answer to `{command}` was not understood: {problem}")]
    Malformed { command: String, problem: String },
    #[error("`{0}` is not a command this app sends")]
    Refused(String),
    #[error("no Dygma keyboard was found on USB")]
    NotFound,
    #[error("the keyboard holds a different amount of data than this app expects for this board, so nothing was written")]
    Mismatch,
    #[error("{0}")]
    Unfit(String),
    #[error("could not talk to the keyboard: {0}")]
    Io(#[from] std::io::Error),
}

/// A Focus conversation over any byte stream.
pub struct Focus<T: Read + Write> {
    stream: T,
    /// The commands the keyboard listed under `help`, once asked.
    commands: Option<Vec<String>>,
}

impl<T: Read + Write> Focus<T> {
    pub fn new(stream: T) -> Self {
        Self {
            stream,
            commands: None,
        }
    }

    /// The keyboard's own command list, asked for once.
    fn commands(&mut self) -> Result<&[String], FocusError> {
        if self.commands.is_none() {
            let listed = self
                .exchange("help")?
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            self.commands = Some(listed);
        }
        Ok(self.commands.as_deref().expect("just set"))
    }

    /// Whether the keyboard has a command, by its own account.
    pub fn has(&mut self, command: &str) -> Result<bool, FocusError> {
        if ALWAYS.contains(&command) {
            return Ok(true);
        }
        Ok(self.commands()?.iter().any(|c| c == command))
    }

    /// Ends the conversation and gives the stream back.
    pub fn into_inner(self) -> T {
        self.stream
    }

    /// Sends one line and collects the answer, which ends with a line that
    /// is a single full stop.
    fn exchange(&mut self, line: &str) -> Result<String, FocusError> {
        let command = line.split(' ').next().unwrap_or(line).to_string();
        self.stream.write_all(line.as_bytes())?;
        self.stream.write_all(b"\n")?;
        self.stream.flush()?;
        let mut answer = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err(FocusError::Timeout(command)),
                Ok(n) => answer.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    return Err(FocusError::Timeout(command));
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
            let text = String::from_utf8_lossy(&answer).replace("\r\n", "\n");
            if text == ".\n" {
                return Ok(String::new());
            }
            if let Some(body) = text.strip_suffix("\n.\n") {
                return Ok(body.trim().to_string());
            }
        }
    }

    /// Asks for a value. Only the commands in the read list are sent, and
    /// only when the keyboard lists them; a command it lacks reads as
    /// `None`.
    fn read(&mut self, command: &str) -> Result<Option<String>, FocusError> {
        if !readable(command) {
            return Err(FocusError::Refused(command.to_string()));
        }
        if !self.has(command)? {
            return Ok(None);
        }
        self.exchange(command).map(Some)
    }

    /// A value the keyboard must have.
    fn read_required(&mut self, command: &str) -> Result<String, FocusError> {
        self.read(command)?.ok_or_else(|| FocusError::Malformed {
            command: command.to_string(),
            problem: "the keyboard does not list this command".into(),
        })
    }

    /// Numbers from a command the keyboard may lack, which reads as empty.
    fn read_numbers_if_any<N: std::str::FromStr>(
        &mut self,
        command: &str,
    ) -> Result<Vec<N>, FocusError> {
        if !self.has(command)? {
            return Ok(Vec::new());
        }
        self.read_numbers(command)
    }

    fn read_numbers<N: std::str::FromStr>(&mut self, command: &str) -> Result<Vec<N>, FocusError> {
        self.read_required(command)?
            .split_whitespace()
            .map(|token| {
                token.parse().map_err(|_| FocusError::Malformed {
                    command: command.to_string(),
                    problem: format!("`{token}` is not a number in range"),
                })
            })
            .collect()
    }

    /// Writes a value. Only the commands in [`WRITES`] are sent, and only
    /// when the keyboard lists them.
    fn write<N: ToString>(&mut self, command: &str, values: &[N]) -> Result<(), FocusError> {
        if !writable(command) || !self.has(command)? {
            return Err(FocusError::Refused(command.to_string()));
        }
        let payload: Vec<String> = values.iter().map(ToString::to_string).collect();
        self.exchange(&format!("{command} {}", payload.join(" ")))?;
        Ok(())
    }

    /// Writes `saved` back to this keyboard: see [`restore`].
    pub fn restore_image(
        &mut self,
        saved: &Image,
        backup: impl FnOnce(&Image) -> std::io::Result<()>,
    ) -> Result<Applied, FocusError> {
        let before = self.read_image()?;
        let new = before.with_backup(saved)?;
        if new == before {
            return Ok(Applied {
                before,
                sent: Vec::new(),
            });
        }
        backup(&before)?;
        let sent = self.write_changes(&before, &new)?;
        Ok(Applied { before, sent })
    }

    /// Reads everything this app works with from the keyboard.
    pub fn read_image(&mut self) -> Result<Image, FocusError> {
        Ok(Image {
            version: self.read_required("version")?,
            hardware: self.read("hardware.version")?.unwrap_or_default(),
            chip_id: self.read_required("hardware.chip_id")?,
            commands: self.commands()?.to_vec(),
            keymap: self.read_numbers("keymap.custom")?,
            settings: {
                let mut found = std::collections::BTreeMap::new();
                for mapping in settings::MAPPINGS {
                    if let Some(text) = self.read(mapping.command)? {
                        found.insert(mapping.command.to_string(), text.trim().to_string());
                    }
                }
                found
            },
            battery: {
                let mut found = std::collections::BTreeMap::new();
                for command in [
                    "wireless.battery.left.level",
                    "wireless.battery.right.level",
                    "wireless.battery.left.status",
                    "wireless.battery.right.status",
                ] {
                    if let Some(text) = self.read(command)? {
                        found.insert(command.to_string(), text.trim().to_string());
                    }
                }
                found
            },
            extra: {
                // Everything else the keyboard lists that a backup keeps,
                // as the keyboard wrote it.
                let listed: Vec<String> = self
                    .commands()?
                    .iter()
                    .filter(|c| backup_extra(c))
                    .cloned()
                    .collect();
                let mut found = std::collections::BTreeMap::new();
                for command in listed {
                    if let Some(text) = self.read(&command)? {
                        found.insert(command, text.trim().to_string());
                    }
                }
                found
            },
            palette: self.read_numbers("palette")?,
            colormap: self.read_numbers("colormap.map")?,
            superkeys: self.read_numbers_if_any("superkeys.map")?,
            macros: self.read_numbers_if_any("macros.map")?,
            macro_memory: self
                .read("macros.memory")?
                .and_then(|t| t.trim().parse().ok()),
            chords: self.read_numbers_if_any("chords.map")?,
            chords_memory: self
                .read("chords.memory")?
                .and_then(|t| t.trim().parse().ok()),
            timing: self.read_numbers_if_any("timing.map")?,
            timing_capacity: self
                .read("timing.capacity")?
                .and_then(|t| t.trim().parse().ok()),
            timing_defaults: self.read_numbers_if_any("timing.defaults")?,
        })
    }

    /// Writes the parts of `new` that differ from `current`, which must be
    /// what the keyboard holds now. Returns the commands that were sent.
    pub fn write_changes(
        &mut self,
        current: &Image,
        new: &Image,
    ) -> Result<Vec<String>, FocusError> {
        // The same sizes, or something is wrong and nothing is sent. A
        // table only one side has means a command the keyboard lacks.
        if current.keymap.len() != new.keymap.len()
            || current.palette.len() != new.palette.len()
            || current.colormap.len() != new.colormap.len()
            || current.chords.is_empty() != new.chords.is_empty()
            || current.timing.is_empty() != new.timing.is_empty()
        {
            return Err(FocusError::Mismatch);
        }
        let mut sent = Vec::new();
        // Colors before keys, so that a failure part-way leaves the keys,
        // which matter more, as they were.
        if current.palette != new.palette {
            self.write("palette", &new.palette)?;
            sent.push("palette".into());
        }
        if current.colormap != new.colormap {
            self.write("colormap.map", &new.colormap)?;
            sent.push("colormap.map".into());
        }
        // Macros before the superkeys that may name them, and both before
        // the keys that name either.
        if current.macros != new.macros {
            self.write("macros.map", &new.macros)?;
            sent.push("macros.map".into());
        }
        if current.superkeys != new.superkeys {
            self.write("superkeys.map", &new.superkeys)?;
            sent.push("superkeys.map".into());
        }
        // The Curator firmware's tables, after the superkeys and macros a
        // chord may name, and before the keys.
        if current.timing != new.timing {
            self.write("timing.map", &new.timing)?;
            sent.push("timing.map".into());
        }
        if current.chords != new.chords {
            self.write("chords.map", &new.chords)?;
            sent.push("chords.map".into());
        }
        if current.keymap != new.keymap {
            self.write("keymap.custom", &new.keymap)?;
            sent.push("keymap.custom".into());
        }
        // Settings last, each one value, only where the keyboard has the
        // command and the value differs.
        for mapping in settings::MAPPINGS {
            let (Some(before), Some(after)) = (
                current.settings.get(mapping.command),
                new.settings.get(mapping.command),
            ) else {
                continue;
            };
            if before != after {
                self.write(mapping.command, std::slice::from_ref(after))?;
                sent.push(mapping.command.into());
            }
        }
        // What a restore brings back beyond those, as it was read.
        for (command, after) in &new.extra {
            let Some(before) = current.extra.get(command) else {
                continue;
            };
            if before != after {
                self.write(command, std::slice::from_ref(after))?;
                sent.push(command.clone());
            }
        }
        Ok(sent)
    }
}

/// What a Dygma keyboard holds, in its own form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub version: String,
    /// What the firmware calls the keyboard, such as `Dygma Defy`.
    #[serde(default)]
    pub hardware: String,
    pub chip_id: String,
    /// Every command the keyboard listed under `help`.
    #[serde(default)]
    pub commands: Vec<String>,
    /// One key code per slot, layer after layer.
    pub keymap: Vec<u16>,
    /// The settings the keyboard listed, by Focus command, as it wrote
    /// them.
    #[serde(default)]
    pub settings: std::collections::BTreeMap<String, String>,
    /// The halves' battery levels and states, by command, on a wireless
    /// keyboard.
    #[serde(default)]
    pub battery: std::collections::BTreeMap<String, String>,
    /// Everything else the keyboard listed that a backup keeps, by
    /// command, as the keyboard wrote it: what [`backup_extra`] allows.
    #[serde(default)]
    pub extra: std::collections::BTreeMap<String, String>,
    /// The palette's color channels, color after color.
    pub palette: Vec<u8>,
    /// One palette index per light, layer after layer.
    pub colormap: Vec<u8>,
    /// The superkey map as the keyboard stores it; empty on firmware
    /// without superkeys.
    #[serde(default)]
    pub superkeys: Vec<u16>,
    /// The macro map as the keyboard stores it; empty on firmware
    /// without macros.
    #[serde(default)]
    pub macros: Vec<u16>,
    /// The room for macros, in bytes, when the keyboard says.
    #[serde(default)]
    pub macro_memory: Option<u32>,
    /// The chord map as the Curator firmware stores it; empty on firmware
    /// without chords.
    #[serde(default)]
    pub chords: Vec<u16>,
    /// The room for chords, in bytes, when the keyboard says.
    #[serde(default)]
    pub chords_memory: Option<u16>,
    /// The per-key timing table as the Curator firmware stores it; empty
    /// on firmware without it.
    #[serde(default)]
    pub timing: Vec<u16>,
    /// How many timing entries the keyboard holds, when it says.
    #[serde(default)]
    pub timing_capacity: Option<u16>,
    /// The keyboard's own seven timing values, when it says.
    #[serde(default)]
    pub timing_defaults: Vec<u16>,
}

fn channels(profile: &DygmaProfile) -> usize {
    if profile.rgbw {
        4
    } else {
        3
    }
}

/// A palette entry as the color it shows.
fn shown(entry: &[u8]) -> Rgb {
    let white = entry.get(3).copied().unwrap_or(0);
    Rgb(
        entry[0].saturating_add(white),
        entry[1].saturating_add(white),
        entry[2].saturating_add(white),
    )
}

/// A color as a palette entry. With a white channel, the part all three
/// colors share is moved onto it, as Dygma's own app does.
fn entry(color: Rgb, profile: &DygmaProfile) -> Vec<u8> {
    if !profile.rgbw {
        return vec![color.0, color.1, color.2];
    }
    let white = color.0.min(color.1).min(color.2);
    vec![color.0 - white, color.1 - white, color.2 - white, white]
}

impl Image {
    /// Whether the image has the sizes the board's firmware should hold.
    pub fn fits(&self, profile: &DygmaProfile) -> bool {
        self.keymap.len() == profile.layers * profile.slots
            && self.palette.len() == profile.palette * channels(profile)
            && self.colormap.len() == profile.layers * profile.leds
            && self
                .colormap
                .iter()
                .all(|i| usize::from(*i) < profile.palette)
    }

    /// The image as a file, for a backup taken before writing.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("an image always serializes")
    }

    /// A backup file read back.
    pub fn from_json(text: &str) -> Result<Image, FocusError> {
        serde_json::from_str(text).map_err(|e| FocusError::Unfit(format!("not a backup file: {e}")))
    }

    /// The image this keyboard should hold to be as `backup` was: the
    /// backup's keys, colors, superkeys, macros and settings, except the
    /// lights and the wireless side, which a restore leaves alone.
    pub fn with_backup(&self, backup: &Image) -> Result<Image, FocusError> {
        if self.hardware != backup.hardware || self.chip_id.trim() != backup.chip_id.trim() {
            return Err(FocusError::Unfit(format!(
                "this backup is from another keyboard ({} {}); this one is {} {}",
                backup.hardware.trim(),
                backup.chip_id.trim(),
                self.hardware.trim(),
                self.chip_id.trim()
            )));
        }
        if self.keymap.len() != backup.keymap.len()
            || self.palette.len() != backup.palette.len()
            || self.colormap.len() != backup.colormap.len()
        {
            return Err(FocusError::Mismatch);
        }
        let mut image = self.clone();
        image.keymap = backup.keymap.clone();
        image.palette = backup.palette.clone();
        image.colormap = backup.colormap.clone();
        if !self.superkeys.is_empty() && !backup.superkeys.is_empty() {
            image.superkeys = backup.superkeys.clone();
        }
        if !self.macros.is_empty() && !backup.macros.is_empty() {
            image.macros = backup.macros.clone();
        }
        // The Curator firmware's tables come back only between keyboards
        // that both have them.
        if !self.chords.is_empty() && !backup.chords.is_empty() {
            image.chords = backup.chords.clone();
        }
        if !self.timing.is_empty() && !backup.timing.is_empty() {
            image.timing = backup.timing.clone();
        }
        for (command, value) in &backup.settings {
            if restorable(command) && self.settings.contains_key(command) {
                image.settings.insert(command.clone(), value.clone());
            }
        }
        for (command, value) in &backup.extra {
            if restorable(command) && self.extra.contains_key(command) {
                image.extra.insert(command.clone(), value.clone());
            }
        }
        Ok(image)
    }

    /// The keyboard's settings as the board takes them.
    pub fn carried(&self) -> kc_model::Carried {
        let mut carried = kc_model::Carried::default();
        for mapping in settings::MAPPINGS {
            if let Some(text) = self.settings.get(mapping.command) {
                if let Some(value) = settings::value_of(mapping.key, text) {
                    carried.settings.insert(mapping.key.to_string(), value);
                }
            }
        }
        carried
    }

    /// The halves' batteries in words, on a wireless keyboard.
    pub fn battery_summary(&self) -> Option<String> {
        let state = |text: &str| match text.trim() {
            "0" => "discharging",
            "1" => "charging",
            "2" => "full",
            "3" => "faulty",
            "4" => "disconnected",
            _ => "unknown",
        };
        let left = self.battery.get("wireless.battery.left.level")?;
        let right = self.battery.get("wireless.battery.right.level")?;
        let left_state = self
            .battery
            .get("wireless.battery.left.status")
            .map_or("", |s| state(s));
        let right_state = self
            .battery
            .get("wireless.battery.right.status")
            .map_or("", |s| state(s));
        Some(format!(
            "Battery: left {left}% {left_state}, right {right}% {right_state}"
        ))
    }

    /// The image with the board's settings written in, where the keyboard
    /// has the command; the rest of the image is unchanged.
    pub fn with_settings(&self, config: &kc_model::FirmwareConfig) -> Image {
        let mut image = self.clone();
        for mapping in settings::MAPPINGS {
            let Some(value) = config.settings.get(mapping.key) else {
                continue;
            };
            let Some(current) = self.settings.get(mapping.command) else {
                continue;
            };
            if let Some(text) = settings::text_of(mapping.key, value, current) {
                image.settings.insert(mapping.command.to_string(), text);
            }
        }
        image
    }

    /// Whether the keyboard runs `fork` rather than Dygma's own firmware,
    /// by the version it reports. The commands it lists are the surer
    /// sign, and gate every read and write.
    pub fn runs_fork(&self, fork: &DygmaFork) -> bool {
        self.version.contains(&fork.version_mark)
    }

    /// The firmware the keyboard runs, for messages: Dygma's own, or
    /// `fork` when the keyboard runs it.
    pub fn firmware_name(&self, fork: Option<&DygmaFork>) -> String {
        let version = self.version.trim();
        match fork {
            Some(fork) if self.runs_fork(fork) => format!("the {} {version}", fork.name),
            _ => format!("Dygma's firmware {version}"),
        }
    }

    /// A layout holding what the image holds: every layer, with its keys
    /// and the color under each key.
    pub fn to_project(
        &self,
        name: &str,
        board: &Board,
        profile: &DygmaProfile,
    ) -> Result<Project, FocusError> {
        self.to_project_with_notes(name, board, profile)
            .map(|(project, _)| project)
    }

    /// [`Image::to_project`], with what could not be brought along: the
    /// Curator firmware's chords and timing that the layout has no key
    /// for, or that the app cannot show.
    pub fn to_project_with_notes(
        &self,
        name: &str,
        board: &Board,
        profile: &DygmaProfile,
    ) -> Result<(Project, Vec<String>), FocusError> {
        if !self.fits(profile) {
            return Err(FocusError::Mismatch);
        }
        let mut project = Project::new(name, board);
        project.layers[0].name = "Layer 1".into();
        for number in 2..=profile.layers {
            project
                .add_layer(format!("Layer {number}"))
                .map_err(|e| FocusError::Unfit(e.to_string()))?;
        }
        let layers: Vec<LayerId> = project.layers.iter().map(|l| l.id).collect();
        let size = channels(profile);
        let table = superkeys::parse(&self.superkeys);
        let macro_defs = self.macro_definitions(&mut project, &layers, &table);
        let macro_binding = |code: u16| -> Option<Binding> {
            let id = macro_defs.get(&macros::index(code)?)?;
            Some(Binding::user(*id, vec![]))
        };
        for (index, id) in layers.iter().enumerate() {
            let mut lights = Vec::with_capacity(profile.key_slots.len());
            for (position, (slot, led)) in
                profile.key_slots.iter().zip(&profile.key_leds).enumerate()
            {
                let code = self.keymap[index * profile.slots + slot];
                project
                    .set_binding(*id, position, codec::decode(code, &layers))
                    .map_err(|e| FocusError::Unfit(e.to_string()))?;
                // A macro becomes the key's own; a superkey becomes the
                // key's slots. One this app cannot place stays as the number.
                if let Some(binding) = macro_binding(code) {
                    let _ = project.set_binding(*id, position, binding);
                }
                let entry = usize::from(code.wrapping_sub(superkeys::FIRST));
                if code >= superkeys::FIRST && entry < superkeys::SLOTS {
                    if let Some(setup) = table
                        .get(entry)
                        .and_then(|a| superkeys::to_setup(a, &layers, &macro_binding))
                    {
                        let _ = project.apply_setup(*id, position, &setup);
                    }
                }
                let color = usize::from(self.colormap[index * profile.leds + led]);
                let color = shown(&self.palette[color * size..(color + 1) * size]);
                lights.push(if color == Rgb(0, 0, 0) {
                    KeyLight::Off
                } else {
                    KeyLight::Color(color)
                });
            }
            // The underglow lights the layout draws, after the keys'.
            let mut underglow = Vec::new();
            if let Some(first) = profile.underglow_first {
                let spots = board
                    .layout(&board.default_layout)
                    .map_or(0, |l| l.underglow.len());
                for led in first..(first + spots).min(profile.leds) {
                    let color = usize::from(self.colormap[index * profile.leds + led]);
                    let color = shown(&self.palette[color * size..(color + 1) * size]);
                    underglow.push(if color == Rgb(0, 0, 0) {
                        KeyLight::Off
                    } else {
                        KeyLight::Color(color)
                    });
                }
            }
            let lighting = project
                .lighting_mut(*id)
                .map_err(|e| FocusError::Unfit(e.to_string()))?;
            lighting.keys = lights;
            lighting.underglow = underglow;
        }
        // The Curator firmware's chords become combos, and its timing the
        // keys' feels, once every key is in place.
        let mut notes = chords::apply(
            &mut project,
            &chords::parse(&self.chords),
            profile,
            &macro_binding,
        );
        notes.extend(timing::apply(
            &mut project,
            &timing::parse(&self.timing),
            profile,
            &self.timing_defaults,
        ));
        // A macro several keys share is a template; one key's is its own.
        // They were made as templates so that building the keys, which
        // tidies as it goes, could not drop one before its key was set.
        for (index, id) in &macro_defs {
            if project.top_owners(*id).len() > 1 {
                let _ = project.make_template(*id, format!("Macro {}", index + 1));
            } else if let Ok(def) = project.behavior_mut(*id) {
                def.scope = Scope::Key;
            }
        }
        project.tidy();
        Ok((project, notes))
    }

    /// A macro definition for every macro the keymap names, on a key or
    /// inside a superkey, by its entry in the map. Macros this app cannot
    /// read get none and stay as numbers.
    fn macro_definitions(
        &self,
        project: &mut Project,
        layers: &[LayerId],
        superkeys: &[superkeys::Actions],
    ) -> std::collections::HashMap<usize, BehaviorId> {
        let entries = macros::parse(&self.macros);
        let mut wanted: Vec<usize> = Vec::new();
        let mut note = |code: u16| {
            if let Some(index) = macros::index(code) {
                if !wanted.contains(&index) {
                    wanted.push(index);
                }
            }
        };
        for code in &self.keymap {
            note(*code);
            if let Some(entry) = code
                .checked_sub(superkeys::FIRST)
                .and_then(|i| superkeys.get(usize::from(i)))
            {
                for action in entry {
                    note(*action);
                }
            }
        }
        wanted.sort_unstable();
        let mut defs = std::collections::HashMap::new();
        for index in wanted {
            let Some(macros::Entry::Known(actions)) = entries.get(index) else {
                continue;
            };
            let Some(steps) = macros::to_steps(actions, layers) else {
                continue;
            };
            let label = project.free_label("macro");
            let Ok(id) = project.add_behavior(
                label,
                "",
                BehaviorKind::Macro(Macro {
                    wait_ms: None,
                    tap_ms: None,
                    params: 0,
                    steps,
                }),
            ) else {
                continue;
            };
            defs.insert(index, id);
        }
        defs
    }

    /// The image the keyboard should hold for `project`, starting from
    /// what it holds now. Slots and lights the layout says nothing about
    /// (the unused corners of the grid, the underglow where none has been
    /// painted) are left as they are, and so are the palette colors they
    /// use.
    pub fn with_project(
        &self,
        project: &Project,
        profile: &DygmaProfile,
        features: &[Capability],
    ) -> Result<Image, FocusError> {
        if !self.fits(profile) {
            return Err(FocusError::Mismatch);
        }
        let problems = check(project, profile, features);
        if let Some(problem) = problems.iter().find(|p| p.severity == Severity::Error) {
            return Err(FocusError::Unfit(format!(
                "the layout has {} problem(s) to fix first, such as: {}",
                problems.len(),
                problem.message
            )));
        }
        let per_key_timing = features.contains(&Capability::PerKeyTiming);
        let mut image = self.clone();
        let layers: Vec<LayerId> = project.layers.iter().map(|l| l.id).collect();
        let size = channels(profile);

        // Macros and superkeys the keyboard has stay; a macro or a key
        // setup reuses an identical entry or takes a new one.
        let memory = self
            .macro_memory
            .map_or(macros::DEFAULT_MEMORY, |m| m as usize);
        let mut macro_table = macros::parse(&self.macros);
        let codes = place_macros(project, &layers, &mut macro_table, memory)
            .map_err(|(_, reason)| FocusError::Unfit(reason))?;
        let mut table = superkeys::parse(&self.superkeys);
        for index in 0..profile.layers {
            let layer = project.layers.get(index);
            for (position, slot) in profile.key_slots.iter().enumerate() {
                // A layer the layout does not have lets the ones below
                // show through.
                let code = match layer.and_then(|l| l.bindings.get(position)) {
                    Some(binding) if is_macro(project, binding) => {
                        encode_any(binding, &layers, &codes).map_err(FocusError::Unfit)?
                    }
                    Some(binding) if binding.user_behavior().is_some() => setup_code(
                        project,
                        binding,
                        &layers,
                        &codes,
                        &mut table,
                        per_key_timing,
                    )
                    .map_err(FocusError::Unfit)?,
                    Some(binding) => codec::encode(binding, &layers).map_err(FocusError::Unfit)?,
                    None => codec::TRANSPARENT,
                };
                image.keymap[index * profile.slots + slot] = code;
            }
        }
        // The Curator firmware's chords, from the layout's combos, where
        // the firmware the board runs has combos. The layout's combos are
        // the whole map, as its keys are the whole keymap.
        let fork = profile.fork.as_ref();
        let needs = |what: &str| {
            FocusError::Unfit(format!(
                "this keyboard runs {}, which has no {what}; the layout needs the {}",
                self.firmware_name(fork),
                fork.map_or("board's firmware", |f| f.name.as_str())
            ))
        };
        if features.contains(&Capability::Combos) {
            let memory = self.chords_memory.map_or(chords::MAP_BYTES, usize::from);
            let placed =
                chords::from_project(project, &layers, profile, &codes, &mut table, memory)
                    .map_err(|(_, reason)| FocusError::Unfit(reason))?;
            if self.chords.is_empty() && !placed.is_empty() {
                return Err(needs("combos"));
            }
            if !self.chords.is_empty() {
                image.chords = chords::serialize(&placed);
            }
        }
        // Its timing table, from the keys' feels.
        if per_key_timing {
            let entries = timing::from_project(project, profile)
                .map_err(|(_, reason)| FocusError::Unfit(reason))?;
            if self.timing.is_empty() && !entries.is_empty() {
                return Err(needs("timing of a key's own"));
            }
            let capacity = self.timing_capacity.map_or(timing::CAPACITY, usize::from);
            if entries.len() > capacity {
                return Err(FocusError::Unfit(format!(
                    "the keyboard holds a feel of its own for {capacity} keys, and the layout gives one to {}",
                    entries.len()
                )));
            }
            if !self.timing.is_empty() {
                image.timing = timing::serialize(&entries);
            }
        }
        if self.superkeys.is_empty() && !table.is_empty() {
            return Err(FocusError::Unfit(
                "this keyboard's firmware has no superkeys, which the layout needs".into(),
            ));
        }
        if !self.superkeys.is_empty() {
            image.superkeys = superkeys::serialize(&table);
        }
        if self.macros.is_empty() && !macro_table.is_empty() {
            return Err(FocusError::Unfit(
                "this keyboard's firmware has no macros, which the layout needs".into(),
            ));
        }
        if !self.macros.is_empty() {
            image.macros = macros::serialize(&macro_table);
        }

        // The colors the layout's keys want. A key that already shows its
        // color keeps the palette entry it has, so that a layout read from
        // the keyboard and applied unchanged writes nothing.
        let key_leds = &profile.key_leds;
        let showing =
            |image: &Image, slot: usize| shown(&image.palette[slot * size..(slot + 1) * size]);
        let mut changed: Vec<(usize, Rgb)> = Vec::new();
        let mut ours = vec![false; self.colormap.len()];
        for (index, layer) in project.layers.iter().enumerate().take(profile.layers) {
            let Some(lighting) = project.lighting.iter().find(|l| l.layer == layer.id) else {
                continue;
            };
            // The lights under the keys, then the underglow where the
            // layout has painted it.
            let keys = lighting.keys.iter().zip(key_leds.iter().copied());
            let underglow = profile
                .underglow_first
                .into_iter()
                .flat_map(|first| lighting.underglow.iter().zip(first..profile.leds));
            for (light, led) in keys.chain(underglow) {
                let color = match light {
                    KeyLight::Color(color) => *color,
                    // Dygma has no see-through color: an unlit light is off.
                    _ => Rgb(0, 0, 0),
                };
                let at = index * profile.leds + led;
                if showing(self, usize::from(self.colormap[at])) != color {
                    ours[at] = true;
                    changed.push((at, color));
                }
            }
        }
        // Palette entries still in use by every light that is not changing:
        // the underglow left alone, and lights keeping their color.
        let mut held = vec![false; profile.palette];
        for (at, slot) in self.colormap.iter().enumerate() {
            if !ours[at] {
                held[usize::from(*slot)] = true;
            }
        }
        // Each new color gets an entry that already shows it, or else one
        // nothing needs.
        let mut assigned: Vec<(Rgb, usize)> = Vec::new();
        for (at, color) in changed {
            let known = assigned.iter().find(|(c, _)| *c == color).map(|(_, s)| *s);
            let slot = match known {
                Some(slot) => slot,
                None => {
                    let existing = (0..profile.palette).find(|i| showing(&image, *i) == color);
                    let slot = match existing {
                        Some(slot) => slot,
                        None => {
                            let free = (0..profile.palette).find(|i| !held[*i]).ok_or_else(|| {
                                FocusError::Unfit(format!(
                                    "the keyboard's palette holds {} colors, and the layout's colors together with the underglow need more",
                                    profile.palette
                                ))
                            })?;
                            image.palette[free * size..(free + 1) * size]
                                .copy_from_slice(&entry(color, profile));
                            free
                        }
                    };
                    held[slot] = true;
                    assigned.push((color, slot));
                    slot
                }
            };
            image.colormap[at] = u8::try_from(slot).expect("a palette is small");
        }
        Ok(image)
    }
}

/// What stops a layout from being written to Dygma firmware, and what the
/// firmware will not honor of it. `features` is what the board's firmware
/// profile can do: combos and a key's own timing exist on the Curator
/// firmware and are refused on Dygma's own.
pub fn check(project: &Project, profile: &DygmaProfile, features: &[Capability]) -> Vec<Problem> {
    let per_key_timing = features.contains(&Capability::PerKeyTiming);
    let mut problems = Vec::new();
    let mut error = |location: Location, message: String| {
        problems.push(Problem {
            severity: Severity::Error,
            location,
            message,
        });
    };
    if project.layers.len() > profile.layers {
        error(
            Location::Project,
            format!(
                "the layout has {} layers, and this keyboard holds {}",
                project.layers.len(),
                profile.layers
            ),
        );
    }
    let layers: Vec<LayerId> = project.layers.iter().map(|l| l.id).collect();
    let mut macro_table: Vec<macros::Entry> = Vec::new();
    let codes = match place_macros(project, &layers, &mut macro_table, macros::DEFAULT_MEMORY) {
        Ok(codes) => codes,
        Err((location, reason)) => {
            error(location, reason);
            macros::Codes::new()
        }
    };
    let mut table: Vec<superkeys::Actions> = Vec::new();
    for layer in &project.layers {
        for (position, binding) in layer.bindings.iter().enumerate() {
            let result = if is_macro(project, binding) {
                encode_any(binding, &layers, &codes).map(|_| ())
            } else if binding.user_behavior().is_some() {
                setup_code(
                    project,
                    binding,
                    &layers,
                    &codes,
                    &mut table,
                    per_key_timing,
                )
                .map(|_| ())
            } else {
                codec::encode(binding, &layers).map(|_| ())
            };
            if let Err(reason) = result {
                error(
                    Location::Key {
                        layer: layer.id,
                        position,
                    },
                    reason,
                );
            }
        }
    }
    // The Curator firmware's chords, where the firmware has combos; on
    // firmware without them the layout keeps its combos out of the way.
    if features.contains(&Capability::Combos) {
        if let Err((location, reason)) = chords::from_project(
            project,
            &layers,
            profile,
            &codes,
            &mut table,
            chords::MAP_BYTES,
        ) {
            error(location, reason);
        }
    }
    if table.len() > superkeys::SLOTS || superkeys::bytes(&table) > superkeys::MAP_BYTES {
        error(
            Location::Project,
            format!(
                "the layout needs {} superkeys, and the keyboard holds at most {} in {} bytes",
                table.len(),
                superkeys::SLOTS,
                superkeys::MAP_BYTES
            ),
        );
    }
    // Its timing table, from the keys' feels, and what it cannot honor.
    if per_key_timing {
        if let Err((location, reason)) = timing::from_project(project, profile) {
            error(location, reason);
        }
    }
    let mut colors: Vec<Rgb> = Vec::new();
    for lighting in &project.lighting {
        for light in lighting.keys.iter().chain(&lighting.underglow) {
            match light {
                KeyLight::Color(color) if !colors.contains(color) => colors.push(*color),
                KeyLight::Lock { .. } | KeyLight::Battery { .. } => {
                    error(
                        Location::Lighting(lighting.layer),
                        "Dygma firmware has no lock or battery lights; use a plain color".into(),
                    );
                    break;
                }
                _ => {}
            }
        }
    }
    if colors.len() > profile.palette {
        error(
            Location::Project,
            format!(
                "the layout uses {} different colors, and this keyboard's palette holds {}",
                colors.len(),
                profile.palette
            ),
        );
    }
    // Dygma's firmware has one flag per modifier, with no side: a right-hand
    // modifier is written as the left one. Said once, not at every key.
    let right_handed = project.layers.iter().any(|layer| {
        layer.bindings.iter().any(|binding| match binding {
            kc_model::Binding::Behavior { params, .. } => params.iter().any(|p| match p {
                kc_model::Param::Key(expr) => expr.mods.iter().any(|m| {
                    matches!(
                        m,
                        kc_zmk::Modifier::RCtrl | kc_zmk::Modifier::RShift | kc_zmk::Modifier::RGui
                    )
                }),
                _ => false,
            }),
            _ => false,
        })
    });
    if right_handed {
        problems.push(Problem {
            severity: Severity::Warning,
            location: Location::Project,
            message: "Dygma firmware has no right-hand modifiers: right Control, Shift and Command are written as the left ones".into(),
        });
    }
    if features.contains(&Capability::Combos) {
        problems.extend(chords::warnings(project));
    }
    if per_key_timing {
        problems.extend(timing::warnings(project));
    }
    problems
}

/// Whether a binding can be written to Dygma firmware, for offering only
/// what works. `features` is what the board's firmware profile can do.
pub fn expressible(
    binding: &kc_model::Binding,
    project: &Project,
    features: &[Capability],
) -> bool {
    let layers: Vec<LayerId> = project.layers.iter().map(|l| l.id).collect();
    if is_macro(project, binding) {
        return macro_steps(project, binding)
            .is_some_and(|steps| macros::from_steps(steps, &layers).is_ok());
    }
    if binding.user_behavior().is_some() {
        // A key setup: its feel is the keyboard's, or the firmware times
        // keys one by one.
        return KeySetup::read(project, binding).is_some_and(|setup| {
            features.contains(&Capability::PerKeyTiming) || superkeys::keyboard_feel(&setup)
        });
    }
    codec::encode(binding, &layers).is_ok()
}

/// The code for a key that is a key setup: the firmware's own hold and
/// tap where the setup is only that, otherwise a superkey, placed in
/// `table`. A feel of the key's own is allowed where the firmware times
/// keys one by one (`per_key_timing`), as that key's timing entry.
fn setup_code(
    project: &Project,
    binding: &Binding,
    layers: &[LayerId],
    codes: &macros::Codes,
    table: &mut Vec<superkeys::Actions>,
    per_key_timing: bool,
) -> Result<u16, String> {
    let setup = KeySetup::read(project, binding)
        .ok_or_else(|| "this key is written in a way Dygma firmware cannot hold".to_string())?;
    if !per_key_timing && !superkeys::keyboard_feel(&setup) {
        return Err("Dygma firmware keeps one timing for every superkey: choose Keyboard default for this key's feel, and set the timing under Settings".into());
    }
    // A hold and tap, and nothing more, is what the firmware's own
    // dual-use keys are, which it turns such superkeys into anyway.
    if let Some(dual) = dual_use(&setup) {
        if let Ok(code) = codec::encode(&dual, layers) {
            return Ok(code);
        }
    }
    let actions = superkeys::actions(project, binding, layers, codes, per_key_timing)?;
    let entry = superkeys::place(table, actions)?;
    Ok(superkeys::FIRST + u16::try_from(entry).expect("few superkeys"))
}

/// The built-in hold and tap a setup is, when it is only a hold and a
/// tap: a modifier or a layer held, a key tapped.
fn dual_use(setup: &KeySetup) -> Option<Binding> {
    let hold = setup.hold.as_ref()?;
    if setup.has_taps() || setup.shifted.is_some() || setup.sticky || !setup.more_taps.is_empty() {
        return None;
    }
    let side = |binding: &Binding| -> Option<Param> {
        match binding {
            Binding::Behavior {
                behavior: BehaviorRef::BuiltIn(label),
                params,
            } => match (label.as_str(), params.as_slice()) {
                ("kp", [key @ Param::Key(_)]) => Some(key.clone()),
                ("mo", [layer @ Param::Layer(_)]) => Some(layer.clone()),
                _ => None,
            },
            _ => None,
        }
    };
    let tap = side(&setup.tap).filter(|p| matches!(p, Param::Key(_)))?;
    Some(match side(hold)? {
        key @ Param::Key(_) => Binding::new("mt", vec![key, tap]),
        layer @ Param::Layer(_) => Binding::new("lt", vec![layer, tap]),
        _ => return None,
    })
}

/// Whether a binding runs one of the layout's macros.
pub(crate) fn is_macro(project: &Project, binding: &Binding) -> bool {
    macro_steps(project, binding).is_some()
}

fn macro_steps<'a>(project: &'a Project, binding: &Binding) -> Option<&'a [MacroStep]> {
    let def = project.behavior(binding.user_behavior()?)?;
    match &def.kind {
        BehaviorKind::Macro(m) => Some(&m.steps),
        _ => None,
    }
}

/// The code for a binding: a macro's entry, or the key code.
pub(crate) fn encode_any(
    binding: &Binding,
    layers: &[LayerId],
    codes: &macros::Codes,
) -> Result<u16, String> {
    match binding.user_behavior() {
        Some(id) => codes
            .get(&id)
            .copied()
            .ok_or_else(|| "this key needs a macro the keyboard cannot hold".to_string()),
        None => codec::encode(binding, layers),
    }
}

/// Places every macro a key reaches, on the key or in one of its slots,
/// in the table, and returns each macro's code. An error names the key
/// whose macro could not be placed.
fn place_macros(
    project: &Project,
    layers: &[LayerId],
    table: &mut Vec<macros::Entry>,
    memory: usize,
) -> Result<macros::Codes, (Location, String)> {
    let mut codes = macros::Codes::new();
    for layer in &project.layers {
        for (position, binding) in layer.bindings.iter().enumerate() {
            let location = Location::Key {
                layer: layer.id,
                position,
            };
            let mut reached: Vec<BehaviorId> = Vec::new();
            if is_macro(project, binding) {
                reached.extend(binding.user_behavior());
            } else if binding.user_behavior().is_some() {
                if let Some(setup) = kc_model::setup::KeySetup::read(project, binding) {
                    let slots = [
                        Some(&setup.tap),
                        setup.hold.as_ref(),
                        setup.tap_hold.as_ref(),
                        setup.double_tap.as_ref(),
                        setup.double_tap_hold.as_ref(),
                    ];
                    for slot in slots.into_iter().flatten() {
                        if is_macro(project, slot) {
                            reached.extend(slot.user_behavior());
                        }
                    }
                }
            }
            for id in reached {
                if codes.contains_key(&id) {
                    continue;
                }
                let def = project.behavior(id).expect("a macro the key reaches");
                let BehaviorKind::Macro(m) = &def.kind else {
                    continue;
                };
                if m.params > 0 {
                    return Err((location, "Dygma macros take no values from the key".into()));
                }
                let actions =
                    macros::from_steps(&m.steps, layers).map_err(|e| (location.clone(), e))?;
                let index =
                    macros::place(table, actions, memory).map_err(|e| (location.clone(), e))?;
                codes.insert(id, macros::code(index));
            }
        }
    }
    Ok(codes)
}

/// The serial port of a connected keyboard of `board`: the one linked as
/// `device` if given, otherwise the first one found.
pub fn find_port(board: &Board, device: Option<&Device>) -> Option<String> {
    let mut ports: Vec<_> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter(|port| !port.port_name.contains("/tty."))
        .filter_map(|port| match port.port_type {
            serialport::SerialPortType::UsbPort(usb) => Some((port.port_name, usb)),
            _ => None,
        })
        .filter(|(_, usb)| {
            board
                .usb
                .iter()
                .any(|id| id.vendor == usb.vid && id.product == usb.pid)
        })
        .filter(|(_, usb)| {
            device.is_none_or(|device| {
                usb.serial_number.as_deref().map(str::trim) == Some(device.serial.as_str())
            })
        })
        .map(|(name, _)| name)
        .collect();
    ports.sort();
    ports.into_iter().next()
}

/// Opens a Focus conversation with the keyboard on a serial port.
pub fn open(port: &str) -> Result<Focus<Box<dyn serialport::SerialPort>>, FocusError> {
    let stream = serialport::new(port, 115_200)
        .timeout(Duration::from_secs(5))
        .open()
        .map_err(|e| FocusError::Io(std::io::Error::other(e.to_string())))?;
    Ok(Focus::new(stream))
}

/// Reads what the connected keyboard of `board` holds.
pub fn read(board: &Board, device: Option<&Device>) -> Result<Image, FocusError> {
    let port = find_port(board, device).ok_or(FocusError::NotFound)?;
    open(&port)?.read_image()
}

/// What an apply or a restore did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// What the keyboard held before, for the backup.
    pub before: Image,
    /// The commands that were sent; none when the keyboard already matched.
    pub sent: Vec<String>,
}

/// Works out what the keyboard should hold for `project` and writes what
/// differs. `backup` is called with what the keyboard holds before
/// anything is sent, and the write only goes ahead if it succeeds.
pub fn apply(
    project: &Project,
    board: &Board,
    profile: &DygmaProfile,
    features: &[Capability],
    config: &kc_model::FirmwareConfig,
    device: Option<&Device>,
    backup: impl FnOnce(&Image) -> std::io::Result<()>,
) -> Result<Applied, FocusError> {
    let port = find_port(board, device).ok_or(FocusError::NotFound)?;
    let mut focus = open(&port)?;
    let before = focus.read_image()?;
    let new = before
        .with_project(project, profile, features)?
        .with_settings(config);
    if new == before {
        return Ok(Applied {
            before,
            sent: Vec::new(),
        });
    }
    backup(&before)?;
    let sent = focus.write_changes(&before, &new)?;
    Ok(Applied { before, sent })
}

/// Writes a backup back to the keyboard it was taken from: everything in
/// it that differs from what the keyboard holds now, except the lights
/// and the wireless side. `backup` is called with what the keyboard holds
/// before anything is sent, and the write only goes ahead if it succeeds.
pub fn restore(
    board: &Board,
    device: Option<&Device>,
    saved: &Image,
    backup: impl FnOnce(&Image) -> std::io::Result<()>,
) -> Result<Applied, FocusError> {
    let port = find_port(board, device).ok_or(FocusError::NotFound)?;
    let mut focus = open(&port)?;
    focus.restore_image(saved, backup)
}
