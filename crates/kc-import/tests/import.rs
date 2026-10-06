//! Importing keymaps: the app's own output, and a vendor's factory file.

use std::path::PathBuf;

use kc_boards::Board;
use kc_import::{import_conf, import_keymap, ImportError};
use kc_model::behavior::BehaviorKind;
use kc_model::features::{
    InputProcessor, PointingConfig, PointingOverride, PointingProfile, RawBlocks, SettingValue,
};
use kc_model::{file, Binding, LayerId, Param, Project, Severity};

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
    // Everything in it is modeled, the tap-to-right-click processor too.
    assert_eq!(report.raw_blocks, [] as [&str; 0]);
    assert_eq!(report.notes, [] as [&str; 0]);

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
    assert_eq!(imported.pointing, template_pointing(&imported, &template));
    assert_eq!(imported.raw, template.raw);

    let errors: Vec<_> = kc_model::validate(&imported, &go60)
        .into_iter()
        .filter(|p| p.severity == Severity::Error)
        .collect();
    assert_eq!(errors, []);
    // And it generates a keymap again.
    assert!(kc_emit::generate(&imported, &go60).is_ok());
}

/// The template's pointing settings, with its layer IDs replaced by the
/// imported project's for the layers of the same name.
fn template_pointing(imported: &Project, template: &Project) -> Vec<PointingConfig> {
    let layer = |id: LayerId| {
        let name = &template.layer(id).unwrap().name;
        imported.layers.iter().find(|l| l.name == *name).unwrap().id
    };
    let mut pointing = template.pointing.clone();
    for device in &mut pointing {
        for layer_override in &mut device.overrides {
            for id in &mut layer_override.layers {
                *id = layer(*id);
            }
        }
        for processor in device.processors_mut() {
            if let InputProcessor::TempLayer { layer: id, .. } = processor {
                *id = layer(*id);
            }
        }
    }
    // The order devices are listed in is the file's.
    pointing.sort_by_key(|d| {
        imported
            .pointing
            .iter()
            .position(|i| i.listener == d.listener)
    });
    pointing
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
#ifdef MY_SWITCH
#include "never.dtsi"
#endif
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
    // The unknown behavior, and the tap-dance that needs it, stay as text.
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
    assert!(!project.raw.devicetree.contains("never.dtsi"));
    assert!(report.notes.iter().any(|n| n.contains("MY_SWITCH")));
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
fn imprint_variants_are_recognized_by_key_count() {
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

/// A layout in the MoErgo Layout Editor's export format, using its
/// shorthands and every section the importer reads.
fn moergo_export() -> String {
    let key =
        |binding: &str| format!(r#"{{"value": "&kp", "params": [{{"value": "{binding}"}}]}}"#);
    let mut base: Vec<String> = (0..60).map(|_| key("A")).collect();
    base[0] = r#"{"value": "&kp", "params": [{"value": "LG", "params": [{"value": "LA", "params": [{"value": "K"}]}]}]}"#.into();
    base[1] = r#"{"value": "&magic"}"#.into();
    base[2] = r#"{"value": "&layer", "params": [{"value": 1}]}"#.into();
    base[3] = r#"{"value": "&reset"}"#.into();
    base[4] = r#"{"value": "&bt_2"}"#.into();
    base[5] = r#"{"value": "&hrm", "params": [{"value": "LSHFT"}, {"value": "F"}]}"#.into();
    base[6] = r#"{"value": "&hello"}"#.into();
    base[7] = r#"{"value": "&mo", "params": [{"value": "1"}]}"#.into();
    let layer = |keys: &[String]| format!("[{}]", keys.join(","));
    let other: Vec<String> = (0..60)
        .map(|_| r#"{"value": "&trans"}"#.to_string())
        .collect();
    format!(
        r#"{{
  "keyboard": "go60", "title": "My Go60", "layer_names": ["Base", "Magic"],
  "layers": [{}, {}],
  "holdTaps": [{{"name": "&hrm", "bindings": ["&kp", "&kp"], "tappingTermMs": 190,
     "flavor": "balanced", "quickTapMs": 300, "requirePriorIdleMs": 100,
     "holdTriggerOnRelease": true, "holdTriggerKeyPositions": [6, 7, 8]}}],
  "macros": [{{"name": "&hello", "waitMs": 10, "tapMs": 20, "params": [],
     "bindings": [{{"value": "&macro_press"}}, {{"value": "&kp", "params": [{{"value": "LSHFT"}}]}},
                  {{"value": "&macro_tap"}}, {{"value": "&kp", "params": [{{"value": "H"}}]}},
                  {{"value": "&macro_release"}}, {{"value": "&kp", "params": [{{"value": "LSHFT"}}]}}]}}],
  "combos": [{{"name": "esc combo", "binding": {{"value": "&kp", "params": [{{"value": "ESC"}}]}},
     "keyPositions": [13, 14], "timeoutMs": 50, "layers": [0]}}],
  "inputListeners": [{{"code": "&cirque_rh_listener",
     "inputProcessors": [{{"code": "&zip_xy_scaler", "params": [3, 1]}},
                         {{"code": "&zip_xy_transform", "params": [["INPUT_TRANSFORM_Y_INVERT"]]}},
                         {{"code": "&zip_click_to_right_click_mapper", "params": []}}],
     "nodes": [{{"code": "layer_1", "layers": [1], "inputProcessors": [{{"code": "&zip_xy_scaler", "params": [9, 1]}}]}}]}}],
  "config_parameters": [{{"paramName": "DEEP_SLEEP", "value": "y"}},
                        {{"paramName": "DEEP_SLEEP_TIMEOUT_MS", "value": "900000"}},
                        {{"paramName": "SOMETHING_NEW", "value": "1"}}],
  "layout_parameters": {{"cirque_touch_sensitivity": "high"}},
  "custom_defined_behaviors": "", "custom_devicetree": ""
}}"#,
        layer(&base),
        layer(&other)
    )
}

#[test]
fn moergo_layout_editor_exports_import_with_their_shorthands_written_out() {
    use kc_import::import_moergo;
    use kc_model::behavior::{BehaviorKind, Flavor, MacroStep};
    use kc_model::features::InputProcessor;
    use kc_model::text::{format_binding, LayerStyle};

    let go60 = board("moergo-go60");
    let (project, report) = import_moergo(&moergo_export(), &go60).unwrap();
    assert_eq!(project.name, "My Go60");
    assert_eq!(report.layers, 2);
    assert_eq!(report.raw_bindings, 0);
    assert_eq!(report.combos, 1);

    let base = &project.layers[0];
    let show =
        |position: usize| format_binding(&project, &base.bindings[position], LayerStyle::Index);
    assert_eq!(show(0), "&kp LG(LA(K))");
    // The editor's shorthands become the behaviors they stand for.
    assert_eq!(show(1), "&magic 1 0");
    assert_eq!(show(2), "&layer_td_1");
    assert_eq!(show(3), "&sys_reset");
    assert_eq!(show(4), "&bt_2");
    assert_eq!(show(5), "&hrm LSHFT F");
    assert_eq!(show(7), "&mo 1");

    let behavior = |label: &str| {
        &project
            .behaviors
            .iter()
            .find(|b| b.label == label)
            .unwrap_or_else(|| panic!("no &{label}"))
            .kind
    };
    // Only what is used is brought in: Bluetooth profile 2 and what it needs.
    assert!(matches!(behavior("magic"), BehaviorKind::HoldTap(_)));
    assert!(matches!(
        behavior("rgb_ug_status_macro"),
        BehaviorKind::Macro(_)
    ));
    assert!(matches!(behavior("bt_select_2"), BehaviorKind::Macro(_)));
    assert!(!project.behaviors.iter().any(|b| b.label == "bt_0"));
    assert!(matches!(behavior("layer_td_1"), BehaviorKind::TapDance(t) if t.bindings.len() == 2));
    let BehaviorKind::HoldTap(hrm) = behavior("hrm") else {
        panic!("&hrm is a hold-tap");
    };
    assert_eq!(
        (hrm.flavor, hrm.tapping_term_ms, hrm.quick_tap_ms),
        (Flavor::Balanced, 190, Some(300))
    );
    assert_eq!(hrm.hold_trigger_key_positions, [6, 7, 8]);
    assert!(hrm.hold_trigger_on_release);
    let BehaviorKind::Macro(hello) = behavior("hello") else {
        panic!("&hello is a macro");
    };
    assert_eq!((hello.wait_ms, hello.tap_ms), (Some(10), Some(20)));
    assert!(matches!(
        hello.steps.as_slice(),
        [
            MacroStep::Press(_),
            MacroStep::Tap(_),
            MacroStep::Release(_)
        ]
    ));

    assert_eq!(project.combos[0].key_positions, [13, 14]);
    assert_eq!(project.combos[0].layers, [project.layers[0].id]);
    let pad = &project.pointing[0];
    assert_eq!(pad.listener, "cirque_rh_listener");
    assert!(matches!(
        pad.processors[0],
        InputProcessor::Scale {
            multiplier: 3,
            divisor: 1
        }
    ));
    assert!(matches!(
        pad.processors[1],
        InputProcessor::Transform {
            invert_y: true,
            scroll: false,
            ..
        }
    ));
    assert_eq!(pad.processors[2], InputProcessor::RightClick);
    assert_eq!(pad.overrides[0].layers, [project.layers[1].id]);
    assert_eq!(project.raw.devicetree, "");

    assert_eq!(
        project.settings["CONFIG_ZMK_SLEEP"],
        SettingValue::Bool(true)
    );
    assert_eq!(
        project.settings["CONFIG_ZMK_IDLE_SLEEP_TIMEOUT"],
        SettingValue::Int(900000)
    );
    assert!(report.notes.iter().any(|n| n.contains("SOMETHING_NEW")));
    assert!(report.notes.iter().any(|n| n.contains("sensitivity")));

    // The result is a valid project that generates a config.
    assert!(kc_emit::generate(&project, &go60).is_ok());
    assert!(matches!(
        import_moergo("{}", &go60),
        Err(ImportError::NotAnExport(_))
    ));
    assert!(matches!(
        import_moergo("nope", &go60),
        Err(ImportError::NotAnExport(_))
    ));
}

#[test]
fn files_find_their_board() {
    use kc_import::import_file;

    let boards = kc_boards::built_in().unwrap();
    let row = |keys: usize| vec!["&kp A"; keys].join(" ");
    let keymap = |keys: usize, extra: &str| {
        format!(
            "/ {{ keymap {{ compatible = \"zmk,keymap\"; base {{ bindings = <{}>; }}; }}; }};\n{extra}",
            row(keys)
        )
    };
    let board_of = |name: &str, text: &str| {
        let (project, index, _) = import_file(name, text, None, &boards).unwrap();
        assert_eq!(boards[index].id, project.board);
        project.board
    };
    // 82 keys can only be the Imprint.
    assert_eq!(board_of("x.keymap", &keymap(82, "")), "cyboard-imprint");
    // 60 keys fits both; the file's name or contents decide.
    assert_eq!(board_of("go60.keymap", &keymap(60, "")), "moergo-go60");
    assert_eq!(
        board_of("imprint.keymap", &keymap(60, "")),
        "cyboard-imprint"
    );
    assert_eq!(
        board_of("x.keymap", &keymap(60, "&cirque_lh_listener { };")),
        "moergo-go60"
    );
    // A Layout Editor export is recognized by being JSON.
    assert_eq!(board_of("layout.json", &moergo_export()), "moergo-go60");

    let (project, _, report) = import_file(
        "config/go60.keymap",
        &keymap(60, ""),
        Some("CONFIG_ZMK_SLEEP=y\n"),
        &boards,
    )
    .unwrap();
    assert_eq!(project.name, "go60");
    assert_eq!(
        project.settings["CONFIG_ZMK_SLEEP"],
        SettingValue::Bool(true)
    );
    assert!(report
        .summary()
        .starts_with("Imported 1 layer(s), 0 behavior(s) and 0 combo(s)."));
    assert!(import_file("x.keymap", &keymap(7, ""), None, &boards).is_err());
}

/// A keymap downloaded from MoErgo's Layout Editor: helper macros with
/// arguments, behaviors chosen by `#ifdef`, mouse-key listeners, a
/// right-click processor and described behaviors. All of it is read into
/// the model, and nothing is left as text.
#[test]
fn moergo_layout_editor_keymaps_import_completely() {
    let go60 = board("moergo-go60");
    let source = include_str!("data/moergo-editor.keymap");
    let (project, report) = import_keymap("Editor", source, &go60).unwrap();
    assert_eq!(report.raw_blocks, [] as [&str; 0]);
    assert_eq!(report.notes, [] as [&str; 0]);
    assert_eq!(report.raw_bindings, 0);
    assert_eq!(project.raw, RawBlocks::default());
    assert_eq!((report.layers, report.combos), (4, 1));

    let behavior = |label: &str| {
        project
            .behaviors
            .iter()
            .find(|b| b.label == label)
            .unwrap_or_else(|| panic!("no &{label}"))
    };
    // `ZMK_TD_LAYER(lower, LAYER_Lower)` is a tap-dance once expanded, and
    // `LAYER_Lower` is the default the file gives it.
    let base = project.layers[0].id;
    let BehaviorKind::TapDance(lower) = &behavior("lower").kind else {
        panic!("&lower is not a tap-dance");
    };
    assert_eq!(
        lower.bindings,
        [
            Binding::new("mo", vec![Param::Layer(base)]),
            Binding::new("to", vec![Param::Layer(base)])
        ]
    );
    // ZMK's `bt.h` defines `BT_DISC_CMD`, so the `#ifdef` branch is the one
    // read, and each behavior is defined once.
    assert!(matches!(behavior("bt_0").kind, BehaviorKind::TapDance(_)));
    assert!(matches!(
        behavior("bt_select_0").kind,
        BehaviorKind::Macro(_)
    ));
    assert_eq!(report.behaviors, 8);
    assert_eq!(project.behaviors.len(), 8);
    // The comment above a behavior is its description.
    assert_eq!(
        behavior("AS_HT_v2_TKZ").description,
        "AutoShift Helper - &AS main macro is chained to &AS_HT hold tap and &AS_Shifted macro. More: https://github.com/nickcoutsos/keymap-editor/wiki/Autoshift-using-ZMK-behaviors"
    );
    assert_eq!(
        behavior("mod_tab_v1_TKZ").description,
        "mod_tab_switcher - TailorKey"
    );
    assert_eq!(behavior("magic").description, "");

    let (mouse, slow) = (project.layers[1].id, project.layers[2].id);
    let device = |listener: &str| {
        project
            .pointing
            .iter()
            .find(|d| d.listener == listener)
            .unwrap_or_else(|| panic!("no {listener}"))
    };
    assert_eq!(project.pointing.len(), 4);
    let keys = device("mmv_input_listener");
    assert_eq!(keys.processors, []);
    assert_eq!(
        keys.overrides,
        [PointingOverride {
            layers: vec![slow],
            processors: vec![InputProcessor::Scale {
                multiplier: 1,
                divisor: 9
            }]
        }]
    );
    assert_eq!(
        device("msc_input_listener").overrides[0].processors,
        [InputProcessor::ScrollScale {
            multiplier: 1,
            divisor: 9
        }]
    );
    let left = device("cirque_lh_listener");
    assert_eq!(left.processors[3], InputProcessor::RightClick);
    let profile = PointingProfile::from_processors(&left.processors, false).unwrap();
    assert!(profile.scroll && profile.invert_y && profile.right_click);
    assert_eq!(profile.speed, (11, 12));
    assert_eq!(profile.auto_layer, Some((mouse, 250)));
    assert!(PointingProfile::from_processors(&left.overrides[0].processors, false).is_some());

    let problems = kc_model::validate(&project, &go60);
    assert_eq!(problems, []);

    // The keymap written from it defines the right-click processor and
    // each behavior once, keeps the descriptions, and reads back the same.
    let keymap = kc_emit::keymap(&project, &go60).unwrap();
    for once in [
        "bt_0: bt_0 {",
        "lower: lower {",
        "zip_click_to_right_click_mapper: zip_click_to_right_click_mapper {",
        "#include <zephyr/dt-bindings/input/input-event-codes.h>",
        "// mod_tab_switcher - TailorKey",
        "&msc_input_listener {",
    ] {
        assert_eq!(keymap.matches(once).count(), 1, "{once}");
    }
    let (again, report) = import_keymap("Editor", &keymap, &go60).unwrap();
    assert_eq!(report.raw_blocks, [] as [&str; 0]);
    assert_eq!(
        without_combo_ids(&kc_emit::keymap(&again, &go60).unwrap()),
        without_combo_ids(&keymap)
    );
}
