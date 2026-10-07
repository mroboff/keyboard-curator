//! Checks on the board definitions that ship with the app.

use kc_boards::board::{Board, Capability, Delivery, Side};
use kc_boards::built_in;

fn board(id: &str) -> Board {
    built_in()
        .expect("built-in boards load")
        .into_iter()
        .find(|b| b.id == id)
        .expect("board exists")
}

/// No two keys may sit on top of each other once rotation is applied.
/// Vendor thumb fans overlap slightly at their inner corners, so this
/// compares centers rather than outlines.
fn assert_keys_distinct(board: &Board) {
    for layout in &board.layouts {
        let centers: Vec<_> = layout.keys.iter().map(|k| k.center()).collect();
        for (i, a) in centers.iter().enumerate() {
            for (j, b) in centers.iter().enumerate().skip(i + 1) {
                let distance = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
                assert!(
                    distance > 85.,
                    "{}: keys {i} and {j} are {distance:.0} units apart",
                    layout.id
                );
            }
        }
    }
}

/// Every key of the LED layout has exactly one LED across the two halves.
fn assert_every_key_lit(board: &Board) {
    let mut lit: Vec<usize> = board
        .halves
        .iter()
        .flat_map(|h| h.leds.as_ref().expect("LED map").chain.clone())
        .collect();
    lit.sort_unstable();
    let keys = board.layout(&board.default_layout).unwrap().keys.len();
    assert_eq!(lit, (0..keys).collect::<Vec<_>>());
}

#[test]
fn imprint_layouts_have_the_documented_key_counts() {
    let imprint = board("cyboard-imprint");
    let counts: Vec<usize> = imprint.layouts.iter().map(|l| l.keys.len()).collect();
    assert_eq!(counts, [82, 76, 72, 70, 64, 60, 58, 52, 48]);
    assert_eq!(
        imprint.default_layout,
        "physical_layout_imprint_function_row_full_bottom_row"
    );
    assert_keys_distinct(&imprint);
}

#[test]
fn imprint_thumb_arcs_fan_out_from_shared_coordinates() {
    let imprint = board("cyboard-imprint");
    let keys = &imprint.layout(&imprint.default_layout).unwrap().keys;
    // Positions 70..=72 are the left upper arc: same x/y, different rotation.
    assert!(keys[70..=72].iter().all(|k| (k.x, k.y) == (550, 575)));
    let xs: Vec<f32> = keys[70..=72].iter().map(|k| k.center().x).collect();
    assert!(xs[0] < xs[1] && xs[1] < xs[2], "left arc fans rightwards");
    // The right upper arc mirrors it and ends on the unrotated inner key.
    let xs: Vec<f32> = keys[73..=75].iter().map(|k| k.center().x).collect();
    assert!(xs[0] < xs[1] && xs[1] < xs[2]);
}

#[test]
fn imprint_halves_and_firmware() {
    let imprint = board("cyboard-imprint");
    assert_eq!(imprint.brightness_cap, 50);
    assert!(imprint.half(Side::Left).unwrap().central);
    assert_every_key_lit(&imprint);
    assert!(imprint
        .halves
        .iter()
        .all(|h| !h.leds.as_ref().unwrap().verified));

    let lit = imprint.profile("kc-zmk-0.3-perkey").unwrap();
    assert!(lit.capabilities.contains(&Capability::PerKeyLighting));
    let backend = lit.lighting.as_ref().unwrap();
    assert!(backend.led_map_overlay && backend.start_effect == Some(4));
    assert_eq!(
        lit.zmk.as_ref().unwrap().revision.len(),
        40,
        "pinned to a commit"
    );

    let profile = imprint.profile("cyboard-zmk-0.3").unwrap();
    assert_eq!(profile.zmk.as_ref().unwrap().revision, "v0.3.0");
    assert!(profile.capabilities.contains(&Capability::Studio));
    assert!(!profile.capabilities.contains(&Capability::PerKeyLighting));
    assert_eq!(profile.builds[0].shield.as_deref(), Some("imprint_left"));
}

#[test]
fn go60_layout_and_binding_order() {
    let go60 = board("moergo-go60");
    let keys = &go60.layout("physical_layout0").unwrap().keys;
    assert_eq!(keys.len(), 60);
    assert_keys_distinct(&go60);
    // Bottom rows come before the thumbs, left hand first.
    assert!(keys[48..=50].iter().all(|k| k.y == 400 && k.x < 600));
    assert!(keys[51..=53].iter().all(|k| k.y == 400 && k.x > 1100));
    // Thumbs: left T1..T3 turn clockwise, right T3..T1 counterclockwise.
    assert!(keys[54..=56].iter().all(|k| k.rot > 0));
    assert!(keys[57..=59].iter().all(|k| k.rot < 0));
    assert_eq!(keys[57].rot, -3700);
}

#[test]
fn go60_halves_and_firmware() {
    let go60 = board("moergo-go60");
    assert_eq!(go60.brightness_cap, 40);
    assert_eq!(go60.flash.order, [Side::Right, Side::Left]);
    assert_eq!(
        go60.half(Side::Right).unwrap().bootloader_volume,
        "GO60RHBOOT"
    );
    assert_every_key_lit(&go60);

    let profile = go60.profile("moergo-zmk-26.09").unwrap();
    assert!(profile.capabilities.contains(&Capability::RgbStatus));
    assert_eq!(profile.builds.len(), 2);
    assert!(profile.lighting.is_none());

    // The community lighting firmware is a second, opt-in profile.
    let lit = go60.profile("moergo-zmk-perkey").unwrap();
    assert!(lit.capabilities.contains(&Capability::PerKeyLighting));
    assert!(!lit.capabilities.contains(&Capability::Studio));
    assert_eq!(
        lit.zmk.as_ref().unwrap().revision.len(),
        40,
        "pinned to a commit"
    );
    assert!(lit
        .lighting
        .as_ref()
        .is_some_and(|l| l.transparent && l.start_effect.is_none()));
}

#[test]
fn validation_rejects_broken_definitions() {
    use kc_boards::BoardError;

    let src = include_str!("../boards/moergo-go60.toml");
    let two_centrals = src.replace("central = false", "central = true");
    assert_eq!(
        Board::from_toml(&two_centrals),
        Err(BoardError::CentralCount(2))
    );

    let bad_default = src.replace(
        "default_layout = \"physical_layout0\"",
        "default_layout = \"missing\"",
    );
    assert!(matches!(
        Board::from_toml(&bad_default),
        Err(BoardError::UnknownLayout {
            field: "default_layout",
            ..
        })
    ));

    let bad_led = src.replace("  54, 55, 56, 5,", "  54, 55, 60, 5,");
    assert!(matches!(
        Board::from_toml(&bad_led),
        Err(BoardError::LedOutOfRange { position: 60, .. })
    ));

    let unbacked = src.replace("lighting = { transparent = true }\n", "");
    assert_eq!(
        Board::from_toml(&unbacked),
        Err(BoardError::LightingMismatch("moergo-zmk-perkey".into()))
    );

    let too_bright = src.replace("brightness_cap = 40", "brightness_cap = 140");
    assert_eq!(
        Board::from_toml(&too_bright),
        Err(BoardError::BrightnessCap(140))
    );
}

#[test]
fn every_firmware_belongs_to_a_family_with_base_features() {
    use kc_boards::{Delivery, Family};

    for board in kc_boards::built_in().unwrap() {
        assert!(!board.families().is_empty(), "{}", board.id);
        for profile in &board.firmware {
            let features = profile.features();
            // A profile's own capabilities sit on top of its family's.
            for feature in profile.family.base_features() {
                assert!(features.contains(feature), "{}", profile.id);
            }
            for feature in &profile.capabilities {
                assert!(features.contains(feature), "{}", profile.id);
            }
        }
    }
    assert_eq!(Family::Zmk.delivery(), Delivery::Build);
    assert_eq!(Family::Rmk.delivery(), Delivery::Build);
    assert_eq!(Family::Dygma.delivery(), Delivery::Live);
    // ZMK is built from devicetree and has combos; Dygma has neither.
    assert!(Family::Zmk.base_features().contains(&Capability::Combos));
    assert!(!Family::Dygma.base_features().contains(&Capability::Combos));
    assert!(!Family::Dygma
        .base_features()
        .contains(&Capability::Devicetree));
}

#[test]
fn glove80_layout_and_binding_order() {
    let glove80 = board("moergo-glove80");
    let keys = &glove80.layout("physical_layout0").unwrap().keys;
    assert_eq!(keys.len(), 80);
    assert_keys_distinct(&glove80);
    // Five finger rows of ten or twelve keys, then the bottom row with the
    // thumb fans between the hands, then the lowest row with the rest.
    assert_eq!(keys[0].y, 50);
    assert!(keys[46..=51]
        .iter()
        .all(|k| (k.y == 400 || k.y == 450) && k.rot == 0 && k.x < 600));
    assert!(keys[52..=54].iter().all(|k| k.rot > 0 && k.rx == 450));
    assert!(keys[55..=57].iter().all(|k| k.rot < 0 && k.rx == 1350));
    assert!(keys[69..=71].iter().all(|k| k.rot > 0 && k.y == 550));
    assert!(keys[72..=74].iter().all(|k| k.rot < 0 && k.y == 550));
    assert!(keys[75..=79].iter().all(|k| k.rot == 0 && k.x >= 1300));
    assert_eq!(glove80.starter_keys.len(), 80);
    assert_eq!(glove80.starter_keys[23], "Q");
    assert_eq!(glove80.starter_keys[64], "");
}

#[test]
fn glove80_halves_and_firmware() {
    let glove80 = board("moergo-glove80");
    assert_eq!(glove80.brightness_cap, 80);
    assert_eq!(glove80.flash.order, [Side::Right, Side::Left]);
    assert_eq!(
        glove80.half(Side::Left).unwrap().bootloader_volume,
        "GLV80LHBOOT"
    );
    assert_eq!(
        glove80.half(Side::Right).unwrap().bootloader_volume,
        "GLV80RHBOOT"
    );
    assert!(glove80.half(Side::Left).unwrap().central);
    assert_every_key_lit(&glove80);
    // The chains start at the thumb fans.
    let left = glove80.half(Side::Left).unwrap().leds.as_ref().unwrap();
    assert_eq!(&left.chain[..3], &[52, 53, 54]);
    assert_eq!(left.chain.len(), 40);

    let profile = glove80.profile("moergo-zmk-26.09").unwrap();
    assert!(profile.capabilities.contains(&Capability::Studio));
    assert!(profile.capabilities.contains(&Capability::Pointing));
    assert_eq!(profile.builds.len(), 2);
    assert_eq!(profile.builds[0].board, "glove80_lh");
    assert_eq!(profile.config_name, "glove80");

    let lit = glove80.profile("moergo-zmk-perkey").unwrap();
    assert!(lit.capabilities.contains(&Capability::PerKeyLighting));
    assert!(!lit.capabilities.contains(&Capability::Studio));
    assert_eq!(lit.zmk.as_ref().unwrap().revision.len(), 40);
    assert!(lit.lighting.as_ref().is_some_and(|l| l.transparent));

    // moergo-rmk, delivered from its release and configured live.
    let rmk = glove80.profile("moergo-rmk").unwrap();
    assert_eq!(rmk.delivery(), Delivery::Released);
    let data = rmk.rmk.as_ref().unwrap();
    assert_eq!(data.ble_profiles, 4);
    let matrix = data.matrix.as_ref().unwrap();
    assert_eq!((matrix.rows, matrix.cols), (6, 14));
    assert_eq!(matrix.positions.len(), 80);
    // The left thumb fan is matrix column 6, read down from row 0.
    assert_eq!(matrix.position(52), Some([0, 6]));
    assert_eq!(matrix.position(69), Some([3, 6]));
    assert_eq!(matrix.key_at(0, 5), None, "no key in that cell");
    let release = data.release.as_ref().unwrap();
    assert!(release.asset.is_none(), "each UF2 is an asset of its own");
    assert_eq!(release.files.len(), 2);
    assert_eq!(
        data.leds.iter().map(|l| l.first).collect::<Vec<_>>(),
        [0, 40]
    );
    assert!(data.pointing.is_empty());
    // The right half's LEDs follow the left's in the firmware's numbering.
    assert_eq!(rmk.led_of(&glove80, 57), Some(40));
    assert_eq!(rmk.key_of_led(&glove80, 79), Some(79));
}
