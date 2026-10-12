//! Keys described in slots: reading, writing, templates and tidiness.

use kc_boards::Board;
use kc_model::behavior::{
    BehaviorKind, Flavor, HoldTapSettings, Macro, MacroStep, ModMorphSettings, Scope, Settings,
    StickyKeySettings, TapDanceSettings,
};
use kc_model::setup::{Choice, KeySetup, Part, SetupError};
use kc_model::{
    file, validate, words, Binding, Editor, FirmwareConfig, KeyExpr, Location, Param, Project,
};
use kc_zmk::Modifier;

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

fn go60() -> (Project, Board) {
    let board = board("moergo-go60");
    let mut p = Project::new("Slots", &board);
    p.add_layer("Nav").unwrap();
    (p, board)
}

fn errors(project: &Project, board: &Board) -> Vec<String> {
    validate(project, board, &FirmwareConfig::stock(board))
        .into_iter()
        .filter(|p| p.severity == kc_model::Severity::Error)
        .map(|p| format!("{:?}: {}", p.location, p.message))
        .collect()
}

fn text(project: &Project, layer: kc_model::LayerId, position: usize) -> String {
    kc_model::text::format_binding(
        project,
        project.binding(layer, position).unwrap(),
        kc_model::text::LayerStyle::Index,
    )
}

fn tuned() -> HoldTapSettings {
    HoldTapSettings {
        flavor: Flavor::Balanced,
        tapping_term_ms: 250,
        ..HoldTapSettings::default()
    }
}

#[test]
fn a_plain_key_reads_and_writes_as_its_tap() {
    let (mut p, board) = go60();
    let base = p.layers[0].id;
    p.set_binding(base, 0, kp("A")).unwrap();

    let setup = KeySetup::of(&p, base, 0).unwrap();
    assert_eq!(setup.tap, kp("A"));
    assert!(!setup.has_extras());
    p.apply_setup(base, 0, &setup).unwrap();
    assert_eq!(p.binding(base, 0), Some(&kp("A")));
    assert!(p.behaviors.is_empty());

    // Transparent and raw keys.
    p.set_binding(base, 1, Binding::trans()).unwrap();
    assert_eq!(KeySetup::of(&p, base, 1).unwrap().tap, Binding::trans());
    p.set_binding(
        base,
        2,
        Binding::Raw {
            raw: "&weird 1".into(),
        },
    )
    .unwrap();
    assert_eq!(KeySetup::of(&p, base, 2), None);
    assert!(errors(&p, &board).is_empty());
}

#[test]
fn holds_with_the_default_feel_use_built_in_behaviors() {
    let (mut p, board) = go60();
    let (base, nav) = (p.layers[0].id, p.layers[1].id);

    let mut setup = KeySetup::plain(kp("A"));
    setup.hold = Some(kp("LSHFT"));
    p.apply_setup(base, 0, &setup).unwrap();
    assert_eq!(text(&p, base, 0), "&mt LSHFT A");
    assert!(p.behaviors.is_empty(), "no definition needed");

    setup.hold = Some(Binding::layer("mo", nav));
    p.apply_setup(base, 1, &setup).unwrap();
    assert_eq!(text(&p, base, 1), "&lt 1 A");

    let back = KeySetup::of(&p, base, 1).unwrap();
    assert_eq!(back.hold, Some(Binding::layer("mo", nav)));
    assert_eq!(back.tap, kp("A"));
    assert_eq!(back.feel.hold, Choice::Default);

    // A hold that no built-in can do gets a definition with default settings.
    setup.hold = Some(Binding::new("caps_word", vec![]));
    p.apply_setup(base, 2, &setup).unwrap();
    assert_eq!(text(&p, base, 2), "&ht_1 0 A");
    let back = KeySetup::of(&p, base, 2).unwrap();
    assert_eq!(back.hold, Some(Binding::new("caps_word", vec![])));
    assert_eq!(back.feel.hold, Choice::Default);
    assert_eq!(p.behaviors[0].scope, Scope::Key);
    assert!(errors(&p, &board).is_empty());
}

#[test]
fn a_custom_feel_gets_a_key_owned_definition_that_keys_share_until_edited() {
    let (mut p, board) = go60();
    let base = p.layers[0].id;

    let mut setup = KeySetup::plain(kp("A"));
    setup.hold = Some(kp("LSHFT"));
    setup.feel.hold = Choice::Custom(tuned());
    p.apply_setup(base, 0, &setup).unwrap();
    assert_eq!(text(&p, base, 0), "&ht_1 LSHFT A");
    let def = &p.behaviors[0];
    assert_eq!(
        (def.scope, def.follows, def.label.as_str()),
        (Scope::Key, None, "ht_1")
    );
    assert!(matches!(&def.kind, BehaviorKind::HoldTap(h) if h.flavor == Flavor::Balanced));

    // The same feel on another key shares the definition.
    setup.tap = kp("S");
    setup.hold = Some(kp("LCTRL"));
    p.apply_setup(base, 1, &setup).unwrap();
    assert_eq!(text(&p, base, 1), "&ht_1 LCTRL S");
    assert_eq!(p.behaviors.len(), 1);

    // Reading gives the custom feel back; changing the tap keeps the label.
    let mut read = KeySetup::of(&p, base, 0).unwrap();
    assert_eq!(read.feel.hold, Choice::Custom(tuned()));
    read.tap = kp("B");
    p.apply_setup(base, 0, &read).unwrap();
    assert_eq!(text(&p, base, 0), "&ht_1 LSHFT B");

    // Changing one key's feel leaves the other key alone: copy on write.
    let mut read = KeySetup::of(&p, base, 0).unwrap();
    read.feel.hold = Choice::Custom(HoldTapSettings {
        tapping_term_ms: 300,
        ..tuned()
    });
    p.apply_setup(base, 0, &read).unwrap();
    assert_eq!(text(&p, base, 0), "&ht_2 LSHFT B");
    assert_eq!(text(&p, base, 1), "&ht_1 LCTRL S");
    assert_eq!(p.behaviors.len(), 2);

    // Alone on its definition, the key edits it in place.
    let mut read = KeySetup::of(&p, base, 0).unwrap();
    read.feel.hold = Choice::Custom(HoldTapSettings {
        tapping_term_ms: 310,
        ..tuned()
    });
    p.apply_setup(base, 0, &read).unwrap();
    assert_eq!(text(&p, base, 0), "&ht_2 LSHFT B");
    assert_eq!(p.behaviors.len(), 2);

    // Back to the default feel: the definition goes.
    read.feel.hold = Choice::Default;
    p.apply_setup(base, 0, &read).unwrap();
    assert_eq!(text(&p, base, 0), "&mt LSHFT B");
    assert_eq!(p.behaviors.len(), 1);
    assert!(errors(&p, &board).is_empty());
}

#[test]
fn second_taps_nest_a_multi_tap_around_the_hold() {
    let (mut p, board) = go60();
    let (base, nav) = (p.layers[0].id, p.layers[1].id);

    let mut setup = KeySetup::plain(kp("A"));
    setup.double_tap = Some(kp("B"));
    p.apply_setup(base, 0, &setup).unwrap();
    assert_eq!(text(&p, base, 0), "&td_1");
    let back = KeySetup::of(&p, base, 0).unwrap();
    assert_eq!(
        (back.tap.clone(), back.double_tap.clone()),
        (kp("A"), Some(kp("B")))
    );
    assert_eq!(back.feel.taps, Choice::Default);

    // Hold, double tap, tap-then-hold and a third tap together.
    setup.hold = Some(kp("LSHFT"));
    setup.tap_hold = Some(Binding::layer("mo", nav));
    setup.more_taps = vec![kp("C")];
    setup.feel.taps = Choice::Custom(TapDanceSettings {
        tapping_term_ms: 150,
    });
    p.apply_setup(base, 0, &setup).unwrap();
    let back = KeySetup::of(&p, base, 0).unwrap();
    assert_eq!(back.tap, kp("A"));
    assert_eq!(back.hold, Some(kp("LSHFT")));
    assert_eq!(back.double_tap, Some(kp("B")));
    assert_eq!(back.tap_hold, Some(Binding::layer("mo", nav)));
    assert_eq!(back.more_taps, [kp("C")]);
    assert_eq!(
        back.feel.taps,
        Choice::Custom(TapDanceSettings {
            tapping_term_ms: 150
        })
    );
    assert_eq!(back.feel.hold, Choice::Default);
    let dance = p
        .behavior(p.binding(base, 0).unwrap().user_behavior().unwrap())
        .unwrap();
    let BehaviorKind::TapDance(td) = &dance.kind else {
        panic!("a multi-tap")
    };
    assert_eq!(td.bindings.len(), 3);
    assert_eq!(
        kc_model::text::format_binding(&p, &td.bindings[0], kc_model::text::LayerStyle::Index),
        "&mt LSHFT A"
    );
    assert_eq!(
        kc_model::text::format_binding(&p, &td.bindings[1], kc_model::text::LayerStyle::Index),
        "&lt 1 B"
    );
    assert_eq!(p.behaviors.len(), 1, "the hold-taps are built in");

    // Tap-then-hold with nothing on the double tap.
    setup.double_tap = None;
    p.apply_setup(base, 0, &setup).unwrap();
    let back = KeySetup::of(&p, base, 0).unwrap();
    assert_eq!(back.double_tap, None);
    assert_eq!(back.tap_hold, Some(Binding::layer("mo", nav)));

    assert_eq!(
        words::describe(&p, p.binding(base, 0).unwrap()),
        "A, hold for Shift; tap twice for Nothing, hold for Nav; three times for C"
    );
    assert!(errors(&p, &board).is_empty());
}

#[test]
fn shift_overrides_and_sticky_keys_wrap_the_tap() {
    let (mut p, board) = go60();
    let (base, nav) = (p.layers[0].id, p.layers[1].id);

    let mut setup = KeySetup::plain(kp("COMMA"));
    setup.shifted = Some(kp("SEMICOLON"));
    p.apply_setup(base, 0, &setup).unwrap();
    assert_eq!(text(&p, base, 0), "&mm_1");
    let back = KeySetup::of(&p, base, 0).unwrap();
    assert_eq!(back.shifted, Some(kp("SEMICOLON")));
    assert_eq!(back.feel.shifted, Choice::Default);
    assert_eq!(
        words::describe(&p, p.binding(base, 0).unwrap()),
        ",, with ⇧ held ;"
    );

    setup.feel.shifted = Choice::Custom(ModMorphSettings {
        mods: vec![],
        keep_mods: vec![],
    });
    assert_eq!(p.apply_setup(base, 0, &setup), Err(SetupError::NoModifiers));
    setup.feel.shifted = Choice::Custom(ModMorphSettings {
        mods: vec![Modifier::LAlt],
        keep_mods: vec![Modifier::LAlt],
    });
    p.apply_setup(base, 0, &setup).unwrap();
    let back = KeySetup::of(&p, base, 0).unwrap();
    assert!(matches!(back.feel.shifted, Choice::Custom(ref s) if s.mods == [Modifier::LAlt]));

    // Sticky modifiers and layers use the built-ins by default.
    let mut sticky = KeySetup::plain(kp("LSHFT"));
    sticky.sticky = true;
    p.apply_setup(base, 1, &sticky).unwrap();
    assert_eq!(text(&p, base, 1), "&sk LSHFT");
    sticky.tap = Binding::layer("mo", nav);
    p.apply_setup(base, 2, &sticky).unwrap();
    assert_eq!(text(&p, base, 2), "&sl 1");
    let back = KeySetup::of(&p, base, 2).unwrap();
    assert!(back.sticky);
    assert_eq!(back.tap, Binding::layer("mo", nav));

    // With its own feel, or under a hold, it needs a definition.
    sticky.tap = kp("LSHFT");
    sticky.feel.sticky = Choice::Custom(StickyKeySettings {
        release_after_ms: 500,
        ..StickyKeySettings::default()
    });
    p.apply_setup(base, 1, &sticky).unwrap();
    assert_eq!(text(&p, base, 1), "&sk_1 LSHFT");
    sticky.feel.sticky = Choice::Default;
    sticky.hold = Some(Binding::layer("mo", nav));
    p.apply_setup(base, 1, &sticky).unwrap();
    assert_eq!(text(&p, base, 1), "&ht_1 1 LSHFT");
    let back = KeySetup::of(&p, base, 1).unwrap();
    assert!(back.sticky);
    assert_eq!(back.hold, Some(Binding::layer("mo", nav)));
    assert_eq!(back.tap, kp("LSHFT"));
    assert_eq!(
        words::describe(&p, p.binding(base, 1).unwrap()),
        "Sticky Shift, hold for Nav"
    );
    assert!(errors(&p, &board).is_empty());
}

#[test]
fn sides_must_be_single_things() {
    let (mut p, _) = go60();
    let base = p.layers[0].id;
    let mut setup = KeySetup::plain(Binding::new(
        "bt",
        vec![Param::Command {
            name: "BT_SEL".into(),
            args: vec![1],
        }],
    ));
    setup.hold = Some(kp("LSHFT"));
    assert_eq!(p.apply_setup(base, 0, &setup), Err(SetupError::NotASide));
    setup.tap = Binding::Raw { raw: "&x".into() };
    assert_eq!(p.apply_setup(base, 0, &setup), Err(SetupError::NotASide));
    setup.tap = kp("A");
    setup.hold = None;
    setup.feel.hold = Choice::Template(kc_model::BehaviorId(99));
    setup.tap_hold = Some(kp("LSHFT"));
    assert_eq!(
        p.apply_setup(base, 0, &setup),
        Err(SetupError::NoSuchTemplate)
    );
}

#[test]
fn templates_are_saved_from_a_key_and_followed_by_others() {
    let (mut p, board) = go60();
    let (base, nav) = (p.layers[0].id, p.layers[1].id);

    let mut setup = KeySetup::plain(kp("A"));
    setup.hold = Some(kp("LSHFT"));
    setup.feel.hold = Choice::Custom(tuned());
    p.apply_setup(base, 0, &setup).unwrap();
    let template = p
        .save_as_template(base, 0, Part::Hold, "Home-row mod")
        .unwrap();
    let def = p.behavior(template).unwrap();
    assert_eq!(
        (def.scope, def.label.as_str(), def.name.as_str()),
        (Scope::Template, "home_row_mod", "Home-row mod")
    );
    assert_eq!(text(&p, base, 0), "&home_row_mod LSHFT A");
    assert_eq!(
        p.behaviors.len(),
        1,
        "the key-owned definition became the template"
    );
    assert_eq!(
        KeySetup::of(&p, base, 0).unwrap().feel.hold,
        Choice::Template(template)
    );
    assert_eq!(p.behavior_templates().count(), 1);

    // A second key with a layer hold follows the template through a
    // hidden variant, since its sides differ.
    let mut other = KeySetup::plain(kp("SPACE"));
    other.hold = Some(Binding::layer("mo", nav));
    other.feel.hold = Choice::Template(template);
    p.apply_setup(base, 1, &other).unwrap();
    assert_eq!(text(&p, base, 1), "&ht_1 1 SPACE");
    let variant = p
        .behavior(p.binding(base, 1).unwrap().user_behavior().unwrap())
        .unwrap()
        .clone();
    assert_eq!(
        (variant.scope, variant.follows),
        (Scope::Key, Some(template))
    );
    assert_eq!(
        KeySetup::of(&p, base, 1).unwrap().feel.hold,
        Choice::Template(template)
    );
    assert_eq!(p.behavior_templates().count(), 1);

    let mut uses = p.template_uses(template);
    uses.sort_by_key(|l| format!("{l:?}"));
    assert_eq!(
        uses,
        [
            Location::Key {
                layer: base,
                position: 0
            },
            Location::Key {
                layer: base,
                position: 1
            }
        ]
    );

    // Changing the template changes both keys.
    p.set_template_settings(
        template,
        &Settings::HoldTap(HoldTapSettings {
            tapping_term_ms: 400,
            ..tuned()
        }),
    )
    .unwrap();
    for id in [template, variant.id] {
        let BehaviorKind::HoldTap(h) = &p.behavior(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(h.tapping_term_ms, 400);
    }

    // Detaching keeps the settings as the key's own.
    p.detach(base, 1, Part::Hold).unwrap();
    let back = KeySetup::of(&p, base, 1).unwrap();
    assert!(matches!(back.feel.hold, Choice::Custom(ref s) if s.tapping_term_ms == 400));
    assert_eq!(
        p.template_uses(template),
        [Location::Key {
            layer: base,
            position: 0
        }]
    );

    // Renaming and duplicating.
    p.rename_template(template, "HRM").unwrap();
    let copy = p.duplicate_template(template, "HRM slow").unwrap();
    assert_eq!(p.behavior(copy).unwrap().label, "hrm_slow");
    assert_eq!(p.behavior_templates().count(), 2);
    assert!(p.template_uses(copy).is_empty());

    // Deleting a used template leaves the key with its settings.
    p.delete_template(template).unwrap();
    assert_eq!(p.behavior_templates().count(), 1);
    let back = KeySetup::of(&p, base, 0).unwrap();
    assert!(matches!(back.feel.hold, Choice::Custom(ref s) if s.tapping_term_ms == 400));
    assert_eq!(
        text(&p, base, 0),
        "&home_row_mod LSHFT A",
        "the label stays"
    );
    assert!(errors(&p, &board).is_empty());

    // Saving the other parts.
    let mut multi = KeySetup::plain(kp("Q"));
    multi.double_tap = Some(kp("ESC"));
    multi.feel.taps = Choice::Custom(TapDanceSettings {
        tapping_term_ms: 180,
    });
    p.apply_setup(base, 2, &multi).unwrap();
    let taps = p
        .save_as_template(base, 2, Part::Taps, "Quick double")
        .unwrap();
    assert_eq!(
        KeySetup::of(&p, base, 2).unwrap().feel.taps,
        Choice::Template(taps)
    );
    let follower = p
        .behavior(p.binding(base, 2).unwrap().user_behavior().unwrap())
        .unwrap();
    assert_eq!(follower.follows, Some(taps));
    assert_eq!(
        p.template_uses(taps),
        [Location::Key {
            layer: base,
            position: 2
        }]
    );
    assert_eq!(
        p.save_as_template(base, 2, Part::Shifted, "No"),
        Err(SetupError::NotSlots)
    );
}

#[test]
fn macros_built_on_a_key_can_become_templates() {
    let (mut p, board) = go60();
    let base = p.layers[0].id;
    let steps = BehaviorKind::Macro(Macro {
        wait_ms: None,
        tap_ms: None,
        params: 0,
        steps: vec![MacroStep::Tap(vec![kp("H"), kp("I")])],
    });
    let id = p.add_behavior("macro_1", "", steps).unwrap();
    p.behavior_mut(id).unwrap().scope = Scope::Key;
    p.set_binding(base, 0, Binding::user(id, vec![])).unwrap();
    assert_eq!(words::describe(&p, p.binding(base, 0).unwrap()), "Macro");

    p.make_template(id, "Say hi").unwrap();
    let def = p.behavior(id).unwrap();
    assert_eq!((def.scope, def.label.as_str()), (Scope::Template, "say_hi"));
    assert_eq!(
        words::describe(&p, p.binding(base, 0).unwrap()),
        "Macro: Say hi"
    );
    p.set_binding(base, 1, Binding::user(id, vec![])).unwrap();

    // Detaching one key copies the macro for it.
    p.detach_from_template(base, 1, id).unwrap();
    let own = p.binding(base, 1).unwrap().user_behavior().unwrap();
    assert_ne!(own, id);
    assert_eq!(p.behavior(own).unwrap().scope, Scope::Key);
    assert_eq!(p.binding(base, 0).unwrap().user_behavior(), Some(id));
    assert!(errors(&p, &board).is_empty());
}

#[test]
fn edits_through_the_editor_drop_definitions_nothing_uses() {
    let (p, _) = go60();
    let base = p.layers[0].id;
    let mut editor = Editor::new(p);
    let mut setup = KeySetup::plain(kp("A"));
    setup.double_tap = Some(kp("B"));
    editor
        .edit("Set up key", |p| {
            p.apply_setup(base, 0, &setup)
                .map_err(|_| kc_model::ModelError::NoSuchSlot)
        })
        .unwrap();
    assert_eq!(editor.project().behaviors.len(), 1);
    editor
        .edit("Plain key", |p| p.set_binding(base, 0, kp("Z")))
        .unwrap();
    assert!(editor.project().behaviors.is_empty());
    editor.undo();
    assert_eq!(editor.project().behaviors.len(), 1);
}

#[test]
fn older_files_sort_their_definitions_into_templates_and_keys() {
    let (mut p, board) = go60();
    let (base, nav) = (p.layers[0].id, p.layers[1].id);
    let hold = p
        .add_behavior(
            "thumb",
            "Thumb",
            BehaviorKind::HoldTap(kc_model::behavior::HoldTap::new(
                kc_model::BehaviorRef::built_in("mo"),
                kc_model::BehaviorRef::built_in("kp"),
            )),
        )
        .unwrap();
    let once = p
        .add_behavior(
            "td_once",
            "Once",
            BehaviorKind::TapDance(kc_model::behavior::TapDance {
                tapping_term_ms: 200,
                bindings: vec![kp("A"), kp("B")],
            }),
        )
        .unwrap();
    let shared = p
        .add_behavior(
            "td_shared",
            "Shared",
            BehaviorKind::TapDance(kc_model::behavior::TapDance {
                tapping_term_ms: 200,
                bindings: vec![kp("C"), kp("D")],
            }),
        )
        .unwrap();
    let unused = p
        .add_behavior(
            "spare",
            "Spare",
            BehaviorKind::Macro(Macro {
                wait_ms: None,
                tap_ms: None,
                params: 0,
                steps: vec![],
            }),
        )
        .unwrap();
    p.set_binding(
        base,
        0,
        Binding::user(
            hold,
            vec![Param::Layer(nav), Param::Key(KeyExpr::new("SPACE"))],
        ),
    )
    .unwrap();
    p.set_binding(base, 1, Binding::user(once, vec![])).unwrap();
    p.set_binding(base, 2, Binding::user(shared, vec![]))
        .unwrap();
    p.set_binding(base, 3, Binding::user(shared, vec![]))
        .unwrap();

    let old = file::to_json(&p).replacen("\"format\": 3", "\"format\": 2", 1);
    let old = old.replace("\"scope\": \"template\", ", "");
    let loaded = file::from_json(&old).unwrap();
    let scope = |id| loaded.behavior(id).unwrap().scope;
    assert_eq!(scope(hold), Scope::Template, "takes parameters");
    assert_eq!(scope(once), Scope::Key, "used once, takes none");
    assert_eq!(
        loaded.behavior(once).unwrap().name,
        "Once",
        "kept as the key's note"
    );
    assert_eq!(scope(shared), Scope::Template);
    assert_eq!(scope(unused), Scope::Template);
    // Keys sharing a multi-tap each follow it with their own copy, merged
    // while identical.
    let copies: Vec<_> = loaded
        .behaviors
        .iter()
        .filter(|d| d.follows == Some(shared))
        .collect();
    assert_eq!(copies.len(), 1);
    assert_eq!(loaded.binding(base, 2), loaded.binding(base, 3));
    assert_eq!(
        loaded.binding(base, 2).unwrap().user_behavior(),
        Some(copies[0].id)
    );
    assert_eq!(
        KeySetup::of(&loaded, base, 2).unwrap().feel.taps,
        Choice::Template(shared)
    );
    assert_eq!(KeySetup::of(&loaded, base, 1).unwrap().note, "Once");
    assert!(errors(&loaded, &board).is_empty());

    // The shipped templates come out the same way.
    let factory = Project::from_template("Go60", &board);
    let templates: Vec<&str> = factory
        .behavior_templates()
        .map(|d| d.label.as_str())
        .collect();
    assert_eq!(templates, ["magic"]);
    assert!(
        factory
            .behaviors
            .iter()
            .filter(|d| d.scope == Scope::Key)
            .count()
            > 5
    );
    for layer in &factory.layers {
        for position in 0..factory.key_count {
            let binding = factory.binding(layer.id, position).unwrap();
            if binding.user_behavior().is_some() {
                assert!(
                    KeySetup::of(&factory, layer.id, position).is_some(),
                    "{binding:?}"
                );
            }
        }
    }
}

#[test]
fn problems_inside_a_key_owned_definition_are_reported_at_the_key() {
    let (mut p, board) = go60();
    let base = p.layers[0].id;
    let mut setup = KeySetup::plain(kp("A"));
    setup.double_tap = Some(Binding::layer("mo", kc_model::LayerId(999)));
    p.apply_setup(base, 0, &setup).unwrap();
    let problems = errors(&p, &board);
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0].starts_with(&format!(
            "{:?}",
            Location::Key {
                layer: base,
                position: 0
            }
        )),
        "{problems:?}"
    );
}

#[test]
fn copied_keys_bring_their_definitions_along() {
    use kc_model::clipboard::{copy_keys, paste};

    let (mut p, board) = go60();
    let (base, nav) = (p.layers[0].id, p.layers[1].id);
    let mut setup = KeySetup::plain(kp("A"));
    setup.hold = Some(Binding::layer("mo", nav));
    setup.double_tap = Some(kp("B"));
    setup.feel.hold = Choice::Custom(tuned());
    p.apply_setup(base, 0, &setup).unwrap();
    let template = p.save_as_template(base, 0, Part::Hold, "Thumb").unwrap();
    assert_eq!(p.behaviors.len(), 2);

    let clip = copy_keys(&p, base, &[0]).unwrap();
    // Within the project: nothing is duplicated.
    assert_eq!(paste(&mut p, nav, &[4], &clip), Ok(1));
    assert_eq!(p.behaviors.len(), 2);
    assert_eq!(p.binding(nav, 4), p.binding(base, 0));

    // Into another project: the template and the key's definition are
    // recreated, once.
    let mut other = Project::new("Other", &board);
    other.add_layer("Nav").unwrap();
    let (other_base, other_nav) = (other.layers[0].id, other.layers[1].id);
    assert_eq!(paste(&mut other, other_base, &[0, 1], &clip), Ok(2));
    assert_eq!(other.behaviors.len(), 2);
    let brought = KeySetup::of(&other, other_base, 0).unwrap();
    assert_eq!(brought.hold, Some(Binding::layer("mo", other_nav)));
    assert_eq!(brought.double_tap, Some(kp("B")));
    let thumb = other.behavior_templates().next().unwrap();
    assert_eq!(
        (thumb.label.as_str(), thumb.name.as_str()),
        ("thumb", "Thumb")
    );
    assert_eq!(brought.feel.hold, Choice::Template(thumb.id));
    assert_ne!(thumb.id, template, "IDs are the target's own");
    assert!(errors(&other, &board).is_empty());

    // Without the layer, the binding that names it is kept as text.
    let mut bare = Project::new("Bare", &board);
    let bare_base = bare.layers[0].id;
    assert_eq!(paste(&mut bare, bare_base, &[0], &clip), Ok(1));
    assert!(errors(&bare, &board).is_empty() || !bare.behaviors.is_empty());
}

#[test]
fn settings_and_bindings_are_described_in_words() {
    let (mut p, _) = go60();
    let (base, nav) = (p.layers[0].id, p.layers[1].id);
    assert_eq!(
        Settings::HoldTap(HoldTapSettings::home_row()).summary(),
        "Balanced, counts as a hold after 280 ms, tap twice quickly to repeat, no hold right after typing, only with keys on the other hand, decided when the other key is released"
    );
    assert_eq!(
        Settings::TapDance(TapDanceSettings::default()).summary(),
        "a second tap within 200 ms"
    );
    assert_eq!(
        Settings::ModMorph(ModMorphSettings::default()).summary(),
        "with ⇧ held"
    );
    assert_eq!(
        Settings::StickyKey(StickyKeySettings::default()).summary(),
        "lets go after 1000 ms, lets modifiers through"
    );
    assert_eq!(Flavor::TapPreferred.name(), "Favor tap");

    p.set_binding(base, 0, Binding::layer("tog", nav)).unwrap();
    assert_eq!(
        words::describe(&p, p.binding(base, 0).unwrap()),
        "Toggle Nav"
    );
    assert_eq!(
        words::describe(&p, &Binding::layer("sl", nav)),
        "Nav for the next key"
    );
    assert_eq!(
        words::describe(&p, &Binding::trans()),
        "Same as the layer below"
    );
    assert_eq!(words::describe(&p, &kp("LC(LS(K))")), "⌃⇧K");
    assert_eq!(
        words::describe(
            &p,
            &Binding::new(
                "bt",
                vec![Param::Command {
                    name: "BT_SEL".into(),
                    args: vec![2]
                }]
            )
        ),
        "BT 2"
    );
    assert_eq!(words::modifier_name(Modifier::RGui), "Right ⌘");
    assert_eq!(
        words::modifier_symbols(&[Modifier::LShift, Modifier::RShift, Modifier::LCtrl]),
        "⇧⌃"
    );
}
