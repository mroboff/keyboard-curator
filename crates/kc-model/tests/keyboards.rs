//! The user's saved keyboards: firmware, devices and layouts.

use std::path::PathBuf;

use kc_boards::Board;
use kc_model::features::{KeyLight, Rgb, SettingValue};
use kc_model::keyboards::{hidden, KeyboardError, KeyboardsFileError, Placement};
use kc_model::{file, validate, Carried, Device, FirmwareConfig, Keyboards, Project, Severity};
use kc_zmk::Feature;

const GO60_STOCK: &str = "moergo-zmk-26.09";
const GO60_PERKEY: &str = "moergo-zmk-perkey";

fn board(id: &str) -> Board {
    kc_boards::built_in()
        .unwrap()
        .into_iter()
        .find(|b| b.id == id)
        .unwrap()
}

fn device(serial: &str) -> Device {
    Device {
        vendor: 0x16c0,
        product: 0x27db,
        serial: serial.into(),
    }
}

#[test]
fn keyboards_are_added_named_and_removed() {
    let go60 = board("moergo-go60");
    let mut keyboards = Keyboards::default();
    assert!(keyboards.is_empty());

    assert_eq!(keyboards.suggest_name(&go60), "My Go60");
    let first = keyboards.add("  My Go60 ", &go60, GO60_STOCK).unwrap();
    assert_eq!(keyboards.get(first).unwrap().name, "My Go60");
    // A new keyboard has its firmware's default settings and no layouts.
    let added = keyboards.get(first).unwrap();
    assert_eq!(added.firmware, FirmwareConfig::new(GO60_STOCK));
    assert!(added.layouts.is_empty() && added.current.is_none());
    assert_eq!(keyboards.suggest_name(&go60), "My Go60 2");
    let second = keyboards.add("Travel", &go60, GO60_PERKEY).unwrap();
    assert_ne!(first, second);

    assert_eq!(
        keyboards.add(" ", &go60, GO60_STOCK),
        Err(KeyboardError::EmptyName)
    );
    assert!(matches!(
        keyboards.add("Odd", &go60, "cyboard-zmk-0.3"),
        Err(KeyboardError::NoSuchFirmware { .. })
    ));

    keyboards.rename(second, "Office").unwrap();
    assert_eq!(keyboards.rename(second, ""), Err(KeyboardError::EmptyName));
    assert_eq!(keyboards.get(second).unwrap().name, "Office");

    keyboards.remove(first).unwrap();
    assert!(keyboards.get(first).is_none());
    assert_eq!(
        keyboards.rename(first, "Gone"),
        Err(KeyboardError::NoSuchKeyboard)
    );
    // A removed keyboard's ID is never handed out again.
    let third = keyboards.add("Third", &go60, GO60_STOCK).unwrap();
    assert!(third != first && third != second);
}

#[test]
fn firmware_and_its_settings_belong_to_the_keyboard() {
    let go60 = board("moergo-go60");
    let imprint = board("cyboard-imprint");
    let mut keyboards = Keyboards::default();
    let id = keyboards.add("Desk", &go60, GO60_STOCK).unwrap();

    keyboards
        .set_setting(id, "CONFIG_ZMK_SLEEP", Some(SettingValue::Bool(true)))
        .unwrap();
    keyboards
        .set_setting(
            id,
            "CONFIG_ZMK_STUDIO_LOCKING",
            Some(SettingValue::Bool(false)),
        )
        .unwrap();
    keyboards.set_raw_conf(id, "CONFIG_FOO=y").unwrap();

    // Changing the firmware keeps every setting, including one the new
    // firmware lacks, which is simply not offered until it comes back.
    keyboards.set_firmware(id, &go60, GO60_PERKEY).unwrap();
    let firmware = &keyboards.get(id).unwrap().firmware;
    assert_eq!(firmware.profile, GO60_PERKEY);
    assert_eq!(firmware.settings.len(), 2);
    assert_eq!(firmware.raw_conf, "CONFIG_FOO=y");
    assert!(firmware.offers("CONFIG_ZMK_SLEEP", &go60));
    assert!(!firmware.offers("CONFIG_ZMK_STUDIO_LOCKING", &go60));
    assert!(!firmware.features(&go60).contains(&Feature::Studio));

    assert!(matches!(
        keyboards.set_firmware(id, &imprint, "cyboard-zmk-0.3"),
        Err(KeyboardError::NoSuchFirmware { .. })
    ));
    assert!(matches!(
        keyboards.set_firmware(id, &go60, "nope"),
        Err(KeyboardError::NoSuchFirmware { .. })
    ));
    assert_eq!(keyboards.get(id).unwrap().firmware.profile, GO60_PERKEY);

    // Clearing a setting returns it to the board's default.
    keyboards.set_setting(id, "CONFIG_ZMK_SLEEP", None).unwrap();
    assert!(!keyboards
        .get(id)
        .unwrap()
        .firmware
        .settings
        .contains_key("CONFIG_ZMK_SLEEP"));
}

#[test]
fn settings_that_came_with_a_layout_can_be_taken_on() {
    let go60 = board("moergo-go60");
    let mut keyboards = Keyboards::default();
    let id = keyboards.add("Desk", &go60, GO60_STOCK).unwrap();
    keyboards
        .set_setting(id, "CONFIG_ZMK_SLEEP", Some(SettingValue::Bool(false)))
        .unwrap();
    keyboards.set_raw_conf(id, "CONFIG_FOO=y").unwrap();

    let carried = Carried {
        settings: [
            ("CONFIG_ZMK_SLEEP".to_string(), SettingValue::Bool(true)),
            (
                "CONFIG_ZMK_IDLE_SLEEP_TIMEOUT".to_string(),
                SettingValue::Int(900_000),
            ),
        ]
        .into(),
        raw_conf: "CONFIG_BAR=y\n".into(),
    };
    assert!(!carried.is_empty());
    assert!(carried.summary().ends_with("1 custom line"));
    assert!(Carried::default().is_empty());

    keyboards.absorb(id, carried.clone()).unwrap();
    let firmware = &keyboards.get(id).unwrap().firmware;
    // What the layout carried wins where both set the same option.
    assert_eq!(
        firmware.settings["CONFIG_ZMK_SLEEP"],
        SettingValue::Bool(true)
    );
    assert_eq!(firmware.settings.len(), 2);
    assert_eq!(firmware.raw_conf, "CONFIG_FOO=y\nCONFIG_BAR=y\n");

    // Taking the same lines on twice does not repeat them.
    keyboards.absorb(id, carried).unwrap();
    assert_eq!(
        keyboards.get(id).unwrap().firmware.raw_conf,
        "CONFIG_FOO=y\nCONFIG_BAR=y\n"
    );
}

#[test]
fn a_device_is_linked_to_one_keyboard_at_a_time() {
    let go60 = board("moergo-go60");
    let mut keyboards = Keyboards::default();
    let desk = keyboards.add("Desk", &go60, GO60_STOCK).unwrap();
    let travel = keyboards.add("Travel", &go60, GO60_STOCK).unwrap();

    // A keyboard needs no device at all.
    assert!(keyboards.get(desk).unwrap().device.is_none());

    keyboards.link(desk, device("AAA")).unwrap();
    assert_eq!(keyboards.linked_to(&device("AAA")).unwrap().id, desk);
    // Linking the same device again is harmless.
    keyboards.link(desk, device("AAA")).unwrap();

    assert_eq!(
        keyboards.link(travel, device("AAA")),
        Err(KeyboardError::DeviceTaken { by: desk })
    );
    assert!(keyboards.get(travel).unwrap().device.is_none());

    // Taking the device over unlinks it from the other keyboard.
    assert_eq!(keyboards.relink(travel, device("AAA")), Ok(Some(desk)));
    assert!(keyboards.get(desk).unwrap().device.is_none());
    assert_eq!(keyboards.linked_to(&device("AAA")).unwrap().id, travel);
    assert_eq!(keyboards.relink(travel, device("AAA")), Ok(None));

    // The same serial from another product is another device.
    let other = Device {
        product: 1,
        ..device("AAA")
    };
    keyboards.link(desk, other).unwrap();

    keyboards.unlink(travel).unwrap();
    assert!(keyboards.linked_to(&device("AAA")).is_none());
}

#[test]
fn each_keyboard_lists_its_layouts_and_has_one_current() {
    let go60 = board("moergo-go60");
    let mut keyboards = Keyboards::default();
    let desk = keyboards.add("Desk", &go60, GO60_STOCK).unwrap();
    let travel = keyboards.add("Travel", &go60, GO60_STOCK).unwrap();
    let path = |name: &str| PathBuf::from(format!("/layouts/{name}.kcproj"));

    for name in ["a", "b", "c"] {
        keyboards.note_layout(desk, path(name)).unwrap();
    }
    // Using one again moves it to the front without listing it twice.
    keyboards.note_layout(desk, path("a")).unwrap();
    assert_eq!(
        keyboards.get(desk).unwrap().layouts,
        [path("a"), path("c"), path("b")]
    );
    assert!(keyboards.get(travel).unwrap().layouts.is_empty());

    // The same file can be a layout of two keyboards.
    keyboards.set_current(travel, Some(path("a"))).unwrap();
    assert_eq!(keyboards.get(travel).unwrap().layouts, [path("a")]);
    assert_eq!(keyboards.get(travel).unwrap().current, Some(path("a")));
    assert_eq!(keyboards.get(desk).unwrap().current, None);

    // Making a listed layout current does not reorder the list.
    keyboards.set_current(desk, Some(path("b"))).unwrap();
    assert_eq!(keyboards.get(desk).unwrap().layouts.len(), 3);
    keyboards.forget_layout(desk, &path("c")).unwrap();
    assert_eq!(keyboards.get(desk).unwrap().current, Some(path("b")));
    // Forgetting the current layout leaves the keyboard on the factory one.
    keyboards.forget_layout(desk, &path("b")).unwrap();
    assert_eq!(keyboards.get(desk).unwrap().current, None);
    assert_eq!(keyboards.get(desk).unwrap().layouts, [path("a")]);

    keyboards.set_current(travel, None).unwrap();
    assert_eq!(keyboards.get(travel).unwrap().current, None);
    assert_eq!(keyboards.get(travel).unwrap().layouts, [path("a")]);
}

#[test]
fn the_keyboards_file_round_trips_and_is_repaired() {
    let go60 = board("moergo-go60");
    let mut keyboards = Keyboards::default();
    let desk = keyboards.add("Desk", &go60, GO60_STOCK).unwrap();
    keyboards.link(desk, device("AAA")).unwrap();
    keyboards
        .set_repo_dir(desk, Some(PathBuf::from("/repos/go60")))
        .unwrap();
    keyboards
        .set_setting(desk, "CONFIG_ZMK_SLEEP", Some(SettingValue::Bool(true)))
        .unwrap();
    keyboards
        .set_current(desk, Some(PathBuf::from("/p/a")))
        .unwrap();

    let text = keyboards.to_json();
    assert_eq!(Keyboards::from_json(&text).unwrap(), keyboards);

    let dir = std::env::temp_dir().join(format!("kc-keyboards-{}", std::process::id()));
    let path = dir.join("nested").join("keyboards.json");
    // A missing file is simply no keyboards yet.
    assert!(Keyboards::load(&path).unwrap().is_empty());
    keyboards.save(&path).unwrap();
    assert_eq!(Keyboards::load(&path).unwrap(), keyboards);
    let _ = std::fs::remove_dir_all(dir);

    // A file edited by hand so that two keyboards claim one device, with a
    // counter that would reuse an ID: the first keyboard keeps the device.
    let damaged = r#"{
        "format": 2,
        "keyboards": [
            {"id": 1, "name": "A", "board": "moergo-go60",
             "firmware": {"profile": "moergo-zmk-26.09"},
             "device": {"vendor": 5824, "product": 10203, "serial": "AAA"}, "repo_dir": null},
            {"id": 4, "name": "B", "board": "moergo-go60",
             "firmware": {"profile": "moergo-zmk-26.09"},
             "device": {"vendor": 5824, "product": 10203, "serial": "AAA"}, "repo_dir": null}
        ],
        "next_id": 2
    }"#;
    let mut repaired = Keyboards::from_json(damaged).unwrap();
    let devices: Vec<bool> = repaired.iter().map(|k| k.device.is_some()).collect();
    assert_eq!(devices, [true, false]);
    let added = repaired.add("C", &go60, GO60_STOCK).unwrap();
    assert_eq!(added.0, 5);

    assert!(matches!(
        Keyboards::from_json(r#"{"format": 99, "keyboards": [], "next_id": 1}"#),
        Err(KeyboardsFileError::Newer { found: 99, .. })
    ));
    assert!(matches!(
        Keyboards::from_json("not json"),
        Err(KeyboardsFileError::Invalid(_))
    ));
}

#[test]
fn keyboards_saved_by_the_first_format_are_brought_forward() {
    // Format 1 held only the firmware's ID, and listed recent projects.
    let old = r#"{
        "format": 1,
        "keyboards": [
            {"id": 1, "name": "My Go60", "board": "moergo-go60",
             "firmware": "moergo-zmk-perkey", "device": null, "repo_dir": null,
             "recent": ["/p/a.kcproj"]}
        ],
        "next_id": 2
    }"#;
    let keyboards = Keyboards::from_json(old).unwrap();
    let keyboard = keyboards.iter().next().unwrap();
    assert_eq!(keyboard.firmware, FirmwareConfig::new(GO60_PERKEY));
    assert_eq!(keyboard.layouts, [PathBuf::from("/p/a.kcproj")]);
    assert_eq!(keyboard.current, None);
    // It is written back in the current format.
    assert!(keyboards.to_json().contains("\"format\": 2"));
}

#[test]
fn layouts_are_placed_under_a_keyboard_of_their_board() {
    let go60 = board("moergo-go60");
    let imprint = board("cyboard-imprint");
    let project = Project::new("Mine", &go60);

    let mut keyboards = Keyboards::default();
    assert_eq!(keyboards.place(&project, None), Placement::NoKeyboard);

    let other = keyboards
        .add("Imprint", &imprint, "cyboard-zmk-0.3")
        .unwrap();
    assert!(!keyboards.get(other).unwrap().suits(&project));
    assert_eq!(keyboards.place(&project, None), Placement::NoKeyboard);
    assert_eq!(
        keyboards.place(&project, Some(other)),
        Placement::NoKeyboard
    );

    // The only keyboard of the board is used, whatever firmware it runs.
    let lit = keyboards.add("Lit", &go60, GO60_PERKEY).unwrap();
    assert_eq!(keyboards.place(&project, None), Placement::Open(lit));
    assert_eq!(keyboards.place(&project, Some(other)), Placement::Open(lit));

    // With two, the selected one is used; otherwise the user chooses.
    let stock = keyboards.add("Stock", &go60, GO60_STOCK).unwrap();
    assert_eq!(
        keyboards.place(&project, Some(stock)),
        Placement::Open(stock)
    );
    assert_eq!(keyboards.place(&project, Some(lit)), Placement::Open(lit));
    assert_eq!(
        keyboards.place(&project, None),
        Placement::Choose(vec![lit, stock])
    );
    assert_eq!(
        keyboards.place(&project, Some(other)),
        Placement::Choose(vec![lit, stock])
    );

    // A selection that no longer exists is ignored.
    keyboards.remove(stock).unwrap();
    assert_eq!(keyboards.place(&project, Some(stock)), Placement::Open(lit));
}

#[test]
fn a_layout_keeps_what_a_firmware_lacks_out_of_sight() {
    let go60 = board("moergo-go60");
    let mut project = Project::new("Lit", &go60);
    let base = project.layers[0].id;
    project.lighting_mut(base).unwrap().keys[0] = KeyLight::Color(Rgb(255, 0, 0));

    let stock = FirmwareConfig::new(GO60_STOCK);
    let lit = FirmwareConfig::new(GO60_PERKEY);
    // The same layout is valid with either firmware.
    for config in [&stock, &lit] {
        assert!(validate(&project, &go60, config)
            .iter()
            .all(|p| p.severity != Severity::Error));
    }

    let with_stock = hidden(&project, stock.features(&go60));
    assert!(with_stock.lighting && !with_stock.pointing);
    assert_eq!(
        with_stock.summary().unwrap(),
        "Per-key colors in this layout are hidden, because this board's firmware does not have the feature. Nothing is removed from the file."
    );
    let with_lit = hidden(&project, lit.features(&go60));
    assert!(with_lit.is_empty());
    assert_eq!(with_lit.summary(), None);

    let imprint = board("cyboard-imprint");
    let pointing = Project::from_template("Mine", &imprint);
    assert!(!pointing.pointing.is_empty());
    assert!(hidden(&pointing, &[Feature::Pointing]).is_empty());
    assert!(hidden(&pointing, &[]).pointing);
}

#[test]
fn a_layout_file_from_before_the_split_hands_its_settings_back() {
    // Format 1 kept the firmware and its settings in the layout file.
    let go60 = board("moergo-go60");
    let current = file::to_json(&Project::new("Mine", &go60));
    let mut old: serde_json::Value = serde_json::from_str(&current).unwrap();
    old["format"] = 1.into();
    old["firmware"] = "moergo-zmk-perkey".into();
    old["settings"] = serde_json::json!({"CONFIG_ZMK_SLEEP": true});
    old["raw"]["conf"] = "CONFIG_FOO=y".into();

    let (project, carried) = file::from_json_carrying(&old.to_string()).unwrap();
    assert_eq!(project, Project::new("Mine", &go60));
    assert_eq!(
        carried.settings["CONFIG_ZMK_SLEEP"],
        SettingValue::Bool(true)
    );
    assert_eq!(carried.raw_conf, "CONFIG_FOO=y");

    // A current file carries nothing, and holds nothing about firmware.
    let (again, nothing) = file::from_json_carrying(&current).unwrap();
    assert_eq!(again, project);
    assert!(nothing.is_empty());
    assert!(!current.contains("firmware") && !current.contains("settings"));
    assert!(current.contains("\"format\": 2"));
}
