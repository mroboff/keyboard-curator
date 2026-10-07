//! Layouts as RMK keymaps, and the repositories that build them.

use kc_boards::board::{FirmwareProfile, RmkProfile};
use kc_boards::Board;
use kc_model::{Binding, FirmwareConfig, KeyExpr, LayerId, Param, Project, Severity};
use kc_rmk::{action, check, check_keymap, expressible, generate};
use kc_zmk::Modifier;

fn board(id: &str) -> (Board, FirmwareProfile, RmkProfile) {
    let board = kc_boards::built_in()
        .unwrap()
        .into_iter()
        .find(|b| b.id == id)
        .unwrap();
    let profile = board
        .firmware
        .iter()
        .find(|p| p.rmk.is_some())
        .unwrap()
        .clone();
    let rmk = profile.rmk.clone().unwrap();
    (board, profile, rmk)
}

fn key(name: &str) -> Param {
    Param::Key(KeyExpr::new(name))
}

fn command(label: &str, name: &str, args: &[u32]) -> Binding {
    Binding::new(
        label,
        vec![Param::Command {
            name: name.into(),
            args: args.to_vec(),
        }],
    )
}

#[test]
fn bindings_become_rmk_key_actions() {
    let layers: Vec<LayerId> = (1..=4).map(LayerId).collect();
    let cases: Vec<(Binding, &str)> = vec![
        (Binding::trans(), "_"),
        (Binding::none(), "No"),
        (Binding::kp(KeyExpr::new("A")), "A"),
        (Binding::kp(KeyExpr::new("N1")), "Kc1"),
        (Binding::kp(KeyExpr::new("RET")), "Enter"),
        (Binding::kp(KeyExpr::new("LSHFT")), "LShift"),
        (
            Binding::kp(KeyExpr::new("C").with(Modifier::LCtrl)),
            "WM(C, LCtrl)",
        ),
        (
            Binding::kp(
                KeyExpr::new("TAB")
                    .with(Modifier::LGui)
                    .with(Modifier::LShift),
            ),
            "WM(Tab, LShift | LGui)",
        ),
        // A key whose name means a shifted key carries the shift.
        (Binding::kp(KeyExpr::new("EXCL")), "WM(Kc1, LShift)"),
        (Binding::kp(KeyExpr::new("C_VOL_UP")), "AudioVolUp"),
        (Binding::layer("mo", layers[1]), "MO(1)"),
        (Binding::layer("to", layers[0]), "TO(0)"),
        (Binding::layer("tog", layers[3]), "TG(3)"),
        (Binding::layer("sl", layers[2]), "OSL(2)"),
        (Binding::new("sk", vec![key("LCTRL")]), "OSM(LCtrl)"),
        (
            Binding::new("mt", vec![key("LSHFT"), key("A")]),
            "MT(A, LShift)",
        ),
        (
            Binding::new("lt", vec![Param::Layer(layers[1]), key("SPACE")]),
            "LT(1, Space)",
        ),
        (Binding::new("bootloader", vec![]), "Bootloader"),
        (Binding::new("sys_reset", vec![]), "Reboot"),
        (Binding::new("caps_word", vec![]), "CapsWordToggle"),
        // With five Bluetooth profiles: select, next, previous, clear,
        // then the USB or Bluetooth toggle.
        (command("bt", "BT_SEL", &[2]), "User2"),
        (command("bt", "BT_NXT", &[]), "User5"),
        (command("bt", "BT_PRV", &[]), "User6"),
        (command("bt", "BT_CLR", &[]), "User7"),
        (command("out", "OUT_TOG", &[]), "User8"),
    ];
    for (binding, expected) in &cases {
        assert_eq!(
            action(binding, &layers, 5).as_deref(),
            Ok(*expected),
            "{binding:?}"
        );
    }
}

#[test]
fn what_rmk_lacks_is_refused_with_a_reason() {
    let layers: Vec<LayerId> = (1..=2).map(LayerId).collect();
    let refused = |binding: Binding| action(&binding, &layers, 3).unwrap_err();
    assert!(refused(Binding::Raw {
        raw: "&my_macro".into()
    })
    .contains("devicetree"));
    assert!(refused(command("rgb_ug", "RGB_TOG", &[])).contains("nothing for &rgb_ug"));
    assert!(refused(Binding::new("sk", vec![key("A")])).contains("single modifier"));
    assert!(refused(Binding::layer("mo", LayerId(9))).contains("no longer exists"));
    // A profile the firmware does not keep.
    assert!(refused(command("bt", "BT_SEL", &[4])).contains("keeps 3 Bluetooth profiles"));
}

#[test]
fn the_imprint_gets_a_whole_rmk_project_on_the_vendors_flash_layout() {
    let (imprint, profile, rmk) = board("cyboard-imprint");
    let mut project = Project::new("Mine", &imprint);
    let base = project.layers[0].id;
    let nav = project.add_layer("Nav").unwrap();
    project
        .set_binding(base, 0, Binding::layer("mo", nav))
        .unwrap();
    project.add_combo("Escape", vec![13, 14], Binding::kp(KeyExpr::new("ESC")));
    assert!(check_keymap(&project, &rmk).is_empty());

    let firmware = FirmwareConfig::new(profile.id.clone());
    let files = generate(&project, &imprint, &profile, &firmware).unwrap();
    let file = |path: &str| {
        files
            .iter()
            .find(|(p, _)| p == path)
            .unwrap_or_else(|| panic!("no {path}"))
            .1
            .clone()
    };
    let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        [
            "keyboard.toml",
            "Cargo.toml",
            ".cargo/config.toml",
            "memory.x",
            "build.rs",
            "src/central.rs",
            "src/peripheral.rs",
            "tools/uf2.py",
            ".github/workflows/build.yml",
        ]
    );

    // keyboard.toml is valid TOML holding the hardware and the keymap.
    let config: toml::Table = toml::from_str(&file("keyboard.toml")).unwrap();
    assert_eq!(config["keyboard"]["chip"].as_str(), Some("nrf52840"));
    assert_eq!(config["layout"]["rows"].as_integer(), Some(14));
    assert_eq!(
        config["split"]["central"]["matrix"]["row2col"].as_bool(),
        Some(true)
    );
    assert_eq!(config["host"]["vial_enabled"].as_bool(), Some(false));
    assert_eq!(config["keymap"]["layers"].as_integer(), Some(2));
    let layers = config["keymap"]["layer"].as_array().unwrap();
    assert_eq!(layers.len(), 2);
    // One action per key: 82 on the Imprint. No action on this layer has
    // a space inside it but the layer key, which has none either.
    let first = layers[0]["keys"].as_str().unwrap();
    assert_eq!(first.split_whitespace().count(), 82);
    assert!(first.trim_start().starts_with("MO(1)"));
    // The layout's map names the same number of keys.
    let map = config["layout"]["map"].as_str().unwrap();
    assert_eq!(map.matches('(').count(), 82);
    // A combo is given by what its keys do on the base layer.
    let combos = config["behavior"]["combo"]["combos"].as_array().unwrap();
    assert_eq!(combos.len(), 1);
    assert_eq!(combos[0]["output"].as_str(), Some("Escape"));
    assert_eq!(combos[0]["actions"].as_array().unwrap().len(), 2);
    // The LED power rail is held off: this firmware drives no LEDs.
    assert_eq!(
        config["split"]["central"]["output"][0]["initial_state_active"].as_bool(),
        Some(false)
    );

    // Linked where Cyboard's firmware starts, and short of its settings.
    let memory = file("memory.x");
    assert!(memory.contains("FLASH : ORIGIN = 0x00026000, LENGTH = 0x000c6000"));
    assert!(rmk.flash_origin + rmk.flash_length <= 0xEC000);
    // RMK is pinned to a release.
    assert!(file("Cargo.toml")
        .contains("rmk = { git = \"https://github.com/rmk-rs/rmk\", tag = \"rmk-v0.9.0\""));
    // The left half is the central one; each half gets its own file.
    let workflow = file(".github/workflows/build.yml");
    assert!(workflow.contains(
        "python3 tools/uf2.py central.bin imprint_left.uf2 --base 0x26000 --family 0xada52840"
    ));
    assert!(workflow.contains("python3 tools/uf2.py peripheral.bin imprint_right.uf2"));
}

#[test]
fn the_go60_gets_moergo_rmks_runtime_configuration_instead_of_a_build() {
    let (go60, profile, rmk) = board("moergo-go60");
    let firmware = FirmwareConfig::new(profile.id.clone());
    let project = Project::new("Mine", &go60);
    let files = generate(&project, &go60, &profile, &firmware).unwrap();
    let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, ["go60.toml"]);
    // The file is one moergo-rmk's own tools read.
    let config = kc_rmk::moergo_config::RuntimeConfig::from_toml(&files[0].1).unwrap();
    assert_eq!((config.rows, config.cols), (5, 14));
    assert_eq!(config.layers.len(), 1);
    let keys = &config.layers[0].keys;
    // Five rows of fourteen cells: sixty keys and ten holes.
    assert_eq!(keys.split_whitespace().count(), 70);
    assert_eq!(keys.split_whitespace().filter(|c| *c == "--").count(), 10);
    assert!(keys.contains("KC_EQL KC_1 KC_2"));
    // The firmware comes from a pinned release, built from a pinned commit
    // with a pinned RMK, which the app's protocol client matches.
    assert_eq!(rmk.source.revision.len(), 40);
    assert_eq!(rmk.rmk_revision.len(), 40);
    let release = rmk.release.as_ref().unwrap();
    assert_eq!(release.repository, "colonelpanic8/moergo-rmk");
    assert_eq!(release.files.len(), 2);
    // Sixteen layers, not seventeen.
    let mut tall = Project::new("Tall", &go60);
    for number in 2..=17 {
        tall.add_layer(format!("Layer {number}")).unwrap();
    }
    assert!(check(&tall, &go60, &profile, &firmware)
        .iter()
        .any(|p| p.message.contains("17 layers, and this firmware holds 16")));
}

#[test]
fn a_layout_rmk_cannot_hold_is_reported_and_not_generated() {
    let (imprint, profile, rmk) = board("cyboard-imprint");
    // The factory layout leans on behaviors defined in the layout, which
    // are not translated yet.
    let factory = Project::from_template("Factory", &imprint);
    let problems = check_keymap(&factory, &rmk);
    assert!(!problems.is_empty());
    assert!(problems.iter().all(|p| p.severity == Severity::Error));
    let firmware = FirmwareConfig::new(profile.id.clone());
    assert!(generate(&factory, &imprint, &profile, &firmware).is_err());

    let mut project = Project::new("Mine", &imprint);
    let base = project.layers[0].id;
    assert!(expressible(&Binding::kp(KeyExpr::new("A")), &project, &rmk));
    let lighting = command("rgb_ug", "RGB_TOG", &[]);
    assert!(!expressible(&lighting, &project, &rmk));
    project.set_binding(base, 0, lighting).unwrap();
    // A combo over a key that does nothing has no way to be named.
    project.add_combo("Bad", vec![60, 61], Binding::kp(KeyExpr::new("ESC")));
    let problems = check_keymap(&project, &rmk);
    assert!(problems
        .iter()
        .any(|p| p.message.contains("nothing for &rgb_ug")));
    assert!(problems
        .iter()
        .any(|p| p.message.contains("must each do something")));
}
