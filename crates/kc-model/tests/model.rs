//! Behavior of the project model against the real board definitions.

use kc_boards::Board;
use kc_model::behavior::{BehaviorKind, HoldTap, Macro, MacroStep, TapDance};
use kc_model::features::{
    ConditionalLayer, InputProcessor, KeyLight, PointingConfig, PointingOverride, Rgb, SettingValue,
};
use kc_model::validate::BRIGHTNESS_MAX_SETTING;
use kc_model::{
    file, validate, BehaviorRef, Binding, Editor, FirmwareConfig, KeyExpr, LayerId, Location,
    ModelError, Param, Project, Severity,
};

fn board(id: &str) -> Board {
    kc_boards::built_in()
        .unwrap()
        .into_iter()
        .find(|b| b.id == id)
        .unwrap()
}

fn kp(key: &str) -> Binding {
    Binding::kp(key.parse::<KeyExpr>().unwrap())
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

/// The board's first firmware with default settings.
fn stock(board: &Board) -> FirmwareConfig {
    FirmwareConfig::stock(board)
}

fn errors(project: &Project, board: &Board) -> Vec<String> {
    errors_with(project, board, &stock(board))
}

fn errors_with(project: &Project, board: &Board, config: &FirmwareConfig) -> Vec<String> {
    validate(project, board, config)
        .into_iter()
        .filter(|p| p.severity == Severity::Error)
        .map(|p| p.message)
        .collect()
}

/// A Go60 project that touches every part of the model.
fn rich_project() -> (Project, Board) {
    let go60 = board("moergo-go60");
    let mut p = Project::new("Rich", &go60);
    let base = p.layers[0].id;
    let nav = p.add_layer("Nav").unwrap();
    let magic = p.add_layer("Magic").unwrap();

    p.set_binding(base, 0, kp("LC(LS(K))")).unwrap();
    p.set_binding(base, 1, Binding::layer("mo", nav)).unwrap();
    p.set_binding(base, 2, command("bt", "BT_SEL", &[2]))
        .unwrap();
    p.set_binding(base, 3, command("rgb_ug", "RGB_STATUS", &[]))
        .unwrap();
    p.set_binding(
        base,
        4,
        Binding::new("mkp", vec![Param::Constant("LCLK".into())]),
    )
    .unwrap();

    let hold_tap = p
        .add_behavior(
            "magic",
            "Magic",
            BehaviorKind::HoldTap(HoldTap::new(
                BehaviorRef::built_in("mo"),
                BehaviorRef::built_in("kp"),
            )),
        )
        .unwrap();
    p.set_binding(
        base,
        36,
        Binding::user(
            hold_tap,
            vec![Param::Layer(magic), Param::Key(KeyExpr::new("ESC"))],
        ),
    )
    .unwrap();
    p.add_behavior(
        "nav_td",
        "Nav tap-dance",
        BehaviorKind::TapDance(TapDance {
            tapping_term_ms: 200,
            bindings: vec![Binding::layer("mo", nav), Binding::layer("to", nav)],
        }),
    )
    .unwrap();
    p.add_behavior(
        "hello",
        "Type hello",
        BehaviorKind::Macro(Macro {
            wait_ms: Some(30),
            tap_ms: None,
            params: 0,
            steps: vec![
                MacroStep::Tap(vec![kp("H"), kp("I")]),
                MacroStep::PauseForRelease,
            ],
        }),
    )
    .unwrap();

    let combo = p.add_combo("Escape", vec![0, 1], kp("ESC"));
    p.combo_mut(combo).unwrap().layers = vec![base, nav];
    p.conditional_layers.push(ConditionalLayer {
        if_layers: vec![nav, magic],
        then_layer: base,
    });
    p.pointing.push(PointingConfig {
        listener: "cirque_rh_listener".into(),
        processors: vec![InputProcessor::Scale {
            multiplier: 3,
            divisor: 1,
        }],
        overrides: vec![PointingOverride {
            layers: vec![nav],
            processors: vec![InputProcessor::ToScroll],
        }],
    });
    p.raw.devicetree = "/* custom */".into();
    (p, go60)
}

#[test]
fn new_projects_match_their_board() {
    let imprint = board("cyboard-imprint");
    let p = Project::new("Mine", &imprint);
    assert_eq!((p.key_count, p.layers.len()), (82, 1));
    // The base layer starts from the board's factory keys.
    assert_eq!(p.binding(p.layers[0].id, 25), Some(&kp("Q")));
    assert_eq!(p.binding(p.layers[0].id, 60), Some(&Binding::trans()));
    assert!(errors(&p, &imprint).is_empty());

    let go60 = board("moergo-go60");
    assert_eq!(Project::new("Mine", &go60).key_count, 60);
    // A project checked against the wrong board says so and stops there.
    let problems = errors(&p, &go60);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("cyboard-imprint"));
}

#[test]
fn the_rich_project_is_valid() {
    let (p, go60) = rich_project();
    assert_eq!(errors(&p, &go60), Vec::<String>::new());
}

#[test]
fn reordering_layers_keeps_every_reference() {
    let (mut p, go60) = rich_project();
    let nav = p.layers[1].id;
    let base = p.layers[0].id;
    assert_eq!(p.layer_index(nav), Some(1));

    p.move_layer(nav, 2).unwrap();
    assert_eq!(p.layer_index(nav), Some(2));
    // The binding still names the same layer, now at a new index.
    assert_eq!(p.binding(base, 1), Some(&Binding::layer("mo", nav)));
    assert!(errors(&p, &go60).is_empty());
    assert_eq!(
        p.move_layer(LayerId(999), 0),
        Err(ModelError::NoSuchLayer(LayerId(999)))
    );
}

#[test]
fn a_referenced_layer_cannot_be_removed_by_accident() {
    let (mut p, go60) = rich_project();
    let (base, nav) = (p.layers[0].id, p.layers[1].id);

    let Err(ModelError::LayerInUse(references)) = p.remove_layer(nav) else {
        panic!("nav is referenced");
    };
    assert!(references.contains(&Location::Key {
        layer: base,
        position: 1
    }));
    assert!(references.contains(&Location::ConditionalLayer(0)));
    assert!(references.contains(&Location::Pointing("cirque_rh_listener".into())));
    assert!(references
        .iter()
        .any(|r| matches!(r, Location::Behavior(_))));
    assert!(references.iter().any(|r| matches!(r, Location::Combo(_))));
    assert_eq!(p.layers.len(), 3, "nothing was removed");

    p.remove_layer_and_references(nav).unwrap();
    assert_eq!(p.layers.len(), 2);
    assert_eq!(p.binding(base, 1), Some(&Binding::none()));
    assert!(p.conditional_layers.is_empty());
    assert_eq!(p.combos[0].layers, [base]);
    assert!(p.pointing[0].overrides.is_empty());
    assert!(
        errors(&p, &go60).is_empty(),
        "no dangling references remain"
    );
}

#[test]
fn layer_limits_are_enforced() {
    let go60 = board("moergo-go60");
    let mut p = Project::new("Limits", &go60);
    assert_eq!(p.remove_layer(p.layers[0].id), Err(ModelError::LastLayer));
    for i in 1..32 {
        p.add_layer(format!("L{i}")).unwrap();
    }
    assert_eq!(p.add_layer("too many"), Err(ModelError::TooManyLayers));
    assert_eq!(p.set_reserved_layers(1), Err(ModelError::TooManyLayers));
    assert_eq!(
        p.duplicate_layer(p.layers[0].id),
        Err(ModelError::TooManyLayers)
    );
    assert_eq!(
        p.set_binding(p.layers[0].id, 60, Binding::none()),
        Err(ModelError::NoSuchPosition {
            position: 60,
            keys: 60
        })
    );
}

#[test]
fn duplicating_a_layer_copies_bindings_and_lighting() {
    let (mut p, _) = rich_project();
    let base = p.layers[0].id;
    p.lighting_mut(base).unwrap().keys[0] = KeyLight::Color(Rgb(255, 0, 0));
    let copy = p.duplicate_layer(base).unwrap();
    assert_eq!(p.layer_index(copy), Some(1));
    assert_eq!(p.layer(copy).unwrap().name, "Base copy");
    assert_eq!(
        p.layer(copy).unwrap().bindings,
        p.layer(base).unwrap().bindings
    );
    assert_eq!(
        p.lighting(copy).unwrap().keys[0],
        KeyLight::Color(Rgb(255, 0, 0))
    );
}

#[test]
fn behavior_labels_and_references_are_protected() {
    let (mut p, _) = rich_project();
    let magic = p.behaviors[0].id;
    let kind = p.behaviors[0].kind.clone();
    assert_eq!(
        p.add_behavior("magic", "Again", kind.clone()),
        Err(ModelError::LabelTaken("magic".into()))
    );
    assert_eq!(
        p.add_behavior("kp", "Clash", kind.clone()),
        Err(ModelError::LabelTaken("kp".into()))
    );
    assert_eq!(
        p.add_behavior("9lives", "Bad", kind),
        Err(ModelError::InvalidLabel("9lives".into()))
    );
    assert!(matches!(p.remove_behavior(magic), Err(ModelError::BehaviorInUse(r)) if r.len() == 1));

    let base = p.layers[0].id;
    p.set_binding(base, 36, Binding::trans()).unwrap();
    p.remove_behavior(magic).unwrap();
    p.rename_behavior_label(p.behaviors[0].id, "magic").unwrap();
}

#[test]
fn validation_reports_what_the_firmware_would_reject() {
    let imprint = board("cyboard-imprint");
    let mut p = Project::new("Bad", &imprint);
    let base = p.layers[0].id;

    // RGB_STATUS exists only in MoErgo's firmware.
    p.set_binding(base, 0, command("rgb_ug", "RGB_STATUS", &[]))
        .unwrap();
    p.set_binding(base, 1, command("bt", "BT_SEL", &[9]))
        .unwrap();
    p.set_binding(base, 2, Binding::new("kp", vec![])).unwrap();
    p.set_binding(
        base,
        3,
        Binding::new("mo", vec![Param::Key(KeyExpr::new("A"))]),
    )
    .unwrap();
    p.set_binding(base, 4, Binding::new("nope", vec![]))
        .unwrap();
    p.set_binding(base, 5, Binding::layer("mo", LayerId(999)))
        .unwrap();
    p.set_binding(base, 6, command("bl", "BL_TOG", &[]))
        .unwrap();
    p.set_binding(base, 7, kp("NOT_A_KEY")).unwrap();
    let mut config = stock(&imprint);
    config
        .settings
        .insert(BRIGHTNESS_MAX_SETTING.into(), SettingValue::Int(80));
    p.lighting_mut(base).unwrap().keys[0] = KeyLight::Off;
    p.add_combo("Solo", vec![200], Binding::trans());

    let problems = validate(&p, &imprint, &config);
    let found = |text: &str| problems.iter().any(|p| p.message.contains(text));
    assert!(found("RGB_STATUS is not supported"));
    assert!(found("BT_SEL profile must be between 0 and 4, found 9"));
    assert!(found("&kp takes 1 parameter(s), found 0"));
    assert!(found("&mo has the wrong kind of value"));
    assert!(found("&nope is not a ZMK behavior"));
    assert!(found("refers to a layer that no longer exists"));
    assert!(found("&bl is not supported"));
    assert!(found("brightness 80 is above this board's limit of 50"));
    // Colors on firmware without per-key lighting are kept but hidden.
    assert!(!found("per-key lighting"));
    assert!(found("a combo needs at least two keys"));
    assert!(found("uses key position 200"));

    let unknown = problems
        .iter()
        .find(|p| p.message.contains("NOT_A_KEY"))
        .unwrap();
    assert_eq!(unknown.severity, Severity::Warning);
    assert_eq!(
        unknown.location,
        Location::Key {
            layer: base,
            position: 7
        }
    );
}

#[test]
fn mouse_keys_are_pointing_devices_on_every_board() {
    for id in ["moergo-go60", "cyboard-imprint"] {
        let board = board(id);
        let mut p = Project::new("Mine", &board);
        p.pointing.push(PointingConfig {
            listener: "msc_input_listener".into(),
            processors: vec![InputProcessor::ScrollScale {
                multiplier: 1,
                divisor: 2,
            }],
            overrides: vec![],
        });
        assert_eq!(errors(&p, &board), [] as [&str; 0], "{id}");
        p.pointing[0].listener = "some_other_listener".into();
        assert_eq!(errors(&p, &board).len(), 1, "{id}");
    }
}

#[test]
fn custom_devicetree_may_not_define_a_label_twice() {
    let (mut p, go60) = rich_project();
    assert!(errors(&p, &go60).is_empty());
    // A behavior the project already has.
    p.raw.behaviors =
        "hello: hello {\n    compatible = \"zmk,behavior-macro\";\n    bindings = <&kp A>;\n};"
            .into();
    let found = errors(&p, &go60);
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("`&hello` is also defined"), "{found:?}");
    // The same label twice within the custom text.
    p.raw.behaviors = "mine: mine { a = <1>; };".into();
    p.raw.devicetree = "/ {\n    x {\n        mine: other { b = \"c: d\"; };\n    };\n};".into();
    let found = errors(&p, &go60);
    assert_eq!(found.len(), 1);
    assert!(
        found[0].contains("defines `mine` more than once"),
        "{found:?}"
    );
    p.raw.devicetree = "&mine { a = <2>; };\n/ { y: z { }; };".into();
    assert!(errors(&p, &go60).is_empty());
}

#[test]
fn undo_and_redo_restore_exact_states() {
    let (project, _) = rich_project();
    let original = project.clone();
    let base = project.layers[0].id;
    let mut editor = Editor::new(project);
    assert!(!editor.is_dirty());

    editor
        .edit("Set key", |p| p.set_binding(base, 10, kp("Q")))
        .unwrap();
    let after_first = editor.project().clone();
    let added = editor.edit("Add layer", |p| p.add_layer("Extra")).unwrap();
    assert!(editor.is_dirty());
    assert_eq!(editor.undo_label(), Some("Add layer"));

    assert_eq!(editor.undo().as_deref(), Some("Add layer"));
    assert_eq!(editor.project(), &after_first);
    assert_eq!(editor.undo().as_deref(), Some("Set key"));
    assert_eq!(editor.project(), &original);
    assert!(!editor.is_dirty(), "back at the saved state");
    assert_eq!(editor.undo(), None);

    assert_eq!(editor.redo().as_deref(), Some("Set key"));
    assert_eq!(editor.redo().as_deref(), Some("Add layer"));
    assert!(editor.project().layer(added).is_some());
    assert_eq!(editor.redo(), None);

    // A new edit after undoing discards the redo history.
    editor.undo();
    editor
        .edit("Set key", |p| p.set_binding(base, 11, kp("W")))
        .unwrap();
    assert_eq!(editor.redo(), None);
}

#[test]
fn failed_and_empty_edits_leave_no_trace() {
    let (project, _) = rich_project();
    let original = project.clone();
    let base = project.layers[0].id;
    let mut editor = Editor::new(project);

    let result = editor.edit("Broken", |p| {
        p.set_binding(base, 0, kp("Z"))?;
        p.set_binding(base, 999, kp("Z"))
    });
    assert!(result.is_err());
    assert_eq!(
        editor.project(),
        &original,
        "the partial change was rolled back"
    );

    let same = original.binding(base, 0).unwrap().clone();
    editor
        .edit("No-op", |p| p.set_binding(base, 0, same))
        .unwrap();
    assert_eq!(editor.undo_label(), None);
    assert!(!editor.is_dirty());
}

#[test]
fn grouped_edits_undo_as_one_step() {
    let (project, _) = rich_project();
    let original = project.clone();
    let base = project.layers[0].id;
    let mut editor = Editor::new(project);

    editor.begin_group("Paint keys");
    for position in 20..25 {
        editor
            .edit("Set key", |p| p.set_binding(base, position, kp("X")))
            .unwrap();
    }
    editor.end_group();
    editor.mark_saved();

    assert_eq!(editor.undo().as_deref(), Some("Paint keys"));
    assert_eq!(editor.project(), &original);
    assert!(editor.is_dirty(), "the saved state is one redo away");
    editor.redo();
    assert!(!editor.is_dirty());
}

/// Random edit sequences always undo back to the start and redo to the end.
#[test]
fn any_edit_sequence_round_trips_through_undo() {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % bound as u64) as usize
    };
    for _ in 0..20 {
        let (project, _) = rich_project();
        let original = project.clone();
        let mut editor = Editor::new(project);
        let mut steps = 0;
        for _ in 0..40 {
            let layers: Vec<LayerId> = editor.project().layers.iter().map(|l| l.id).collect();
            let layer = layers[next(layers.len())];
            let (position, to) = (next(60), next(layers.len()));
            let before = editor.project().clone();
            let _ = match next(6) {
                0 => editor.edit("set", |p| p.set_binding(layer, position, kp("A"))),
                1 => editor.edit("add", |p| p.add_layer("New").map(|_| ())),
                2 => editor.edit("move", |p| p.move_layer(layer, to)),
                3 => editor.edit("remove", |p| p.remove_layer_and_references(layer)),
                4 => editor.edit("duplicate", |p| p.duplicate_layer(layer).map(|_| ())),
                _ => editor.edit("light", |p| {
                    p.lighting_mut(layer)?.keys[position] = KeyLight::Off;
                    Ok(())
                }),
            };
            if editor.project() != &before {
                steps += 1;
            }
        }
        let last = editor.project().clone();
        let undone = std::iter::from_fn(|| editor.undo()).count();
        assert_eq!(undone, steps);
        assert_eq!(editor.project(), &original);
        while editor.redo().is_some() {}
        assert_eq!(editor.project(), &last);
    }
}

#[test]
fn project_files_round_trip_and_diff_cleanly() {
    let (mut p, _) = rich_project();
    let base = p.layers[0].id;
    p.lighting_mut(base).unwrap().keys[5] = KeyLight::Color(Rgb(0, 128, 255));

    let text = file::to_json(&p);
    assert_eq!(file::from_json(&text).unwrap(), p);
    assert!(text.starts_with("{\n  \"format\": 2,\n  \"name\": \"Rich\","));
    // Each binding sits on its own line, so changing a key changes one line.
    assert!(text
        .contains("\n        {\"behavior\": \"kp\", \"params\": [{\"key\": \"LC(LS(K))\"}]},\n"));
    assert!(text.lines().all(|l| l.len() <= 140), "no sprawling lines");

    let mut changed = p.clone();
    changed.set_binding(base, 20, kp("Z")).unwrap();
    let other = file::to_json(&changed);
    let differing = text
        .lines()
        .zip(other.lines())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        (differing, text.lines().count()),
        (1, other.lines().count())
    );
}

#[test]
fn project_files_save_and_load_from_disk() {
    let (p, _) = rich_project();
    let dir = std::env::temp_dir().join(format!("kc-model-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("rich.{}", file::EXTENSION));
    file::save(&p, &path).unwrap();
    assert_eq!(file::load(&path).unwrap(), p);
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        1,
        "no temp file left"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn unreadable_project_files_explain_themselves() {
    let (p, _) = rich_project();
    let newer = file::to_json(&p).replacen("\"format\": 2", "\"format\": 99", 1);
    assert!(matches!(
        file::from_json(&newer),
        Err(file::FileError::Newer {
            found: 99,
            supported: 2
        })
    ));
    assert!(matches!(
        file::from_json("{}"),
        Err(file::FileError::NotAProject(_))
    ));
    assert!(matches!(
        file::from_json("not json"),
        Err(file::FileError::NotAProject(_))
    ));
    assert!(matches!(
        file::from_json(r#"{"format": 1}"#),
        Err(file::FileError::Invalid(_))
    ));
}

#[test]
fn keycaps_show_what_a_key_does() {
    use kc_model::keycap::{keycap, KeycapKind};

    let (mut p, _) = rich_project();
    let (base, nav) = (p.layers[0].id, p.layers[1].id);
    let cap = |p: &Project, layer, position| keycap(p, layer, position).unwrap();

    let chord = cap(&p, base, 0);
    assert_eq!(
        (chord.legend.as_str(), chord.kind),
        ("⌃⇧K", KeycapKind::Key)
    );

    let momentary = cap(&p, base, 1);
    assert_eq!(momentary.legend, "Nav");
    assert_eq!(momentary.hold.as_deref(), Some("hold"));
    assert_eq!(momentary.kind, KeycapKind::Layer);

    assert_eq!(cap(&p, base, 2).legend, "BT SEL 2");
    assert_eq!(cap(&p, base, 2).kind, KeycapKind::System);

    // The user-defined hold-tap shows both sides.
    let magic = cap(&p, base, 36);
    assert_eq!(
        (magic.legend.as_str(), magic.hold.as_deref()),
        ("Esc", Some("Magic"))
    );

    // A transparent key shows what it inherits, flagged as transparent.
    let inherited = cap(&p, nav, 0);
    assert_eq!(
        (inherited.legend.as_str(), inherited.kind),
        ("⌃⇧K", KeycapKind::Transparent)
    );
    // On the base layer there is nothing below to inherit.
    p.set_binding(base, 50, Binding::trans()).unwrap();
    assert_eq!(cap(&p, base, 50).legend, "");

    p.set_binding(
        base,
        5,
        Binding::new(
            "mt",
            vec![
                Param::Key(KeyExpr::new("LSHFT")),
                Param::Key(KeyExpr::new("A")),
            ],
        ),
    )
    .unwrap();
    let mod_tap = cap(&p, base, 5);
    assert_eq!(
        (mod_tap.legend.as_str(), mod_tap.hold.as_deref()),
        ("A", Some("Shift"))
    );
    assert!(keycap(&p, base, 60).is_none());
}

#[test]
fn bindings_round_trip_through_keymap_text() {
    use kc_model::text::{format_binding, parse_binding, LayerStyle};

    let (p, _) = rich_project();
    let nav = p.layers[1].id;
    let cases = [
        ("&kp LC(LS(K))", kp("LC(LS(K))")),
        ("&mo 1", Binding::layer("mo", nav)),
        ("&bt BT_SEL 2", command("bt", "BT_SEL", &[2])),
        ("&bt BT_CLR", command("bt", "BT_CLR", &[])),
        (
            "&rgb_ug RGB_COLOR_HSB(120,100,50)",
            command("rgb_ug", "RGB_COLOR_HSB", &[120, 100, 50]),
        ),
        (
            "&mkp LCLK",
            Binding::new("mkp", vec![Param::Constant("LCLK".into())]),
        ),
        ("&trans", Binding::trans()),
        (
            "&mt LSHFT A",
            Binding::new(
                "mt",
                vec![
                    Param::Key(KeyExpr::new("LSHFT")),
                    Param::Key(KeyExpr::new("A")),
                ],
            ),
        ),
        (
            "&lt 1 SPACE",
            Binding::new(
                "lt",
                vec![Param::Layer(nav), Param::Key(KeyExpr::new("SPACE"))],
            ),
        ),
        ("&nav_td", Binding::user(p.behaviors[1].id, vec![])),
    ];
    for (text, binding) in cases {
        assert_eq!(parse_binding(&p, text), binding, "{text}");
        assert_eq!(format_binding(&p, &binding, LayerStyle::Index), text);
    }

    // Layers can be named, and are emitted as constants when asked.
    assert_eq!(parse_binding(&p, "&mo Nav"), Binding::layer("mo", nav));
    assert_eq!(
        parse_binding(&p, "&mo LAYER_Nav"),
        Binding::layer("mo", nav)
    );
    assert_eq!(
        format_binding(&p, &Binding::layer("mo", nav), LayerStyle::Constant),
        "&mo LAYER_Nav"
    );
    // The user-defined hold-tap takes a layer and a key.
    let magic = parse_binding(&p, "&magic Magic ESC");
    assert_eq!(magic, *p.binding(p.layers[0].id, 36).unwrap());
    assert_eq!(
        format_binding(&p, &magic, LayerStyle::Index),
        "&magic 2 ESC"
    );
    // Loose spacing inside a call is tolerated.
    assert_eq!(
        parse_binding(&p, "  &rgb_ug   RGB_COLOR_HSB(120, 100, 50) "),
        command("rgb_ug", "RGB_COLOR_HSB", &[120, 100, 50])
    );
}

#[test]
fn text_the_model_cannot_read_is_kept_verbatim() {
    use kc_model::text::{format_binding, parse_binding, LayerStyle};

    let (p, _) = rich_project();
    for text in [
        "&unknown 1 2",
        "&kp",
        "&kp A B",
        "&mo 9",
        "&bt BT_SEL",
        "&mkp NOPE",
        "kp A",
        "",
    ] {
        let binding = parse_binding(&p, text);
        assert_eq!(binding, Binding::Raw { raw: text.into() }, "{text}");
        assert_eq!(format_binding(&p, &binding, LayerStyle::Index), text);
    }
}

#[test]
fn the_picker_offers_what_the_firmware_supports() {
    use kc_model::picker::{picker_items, PickerGroup};

    let (p, go60) = rich_project();
    let features = &stock(&go60).features(&go60);
    let items = picker_items(&p, features);
    let find = |label: &str| items.iter().find(|i| i.label == label);

    assert_eq!(find("A").unwrap().binding, kp("A"));
    assert_eq!(find("A").unwrap().group, PickerGroup::Basic);
    assert_eq!(
        find("mo Nav").unwrap().binding,
        Binding::layer("mo", p.layers[1].id)
    );
    assert_eq!(
        find("BT SEL 4").unwrap().binding,
        command("bt", "BT_SEL", &[4])
    );
    assert_eq!(find("Left click").unwrap().group, PickerGroup::Mouse);
    assert_eq!(find("Nav tap-dance").unwrap().group, PickerGroup::Custom);
    // A behavior the user named says what kind it is; built-in keys do not
    // need to.
    assert_eq!(find("Nav tap-dance").unwrap().kind, Some("Tap-dance"));
    assert!(find("Nav tap-dance").unwrap().matches("tap-dance tapped"));
    assert_eq!(find("A").unwrap().kind, None);
    for def in &p.behaviors {
        assert_eq!(find(&def.name).unwrap().kind, Some(def.kind.name()));
        assert!(!def.kind.summary().is_empty());
    }
    assert!(find("Transparent").is_some() && find("Bootloader").is_some());
    // MoErgo's status command is offered on the Go60; backlight is not.
    assert!(find("RGB STATUS").is_some());
    assert!(find("BL TOG").is_none());
    // Multi-argument commands and parameterized behaviors start from
    // defaults and are tuned in the inspector.
    assert_eq!(
        find("RGB COLOR HSB").unwrap().binding,
        command("rgb_ug", "RGB_COLOR_HSB", &[180, 50, 50])
    );
    assert_eq!(
        find("Magic").unwrap().binding,
        Binding::user(
            p.behaviors[0].id,
            vec![Param::Layer(p.layers[0].id), Param::Key(KeyExpr::new("A"))]
        )
    );

    let imprint = board("cyboard-imprint");
    let plain = Project::new("Plain", &imprint);
    let imprint_items = picker_items(&plain, &imprint.firmware[0].features());
    assert!(!imprint_items.iter().any(|i| i.label == "RGB STATUS"));

    // Search matches names, aliases and descriptions, in any order.
    let hits = |q: &str| items.iter().filter(|i| i.matches(q)).count();
    assert!(find("Enter").unwrap().matches("ret"));
    assert!(items
        .iter()
        .any(|i| i.description == "Play/Pause" && i.matches("pause play c_pp")));
    assert!(find("BT SEL 2").unwrap().matches("bluetooth profile"));
    assert_eq!(hits("zzzz"), 0);
    assert_eq!(hits(""), items.len());
    // Every assignable binding is valid for the board.
    let mut all = p.clone();
    for item in &items {
        all.set_binding(all.layers[0].id, 0, item.binding.clone())
            .unwrap();
        assert!(errors(&all, &go60).is_empty(), "{}", item.label);
    }
}

#[test]
fn keys_and_layers_copy_and_paste_as_text() {
    use kc_model::clipboard::{copy_keys, copy_layer, paste, PasteError};

    let (mut p, go60) = rich_project();
    let (base, nav) = (p.layers[0].id, p.layers[1].id);

    // One key pastes onto every target.
    let single = copy_keys(&p, base, &[1]).unwrap();
    assert_eq!(paste(&mut p, nav, &[10, 11], &single), Ok(2));
    assert_eq!(p.binding(nav, 10), Some(&Binding::layer("mo", nav)));
    assert_eq!(
        paste(&mut p, nav, &[10, 11], &single),
        Ok(0),
        "nothing left to change"
    );

    // Several keys keep their arrangement, anchored at the first target.
    let several = copy_keys(&p, base, &[2, 0]).unwrap();
    assert_eq!(paste(&mut p, nav, &[20], &several), Ok(2));
    assert_eq!(p.binding(nav, 20), p.binding(base, 0));
    assert_eq!(p.binding(nav, 22), p.binding(base, 2));
    // Keys that would land off the keyboard are skipped.
    assert_eq!(paste(&mut p, nav, &[59], &several), Ok(1));

    // A whole layer replaces the target layer, references included.
    let layer = copy_layer(&p, base).unwrap();
    let extra = p.add_layer("Extra").unwrap();
    assert!(paste(&mut p, extra, &[], &layer).unwrap() > 40);
    assert_eq!(
        p.layer(extra).unwrap().bindings,
        p.layer(base).unwrap().bindings
    );
    assert!(errors(&p, &go60).is_empty());

    // Layers travel by name, so a paste into another project still resolves.
    let mut other = Project::new("Other", &go60);
    other.add_layer("Nav").unwrap();
    let other_base = other.layers[0].id;
    assert_eq!(paste(&mut other, other_base, &[5], &single), Ok(1));
    assert_eq!(
        other.binding(other_base, 5),
        Some(&Binding::layer("mo", other.layers[1].id))
    );

    assert_eq!(paste(&mut p, nav, &[], &single), Err(PasteError::NoTarget));
    assert_eq!(paste(&mut p, nav, &[0], "hello"), Err(PasteError::NotKeys));
    let mut imprint = Project::new("Imprint", &board("cyboard-imprint"));
    let first = imprint.layers[0].id;
    assert_eq!(
        paste(&mut imprint, first, &[0], &single),
        Err(PasteError::OtherBoard("moergo-go60".into()))
    );
    assert!(copy_keys(&p, base, &[]).is_none());

    p.swap_bindings(base, 0, 1).unwrap();
    assert_eq!(p.binding(base, 0), Some(&Binding::layer("mo", nav)));
    assert!(p.swap_bindings(base, 0, 60).is_err());
}

#[test]
fn slots_reach_bindings_inside_behaviors_and_combos() {
    use kc_model::Slot;

    let (mut p, go60) = rich_project();
    let dance = p.behaviors[1].id;
    let hello = p.behaviors[2].id;
    let combo = p.combos[0].id;
    let nav = p.layers[1].id;

    assert_eq!(
        p.slot(Slot::TapDance {
            behavior: dance,
            index: 1
        }),
        Some(&Binding::layer("to", nav))
    );
    assert_eq!(
        p.slot(Slot::MacroStep {
            behavior: hello,
            step: 0,
            index: 1
        }),
        Some(&kp("I"))
    );
    assert_eq!(p.slot(Slot::Combo(combo)), Some(&kp("ESC")));
    assert_eq!(
        p.slot(Slot::TapDance {
            behavior: hello,
            index: 0
        }),
        None,
        "not a tap-dance"
    );
    assert_eq!(
        p.slot(Slot::MacroStep {
            behavior: hello,
            step: 1,
            index: 0
        }),
        None,
        "a pause holds no binding"
    );

    p.set_slot(
        Slot::TapDance {
            behavior: dance,
            index: 0,
        },
        kp("A"),
    )
    .unwrap();
    p.set_slot(
        Slot::MacroStep {
            behavior: hello,
            step: 0,
            index: 0,
        },
        kp("Y"),
    )
    .unwrap();
    p.set_slot(Slot::Combo(combo), kp("TAB")).unwrap();
    assert_eq!(
        p.slot(Slot::TapDance {
            behavior: dance,
            index: 0
        }),
        Some(&kp("A"))
    );
    assert_eq!(p.slot(Slot::Combo(combo)), Some(&kp("TAB")));
    assert_eq!(
        p.set_slot(
            Slot::TapDance {
                behavior: dance,
                index: 9
            },
            kp("A")
        ),
        Err(ModelError::NoSuchSlot)
    );
    assert!(errors(&p, &go60).is_empty());
}

#[test]
fn raw_behaviors_can_be_assigned_and_missing_bootloader_keys_are_flagged() {
    use kc_model::picker::{picker_items, raw_behaviors};

    let (mut p, go60) = rich_project();
    p.raw.behaviors = "
        td_q: tap_dance_q {
            compatible = \"zmk,behavior-tap-dance\";
            #binding-cells = <0>;
        };
        my_ht: my_hold_tap { compatible = \"zmk,behavior-hold-tap\"; };
        scaled: scaled {
            #binding-cells = <2>;
        };"
    .into();
    assert_eq!(
        raw_behaviors(&p.raw.behaviors),
        [("td_q".to_string(), 0), ("scaled".to_string(), 2)]
    );
    let items = picker_items(&p, &go60.firmware[0].features());
    let scaled = items.iter().find(|i| i.label == "scaled").unwrap();
    assert_eq!(
        scaled.binding,
        Binding::Raw {
            raw: "&scaled 0 0".into()
        }
    );

    // The rich project has no bootloader key.
    let warning = "no key enters the bootloader";
    assert!(validate(&p, &go60, &stock(&go60))
        .iter()
        .any(|w| w.message.contains(warning)));
    p.set_binding(p.layers[2].id, 0, Binding::new("bootloader", vec![]))
        .unwrap();
    assert!(!validate(&p, &go60, &stock(&go60))
        .iter()
        .any(|w| w.message.contains(warning)));
}

#[test]
fn factory_templates_reproduce_the_vendor_layouts_without_raw_bindings() {
    use kc_model::keycap::keycap;

    for (id, layers, behaviors, reserved) in
        [("cyboard-imprint", 5, 0, 27), ("moergo-go60", 5, 12, 0)]
    {
        let board = board(id);
        let p = Project::from_template("Mine", &board);
        assert_eq!(p.name, "Mine");
        assert_eq!(
            (p.layers.len(), p.behaviors.len(), p.reserved_layers),
            (layers, behaviors, reserved),
            "{id}"
        );
        // Everything the vendor ships is expressed in the model itself.
        assert!(
            p.bindings()
                .iter()
                .all(|(_, b)| !matches!(b, Binding::Raw { .. })),
            "{id}"
        );
        assert_eq!(validate(&p, &board, &stock(&board)), [], "{id}");
        assert_eq!(file::from_json(&file::to_json(&p)).unwrap(), p);
    }

    let imprint = Project::from_template("Mine", &board("cyboard-imprint"));
    let names: Vec<&str> = imprint.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Base",
            "Numpad and Nav",
            "Keyboard Control",
            "Auto Mouse",
            "Factory Test"
        ]
    );
    // The left trackball scrolls, as it does out of the box.
    assert_eq!(imprint.pointing[0].listener, "trackball_central_listener");

    let go60 = Project::from_template("Mine", &board("moergo-go60"));
    let base = go60.layers[0].id;
    // The Magic key: hold for the Magic layer, tap for the status lights.
    let magic = keycap(&go60, base, 36).unwrap();
    assert_eq!(magic.hold.as_deref(), Some("Magic"));
    assert_eq!(go60.binding(base, 0), Some(&kp("EQUAL")));
    assert_eq!(go60.pointing.len(), 2);
}

#[test]
fn lights_resolve_through_layers_and_follow_key_types() {
    use kc_model::features::LockKind;
    use kc_model::lighting::{by_key_type, display_color, effective, LAYER_COLOR, MODIFIER_COLOR};

    let go60 = board("moergo-go60");
    let mut p = Project::from_template("Lit", &go60);
    let (base, keypad, symbol) = (p.layers[0].id, p.layers[1].id, p.layers[2].id);
    let red = KeyLight::Color(Rgb(255, 0, 0));
    p.lighting_mut(base).unwrap().keys[5] = red;
    p.lighting_mut(symbol).unwrap().keys[6] = KeyLight::Off;

    // A key with no light of its own shows the nearest one below.
    assert_eq!(effective(&p, base, 5), (red, false));
    assert_eq!(effective(&p, keypad, 5), (red, true));
    assert_eq!(effective(&p, symbol, 5), (red, true));
    assert_eq!(effective(&p, symbol, 6), (KeyLight::Off, false));
    assert_eq!(effective(&p, symbol, 7), (KeyLight::Inherit, false));
    // Layers above do not shine down.
    assert_eq!(effective(&p, keypad, 6), (KeyLight::Inherit, false));

    let lock = KeyLight::Lock {
        lock: LockKind::Caps,
        off: Rgb(0, 0, 0),
        on: Rgb(1, 2, 3),
    };
    assert_eq!(display_color(lock), Some(Rgb(1, 2, 3)));
    assert_eq!(display_color(KeyLight::Off), None);

    let scheme = by_key_type(&p, base);
    assert_eq!(scheme.len(), 60);
    // The Go60's base layer has Shift on the left thumb and the Magic key.
    assert_eq!(scheme[55], KeyLight::Color(MODIFIER_COLOR));
    assert!(
        scheme.contains(&KeyLight::Color(LAYER_COLOR)) || scheme.iter().any(|l| *l != scheme[0])
    );
    let upper = by_key_type(&p, keypad);
    assert!(
        upper.contains(&KeyLight::Inherit),
        "transparent keys inherit"
    );
}

#[test]
fn led_check_patterns_and_unverified_maps() {
    use kc_model::lighting::led_check;

    let imprint = board("cyboard-imprint");
    let keys = &imprint.layout(&imprint.default_layout).unwrap().keys;
    let (rows, columns) = led_check(keys);
    assert_eq!((rows.len(), columns.len()), (82, 82));
    // The first twelve keys are one row; the thirteenth starts the next.
    assert!(rows[..12].iter().all(|l| *l == rows[0]));
    assert_ne!(rows[12], rows[0]);
    // Keys in the same physical column share a color down the board.
    assert_eq!(columns[0], columns[12]);
    assert_ne!(columns[0], columns[1]);

    let mut p = Project::from_template("Check", &imprint);
    let base = p.layers[0].id;
    p.lighting_mut(base).unwrap().keys = rows;
    // The stock firmware has no per-key lighting; the colors are kept
    // out of sight rather than reported.
    assert!(errors(&p, &imprint).is_empty());
    let lit = FirmwareConfig::new("kc-zmk-0.3-perkey");
    assert!(errors_with(&p, &imprint, &lit).is_empty());
    let problems = validate(&p, &imprint, &lit);
    assert!(problems
        .iter()
        .any(|w| w.message.contains("have not been confirmed on hardware")));
    // Our firmware starts in the per-key effect, so no effect key is needed.
    assert!(!problems
        .iter()
        .any(|w| w.message.contains("no key changes the lighting effect")));

    // The LED map is only known for the 82-key layout.
    p.layout = "physical_layout_imprint_number_row".into();
    assert!(errors_with(&p, &imprint, &lit)
        .iter()
        .any(|e| e.contains("not available for this key layout")));
}

#[test]
fn boards_offer_their_templates_with_credit() {
    use kc_model::project::TEMPLATES;
    let imprint = board("cyboard-imprint");
    let ids: Vec<&str> = Project::templates(&imprint).iter().map(|t| t.id).collect();
    assert_eq!(ids, ["factory", "tailorkey"]);
    assert_eq!(
        Project::templates(&board("moergo-go60"))
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>(),
        ["factory"]
    );
    let tailorkey = TEMPLATES
        .iter()
        .find(|t| t.id == "tailorkey" && t.board == "cyboard-imprint")
        .unwrap();
    let credit = tailorkey
        .credit
        .expect("a community layout names its author");
    assert_eq!(credit.author, "Moosy Research");
    assert!(credit.url.starts_with("https://"));
    // Every template loads for its board and is valid on every firmware
    // the board offers with lighting; plain RMK has no lighting or mouse
    // keys, and flags those keys as it should.
    for template in TEMPLATES {
        let board = board(template.board);
        let project = Project::from_template_id(template.id, "Mine", &board).unwrap();
        assert_eq!(
            project.key_count,
            board.layout(&board.default_layout).unwrap().keys.len()
        );
        for profile in board.firmware.iter().filter(|f| {
            f.capabilities
                .contains(&kc_boards::board::Capability::RgbUnderglow)
        }) {
            let config = kc_model::FirmwareConfig::new(profile.id.clone());
            let errors: Vec<_> = kc_model::validate(&project, &board, &config)
                .into_iter()
                .filter(|p| p.severity == kc_model::Severity::Error)
                .collect();
            assert_eq!(errors, [], "{} on {}", template.id, profile.id);
        }
    }
    assert!(Project::from_template_id("nope", "Mine", &imprint).is_none());
    // The factory layout is still the default.
    assert_eq!(
        Project::from_template("Mine", &imprint).layers.len(),
        Project::from_template_id("factory", "Mine", &imprint)
            .unwrap()
            .layers
            .len()
    );
}

/// TailorKey on the Imprint: twelve layers, the home-row mods split by
/// hand with the other hand's keys and the thumbs as their triggers, the
/// trackballs scaled by the mouse-speed layers, and TailorKey's colors.
#[test]
fn the_tailorkey_template_fits_the_imprint() {
    use kc_model::behavior::BehaviorKind;
    use kc_model::features::{InputProcessor, KeyLight};
    let imprint = board("cyboard-imprint");
    let p = Project::from_template_id("tailorkey", "TailorKey", &imprint).unwrap();
    assert_eq!(p.layers.len(), 12);
    assert_eq!(p.layers[0].name, "Base");
    let hold_taps: Vec<_> = p
        .behaviors
        .iter()
        .filter_map(|b| match &b.kind {
            BehaviorKind::HoldTap(h) => Some((b.label.as_str(), h)),
            _ => None,
        })
        .collect();
    assert_eq!(hold_taps.len(), 11);
    let (_, left) = hold_taps.iter().find(|(l, _)| *l == "hrm_pinky").unwrap();
    let (_, right) = hold_taps
        .iter()
        .find(|(l, _)| *l == "hrm_pinky_right")
        .unwrap();
    assert!(!left.opposite_hand_hold && left.hold_trigger_on_release);
    // The left pinky holds for right-hand keys (6, the top row's F6) and
    // the left thumbs (70), never for the left hand's own A (37).
    assert!(left.hold_trigger_key_positions.contains(&6));
    assert!(left.hold_trigger_key_positions.contains(&70));
    assert!(!left.hold_trigger_key_positions.contains(&37));
    assert!(right.hold_trigger_key_positions.contains(&37));
    assert!(!right.hold_trigger_key_positions.contains(&46));
    // A on the left home row and ; on the right use their hand's behavior.
    let label_at = |position: usize| match &p.layers[0].bindings[position] {
        kc_model::Binding::Behavior {
            behavior: kc_model::BehaviorRef::User { user },
            ..
        } => p.behavior(*user).unwrap().label.clone(),
        other => format!("{other:?}"),
    };
    assert_eq!(label_at(37), "hrm_pinky");
    assert_eq!(label_at(46), "hrm_pinky_right");
    assert_eq!(p.combos.len(), 14);
    assert_eq!(p.pointing.len(), 2);
    let right_ball = p
        .pointing
        .iter()
        .find(|c| c.listener == "trackball_peripheral_listener")
        .unwrap();
    assert_eq!(right_ball.overrides.len(), 3);
    assert!(right_ball.overrides.iter().any(|o| o.processors
        == [InputProcessor::Scale {
            multiplier: 12,
            divisor: 1
        }]));
    // TailorKey's colors: the home-row mods on the base layer, and the
    // lock lights on F2 to F4.
    let base = p.lighting(p.layers[0].id).unwrap();
    assert_eq!(base.fade_delay, Some(15));
    assert!(matches!(base.keys[37], KeyLight::Color(_)));
    assert!(matches!(base.keys[3], KeyLight::Lock { .. }));
    assert_eq!(base.keys[0], KeyLight::Off);
    assert_eq!(p.lighting.len(), 10);
}

#[test]
fn keys_move_to_another_layout() {
    let imprint = board("cyboard-imprint");
    let mut p = Project::from_template("Mine", &imprint);
    let base = p.layers[0].id;
    let first = p.layers[0].bindings[1].clone();
    let last = p.layers[0].bindings[81].clone();
    let combo = p.add_combo("pair", vec![1, 2], kc_model::Binding::none());
    p.add_combo("lost", vec![0, 1], kc_model::Binding::none());
    // Reverse the keys, and leave key 0 without a counterpart.
    let mut map: Vec<Option<usize>> = (0..82).map(|i| Some(81 - i)).collect();
    map[0] = None;
    let dropped = p
        .remap_keys(&imprint, &imprint.default_layout, &map)
        .unwrap();
    assert_eq!(dropped, ["lost"]);
    assert_eq!(p.layers[0].bindings[80], first);
    assert_eq!(p.layers[0].bindings[0], last);
    assert_eq!(p.layers[0].bindings[81], kc_model::Binding::trans());
    assert_eq!(p.combos.len(), 1);
    assert_eq!(p.combos[0].id, combo);
    assert_eq!(p.combos[0].key_positions, [80, 79]);
    assert_eq!(p.key_count, 82);
    let _ = base;
    assert!(matches!(
        p.remap_keys(&imprint, &imprint.default_layout, &map[..10]),
        Err(kc_model::ModelError::KeyMapSize {
            given: 10,
            keys: 82
        })
    ));
}
