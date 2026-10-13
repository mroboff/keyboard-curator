//! QMK keyboards with VIA: the protocol a VIA-enabled keyboard speaks over
//! raw HID, what such a keyboard holds, and (in the other modules) the
//! definition that describes one and the keycodes it uses.
//!
//! The command numbers, report layout and value channels are facts from
//! QMK's `quantum/via.h` and `via.c` (GPL-2.0), transcribed as numbers
//! only. The protocol is version 0x0D at the time of writing; the parts
//! used here have been stable since version 0x09.
//!
//! Writing is strict: the client sends only the commands in [`WRITES`] and
//! [`READS`]. Resetting the keymap, the macros or the EEPROM, and jumping
//! to the bootloader, are not among them.

pub mod transport;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Every VIA report is this long, in both directions.
pub const REPORT: usize = 32;
/// The most data one buffer read or write carries: the report minus the
/// command, offset and size bytes.
pub const CHUNK: usize = 28;

/// Command IDs, as `via.h` numbers them.
pub mod command {
    pub const GET_PROTOCOL_VERSION: u8 = 0x01;
    pub const GET_KEYBOARD_VALUE: u8 = 0x02;
    pub const SET_KEYBOARD_VALUE: u8 = 0x03;
    pub const GET_KEYCODE: u8 = 0x04;
    pub const SET_KEYCODE: u8 = 0x05;
    pub const CUSTOM_SET_VALUE: u8 = 0x07;
    pub const CUSTOM_GET_VALUE: u8 = 0x08;
    pub const CUSTOM_SAVE: u8 = 0x09;
    pub const MACRO_GET_COUNT: u8 = 0x0C;
    pub const MACRO_GET_BUFFER_SIZE: u8 = 0x0D;
    pub const MACRO_GET_BUFFER: u8 = 0x0E;
    pub const MACRO_SET_BUFFER: u8 = 0x0F;
    pub const GET_LAYER_COUNT: u8 = 0x11;
    pub const GET_BUFFER: u8 = 0x12;
    pub const SET_BUFFER: u8 = 0x13;
    pub const GET_ENCODER: u8 = 0x14;
    pub const SET_ENCODER: u8 = 0x15;
    /// What the keyboard answers to a command it does not handle.
    pub const UNHANDLED: u8 = 0xFF;
}

/// Keyboard values, the sub-IDs of `GET_KEYBOARD_VALUE` and
/// `SET_KEYBOARD_VALUE`.
pub mod value {
    pub const UPTIME: u8 = 0x01;
    pub const LAYOUT_OPTIONS: u8 = 0x02;
    pub const FIRMWARE_VERSION: u8 = 0x04;
    pub const KEYCODES_VERSION: u8 = 0x06;
}

/// The commands this client reads with.
pub const READS: [u8; 10] = [
    command::GET_PROTOCOL_VERSION,
    command::GET_KEYBOARD_VALUE,
    command::GET_KEYCODE,
    command::CUSTOM_GET_VALUE,
    command::MACRO_GET_COUNT,
    command::MACRO_GET_BUFFER_SIZE,
    command::MACRO_GET_BUFFER,
    command::GET_LAYER_COUNT,
    command::GET_BUFFER,
    command::GET_ENCODER,
];

/// The commands this client writes with, and no others.
pub const WRITES: [u8; 6] = [
    command::SET_KEYBOARD_VALUE,
    command::SET_KEYCODE,
    command::CUSTOM_SET_VALUE,
    command::CUSTOM_SAVE,
    command::MACRO_SET_BUFFER,
    command::SET_BUFFER,
];

/// A lighting (or audio) channel QMK exposes through VIA's custom values:
/// its ID, and each value with its length in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channel {
    pub name: &'static str,
    pub id: u8,
    /// `(value ID, name, bytes)`.
    pub values: &'static [(u8, &'static str, usize)],
}

/// QMK's channels, from `via.h`: brightness, effect, speed and color where
/// the kind of lighting has them; audio has its two switches.
pub const CHANNELS: [Channel; 5] = [
    Channel {
        name: "backlight",
        id: 1,
        values: &[(1, "brightness", 1), (2, "effect", 1)],
    },
    Channel {
        name: "rgblight",
        id: 2,
        values: &[
            (1, "brightness", 1),
            (2, "effect", 1),
            (3, "effect_speed", 1),
            (4, "color", 2),
        ],
    },
    Channel {
        name: "rgb_matrix",
        id: 3,
        values: &[
            (1, "brightness", 1),
            (2, "effect", 1),
            (3, "effect_speed", 1),
            (4, "color", 2),
        ],
    },
    Channel {
        name: "audio",
        id: 4,
        values: &[(1, "enable", 1), (2, "clicky_enable", 1)],
    },
    Channel {
        name: "led_matrix",
        id: 5,
        values: &[
            (1, "brightness", 1),
            (2, "effect", 1),
            (3, "effect_speed", 1),
        ],
    },
];

pub fn channel(name: &str) -> Option<&'static Channel> {
    CHANNELS.iter().find(|c| c.name == name)
}

#[derive(Debug, thiserror::Error)]
pub enum ViaError {
    #[error("the keyboard does not handle command {0:#04x}")]
    Unhandled(u8),
    #[error("the keyboard answered command {sent:#04x} with {got:#04x}")]
    Protocol { sent: u8, got: u8 },
    #[error("command {0:#04x} is not one this app sends")]
    Refused(u8),
    #[error("no VIA keyboard was found on USB. Connect it by cable: VIA does not work over Bluetooth or a wireless dongle. Close VIA if it is open; only one program can talk to the keyboard at a time")]
    NotFound,
    #[error("the keyboard holds a different amount of data than this app expects for this board, so nothing was written")]
    Mismatch,
    #[error("the keyboard's VIA protocol is version {0:#06x}; this app needs 0x0009 or later")]
    TooOld(u16),
    #[error("{0}")]
    Unfit(String),
    #[error("could not talk to the keyboard: {0}")]
    Io(#[from] std::io::Error),
}

/// Moves one report each way.
pub trait Transport {
    fn exchange(&mut self, request: &[u8; REPORT]) -> std::io::Result<[u8; REPORT]>;
}

/// Lighting values by `(channel, value)` ID, as the keyboard gave them.
pub type Lighting = BTreeMap<(u8, u8), Vec<u8>>;

/// The size of a keyboard's switch matrix, which shapes its keymap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Matrix {
    pub rows: u8,
    pub cols: u8,
}

/// Everything this app reads from a VIA keyboard, as the keyboard holds
/// it: the keymap as keycodes, the macro buffer as bytes, the layout
/// options word, and the lighting values of the channels it has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub protocol: u16,
    pub firmware_version: u32,
    #[serde(default)]
    pub keycodes_version: Option<u32>,
    pub layers: u8,
    pub matrix: Matrix,
    /// Layer by layer, row by row, column by column.
    pub keymap: Vec<u16>,
    pub macro_count: u8,
    /// The whole macro buffer, macros separated by zero bytes.
    pub macros: Vec<u8>,
    pub layout_options: u32,
    /// By `(channel, value)` ID, the bytes the keyboard gave.
    #[serde(default, with = "lighting_entries")]
    pub lighting: Lighting,
}

/// The lighting map in JSON as a list of entries, since JSON keys must be
/// strings.
mod lighting_entries {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Entry {
        channel: u8,
        value: u8,
        bytes: Vec<u8>,
    }

    pub fn serialize<S: Serializer>(
        map: &super::Lighting,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let entries: Vec<Entry> = map
            .iter()
            .map(|(&(channel, value), bytes)| Entry {
                channel,
                value,
                bytes: bytes.clone(),
            })
            .collect();
        entries.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<super::Lighting, D::Error> {
        let entries: Vec<Entry> = Vec::deserialize(deserializer)?;
        Ok(entries
            .into_iter()
            .map(|e| ((e.channel, e.value), e.bytes))
            .collect())
    }
}

impl Image {
    /// The keycode at a layer, row and column.
    pub fn keycode(&self, layer: u8, row: u8, col: u8) -> Option<u16> {
        let (rows, cols) = (self.matrix.rows as usize, self.matrix.cols as usize);
        if row as usize >= rows || col as usize >= cols {
            return None;
        }
        self.keymap
            .get((layer as usize * rows + row as usize) * cols + col as usize)
            .copied()
    }

    /// The keymap as the keyboard stores it: big-endian keycodes.
    pub fn keymap_bytes(&self) -> Vec<u8> {
        self.keymap.iter().flat_map(|k| k.to_be_bytes()).collect()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("an image always serializes")
    }

    pub fn from_json(text: &str) -> Result<Image, ViaError> {
        serde_json::from_str(text).map_err(|e| ViaError::Unfit(format!("not a VIA backup: {e}")))
    }
}

/// A VIA conversation over any transport.
pub struct Via<T: Transport> {
    transport: T,
}

fn be16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

impl<T: Transport> Via<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn into_inner(self) -> T {
        self.transport
    }

    /// Sends one command with its data and returns the keyboard's answer,
    /// which echoes the command. Only listed commands go out.
    fn send(&mut self, id: u8, data: &[u8]) -> Result<[u8; REPORT], ViaError> {
        if !READS.contains(&id) && !WRITES.contains(&id) {
            return Err(ViaError::Refused(id));
        }
        let mut request = [0u8; REPORT];
        request[0] = id;
        request[1..1 + data.len()].copy_from_slice(data);
        let answer = self.transport.exchange(&request)?;
        match answer[0] {
            got if got == id => Ok(answer),
            command::UNHANDLED => Err(ViaError::Unhandled(id)),
            got => Err(ViaError::Protocol { sent: id, got }),
        }
    }

    pub fn protocol_version(&mut self) -> Result<u16, ViaError> {
        Ok(be16(&self.send(command::GET_PROTOCOL_VERSION, &[])?[1..]))
    }

    /// A 32-bit keyboard value: uptime, layout options, the firmware or
    /// keycodes version.
    pub fn keyboard_value(&mut self, id: u8) -> Result<u32, ViaError> {
        Ok(be32(&self.send(command::GET_KEYBOARD_VALUE, &[id])?[2..]))
    }

    pub fn set_layout_options(&mut self, options: u32) -> Result<(), ViaError> {
        let mut data = vec![value::LAYOUT_OPTIONS];
        data.extend_from_slice(&options.to_be_bytes());
        self.send(command::SET_KEYBOARD_VALUE, &data).map(|_| ())
    }

    pub fn layer_count(&mut self) -> Result<u8, ViaError> {
        Ok(self.send(command::GET_LAYER_COUNT, &[])?[1])
    }

    pub fn keycode(&mut self, layer: u8, row: u8, col: u8) -> Result<u16, ViaError> {
        Ok(be16(
            &self.send(command::GET_KEYCODE, &[layer, row, col])?[4..],
        ))
    }

    pub fn set_keycode(
        &mut self,
        layer: u8,
        row: u8,
        col: u8,
        keycode: u16,
    ) -> Result<(), ViaError> {
        let [hi, lo] = keycode.to_be_bytes();
        self.send(command::SET_KEYCODE, &[layer, row, col, hi, lo])
            .map(|_| ())
    }

    /// A buffer read in chunks: the keymap or the macros.
    fn read_buffer(&mut self, id: u8, length: usize) -> Result<Vec<u8>, ViaError> {
        let mut out = Vec::with_capacity(length);
        let mut offset = 0usize;
        while offset < length {
            let size = (length - offset).min(CHUNK);
            let [hi, lo] = (offset as u16).to_be_bytes();
            let answer = self.send(id, &[hi, lo, size as u8])?;
            out.extend_from_slice(&answer[4..4 + size]);
            offset += size;
        }
        Ok(out)
    }

    /// Writes the chunks of `after` that differ from `before`, and returns
    /// the offsets written. Both buffers must be the same length.
    fn write_buffer(
        &mut self,
        id: u8,
        before: &[u8],
        after: &[u8],
    ) -> Result<Vec<usize>, ViaError> {
        if before.len() != after.len() {
            return Err(ViaError::Mismatch);
        }
        let mut written = Vec::new();
        for (index, (old, new)) in before.chunks(CHUNK).zip(after.chunks(CHUNK)).enumerate() {
            if old == new {
                continue;
            }
            let offset = index * CHUNK;
            let [hi, lo] = (offset as u16).to_be_bytes();
            let mut data = vec![hi, lo, new.len() as u8];
            data.extend_from_slice(new);
            self.send(id, &data)?;
            written.push(offset);
        }
        Ok(written)
    }

    /// The whole keymap: `layers × rows × cols` keycodes.
    pub fn keymap(&mut self, layers: u8, matrix: Matrix) -> Result<Vec<u16>, ViaError> {
        let count = layers as usize * matrix.rows as usize * matrix.cols as usize;
        let bytes = self.read_buffer(command::GET_BUFFER, count * 2)?;
        Ok(bytes.chunks(2).map(be16).collect())
    }

    pub fn macro_count(&mut self) -> Result<u8, ViaError> {
        Ok(self.send(command::MACRO_GET_COUNT, &[])?[1])
    }

    pub fn macro_buffer_size(&mut self) -> Result<u16, ViaError> {
        Ok(be16(&self.send(command::MACRO_GET_BUFFER_SIZE, &[])?[1..]))
    }

    pub fn macros(&mut self) -> Result<Vec<u8>, ViaError> {
        let size = self.macro_buffer_size()? as usize;
        self.read_buffer(command::MACRO_GET_BUFFER, size)
    }

    /// One lighting value's bytes, or nothing when the keyboard does not
    /// have the channel.
    pub fn custom_value(
        &mut self,
        channel: u8,
        id: u8,
        length: usize,
    ) -> Result<Option<Vec<u8>>, ViaError> {
        match self.send(command::CUSTOM_GET_VALUE, &[channel, id]) {
            Ok(answer) => Ok(Some(answer[3..3 + length].to_vec())),
            Err(ViaError::Unhandled(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn set_custom_value(&mut self, channel: u8, id: u8, bytes: &[u8]) -> Result<(), ViaError> {
        let mut data = vec![channel, id];
        data.extend_from_slice(bytes);
        self.send(command::CUSTOM_SET_VALUE, &data).map(|_| ())
    }

    /// Makes a channel's values outlast a restart.
    pub fn save_custom(&mut self, channel: u8) -> Result<(), ViaError> {
        self.send(command::CUSTOM_SAVE, &[channel]).map(|_| ())
    }

    /// Reads everything this app keeps of a keyboard whose matrix is
    /// `matrix`, asking for the lighting values of `channels`.
    pub fn read_image(&mut self, matrix: Matrix, channels: &[&Channel]) -> Result<Image, ViaError> {
        let protocol = self.protocol_version()?;
        if protocol < 0x0009 {
            return Err(ViaError::TooOld(protocol));
        }
        let firmware_version = self.keyboard_value(value::FIRMWARE_VERSION)?;
        let keycodes_version = match self.keyboard_value(value::KEYCODES_VERSION) {
            Ok(version) => Some(version),
            Err(ViaError::Unhandled(_)) => None,
            Err(error) => return Err(error),
        };
        let layers = self.layer_count()?;
        let keymap = self.keymap(layers, matrix)?;
        let (macro_count, macros) = match self.macro_count() {
            Ok(count) => (count, self.macros()?),
            Err(ViaError::Unhandled(_)) => (0, Vec::new()),
            Err(error) => return Err(error),
        };
        let layout_options = self.keyboard_value(value::LAYOUT_OPTIONS)?;
        let mut lighting = BTreeMap::new();
        for channel in channels {
            for &(id, _, length) in channel.values {
                if let Some(bytes) = self.custom_value(channel.id, id, length)? {
                    lighting.insert((channel.id, id), bytes);
                }
            }
        }
        Ok(Image {
            protocol,
            firmware_version,
            keycodes_version,
            layers,
            matrix,
            keymap,
            macro_count,
            macros,
            layout_options,
            lighting,
        })
    }

    /// Writes what differs between `before` and `after`: the keymap and
    /// the macros chunk by chunk, the layout options, and each lighting
    /// value, saved per channel. Returns what was written, in words.
    pub fn write_changes(
        &mut self,
        before: &Image,
        after: &Image,
    ) -> Result<Vec<String>, ViaError> {
        if before.layers != after.layers
            || before.matrix != after.matrix
            || before.keymap.len() != after.keymap.len()
            || before.macros.len() != after.macros.len()
        {
            return Err(ViaError::Mismatch);
        }
        let mut sent = Vec::new();
        let cols = before.matrix.cols as usize;
        let rows = before.matrix.rows as usize;
        let written = self.write_buffer(
            command::SET_BUFFER,
            &before.keymap_bytes(),
            &after.keymap_bytes(),
        )?;
        if !written.is_empty() {
            for (index, (old, new)) in before.keymap.iter().zip(&after.keymap).enumerate() {
                if old != new {
                    let (layer, rest) = (index / (rows * cols), index % (rows * cols));
                    sent.push(format!(
                        "key layer {layer} row {} col {}",
                        rest / cols,
                        rest % cols
                    ));
                }
            }
        }
        if !self
            .write_buffer(command::MACRO_SET_BUFFER, &before.macros, &after.macros)?
            .is_empty()
        {
            sent.push("macros".into());
        }
        if before.layout_options != after.layout_options {
            self.set_layout_options(after.layout_options)?;
            sent.push("layout options".into());
        }
        let mut touched = Vec::new();
        for (&(channel, id), bytes) in &after.lighting {
            if before.lighting.get(&(channel, id)) == Some(bytes) {
                continue;
            }
            self.set_custom_value(channel, id, bytes)?;
            if !touched.contains(&channel) {
                touched.push(channel);
            }
            let name = CHANNELS
                .iter()
                .find(|c| c.id == channel)
                .and_then(|c| {
                    c.values
                        .iter()
                        .find(|v| v.0 == id)
                        .map(|v| format!("{} {}", c.name, v.1))
                })
                .unwrap_or_else(|| format!("channel {channel} value {id}"));
            sent.push(name);
        }
        for channel in touched {
            self.save_custom(channel)?;
        }
        Ok(sent)
    }
}

/// Opens the VIA keyboard with `ids`, the one with `serial` when given.
pub fn open(
    ids: (u16, u16),
    serial: Option<&str>,
) -> Result<Via<transport::HidTransport>, ViaError> {
    let found = transport::Found::all(Some(ids), serial)?
        .into_iter()
        .next()
        .ok_or(ViaError::NotFound)?;
    Ok(Via::new(found.open()?))
}
