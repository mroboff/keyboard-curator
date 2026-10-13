//! The VIA client against a simulated keyboard.

use std::collections::BTreeMap;

use kc_via::{channel, command, value, Image, Matrix, Transport, Via, ViaError, CHUNK, REPORT};

/// A keyboard that speaks VIA: four layers of a 2×3 matrix, a 64-byte
/// macro buffer, RGB matrix lighting, and a record of every request.
struct Simulated {
    layers: u8,
    matrix: Matrix,
    keymap: Vec<u8>,
    macros: Vec<u8>,
    layout_options: u32,
    lighting: BTreeMap<(u8, u8), Vec<u8>>,
    saved: Vec<u8>,
    requests: Vec<[u8; REPORT]>,
    /// Commands the firmware was built without.
    missing: Vec<u8>,
}

impl Simulated {
    fn new() -> Self {
        let (layers, matrix) = (4u8, Matrix { rows: 2, cols: 3 });
        let count = layers as usize * 2 * 3;
        // Keycodes 0x0004 upward, as big-endian bytes.
        let keymap = (0..count as u16)
            .flat_map(|i| (0x0004 + i).to_be_bytes())
            .collect();
        let mut macros = vec![0u8; 64];
        macros[..3].copy_from_slice(b"hi\0");
        let mut lighting = BTreeMap::new();
        lighting.insert((3, 1), vec![200]);
        lighting.insert((3, 2), vec![5]);
        lighting.insert((3, 3), vec![128]);
        lighting.insert((3, 4), vec![40, 255]);
        Self {
            layers,
            matrix,
            keymap,
            macros,
            layout_options: 0x0000_0002,
            lighting,
            saved: Vec::new(),
            requests: Vec::new(),
            missing: Vec::new(),
        }
    }

    fn sent(&self, id: u8) -> usize {
        self.requests.iter().filter(|r| r[0] == id).count()
    }
}

impl Transport for Simulated {
    fn exchange(&mut self, request: &[u8; REPORT]) -> std::io::Result<[u8; REPORT]> {
        self.requests.push(*request);
        let mut answer = *request;
        let data = &request[1..];
        if self.missing.contains(&request[0]) {
            answer[0] = command::UNHANDLED;
            return Ok(answer);
        }
        match request[0] {
            command::GET_PROTOCOL_VERSION => answer[1..3].copy_from_slice(&0x000Du16.to_be_bytes()),
            command::GET_KEYBOARD_VALUE => {
                let word: u32 = match data[0] {
                    value::LAYOUT_OPTIONS => self.layout_options,
                    value::FIRMWARE_VERSION => 7,
                    value::KEYCODES_VERSION => 0x0000_0003,
                    value::UPTIME => 12_345,
                    _ => {
                        answer[0] = command::UNHANDLED;
                        return Ok(answer);
                    }
                };
                answer[2..6].copy_from_slice(&word.to_be_bytes());
            }
            command::SET_KEYBOARD_VALUE if data[0] == value::LAYOUT_OPTIONS => {
                self.layout_options = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);
            }
            command::GET_LAYER_COUNT => answer[1] = self.layers,
            command::GET_KEYCODE => {
                let index = ((data[0] as usize * self.matrix.rows as usize + data[1] as usize)
                    * self.matrix.cols as usize
                    + data[2] as usize)
                    * 2;
                answer[4..6].copy_from_slice(&self.keymap[index..index + 2]);
            }
            command::SET_KEYCODE => {
                let index = ((data[0] as usize * self.matrix.rows as usize + data[1] as usize)
                    * self.matrix.cols as usize
                    + data[2] as usize)
                    * 2;
                self.keymap[index..index + 2].copy_from_slice(&data[3..5]);
            }
            command::GET_BUFFER | command::MACRO_GET_BUFFER => {
                let (offset, size) = (
                    u16::from_be_bytes([data[0], data[1]]) as usize,
                    data[2] as usize,
                );
                assert!(size <= CHUNK, "chunks are at most {CHUNK} bytes");
                let buffer = if request[0] == command::GET_BUFFER {
                    &self.keymap
                } else {
                    &self.macros
                };
                answer[4..4 + size].copy_from_slice(&buffer[offset..offset + size]);
            }
            command::SET_BUFFER | command::MACRO_SET_BUFFER => {
                let (offset, size) = (
                    u16::from_be_bytes([data[0], data[1]]) as usize,
                    data[2] as usize,
                );
                assert!(size <= CHUNK);
                let buffer = if request[0] == command::SET_BUFFER {
                    &mut self.keymap
                } else {
                    &mut self.macros
                };
                buffer[offset..offset + size].copy_from_slice(&data[3..3 + size]);
            }
            command::MACRO_GET_COUNT => answer[1] = 16,
            command::MACRO_GET_BUFFER_SIZE => {
                answer[1..3].copy_from_slice(&(self.macros.len() as u16).to_be_bytes())
            }
            command::CUSTOM_GET_VALUE => match self.lighting.get(&(data[0], data[1])) {
                Some(bytes) => answer[3..3 + bytes.len()].copy_from_slice(bytes),
                None => answer[0] = command::UNHANDLED,
            },
            command::CUSTOM_SET_VALUE => {
                let length = self.lighting.get(&(data[0], data[1])).map_or(1, Vec::len);
                self.lighting
                    .insert((data[0], data[1]), data[2..2 + length].to_vec());
            }
            command::CUSTOM_SAVE => self.saved.push(data[0]),
            _ => answer[0] = command::UNHANDLED,
        }
        Ok(answer)
    }
}

fn rgb_matrix() -> Vec<&'static kc_via::Channel> {
    vec![
        channel("rgb_matrix").unwrap(),
        channel("backlight").unwrap(),
    ]
}

#[test]
fn an_image_is_read_in_chunks_of_at_most_28_bytes() {
    let mut via = Via::new(Simulated::new());
    let matrix = Matrix { rows: 2, cols: 3 };
    let image = via.read_image(matrix, &rgb_matrix()).unwrap();
    assert_eq!(image.protocol, 0x000D);
    assert_eq!(image.firmware_version, 7);
    assert_eq!(image.keycodes_version, Some(3));
    assert_eq!(image.layers, 4);
    assert_eq!(image.keymap.len(), 24);
    assert_eq!(image.keymap[0], 0x0004);
    assert_eq!(image.keycode(3, 1, 2), Some(0x0004 + 23));
    assert_eq!(image.keycode(0, 2, 0), None);
    assert_eq!(image.macro_count, 16);
    assert_eq!(&image.macros[..3], b"hi\0");
    assert_eq!(image.macros.len(), 64);
    assert_eq!(image.layout_options, 2);
    // The RGB matrix channel answered; the backlight one is not there.
    assert_eq!(image.lighting.len(), 4);
    assert_eq!(image.lighting[&(3, 4)], [40, 255]);
    assert!(!image.lighting.contains_key(&(1, 1)));

    let keyboard = via.into_inner();
    // 48 keymap bytes take two reads, 64 macro bytes three.
    assert_eq!(keyboard.sent(command::GET_BUFFER), 2);
    assert_eq!(keyboard.sent(command::MACRO_GET_BUFFER), 3);
    assert_eq!(keyboard.sent(command::GET_PROTOCOL_VERSION), 1);
    assert_eq!(keyboard.sent(command::SET_BUFFER), 0);

    let again = Image::from_json(&image.to_json()).unwrap();
    assert_eq!(again, image);
}

#[test]
fn only_what_differs_is_written_and_lighting_is_saved_per_channel() {
    let mut via = Via::new(Simulated::new());
    let matrix = Matrix { rows: 2, cols: 3 };
    let before = via.read_image(matrix, &rgb_matrix()).unwrap();

    // Nothing to do when nothing differs.
    assert!(via.write_changes(&before, &before).unwrap().is_empty());
    assert_eq!(via.into_inner().sent(command::SET_BUFFER), 0);

    let mut via = Via::new(Simulated::new());
    let mut after = before.clone();
    after.keymap[23] = 0x5220; // the last key, in the second chunk
    after.macros[3..6].copy_from_slice(b"yo\0");
    after.layout_options = 1;
    after.lighting.insert((3, 1), vec![255]);
    let sent = via.write_changes(&before, &after).unwrap();
    assert_eq!(
        sent,
        [
            "key layer 3 row 1 col 2",
            "macros",
            "layout options",
            "rgb_matrix brightness"
        ]
    );
    let keyboard = via.into_inner();
    // One keymap chunk of two, one macro chunk of three.
    assert_eq!(keyboard.sent(command::SET_BUFFER), 1);
    assert_eq!(keyboard.sent(command::MACRO_SET_BUFFER), 1);
    assert_eq!(keyboard.keymap[46..48], [0x52, 0x20]);
    assert_eq!(&keyboard.macros[..6], b"hi\0yo\0");
    assert_eq!(keyboard.layout_options, 1);
    assert_eq!(keyboard.lighting[&(3, 1)], [255]);
    assert_eq!(keyboard.saved, [3]);

    // What is written reads back the same.
    let mut via = Via::new(keyboard);
    assert_eq!(via.read_image(matrix, &rgb_matrix()).unwrap(), after);
}

#[test]
fn sizes_must_match_and_resets_are_never_sent() {
    let mut via = Via::new(Simulated::new());
    let matrix = Matrix { rows: 2, cols: 3 };
    let before = via.read_image(matrix, &rgb_matrix()).unwrap();
    let mut smaller = before.clone();
    smaller.keymap.pop();
    assert!(matches!(
        via.write_changes(&before, &smaller),
        Err(ViaError::Mismatch)
    ));
    let mut other = before.clone();
    other.layers = 5;
    assert!(matches!(
        via.write_changes(&before, &other),
        Err(ViaError::Mismatch)
    ));
    let keyboard = via.into_inner();
    assert!(keyboard
        .requests
        .iter()
        .all(|r| kc_via::READS.contains(&r[0])));
    for id in [0x06u8, 0x0A, 0x0B, 0x10] {
        assert!(!kc_via::WRITES.contains(&id) && !kc_via::READS.contains(&id));
    }
}

#[test]
fn a_keyboard_without_macros_or_a_keycodes_version_still_reads() {
    let mut keyboard = Simulated::new();
    keyboard.missing = vec![command::MACRO_GET_COUNT];
    let mut via = Via::new(keyboard);
    let image = via.read_image(Matrix { rows: 2, cols: 3 }, &[]).unwrap();
    assert_eq!(image.macro_count, 0);
    assert!(image.macros.is_empty());
    assert!(image.lighting.is_empty());

    let mut keyboard = Simulated::new();
    keyboard.missing = vec![command::GET_LAYER_COUNT];
    let mut via = Via::new(keyboard);
    assert!(matches!(
        via.read_image(Matrix { rows: 2, cols: 3 }, &[]),
        Err(ViaError::Unhandled(command::GET_LAYER_COUNT))
    ));
}
