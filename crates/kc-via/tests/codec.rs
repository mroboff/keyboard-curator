//! Bindings to and from QMK keycodes, macros to and from VIA's buffer,
//! and a layout to and from what a keyboard holds.

use std::collections::BTreeMap;

use kc_model::behavior::{BehaviorKind, Macro, MacroStep};
use kc_model::{Binding, KeyExpr, LayerId, Param};
use kc_via::codec::{decode, encode, kept, Lights};
use kc_via::macros::{self, Action};
use kc_via::{layout, Image, Matrix};

fn kp(key: &str) -> Binding {
    Binding::kp(key.parse::<KeyExpr>().unwrap())
}

fn command(behavior: &str, name: &str) -> Binding {
    Binding::new(
        behavior,
        vec![Param::Command {
            name: name.into(),
            args: Vec::new(),
        }],
    )
}

fn layers() -> Vec<LayerId> {
    (1..=3).map(LayerId).collect()
}

#[test]
fn bindings_round_trip_through_qmk_keycodes() {
    let layers = layers();
    let underglow = Lights { rgb_matrix: false };
    let matrix = Lights { rgb_matrix: true };
    let cases: Vec<(Binding, u16)> = vec![
        (Binding::none(), 0x0000),
        (Binding::trans(), 0x0001),
        (kp("A"), 0x0004),
        (kp("LS(A)"), 0x0204),
        (kp("LC(LS(A))"), 0x0304),
        (kp("RS(A)"), 0x1204),
        (kp("LA(LG(TAB))"), 0x0C2B),
        (kp("EXCL"), 0x021E),
        (kp("LSHFT"), 0x00E1),
        (kp("C_MUTE"), 0x00A8),
        (kp("C_VOL_UP"), 0x00A9),
        (kp("C_PP"), 0x00AE),
        (Binding::layer("mo", LayerId(2)), 0x5221),
        (Binding::layer("tog", LayerId(2)), 0x5261),
        (Binding::layer("to", LayerId(1)), 0x5200),
        (Binding::layer("sl", LayerId(3)), 0x5282),
        (
            Binding::new("sk", vec![Param::Key(KeyExpr::new("LSHFT"))]),
            0x52A2,
        ),
        (
            Binding::new("sk", vec![Param::Key(KeyExpr::new("RGUI"))]),
            0x52B8,
        ),
        (
            Binding::new(
                "mt",
                vec![
                    Param::Key(KeyExpr::new("LCTRL")),
                    Param::Key(KeyExpr::new("A")),
                ],
            ),
            0x2104,
        ),
        (
            Binding::new(
                "lt",
                vec![Param::Layer(LayerId(2)), Param::Key(KeyExpr::new("SPACE"))],
            ),
            0x412C,
        ),
        (
            Binding::new("mkp", vec![Param::Constant("LCLK".into())]),
            0x00D1,
        ),
        (
            Binding::new("mmv", vec![Param::Constant("MOVE_UP".into())]),
            0x00CD,
        ),
        (
            Binding::new("msc", vec![Param::Constant("SCRL_DOWN".into())]),
            0x00DA,
        ),
        (command("bl", "BL_TOG"), 0x7802),
        (command("bl", "BL_INC"), 0x7804),
        (command("rgb_ug", "RGB_TOG"), 0x7820),
        (command("rgb_ug", "RGB_HUI"), 0x7823),
        (Binding::new("bootloader", vec![]), 0x7C00),
        (Binding::new("sys_reset", vec![]), 0x7C01),
        (Binding::new("caps_word", vec![]), 0x7C73),
    ];
    for (binding, code) in &cases {
        assert_eq!(
            encode(binding, &layers, underglow),
            Ok(*code),
            "{binding:?}"
        );
        assert_eq!(decode(*code, &layers, &[]), *binding, "{code:#06x}");
    }
    // A keyboard with an RGB matrix takes the matrix keys instead.
    assert_eq!(
        encode(&command("rgb_ug", "RGB_TOG"), &layers, matrix),
        Ok(0x7842)
    );
    assert_eq!(
        encode(&command("rgb_ug", "RGB_ON"), &layers, matrix),
        Ok(0x7840)
    );
    assert!(encode(&command("rgb_ug", "RGB_ON"), &layers, underglow).is_err());
    assert_eq!(decode(0x7843, &layers, &[]), command("rgb_ug", "RGB_EFF"));
}

#[test]
fn what_qmk_cannot_hold_is_refused_and_what_the_app_cannot_name_is_kept() {
    let layers = layers();
    let lights = Lights::default();
    let refused = [
        kp("LC(RS(A))"),
        kp("LS(C_MUTE)"),
        Binding::new(
            "mt",
            vec![Param::Key(KeyExpr::new("A")), Param::Key(KeyExpr::new("B"))],
        ),
        Binding::new(
            "mt",
            vec![
                Param::Key(KeyExpr::new("LCTRL")),
                Param::Key(KeyExpr::new("LS(A)")),
            ],
        ),
        Binding::new("sk", vec![Param::Key(KeyExpr::new("A"))]),
        Binding::new("caps_word_x", vec![]),
        Binding::layer("mo", LayerId(9)),
    ];
    for binding in &refused {
        assert!(encode(binding, &layers, lights).is_err(), "{binding:?}");
    }
    let many: Vec<LayerId> = (1..=40).map(LayerId).collect();
    assert!(encode(&Binding::layer("mo", LayerId(33)), &many, lights).is_err());
    assert!(encode(
        &Binding::new(
            "lt",
            vec![Param::Layer(LayerId(17)), Param::Key(KeyExpr::new("A"))]
        ),
        &many,
        lights
    )
    .is_err());

    // Default layer, tap-toggle, a custom keycode, a one-shot of two
    // modifiers: kept with the number, and written back as it was.
    let custom = vec!["Mission Control".to_string()];
    for (code, what) in [
        (0x5240u16, "Default layer 1 #21056"),
        (0x52C1, "Tap-toggle layer 2 #21185"),
        (0x7E00, "Mission Control #32256"),
        (0x7E05, "Custom 6 #32261"),
        (0x52A3, "QMK #21155"),
        (0x7806, "Lighting #30726"),
        (0x7702, "Macro 3 #30466"),
    ] {
        let binding = decode(code, &layers, &custom);
        assert_eq!(binding, Binding::Raw { raw: what.into() }, "{code:#06x}");
        assert_eq!(encode(&binding, &layers, lights), Ok(code));
    }
    assert_eq!(
        kept(0x1234, &[]),
        Binding::Raw {
            raw: "QMK #4660".into()
        }
    );
}

#[test]
fn macros_read_as_steps_and_write_back_as_bytes() {
    let buffer = b"Hi!\x01\x01\x04\x01\x04250|\x01\x02\xE1\x01\x03\xE1\0\x01\x01\x2B\0";
    let parsed = macros::parse(buffer, 3);
    assert_eq!(parsed.len(), 3);
    assert_eq!(
        parsed[0],
        [
            Action::Text(b'H'),
            Action::Text(b'i'),
            Action::Text(b'!'),
            Action::Tap(0x04),
            Action::Delay(250),
            Action::Down(0xE1),
            Action::Up(0xE1),
        ]
    );
    assert_eq!(parsed[1], [Action::Tap(0x2B)]);
    assert!(parsed[2].is_empty());
    assert_eq!(macros::serialize(&parsed[..2]), buffer.to_vec());

    let steps = macros::to_steps(&parsed[0]).unwrap();
    assert_eq!(
        steps,
        [
            MacroStep::Tap(vec![kp("LS(H)")]),
            MacroStep::Tap(vec![kp("I")]),
            MacroStep::Tap(vec![kp("EXCL")]),
            MacroStep::Tap(vec![kp("A")]),
            MacroStep::Pause(250),
            MacroStep::Press(vec![kp("LSHFT")]),
            MacroStep::Release(vec![kp("LSHFT")]),
        ]
    );
    // Written back, a tapped key that types a character is text.
    let mut normalized = parsed[0].clone();
    normalized[3] = Action::Text(b'a');
    assert_eq!(macros::from_steps(&steps).unwrap(), normalized);

    // A key with modifiers that is not a typed character is pressed
    // around; a step VIA cannot do is refused.
    assert_eq!(
        macros::from_steps(&[MacroStep::Tap(vec![kp("LC(A)")])]).unwrap(),
        [Action::Down(0xE0), Action::Tap(0x04), Action::Up(0xE0)]
    );
    assert!(macros::from_steps(&[MacroStep::PauseForRelease]).is_err());
    assert!(macros::from_steps(&[MacroStep::Tap(vec![kp("C_MUTE")])]).is_err());
    assert!(macros::from_steps(&[MacroStep::Tap(vec![Binding::layer("mo", LayerId(1))])]).is_err());
    assert_eq!(macros::code(3), 0x7703);
    assert_eq!(macros::index(0x770F), Some(15));
    assert_eq!(macros::index(0x7710), Some(16));
    assert_eq!(macros::index(0x7780), None);
}

const SMALL: &str = r##"{
  "name": "Curator Six",
  "vendorId": "0x4B43",
  "productId": "0x0006",
  "matrix": { "rows": 2, "cols": 3 },
  "menus": ["qmk_rgb_matrix"],
  "layouts": { "keymap": [["0,0", "0,1", "0,2"], ["1,0", "1,1", "1,2"]] }
}"##;

fn six() -> (kc_boards::Board, kc_boards::board::ViaProfile) {
    let board = kc_via::definition::parse(SMALL).unwrap().board().unwrap();
    let profile = board.firmware[0].via.clone().unwrap();
    (board, profile)
}

fn image(keymap: Vec<u16>) -> Image {
    let mut macros = vec![0u8; 64];
    macros[..4].copy_from_slice(b"yo!\0");
    Image {
        protocol: 0x000D,
        firmware_version: 1,
        keycodes_version: None,
        layers: 2,
        matrix: Matrix { rows: 2, cols: 3 },
        keymap,
        macro_count: 16,
        macros,
        layout_options: 0,
        lighting: BTreeMap::new(),
    }
}

#[test]
fn a_keyboards_keymap_becomes_a_layout_and_a_layout_a_keymap() {
    let (board, profile) = six();
    // Layer 0: A B M0 / LCTRL mo(1) raw; layer 1: all transparent but one.
    let held = image(vec![
        0x0004, 0x0005, 0x7700, 0x00E0, 0x5221, 0x7C7B, //
        0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x7842,
    ]);
    let project = layout::to_project(&held, &board, &profile, "Six").unwrap();
    assert_eq!(project.layers.len(), 2);
    assert_eq!(project.layers[1].name, "Layer 2");
    let base = &project.layers[0].bindings;
    assert_eq!(base[0], kp("A"));
    assert_eq!(base[3], kp("LCTRL"));
    assert_eq!(base[4], Binding::layer("mo", project.layers[1].id));
    assert_eq!(
        base[5],
        Binding::Raw {
            raw: "QMK #31867".into()
        }
    );
    assert_eq!(project.layers[1].bindings[5], command("rgb_ug", "RGB_TOG"));
    let macro_id = base[2].user_behavior().expect("the macro key");
    let BehaviorKind::Macro(Macro { steps, .. }) = &project.behavior(macro_id).unwrap().kind else {
        panic!("a macro behavior")
    };
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[2], MacroStep::Tap(vec![kp("EXCL")]));

    // Written back unchanged, nothing differs.
    let same = layout::with_project(&held, &project, &profile).unwrap();
    assert_eq!(same.keymap, held.keymap);
    assert_eq!(same.macros, held.macros);
    assert!(layout::check(&project, &profile, Some(2)).is_empty());

    // Changed: a new key, a second macro, the first macro moved to a
    // different key. Slots are given in order of first use.
    let mut edited = project.clone();
    let (base_id, upper_id) = (edited.layers[0].id, edited.layers[1].id);
    edited.set_binding(base_id, 0, kp("LS(B)")).unwrap();
    let hello = edited
        .add_behavior(
            "hello",
            "",
            BehaviorKind::Macro(Macro {
                wait_ms: None,
                tap_ms: None,
                params: 0,
                steps: vec![MacroStep::Tap(vec![kp("H")]), MacroStep::Pause(10)],
            }),
        )
        .unwrap();
    edited
        .set_binding(base_id, 1, Binding::user(hello, vec![]))
        .unwrap();
    edited.set_binding(upper_id, 0, kp("C_MUTE")).unwrap();
    let new = layout::with_project(&held, &edited, &profile).unwrap();
    assert_eq!(
        &new.keymap[..6],
        &[0x0205, 0x7700, 0x7701, 0x00E0, 0x5221, 0x7C7B]
    );
    assert_eq!(new.keymap[6], 0x00A8);
    assert_eq!(&new.macros[..11], b"h\x01\x0410|\0yo!\0" as &[u8]);
    assert_eq!(new.macros.len(), 64);

    // Too many layers, a key QMK has nothing for, a macro step VIA cannot
    // do: each is named.
    let mut wrong = edited.clone();
    wrong.add_layer("Three").unwrap();
    assert!(layout::with_project(&held, &wrong, &profile).is_err());
    assert_eq!(layout::check(&wrong, &profile, Some(2)).len(), 1);
    let mut combo = edited.clone();
    combo
        .set_binding(base_id, 2, Binding::layer("mo", LayerId(99)))
        .unwrap();
    let problems = layout::check(&combo, &profile, None);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].message.contains("no longer exists"));
    assert!(!layout::expressible(
        &Binding::layer("mo", LayerId(99)),
        &combo,
        &profile
    ));
    assert!(layout::expressible(&kp("LA(F4)"), &combo, &profile));
    assert_eq!(
        layout::with_project(&held, &combo, &profile)
            .unwrap_err()
            .to_string(),
        "key 3 on layer Base: the layer no longer exists"
    );
}
