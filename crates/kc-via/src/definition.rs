//! VIA keyboard definitions: the JSON file that tells VIA what a keyboard
//! is, read into a board. The format is VIA's version 3 (see
//! caniusevia.com/docs/specification): the keys are laid out in Keyboard
//! Layout Editor's JSON, with each key's matrix position in its top-left
//! legend and, for keys that belong to a layout option, the option and
//! choice in its bottom-right one.
//!
//! The KLE walk follows Keyboard Layout Editor's own `serial.js`
//! (ijprest/keyboard-layout-editor, MIT): a cursor moves right by each
//! key's width, properties before a key change the cursor or the key,
//! size resets to 1u after every key, setting a rotation origin moves the
//! cursor to it, and each row starts a unit lower at the origin's x.
//!
//! Definitions are read from a file the user chooses, or fetched when the
//! keyboard is known; none ships with the app, as VIA's own collection is
//! GPL-3.0.

use kc_boards::board::{
    Capability, Family, FirmwareProfile, FlashInfo, Half, LayoutOption, PhysicalLayout, Side,
    UsbId, ViaProfile,
};
use kc_boards::geometry::Key;
use kc_boards::{Board, BoardError};
use serde_json::Value;

use crate::CHANNELS;

#[derive(Debug, thiserror::Error)]
pub enum DefinitionError {
    #[error("not a VIA definition: {0}")]
    Json(String),
    #[error("the definition has no `{0}`")]
    Missing(&'static str),
    #[error("the definition's `{field}` is not {expected}")]
    Wrong {
        field: &'static str,
        expected: &'static str,
    },
    #[error("key {index} has no matrix position in its top-left legend")]
    NoPosition { index: usize },
    #[error("the keymap has no keys in the default layout")]
    NoKeys,
    #[error("{0}")]
    Board(#[from] BoardError),
}

/// One key as KLE lays it out, in key units, before any filtering.
#[derive(Debug, Clone, PartialEq)]
pub struct KleKey {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub rotation: f64,
    pub rx: f64,
    pub ry: f64,
    pub decal: bool,
    /// The legend, split on newlines as written: position 0 is the
    /// top-left legend, position 3 the bottom-right one.
    pub legends: Vec<String>,
}

impl KleKey {
    /// The key in the app's units: hundredths of a key unit and of a
    /// degree, as ZMK's physical layouts have them.
    pub fn key(&self) -> Key {
        let unit = |v: f64| (v * 100.).round() as i32;
        Key::from([
            unit(self.w),
            unit(self.h),
            unit(self.x),
            unit(self.y),
            unit(self.rotation),
            unit(self.rx),
            unit(self.ry),
        ])
    }

    /// The matrix position in the top-left legend, as `(row, col)`.
    pub fn position(&self) -> Option<(u8, u8)> {
        pair(self.legends.first()?)
    }

    /// The layout option and choice in the bottom-right legend.
    pub fn option(&self) -> Option<(u8, u8)> {
        pair(self.legends.get(3)?)
    }
}

fn pair(text: &str) -> Option<(u8, u8)> {
    let (a, b) = text.trim().split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Walks KLE rows into keys, as kle-serial does.
pub fn kle_keys(rows: &[Value]) -> Result<Vec<KleKey>, DefinitionError> {
    let number = |item: &Value, name: &str| item.get(name).and_then(Value::as_f64);
    let mut keys = Vec::new();
    let (mut x, mut y, mut w, mut h) = (0.0f64, 0.0f64, 1.0f64, 1.0f64);
    // The rotation origin is also where each row starts.
    let (mut rotation, mut rx, mut ry) = (0.0f64, 0.0f64, 0.0f64);
    let mut decal = false;
    for row in rows {
        let Some(items) = row.as_array() else {
            // Keyboard metadata, which has nothing the layout needs.
            continue;
        };
        for item in items {
            match item {
                Value::String(legend) => {
                    keys.push(KleKey {
                        x,
                        y,
                        w,
                        h,
                        rotation,
                        rx,
                        ry,
                        decal,
                        legends: legend.split('\n').map(str::to_string).collect(),
                    });
                    x += w;
                    w = 1.;
                    h = 1.;
                    decal = false;
                }
                Value::Object(_) => {
                    if let Some(r) = number(item, "r") {
                        rotation = r;
                    }
                    if let Some(v) = number(item, "rx") {
                        rx = v;
                        x = rx;
                        y = ry;
                    }
                    if let Some(v) = number(item, "ry") {
                        ry = v;
                        x = rx;
                        y = ry;
                    }
                    if let Some(dx) = number(item, "x") {
                        x += dx;
                    }
                    if let Some(dy) = number(item, "y") {
                        y += dy;
                    }
                    if let Some(v) = number(item, "w") {
                        w = v;
                    }
                    if let Some(v) = number(item, "h") {
                        h = v;
                    }
                    if let Some(d) = item.get("d").and_then(Value::as_bool) {
                        decal = d;
                    }
                }
                _ => {
                    return Err(DefinitionError::Wrong {
                        field: "layouts.keymap",
                        expected: "rows of keys and properties",
                    })
                }
            }
        }
        y += 1.;
        x = rx;
    }
    Ok(keys)
}

/// What a definition says about a keyboard.
#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub rows: u8,
    pub cols: u8,
    pub keys: Vec<KleKey>,
    pub options: Vec<LayoutOption>,
    /// `[channel, value]` IDs of the lighting values the menus reach.
    pub lighting: Vec<[u8; 2]>,
    pub custom_keycodes: Vec<String>,
    /// The keycode modules the definition names, such as
    /// `qmk_rgb_matrix_keycodes`.
    pub keycode_modules: Vec<String>,
}

fn id(value: Option<&Value>, field: &'static str) -> Result<u16, DefinitionError> {
    let value = value.ok_or(DefinitionError::Missing(field))?;
    let parsed = match value {
        Value::String(text) => {
            let text = text.trim();
            let digits = text
                .strip_prefix("0x")
                .or_else(|| text.strip_prefix("0X"))
                .unwrap_or(text);
            u16::from_str_radix(digits, 16).ok()
        }
        Value::Number(n) => n.as_u64().and_then(|n| u16::try_from(n).ok()),
        _ => None,
    };
    parsed.ok_or(DefinitionError::Wrong {
        field,
        expected: "a USB identifier such as \"0x5241\"",
    })
}

/// The lighting values a menu entry reaches: VIA's named menus by their
/// channel, and custom menus by the `[id, channel, value]` triples in
/// their contents.
fn lighting_of(menus: &[Value]) -> Vec<[u8; 2]> {
    let mut out: Vec<[u8; 2]> = Vec::new();
    let mut push = |channel: u8, value: u8| {
        if !out.contains(&[channel, value]) {
            out.push([channel, value]);
        }
    };
    let whole = |name: &str, push: &mut dyn FnMut(u8, u8)| {
        if let Some(channel) = CHANNELS.iter().find(|c| c.name == name) {
            for &(value, _, _) in channel.values {
                push(channel.id, value);
            }
        }
    };
    fn walk(value: &Value, push: &mut dyn FnMut(u8, u8)) {
        match value {
            Value::Array(items) => {
                if let [Value::String(id), Value::Number(channel), Value::Number(v)] =
                    items.as_slice()
                {
                    if id.starts_with("id_") {
                        if let (Some(channel), Some(v)) = (channel.as_u64(), v.as_u64()) {
                            if let (Ok(channel), Ok(v)) = (u8::try_from(channel), u8::try_from(v)) {
                                if CHANNELS
                                    .iter()
                                    .any(|c| c.id == channel && c.values.iter().any(|x| x.0 == v))
                                {
                                    push(channel, v);
                                }
                            }
                        }
                        return;
                    }
                }
                for item in items {
                    walk(item, push);
                }
            }
            Value::Object(map) => {
                for item in map.values() {
                    walk(item, push);
                }
            }
            _ => {}
        }
    }
    for menu in menus {
        match menu {
            Value::String(name) => match name.as_str() {
                "qmk_backlight" => whole("backlight", &mut push),
                "qmk_rgblight" => whole("rgblight", &mut push),
                "qmk_backlight_rgblight" => {
                    whole("backlight", &mut push);
                    whole("rgblight", &mut push);
                }
                "qmk_rgb_matrix" => whole("rgb_matrix", &mut push),
                "qmk_led_matrix" => whole("led_matrix", &mut push),
                "qmk_audio" => whole("audio", &mut push),
                _ => {}
            },
            other => walk(other, &mut push),
        }
    }
    out
}

/// Reads a definition file.
pub fn parse(text: &str) -> Result<Definition, DefinitionError> {
    let root: Value =
        serde_json::from_str(text).map_err(|e| DefinitionError::Json(e.to_string()))?;
    let name = match root.get("name") {
        Some(Value::String(name)) => name.trim().to_string(),
        // A name the keyboard chooses at run time: the first option.
        Some(Value::Object(named)) => named
            .get("options")
            .and_then(Value::as_array)
            .and_then(|o| o.first())
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .ok_or(DefinitionError::Missing("name"))?,
        _ => return Err(DefinitionError::Missing("name")),
    };
    if name.is_empty() {
        return Err(DefinitionError::Missing("name"));
    }
    let vendor_id = id(root.get("vendorId"), "vendorId")?;
    let product_id = id(root.get("productId"), "productId")?;
    let matrix = root
        .get("matrix")
        .ok_or(DefinitionError::Missing("matrix"))?;
    let dimension = |field: &'static str| {
        matrix
            .get(field.trim_start_matches("matrix."))
            .and_then(Value::as_u64)
            .and_then(|n| u8::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or(DefinitionError::Wrong {
                field,
                expected: "a count from 1 to 255",
            })
    };
    let rows = dimension("matrix.rows")?;
    let cols = dimension("matrix.cols")?;
    let layouts = root
        .get("layouts")
        .ok_or(DefinitionError::Missing("layouts"))?;
    let keymap = layouts
        .get("keymap")
        .and_then(Value::as_array)
        .ok_or(DefinitionError::Missing("layouts.keymap"))?;
    let keys = kle_keys(keymap)?;
    let options = layouts
        .get("labels")
        .and_then(Value::as_array)
        .map(|labels| {
            labels
                .iter()
                .filter_map(|label| match label {
                    Value::String(text) => Some(LayoutOption {
                        label: text.clone(),
                        choices: vec!["Off".into(), "On".into()],
                    }),
                    Value::Array(items) => {
                        let mut texts = items.iter().filter_map(Value::as_str);
                        let label = texts.next()?.to_string();
                        Some(LayoutOption {
                            label,
                            choices: texts.map(str::to_string).collect(),
                        })
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let lighting = root
        .get("menus")
        .and_then(Value::as_array)
        .map(|menus| lighting_of(menus))
        .unwrap_or_default();
    let custom_keycodes = root
        .get("customKeycodes")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|k| {
                    k.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string()
                })
                .collect()
        })
        .unwrap_or_default();
    let keycode_modules = root
        .get("keycodes")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Ok(Definition {
        name,
        vendor_id,
        product_id,
        rows,
        cols,
        keys,
        options,
        lighting,
        custom_keycodes,
        keycode_modules,
    })
}

impl Definition {
    /// The keys the default layout shows: every key with a matrix
    /// position that is not a decal and belongs to no layout option, or
    /// to choice 0 of one.
    pub fn default_keys(&self) -> Result<Vec<(Key, [u8; 2])>, DefinitionError> {
        let mut out = Vec::new();
        for (index, key) in self.keys.iter().enumerate() {
            if key.decal {
                continue;
            }
            if key.option().is_some_and(|(_, choice)| choice != 0) {
                continue;
            }
            let (row, col) = key
                .position()
                .ok_or(DefinitionError::NoPosition { index })?;
            out.push((key.key(), [row, col]));
        }
        if out.is_empty() {
            return Err(DefinitionError::NoKeys);
        }
        Ok(out)
    }

    /// A board for this keyboard: whole, with one firmware, QMK with VIA,
    /// configured on the keyboard. The maker is the name's first word.
    pub fn board(&self) -> Result<Board, DefinitionError> {
        let keys = self.default_keys()?;
        let slug: String = self
            .name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let mut words = self.name.split_whitespace();
        let vendor = match (words.next(), words.next()) {
            (Some(first), Some(_)) => first.to_string(),
            _ => "VIA".to_string(),
        };
        let board = Board {
            id: format!("via-{slug}"),
            name: self.name.clone(),
            vendor,
            verified: false,
            brightness_cap: 100,
            default_layout: "layout_0".into(),
            starter_keys: Vec::new(),
            usb: vec![UsbId {
                vendor: self.vendor_id,
                product: self.product_id,
                name: self.name.clone(),
            }],
            flash: FlashInfo {
                order: Vec::new(),
                bootloader_entry: String::new(),
                studio_unlock: None,
            },
            halves: vec![Half {
                side: Side::Whole,
                central: true,
                bootloader_volume: None,
                uf2_family: None,
                leds: None,
            }],
            pointing: Vec::new(),
            firmware: vec![FirmwareProfile {
                id: "via".into(),
                name: "QMK with VIA, on the keyboard".into(),
                family: Family::Via,
                experimental: false,
                notes: vec![
                    "The keyboard's own firmware stays. Keys, layers and macros are written to it over VIA's protocol, on the cable; combos, tap-dances and per-key colors would need firmware built for the keyboard, which this app does not do for QMK.".into(),
                ],
                zephyr: String::new(),
                config_name: String::new(),
                workflow: String::new(),
                zmk: None,
                modules: Vec::new(),
                capabilities: Vec::<Capability>::new(),
                lighting: None,
                dygma: None,
                rmk: None,
                via: Some(ViaProfile {
                    rows: self.rows,
                    cols: self.cols,
                    matrix: keys.iter().map(|(_, at)| *at).collect(),
                    options: self.options.clone(),
                    lighting: self.lighting.clone(),
                    custom_keycodes: self.custom_keycodes.clone(),
                }),
                controller: None,
                studio_only: false,
                builds: Vec::new(),
            }],
            layouts: vec![PhysicalLayout {
                id: "layout_0".into(),
                name: self.name.clone(),
                keys: keys.into_iter().map(|(key, _)| key).collect(),
                underglow: Vec::new(),
            }],
        };
        Ok(board.checked()?)
    }
}
