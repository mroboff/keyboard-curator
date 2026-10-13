//! A VIA definition read into a board. The definition here is the app's
//! own: a small keyboard with a rotated thumb cluster, a split backspace
//! as a layout option, a blocker drawn as a decal, and lighting menus.

use kc_boards::board::{Capability, Side};
use kc_boards::Delivery;
use kc_via::definition::{kle_keys, parse};

const DEFINITION: &str = r##"{
  "name": "Curator Test Forty",
  "vendorId": "0x4B43",
  "productId": "0x0001",
  "matrix": { "rows": 3, "cols": 4 },
  "keycodes": ["qmk_rgb_matrix_keycodes"],
  "menus": [
    "qmk_backlight",
    {
      "label": "Lighting",
      "content": [
        { "label": "Brightness", "type": "range", "options": [0, 255], "content": ["id_qmk_rgb_matrix_brightness", 3, 1] },
        { "label": "Color", "type": "color", "content": ["id_qmk_rgb_matrix_color", 3, 4] }
      ]
    }
  ],
  "customKeycodes": [
    { "name": "Mission Control", "title": "Mission Control in macOS", "shortName": "MCtrl" }
  ],
  "layouts": {
    "labels": ["Split Backspace", ["Bottom Row", "Blocker", "Wide"]],
    "keymap": [
      [{ "c": "#777777" }, "0,0", { "x": 0.5, "w": 1.5 }, "0,1", { "w": 2 }, "0,2\n\n\n0,0", { "x": 0.25 }, "0,3\n\n\n0,1", "0,2\n\n\n0,1"],
      [{ "y": 0.25 }, "1,0", { "d": true }, "", "1,1", { "h": 2 }, "1,2"],
      [{ "r": 15, "rx": 1, "ry": 3, "y": -0.5 }, "2,0", { "w": 1.25 }, "2,1"],
      [{ "r": 15, "rx": 1, "ry": 3, "y": 1 }, "2,2\n\n\n1,0", { "d": true }, "blocker\n\n\n1,1", { "w": 2 }, "2,3\n\n\n1,2"]
    ]
  }
}"##;

#[test]
fn kle_rows_are_walked_as_the_editor_lays_them_out() {
    let root: serde_json::Value = serde_json::from_str(DEFINITION).unwrap();
    let rows = root["layouts"]["keymap"].as_array().unwrap();
    let keys = kle_keys(rows).unwrap();
    assert_eq!(keys.len(), 14);
    // Row 1: a gap, a 1.5u key, a 2u key, then the split alternatives.
    let key = |i: usize| <[i32; 7]>::from(keys[i].key());
    assert_eq!(key(0), [100, 100, 0, 0, 0, 0, 0]);
    assert_eq!(key(1), [150, 100, 150, 0, 0, 0, 0]);
    assert_eq!(key(2), [200, 100, 300, 0, 0, 0, 0]);
    assert_eq!(key(3), [100, 100, 525, 0, 0, 0, 0]);
    assert_eq!(keys[3].option(), Some((0, 1)));
    assert_eq!(keys[2].option(), Some((0, 0)));
    assert_eq!(keys[2].position(), Some((0, 2)));
    // Row 2 starts a unit lower, plus the offset; the decal is a key too.
    assert_eq!(key(5), [100, 100, 0, 125, 0, 0, 0]);
    assert!(keys[6].decal);
    assert_eq!(key(8), [100, 200, 300, 125, 0, 0, 0]);
    // Row 3 is rotated: setting the origin moves the cursor there, then
    // the row's own offset applies.
    assert_eq!(key(9), [100, 100, 100, 250, 1500, 100, 300]);
    assert_eq!(key(10), [125, 100, 200, 250, 1500, 100, 300]);
    // Row 4 starts at the origin again, a unit lower by its offset.
    assert_eq!(key(11), [100, 100, 100, 400, 1500, 100, 300]);
    assert_eq!(keys[11].option(), Some((1, 0)));
}

#[test]
fn a_definition_becomes_a_board_with_the_default_layout_and_the_matrix() {
    let definition = parse(DEFINITION).unwrap();
    assert_eq!(definition.name, "Curator Test Forty");
    assert_eq!((definition.vendor_id, definition.product_id), (0x4B43, 1));
    assert_eq!((definition.rows, definition.cols), (3, 4));
    assert_eq!(definition.options.len(), 2);
    assert_eq!(definition.options[0].label, "Split Backspace");
    assert_eq!(definition.options[0].choices, ["Off", "On"]);
    assert_eq!(definition.options[1].label, "Bottom Row");
    assert_eq!(definition.options[1].choices, ["Blocker", "Wide"]);
    // The backlight menu brings both its values; the custom menu two of
    // the RGB matrix's.
    assert_eq!(definition.lighting, [[1, 1], [1, 2], [3, 1], [3, 4]]);
    assert_eq!(definition.custom_keycodes, ["Mission Control"]);
    assert_eq!(definition.keycode_modules, ["qmk_rgb_matrix_keycodes"]);

    let board = definition.board().unwrap();
    assert_eq!(board.id, "via-curator-test-forty");
    assert_eq!(board.vendor, "Curator");
    assert!(!board.verified);
    assert_eq!(board.usb[0].vendor, 0x4B43);
    assert_eq!(board.usb[0].name, "Curator Test Forty");
    assert!(board.half(Side::Whole).unwrap().central);
    let layout = board.layout("layout_0").unwrap();
    // Decals and non-default choices are left out: 14 drawn, 9 shown.
    assert_eq!(layout.keys.len(), 9);
    let profile = &board.firmware[0];
    assert_eq!(profile.delivery(), Delivery::Live);
    assert!(profile.features().contains(&Capability::HoldTaps));
    assert!(!profile.features().contains(&Capability::Combos));
    let via = profile.via.as_ref().unwrap();
    assert_eq!((via.rows, via.cols), (3, 4));
    assert_eq!(
        via.matrix,
        [
            [0, 0],
            [0, 1],
            [0, 2],
            [1, 0],
            [1, 1],
            [1, 2],
            [2, 0],
            [2, 1],
            [2, 2]
        ]
    );
    assert_eq!(via.lighting, [[1, 1], [1, 2], [3, 1], [3, 4]]);
    assert_eq!(via.options.len(), 2);
}

#[test]
fn a_definition_that_is_wrong_says_how() {
    assert!(parse("{").is_err());
    assert!(parse(r#"{"name": "X"}"#).is_err());
    let no_position = DEFINITION.replace(r#""0,0", { "x": 0.5"#, r#""Esc", { "x": 0.5"#);
    assert!(matches!(
        parse(&no_position).unwrap().board(),
        Err(kc_via::definition::DefinitionError::NoPosition { index: 0 })
    ));
    let twice = DEFINITION.replace(r#""1,0", { "d": true }"#, r#""0,0", { "d": true }"#);
    assert!(parse(&twice).unwrap().board().is_err());
    let named = DEFINITION.replace(
        r#""name": "Curator Test Forty""#,
        r#""name": { "content": ["id_board_variant", 0, 3], "options": ["Forty A", "Forty B"] }"#,
    );
    assert_eq!(parse(&named).unwrap().name, "Forty A");
}
