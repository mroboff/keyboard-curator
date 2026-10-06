//! What each firmware family checks, generates and offers.

use kc_boards::Board;
use kc_firmware::{check, errors, expressible, generate, FirmwareError};
use kc_model::{Binding, FirmwareConfig, KeyExpr, Project};

fn board(id: &str) -> Board {
    kc_boards::built_in()
        .unwrap()
        .into_iter()
        .find(|b| b.id == id)
        .unwrap()
}

#[test]
fn zmk_layouts_generate_a_firmware_repository() {
    let go60 = board("moergo-go60");
    let project = Project::from_template("Mine", &go60);
    let config = FirmwareConfig::stock(&go60);
    assert!(errors(&project, &go60, &config).is_empty());
    let files = generate(&project, &go60, &config).unwrap();
    assert!(files.iter().any(|f| f.path == "config/go60.keymap"));
    // Everything can be offered: ZMK is the vocabulary layouts are in.
    assert!(expressible(
        &Binding::new("caps_word", vec![]),
        &project,
        &go60,
        &config
    ));
}

#[test]
fn dygma_layouts_are_checked_for_what_the_firmware_holds_and_never_built() {
    let defy = board("dygma-defy");
    let config = FirmwareConfig::stock(&defy);
    let mut project = Project::new("Mine", &defy);
    let base = project.layers[0].id;
    project
        .set_binding(base, 0, Binding::kp(KeyExpr::new("A")))
        .unwrap();
    // A plain layout has no problems, and none of ZMK's build advice.
    assert_eq!(check(&project, &defy, &config), []);
    assert!(matches!(
        generate(&project, &defy, &config),
        Err(FirmwareError::NoBuild("Dygma"))
    ));

    let caps_word = Binding::new("caps_word", vec![]);
    assert!(!expressible(&caps_word, &project, &defy, &config));
    assert!(expressible(
        &Binding::kp(KeyExpr::new("B")),
        &project,
        &defy,
        &config
    ));
    project.set_binding(base, 1, caps_word).unwrap();
    let problems = errors(&project, &defy, &config);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].message.contains("nothing for &caps_word"));
}

#[test]
fn only_live_firmware_is_read_from_and_applied_to_a_keyboard() {
    let go60 = board("moergo-go60");
    let config = FirmwareConfig::stock(&go60);
    let error = kc_firmware::live::read(&go60, &config, None).unwrap_err();
    assert!(error.contains("built and flashed"));
    let project = Project::new("Mine", &go60);
    let backups = std::env::temp_dir().join("kc-never-written");
    assert!(kc_firmware::live::apply(&project, &go60, &config, None, &backups).is_err());
    assert!(!backups.exists());
}

#[test]
fn rmk_layouts_generate_an_rmk_project_and_flag_what_rmk_lacks() {
    let imprint = board("cyboard-imprint");
    let config = FirmwareConfig::new("rmk-0.9");
    assert_eq!(config.family(&imprint), kc_firmware::Family::Rmk);

    let mut project = Project::new("Mine", &imprint);
    assert!(errors(&project, &imprint, &config).is_empty());
    let files = generate(&project, &imprint, &config).unwrap();
    assert!(files.iter().any(|f| f.path == "keyboard.toml"));
    assert!(files
        .iter()
        .any(|f| f.path == ".github/workflows/build.yml"));
    assert!(!files.iter().any(|f| f.path.ends_with(".keymap")));

    // The same layout still builds as ZMK for the same board.
    let zmk = FirmwareConfig::stock(&imprint);
    assert!(generate(&project, &imprint, &zmk)
        .unwrap()
        .iter()
        .any(|f| f.path == "config/imprint.keymap"));

    // An underglow key means nothing to RMK: it is not offered, and one
    // already in the layout stops the build with the key named.
    let base = project.layers[0].id;
    let lighting = Binding::new(
        "rgb_ug",
        vec![kc_model::Param::Command {
            name: "RGB_TOG".into(),
            args: vec![],
        }],
    );
    assert!(!expressible(&lighting, &project, &imprint, &config));
    project.set_binding(base, 0, lighting).unwrap();
    let Err(FirmwareError::Invalid(problems)) = generate(&project, &imprint, &config) else {
        panic!("a layout RMK cannot hold must not generate");
    };
    assert!(problems.iter().any(|p| p.message.contains("rgb_ug")));
}
