//! Behaviour of the project model against the real board definitions.

use kc_boards::Board;
use kc_model::behavior::{BehaviorKind, HoldTap, Macro, MacroStep, TapDance};
use kc_model::features::{
    ConditionalLayer, InputProcessor, KeyLight, PointingConfig, PointingOverride, Rgb, SettingValue,
};
use kc_model::validate::BRIGHTNESS_MAX_SETTING;
use kc_model::{
    file, validate, BehaviorRef, Binding, Editor, KeyExpr, LayerId, Location, ModelError, Param,
    Project, Severity,
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

fn errors(project: &Project, board: &Board) -> Vec<String> {
    validate(project, board)
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
    p.settings
        .insert("CONFIG_ZMK_SLEEP".into(), SettingValue::Bool(true));
    p.raw.devicetree = "/* custom */".into();
    (p, go60)
}

#[test]
fn new_projects_match_their_board() {
    let imprint = board("cyboard-imprint");
    let p = Project::new("Mine", &imprint);
    assert_eq!((p.key_count, p.layers.len()), (82, 1));
    assert_eq!(p.firmware, "cyboard-zmk-0.3");
    assert!(validate(&p, &imprint).is_empty());

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
    assert_eq!(validate(&p, &go60), []);
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
    assert_eq!(validate(&p, &go60), []);
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
    assert_eq!(validate(&p, &go60), [], "no dangling references remain");
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
fn behaviour_labels_and_references_are_protected() {
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
    p.settings
        .insert(BRIGHTNESS_MAX_SETTING.into(), SettingValue::Int(80));
    p.lighting_mut(base).unwrap().keys[0] = KeyLight::Off;
    p.add_combo("Solo", vec![200], Binding::trans());

    let problems = validate(&p, &imprint);
    let found = |text: &str| problems.iter().any(|p| p.message.contains(text));
    assert!(found("RGB_STATUS is not supported"));
    assert!(found("BT_SEL profile must be between 0 and 4, found 9"));
    assert!(found("&kp takes 1 parameter(s), found 0"));
    assert!(found("&mo has the wrong kind of value"));
    assert!(found("&nope is not a ZMK behaviour"));
    assert!(found("refers to a layer that no longer exists"));
    assert!(found("&bl is not supported"));
    assert!(found("brightness 80 is above this board's limit of 50"));
    assert!(found("per-key lighting is not supported"));
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
    assert!(text.starts_with("{\n  \"format\": 1,\n  \"name\": \"Rich\","));
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
    let newer = file::to_json(&p).replacen("\"format\": 1", "\"format\": 99", 1);
    assert!(matches!(
        file::from_json(&newer),
        Err(file::FileError::Newer {
            found: 99,
            supported: 1
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
