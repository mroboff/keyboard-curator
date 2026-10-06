//! Dygma keyboards, whose firmware is configured live.
//!
//! There is nothing to build: a layout is turned into the keyboard's own
//! stored form (a grid of key codes per layer, and a color per light chosen
//! from a small palette) and written over USB with Dygma's text-based Focus
//! protocol. Reading works the same way in reverse.
//!
//! Every write goes straight to the keyboard's flash, so this crate is
//! strict about what it sends: only the three commands in [`WRITES`], only
//! with payloads of exactly the right size, and only what has changed.
//! Superkeys, macros and settings are never written.

pub mod codec;

use std::io::{Read, Write};
use std::time::Duration;

use kc_boards::board::DygmaProfile;
use kc_boards::Board;
use kc_model::features::{KeyLight, Rgb};
use kc_model::{Device, LayerId, Location, Problem, Project, Severity};
use serde::{Deserialize, Serialize};

/// The commands this crate reads with. None takes an argument, so none can
/// change anything.
const READS: [&str; 5] = [
    "version",
    "hardware.chip_id",
    "keymap.custom",
    "palette",
    "colormap.map",
];

/// The only commands this crate writes with.
pub const WRITES: [&str; 3] = ["keymap.custom", "palette", "colormap.map"];

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
}

impl<T: Read + Write> Focus<T> {
    pub fn new(stream: T) -> Self {
        Self { stream }
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

    /// Asks for a value. Only the commands in the read list are sent.
    fn read(&mut self, command: &str) -> Result<String, FocusError> {
        if !READS.contains(&command) {
            return Err(FocusError::Refused(command.to_string()));
        }
        self.exchange(command)
    }

    fn read_numbers<N: std::str::FromStr>(&mut self, command: &str) -> Result<Vec<N>, FocusError> {
        self.read(command)?
            .split_whitespace()
            .map(|token| {
                token.parse().map_err(|_| FocusError::Malformed {
                    command: command.to_string(),
                    problem: format!("`{token}` is not a number in range"),
                })
            })
            .collect()
    }

    /// Writes a value. Only the commands in [`WRITES`] are sent.
    fn write<N: ToString>(&mut self, command: &str, values: &[N]) -> Result<(), FocusError> {
        if !WRITES.contains(&command) {
            return Err(FocusError::Refused(command.to_string()));
        }
        let payload: Vec<String> = values.iter().map(ToString::to_string).collect();
        self.exchange(&format!("{command} {}", payload.join(" ")))?;
        Ok(())
    }

    /// Reads everything this app works with from the keyboard.
    pub fn read_image(&mut self) -> Result<Image, FocusError> {
        Ok(Image {
            version: self.read("version")?,
            chip_id: self.read("hardware.chip_id")?,
            keymap: self.read_numbers("keymap.custom")?,
            palette: self.read_numbers("palette")?,
            colormap: self.read_numbers("colormap.map")?,
        })
    }

    /// Writes the parts of `new` that differ from `current`, which must be
    /// what the keyboard holds now. Returns the commands that were sent.
    pub fn write_changes(
        &mut self,
        current: &Image,
        new: &Image,
    ) -> Result<Vec<&'static str>, FocusError> {
        // The same sizes, or something is wrong and nothing is sent.
        if current.keymap.len() != new.keymap.len()
            || current.palette.len() != new.palette.len()
            || current.colormap.len() != new.colormap.len()
        {
            return Err(FocusError::Mismatch);
        }
        let mut sent = Vec::new();
        // Colors before keys, so that a failure part-way leaves the keys,
        // which matter more, as they were.
        if current.palette != new.palette {
            self.write("palette", &new.palette)?;
            sent.push("palette");
        }
        if current.colormap != new.colormap {
            self.write("colormap.map", &new.colormap)?;
            sent.push("colormap.map");
        }
        if current.keymap != new.keymap {
            self.write("keymap.custom", &new.keymap)?;
            sent.push("keymap.custom");
        }
        Ok(sent)
    }
}

/// What a Dygma keyboard holds, in its own form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub version: String,
    pub chip_id: String,
    /// One key code per slot, layer after layer.
    pub keymap: Vec<u16>,
    /// The palette's color channels, color after color.
    pub palette: Vec<u8>,
    /// One palette index per light, layer after layer.
    pub colormap: Vec<u8>,
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

    /// A layout holding what the image holds: every layer, with its keys
    /// and the color under each key.
    pub fn to_project(
        &self,
        name: &str,
        board: &Board,
        profile: &DygmaProfile,
    ) -> Result<Project, FocusError> {
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
        for (index, id) in layers.iter().enumerate() {
            let mut lights = Vec::with_capacity(profile.key_slots.len());
            for (position, (slot, led)) in
                profile.key_slots.iter().zip(&profile.key_leds).enumerate()
            {
                let code = self.keymap[index * profile.slots + slot];
                project
                    .set_binding(*id, position, codec::decode(code, &layers))
                    .map_err(|e| FocusError::Unfit(e.to_string()))?;
                let color = usize::from(self.colormap[index * profile.leds + led]);
                let color = shown(&self.palette[color * size..(color + 1) * size]);
                lights.push(if color == Rgb(0, 0, 0) {
                    KeyLight::Off
                } else {
                    KeyLight::Color(color)
                });
            }
            project
                .lighting_mut(*id)
                .map_err(|e| FocusError::Unfit(e.to_string()))?
                .keys = lights;
        }
        Ok(project)
    }

    /// The image the keyboard should hold for `project`, starting from
    /// what it holds now. Slots and lights that are not keys (the unused
    /// corners of the grid, the underglow) are left as they are, and so
    /// are the palette colors they use.
    pub fn with_project(
        &self,
        project: &Project,
        profile: &DygmaProfile,
    ) -> Result<Image, FocusError> {
        if !self.fits(profile) {
            return Err(FocusError::Mismatch);
        }
        let problems = check(project, profile);
        if let Some(problem) = problems.iter().find(|p| p.severity == Severity::Error) {
            return Err(FocusError::Unfit(format!(
                "the layout has {} problem(s) to fix first, such as: {}",
                problems.len(),
                problem.message
            )));
        }
        let mut image = self.clone();
        let layers: Vec<LayerId> = project.layers.iter().map(|l| l.id).collect();
        let size = channels(profile);

        for index in 0..profile.layers {
            let layer = project.layers.get(index);
            for (position, slot) in profile.key_slots.iter().enumerate() {
                // A layer the layout does not have lets the ones below
                // show through.
                let code = match layer.and_then(|l| l.bindings.get(position)) {
                    Some(binding) => codec::encode(binding, &layers).map_err(FocusError::Unfit)?,
                    None => codec::TRANSPARENT,
                };
                image.keymap[index * profile.slots + slot] = code;
            }
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
            for (light, led) in lighting.keys.iter().zip(key_leds) {
                let color = match light {
                    KeyLight::Color(color) => *color,
                    // Dygma has no see-through color: an unlit key is off.
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
        // the underglow, and keys keeping their color.
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

/// What stops a layout from being written to Dygma firmware.
pub fn check(project: &Project, profile: &DygmaProfile) -> Vec<Problem> {
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
    for layer in &project.layers {
        for (position, binding) in layer.bindings.iter().enumerate() {
            if let Err(reason) = codec::encode(binding, &layers) {
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
    let mut colors: Vec<Rgb> = Vec::new();
    for lighting in &project.lighting {
        for light in &lighting.keys {
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
    problems
}

/// Whether a binding can be written to Dygma firmware, for offering only
/// what works.
pub fn expressible(binding: &kc_model::Binding, project: &Project) -> bool {
    let layers: Vec<LayerId> = project.layers.iter().map(|l| l.id).collect();
    codec::encode(binding, &layers).is_ok()
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

/// What an apply did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// What the keyboard held before, for the backup.
    pub before: Image,
    /// The commands that were sent; none when the keyboard already matched.
    pub sent: Vec<&'static str>,
}

/// Works out what the keyboard should hold for `project` and writes what
/// differs. `backup` is called with what the keyboard holds before
/// anything is sent, and the write only goes ahead if it succeeds.
pub fn apply(
    project: &Project,
    board: &Board,
    profile: &DygmaProfile,
    device: Option<&Device>,
    backup: impl FnOnce(&Image) -> std::io::Result<()>,
) -> Result<Applied, FocusError> {
    let port = find_port(board, device).ok_or(FocusError::NotFound)?;
    let mut focus = open(&port)?;
    let before = focus.read_image()?;
    let new = before.with_project(project, profile)?;
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
