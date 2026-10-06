//! The user's saved keyboards, and moving projects between firmwares.

use std::path::PathBuf;

use kc_boards::Board;
use kc_model::features::{KeyLight, Rgb, SettingValue};
use kc_model::keyboards::{
    hidden, preview_retarget, retarget, Fit, KeyboardError, KeyboardsFileError, Placement,
};
use kc_model::{validate, Binding, Device, Editor, Keyboards, Param, Project, Severity};
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
fn firmware_changes_stay_within_the_keyboards_board() {
    let go60 = board("moergo-go60");
    let imprint = board("cyboard-imprint");
    let mut keyboards = Keyboards::default();
    let id = keyboards.add("Desk", &go60, GO60_STOCK).unwrap();

    keyboards.set_firmware(id, &go60, GO60_PERKEY).unwrap();
    assert_eq!(keyboards.get(id).unwrap().firmware, GO60_PERKEY);
    assert!(matches!(
        keyboards.set_firmware(id, &imprint, "cyboard-zmk-0.3"),
        Err(KeyboardError::NoSuchFirmware { .. })
    ));
    assert!(matches!(
        keyboards.set_firmware(id, &go60, "nope"),
        Err(KeyboardError::NoSuchFirmware { .. })
    ));
    assert_eq!(keyboards.get(id).unwrap().firmware, GO60_PERKEY);
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
fn recent_projects_are_kept_per_keyboard() {
    let go60 = board("moergo-go60");
    let mut keyboards = Keyboards::default();
    let desk = keyboards.add("Desk", &go60, GO60_STOCK).unwrap();
    let travel = keyboards.add("Travel", &go60, GO60_STOCK).unwrap();

    for i in 0..12 {
        keyboards
            .note_recent(desk, PathBuf::from(format!("/p/{i}")))
            .unwrap();
    }
    keyboards.note_recent(desk, PathBuf::from("/p/5")).unwrap();
    let recent = &keyboards.get(desk).unwrap().recent;
    assert_eq!(recent.len(), 8);
    assert_eq!(recent[0], PathBuf::from("/p/5"));
    assert_eq!(recent.iter().filter(|p| p.ends_with("5")).count(), 1);
    assert!(keyboards.get(travel).unwrap().recent.is_empty());

    keyboards
        .forget_recent(desk, &PathBuf::from("/p/5"))
        .unwrap();
    assert_eq!(
        keyboards.get(desk).unwrap().recent[0],
        PathBuf::from("/p/11")
    );
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
    keyboards.note_recent(desk, PathBuf::from("/p/a")).unwrap();

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
        "format": 1,
        "keyboards": [
            {"id": 1, "name": "A", "board": "moergo-go60", "firmware": "moergo-zmk-26.09",
             "device": {"vendor": 5824, "product": 10203, "serial": "AAA"}, "repo_dir": null},
            {"id": 4, "name": "B", "board": "moergo-go60", "firmware": "moergo-zmk-26.09",
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
fn projects_fit_keyboards_of_their_board() {
    let go60 = board("moergo-go60");
    let imprint = board("cyboard-imprint");
    let mut keyboards = Keyboards::default();
    let lit = keyboards.add("Lit", &go60, GO60_PERKEY).unwrap();
    let stock = keyboards.add("Stock", &go60, GO60_STOCK).unwrap();
    let other = keyboards
        .add("Imprint", &imprint, "cyboard-zmk-0.3")
        .unwrap();

    let mut project = Project::new("Mine", &go60);
    project.firmware = GO60_STOCK.into();
    assert_eq!(
        keyboards.get(stock).unwrap().fit(&project),
        Some(Fit::Exact)
    );
    assert_eq!(
        keyboards.get(lit).unwrap().fit(&project),
        Some(Fit::OtherFirmware)
    );
    assert_eq!(keyboards.get(other).unwrap().fit(&project), None);
    // Exact fits come first, whatever order the keyboards were added in.
    assert_eq!(
        keyboards.fitting(&project),
        [(stock, Fit::Exact), (lit, Fit::OtherFirmware)]
    );
}

#[test]
fn retargeting_hides_what_the_firmware_lacks_and_loses_nothing() {
    let go60 = board("moergo-go60");
    let mut project = Project::new("Lit", &go60);
    project.firmware = GO60_PERKEY.into();
    let base = project.layers[0].id;
    project.lighting_mut(base).unwrap().keys[0] = KeyLight::Color(Rgb(255, 0, 0));
    assert!(validate(&project, &go60)
        .iter()
        .all(|p| p.severity != Severity::Error));

    let preview = preview_retarget(&project, &go60, GO60_STOCK).unwrap();
    assert!(preview.hidden.lighting);
    assert_eq!(
        preview.summary(),
        "Per-key colours will be hidden. Hidden parts stay in the project and return with a firmware that has them."
    );
    assert!(!preview.hidden.pointing);
    assert_eq!(preview.flagged, 0);
    // Previewing changes nothing.
    assert_eq!(project.firmware, GO60_PERKEY);

    let before = project.clone();
    retarget(&mut project, &go60, GO60_STOCK).unwrap();
    assert_eq!(project.firmware, GO60_STOCK);
    assert_eq!(project.lighting, before.lighting);
    assert!(validate(&project, &go60)
        .iter()
        .all(|p| p.severity != Severity::Error));

    // Going back shows the colours again, with nothing hidden.
    let back = preview_retarget(&project, &go60, GO60_PERKEY).unwrap();
    assert!(back.hidden.is_empty());
    assert_eq!(
        back.summary(),
        "Everything in the project works with this firmware."
    );
    retarget(&mut project, &go60, GO60_PERKEY).unwrap();
    assert_eq!(project, before);

    assert!(matches!(
        retarget(&mut project, &go60, "cyboard-zmk-0.3"),
        Err(KeyboardError::NoSuchFirmware { .. })
    ));
    assert_eq!(project.firmware, GO60_PERKEY);
}

#[test]
fn retargeting_flags_keys_the_firmware_cannot_build() {
    let go60 = board("moergo-go60");
    let mut project = Project::new("Studio", &go60);
    project.firmware = GO60_STOCK.into();
    let base = project.layers[0].id;
    // The stock firmware has ZMK Studio; the per-key one does not.
    project
        .set_binding(base, 0, Binding::new("studio_unlock", vec![]))
        .unwrap();
    project
        .set_binding(
            base,
            1,
            Binding::new(
                "rgb_ug",
                vec![Param::Command {
                    name: "RGB_TOG".into(),
                    args: vec![],
                }],
            ),
        )
        .unwrap();

    let preview = preview_retarget(&project, &go60, GO60_PERKEY).unwrap();
    // Only the Studio key is new trouble; underglow exists on both.
    assert_eq!(preview.flagged, 1);
    assert_eq!(
        preview.summary(),
        "One key or behaviour uses a feature this firmware lacks and will be flagged."
    );
    assert!(preview.hidden.is_empty());
}

#[test]
fn hidden_lists_settings_and_pointing_the_firmware_lacks() {
    let imprint = board("cyboard-imprint");
    let mut project = Project::from_template("Mine", &imprint);
    project.settings.insert(
        "CONFIG_ZMK_RGB_UNDERGLOW_BRT_MAX".into(),
        SettingValue::Int(30),
    );
    assert!(!project.pointing.is_empty());

    let everything = [Feature::RgbUnderglow, Feature::Pointing, Feature::Studio];
    assert!(hidden(&project, &everything).is_empty());

    let bare = hidden(&project, &[]);
    assert!(bare.pointing);
    assert!(!bare.lighting);
    assert_eq!(bare.settings.len(), 1);
}

#[test]
fn the_editor_changes_firmware_outside_the_undo_history() {
    let go60 = board("moergo-go60");
    let mut project = Project::new("Mine", &go60);
    project.firmware = GO60_STOCK.into();
    let mut editor = Editor::new(project);
    editor.mark_saved();
    editor
        .edit("Add Layer", |p| p.add_layer("Nav").map(|_| ()))
        .unwrap();
    editor.mark_saved();

    // The firmware the project already has changes nothing.
    editor.set_firmware(GO60_STOCK);
    assert!(!editor.is_dirty());

    editor.set_firmware(GO60_PERKEY);
    assert_eq!(editor.project().firmware, GO60_PERKEY);
    assert!(editor.is_dirty());

    // Undo and redo move through the edits, never back to the old firmware.
    assert_eq!(editor.undo().as_deref(), Some("Add Layer"));
    assert_eq!(editor.project().firmware, GO60_PERKEY);
    assert_eq!(editor.project().layers.len(), 1);
    assert!(editor.is_dirty());
    editor.redo();
    assert_eq!(editor.project().firmware, GO60_PERKEY);
    assert!(editor.is_dirty());

    editor.mark_saved();
    assert!(!editor.is_dirty());
}

#[test]
fn projects_are_placed_under_the_right_keyboard() {
    let go60 = board("moergo-go60");
    let imprint = board("cyboard-imprint");
    let mut project = Project::new("Mine", &go60);
    project.firmware = GO60_STOCK.into();

    let mut keyboards = Keyboards::default();
    assert_eq!(keyboards.place(&project, None), Placement::NoKeyboard);

    let other = keyboards
        .add("Imprint", &imprint, "cyboard-zmk-0.3")
        .unwrap();
    assert_eq!(keyboards.place(&project, None), Placement::NoKeyboard);
    assert_eq!(
        keyboards.place(&project, Some(other)),
        Placement::NoKeyboard
    );

    // The only keyboard of the board is used, switching firmware if needed.
    let lit = keyboards.add("Lit", &go60, GO60_PERKEY).unwrap();
    assert_eq!(keyboards.place(&project, None), Placement::Retarget(lit));
    assert_eq!(
        keyboards.place(&project, Some(other)),
        Placement::Retarget(lit)
    );

    // An exact fit wins over one that needs retargeting...
    let stock = keyboards.add("Stock", &go60, GO60_STOCK).unwrap();
    assert_eq!(keyboards.place(&project, None), Placement::Open(stock));
    // ...unless another keyboard the project suits is the one selected.
    assert_eq!(
        keyboards.place(&project, Some(lit)),
        Placement::Retarget(lit)
    );
    assert_eq!(
        keyboards.place(&project, Some(stock)),
        Placement::Open(stock)
    );

    // Two exact fits: the user chooses between those two only.
    let second = keyboards.add("Stock 2", &go60, GO60_STOCK).unwrap();
    assert_eq!(
        keyboards.place(&project, None),
        Placement::Choose(vec![(stock, Fit::Exact), (second, Fit::Exact)])
    );

    // A selection that no longer exists is ignored.
    keyboards.remove(second).unwrap();
    assert_eq!(
        keyboards.place(&project, Some(second)),
        Placement::Open(stock)
    );
}
