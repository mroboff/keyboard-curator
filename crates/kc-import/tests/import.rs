//! Importing keymaps: the app's own output, and a vendor's factory file.

use std::path::PathBuf;

use kc_boards::Board;
use kc_import::{import_conf, import_keymap, ImportError};
use kc_model::features::SettingValue;
use kc_model::{file, Binding, Project, Severity};

fn board(id: &str) -> Board {
    kc_boards::built_in()
        .unwrap()
        .into_iter()
        .find(|b| b.id == id)
        .unwrap()
}

/// Generated combo nodes end in an ID that differs between projects.
fn without_combo_ids(keymap: &str) -> String {
    keymap
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("combo_") && trimmed.ends_with('{') {
                let name = trimmed.trim_end_matches(" {");
                let name = name.trim_end_matches(|c: char| c.is_ascii_digit());
                format!("{name}{{")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every fixture: generate its keymap, import that, and generate again.
/// The two keymaps must be the same, so nothing the emitter writes is lost
/// or changed by the importer.
#[test]
fn generated_keymaps_survive_a_round_trip() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut checked = 0;
    for entry in std::fs::read_dir(&root).unwrap().flatten() {
        let path = entry.path().join("project.kcproj");
        let Ok(project) = file::load(&path) else {
            continue;
        };
        let board = board(&project.board);
        let original = kc_emit::keymap(&project, &board).unwrap();
        let (mut imported, report) = import_keymap("Imported", &original, &board).unwrap();
        // The firmware is the user's choice, not something a keymap states.
        imported.firmware = project.firmware.clone();
        let again = kc_emit::keymap(&imported, &board).unwrap();
        assert_eq!(
            without_combo_ids(&again),
            without_combo_ids(&original),
            "{}",
            path.display()
        );
        assert_eq!(report.layers, project.layers.len());
        assert_eq!(report.behaviors, project.behaviors.len());
        assert_eq!(report.combos, project.combos.len());
        assert_eq!(imported.reserved_layers, project.reserved_layers);
        checked += 1;
    }
    assert!(checked >= 6, "only {checked} fixtures were found");
}

/// MoErgo's own factory keymap imports to the same layout as the template
/// new Go60 projects start from.
#[test]
fn the_go60_factory_keymap_imports_to_the_factory_template() {
    let go60 = board("moergo-go60");
    let vendor = include_str!("data/go60.keymap");
    let (imported, report) = import_keymap("Go60", vendor, &go60).unwrap();
    let template = Project::from_template("Go60", &go60);

    assert_eq!(report.layers, 5);
    assert_eq!(report.behaviors, 12);
    assert_eq!(report.raw_bindings, 0);
    // The only thing not modelled is the tap-to-right-click processor.
    assert_eq!(report.raw_blocks, ["input_processors"]);

    let names = |p: &Project| p.layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&imported), names(&template));
    // Compare what each key does by its text, since IDs differ.
    let text = |p: &Project| {
        p.layers
            .iter()
            .map(|l| {
                l.bindings
                    .iter()
                    .map(|b| {
                        kc_model::text::format_binding(p, b, kc_model::text::LayerStyle::Constant)
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(text(&imported), text(&template));
    assert_eq!(imported.pointing.len(), 2);
    assert!(imported
        .raw
        .devicetree
        .contains("zip_click_to_right_click_mapper"));
    assert!(imported
        .raw
        .devicetree
        .starts_with("#include <zephyr/dt-bindings/input/input-event-codes.h>"));

    let errors: Vec<_> = kc_model::validate(&imported, &go60)
        .into_iter()
        .filter(|p| p.severity == Severity::Error)
        .collect();
    assert_eq!(errors, []);
    // And it generates a keymap again.
    assert!(kc_emit::generate(&imported, &go60).is_ok());
}

#[test]
fn what_cannot_be_read_is_kept_and_reported() {
    let go60 = board("moergo-go60");
    let row = |binding: &str| vec![binding; 60].join(" ");
    let keymap = format!(
        r#"
#define MY_KEY LC(A)
#define HRM(a, b) &mt a b
#include "my_helpers.dtsi"
/ {{
    behaviors {{
        odd: odd {{
            compatible = "zmk,behavior-something-new";
            #binding-cells = <0>;
        }};
        td: td {{
            compatible = "zmk,behavior-tap-dance";
            #binding-cells = <0>;
            bindings = <&kp A>, <&odd>;
        }};
    }};
    keymap {{
        compatible = "zmk,keymap";
        default_layer {{ bindings = <{}>; }};
        lower {{ label = "Lower"; bindings = <{}>; }};
    }};
    leds {{ compatible = "gpio-leds"; }};
}};
&some_other_node {{ status = "okay"; }};
"#,
        row("&kp MY_KEY"),
        row("&odd")
    );
    let (project, report) = import_keymap("Odd", &keymap, &go60).unwrap();
    assert_eq!(project.layers[0].name, "default_layer");
    assert_eq!(project.layers[1].name, "Lower");
    // The alias is expanded.
    assert_eq!(
        project.layers[0].bindings[0],
        Binding::kp("LC(A)".parse().unwrap())
    );
    // The unknown behaviour, and the tap-dance that needs it, stay as text.
    assert_eq!(report.behaviors, 0);
    assert!(project.raw.behaviors.contains("zmk,behavior-something-new"));
    assert!(project.raw.behaviors.contains("td: td {"));
    // Bindings to it are kept, and still assignable as written.
    assert_eq!(
        project.layers[1].bindings[0],
        Binding::Raw { raw: "&odd".into() }
    );
    assert_eq!(report.raw_bindings, 60);
    assert!(project.raw.devicetree.contains("gpio-leds"));
    assert!(project.raw.devicetree.contains("&some_other_node"));
    assert!(project
        .raw
        .devicetree
        .contains("#include \"my_helpers.dtsi\""));
    assert!(report.raw_blocks.contains(&"leds".to_string()));
    assert!(report.notes.iter().any(|n| n.contains("HRM")));
    assert!(report.notes.iter().any(|n| n.contains("60 binding(s)")));
}

#[test]
fn unusable_files_explain_why() {
    let go60 = board("moergo-go60");
    assert_eq!(
        import_keymap("x", "/ { };", &go60).unwrap_err(),
        ImportError::NoKeymap
    );
    let small = "/ { keymap { compatible = \"zmk,keymap\"; l { bindings = <&kp A &kp B>; }; }; };";
    assert!(matches!(
        import_keymap("x", small, &go60),
        Err(ImportError::KeyCount { found: 2, .. })
    ));
    let uneven = "/ { keymap { a { bindings = <&kp A &kp B>; }; b { bindings = <&kp A>; }; }; };";
    assert_eq!(
        import_keymap("x", uneven, &go60).unwrap_err(),
        ImportError::UnevenLayers(2, 1)
    );
    assert!(matches!(
        import_keymap("x", "/ { a {", &go60),
        Err(ImportError::Syntax(_))
    ));
}

#[test]
fn imprint_variants_are_recognised_by_key_count() {
    let imprint = board("cyboard-imprint");
    let keymap = format!(
        "/ {{ keymap {{ compatible = \"zmk,keymap\"; base {{ bindings = <{}>; }}; }}; }};",
        vec!["&kp A"; 64].join(" ")
    );
    let (project, _) = import_keymap("Small", &keymap, &imprint).unwrap();
    assert_eq!(project.layout, "physical_layout_imprint_number_row");
    assert_eq!(project.key_count, 64);
}

#[test]
fn conf_files_become_settings_and_extra_lines() {
    let go60 = board("moergo-go60");
    let mut project = Project::new("Conf", &go60);
    import_conf(
        &mut project,
        "# comment\nCONFIG_ZMK_SLEEP=y\nCONFIG_ZMK_IDLE_SLEEP_TIMEOUT=900000\nCONFIG_ZMK_KEYBOARD_NAME=\"Mine\"\n\nCONFIG_ZMK_USB_LOGGING=y\nCONFIG_ZMK_SLEEP_ODD=maybe\n",
    );
    assert_eq!(
        project.settings["CONFIG_ZMK_SLEEP"],
        SettingValue::Bool(true)
    );
    assert_eq!(
        project.settings["CONFIG_ZMK_IDLE_SLEEP_TIMEOUT"],
        SettingValue::Int(900000)
    );
    assert_eq!(
        project.settings["CONFIG_ZMK_KEYBOARD_NAME"],
        SettingValue::Text("Mine".into())
    );
    assert_eq!(
        project.raw.conf,
        "CONFIG_ZMK_USB_LOGGING=y\nCONFIG_ZMK_SLEEP_ODD=maybe"
    );
}
