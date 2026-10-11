//! The key tester: matching what the computer sees to the keys, layers and
//! features of a layout.

use std::time::{Duration, Instant};

use kc_boards::Board;
use kc_model::behavior::{BehaviorKind, HoldTap, Macro, MacroStep, ModMorph, TapDance};
use kc_model::tester::{
    grid, Direction, FacetState, Hit, ModClass, Mode, Mods, Outcome, Signal, Tester,
};
use kc_model::{BehaviorRef, Binding, KeyExpr, LayerId, Param, Project};
use kc_zmk::keycodes::UsagePage;
use kc_zmk::Modifier;

fn board(id: &str) -> Board {
    kc_boards::built_in()
        .unwrap()
        .into_iter()
        .find(|b| b.id == id)
        .unwrap()
}

fn key(name: &str) -> Param {
    Param::Key(KeyExpr::new(name))
}

fn kp(name: &str) -> Binding {
    Binding::kp(KeyExpr::new(name))
}

/// A Go60 layout with nothing but what each test puts on it.
fn blank() -> (Project, LayerId, LayerId) {
    let go60 = board("moergo-go60");
    let mut project = Project::new("Test", &go60);
    let base = project.layers[0].id;
    for position in 0..project.key_count {
        project
            .set_binding(base, position, Binding::trans())
            .unwrap();
    }
    let nav = project.add_layer("Nav").unwrap();
    (project, base, nav)
}

fn hit(layer: LayerId, position: usize, facet: usize) -> Hit {
    Hit {
        layer,
        position,
        facet,
    }
}

fn now() -> Instant {
    Instant::now()
}

fn later(from: Instant, ms: u64) -> Instant {
    from + Duration::from_millis(ms)
}

/// Presses and lets go of a key with nothing held.
fn tap(tester: &mut Tester, name: &str, at: Instant) -> Outcome {
    let outcome = tester.key_down(name, &name.to_uppercase(), Mods::NONE, at);
    tester.key_up(name);
    outcome
}

fn state(tester: &Tester, position: usize, facet: usize) -> FacetState {
    tester.key_view(position).facets[facet].state
}

#[test]
fn every_function_of_a_binding_is_a_facet() {
    let (mut project, base, nav) = blank();
    project.set_binding(base, 0, kp("A")).unwrap();
    project
        .set_binding(base, 1, Binding::new("mt", vec![key("LSHFT"), key("A")]))
        .unwrap();
    project
        .set_binding(
            base,
            2,
            Binding::new("lt", vec![Param::Layer(nav), key("SPACE")]),
        )
        .unwrap();
    project
        .set_binding(base, 3, Binding::layer("mo", nav))
        .unwrap();
    project.set_binding(base, 4, kp("EXCL")).unwrap();
    project
        .set_binding(
            base,
            5,
            Binding::new(
                "bt",
                vec![Param::Command {
                    name: "BT_SEL".into(),
                    args: vec![0],
                }],
            ),
        )
        .unwrap();
    project
        .set_binding(
            base,
            6,
            Binding::new("mmv", vec![Param::Constant("MOVE_LEFT".into())]),
        )
        .unwrap();
    project
        .set_binding(base, 7, Binding::kp(KeyExpr::new("A").with(Modifier::LGui)))
        .unwrap();
    project.set_binding(base, 8, kp("LCTRL")).unwrap();
    // Transparent on Nav: what the base layer has there.
    project.set_binding(nav, 0, Binding::trans()).unwrap();
    project.set_binding(nav, 1, Binding::none()).unwrap();

    let tester = Tester::new(project);
    let a = (UsagePage::Keyboard, 0x04);
    assert_eq!(
        tester.facets(base, 0)[0].signal,
        Signal::Key {
            usage: a,
            mods: Mods::NONE
        }
    );
    let mt = tester.facets(base, 1);
    assert_eq!(mt.len(), 2);
    assert_eq!((mt[0].role.as_str(), mt[0].label.as_str()), ("tap", "A"));
    assert_eq!(mt[1].role, "hold");
    assert_eq!(mt[1].signal, Signal::Modifier(ModClass::Shift));
    let lt = tester.facets(base, 2);
    assert_eq!(lt[1].signal, Signal::Layer(nav));
    assert_eq!(lt[1].label, "Nav");
    assert_eq!(tester.facets(base, 3)[0].signal, Signal::Layer(nav));
    assert_eq!(
        tester.facets(base, 4)[0].signal,
        Signal::Key {
            usage: (UsagePage::Keyboard, 0x1E),
            mods: Mods::NONE.with(ModClass::Shift)
        }
    );
    assert_eq!(tester.facets(base, 5)[0].signal, Signal::Unseen);
    assert!(!tester.facets(base, 5)[0].testable());
    assert_eq!(
        tester.facets(base, 6)[0].signal,
        Signal::Pointer(Direction::Left)
    );
    assert_eq!(
        tester.facets(base, 7)[0].signal,
        Signal::Key {
            usage: a,
            mods: Mods::NONE.with(ModClass::Gui)
        }
    );
    assert_eq!(
        tester.facets(base, 8)[0].signal,
        Signal::Modifier(ModClass::Control)
    );
    assert_eq!(tester.facets(nav, 0), tester.facets(base, 0));
    assert!(tester.facets(nav, 1).is_empty());
    assert!(tester.facets(base, 9).is_empty());
    let mut tester = tester;
    assert!(!tester.key_view(0).inherited);
    tester.show(nav);
    assert!(tester.key_view(0).inherited);
    assert!(!tester.key_view(1).inherited);
}

/// KEY-71: on a layout with many layers, such as TailorKey on the Go60,
/// pressing A lit A, S and Z, because S sends A on a gaming layer and Z
/// sends Cmd-A on a cursor layer.
#[test]
fn a_press_lights_one_key_on_the_shown_layer() {
    let (mut project, base, cursor) = blank();
    let gaming = project.add_layer("Gaming").unwrap();
    project
        .set_binding(base, 25, Binding::new("mt", vec![key("LCTRL"), key("A")]))
        .unwrap();
    project.set_binding(base, 26, kp("S")).unwrap();
    project.set_binding(base, 37, kp("Z")).unwrap();
    project
        .set_binding(
            cursor,
            37,
            Binding::kp(KeyExpr::new("A").with(Modifier::LGui)),
        )
        .unwrap();
    project.set_binding(gaming, 26, kp("A")).unwrap();

    let mut tester = Tester::new(project);
    let outcome = tester.key_down("a", "A", Mods::NONE, now());
    assert_eq!(
        outcome,
        Outcome::Keys {
            layer: base,
            hits: vec![hit(base, 25, 0)],
            switched: false,
            completed: None,
        }
    );
    assert_eq!(state(&tester, 25, 0), FacetState::Pressed);
    assert_eq!(state(&tester, 25, 1), FacetState::Unseen);
    assert_eq!(state(&tester, 26, 0), FacetState::Unseen);
    assert_eq!(state(&tester, 37, 0), FacetState::Unseen);
    tester.key_up("a");
    assert_eq!(state(&tester, 25, 0), FacetState::Seen);
}

#[test]
fn a_key_only_another_layer_sends_moves_there_and_holds_what_leads_to_it() {
    let (mut project, base, nav) = blank();
    project.set_binding(base, 0, kp("A")).unwrap();
    project
        .set_binding(base, 1, Binding::layer("mo", nav))
        .unwrap();
    project.set_binding(base, 3, kp("B")).unwrap();
    project.set_binding(nav, 2, kp("LEFT")).unwrap();
    project.set_binding(nav, 0, Binding::trans()).unwrap();
    project.set_binding(nav, 3, Binding::none()).unwrap();

    let mut tester = Tester::new(project);
    let outcome = tester.key_down("left", "LEFT", Mods::NONE, now());
    assert_eq!(
        outcome,
        Outcome::Keys {
            layer: nav,
            hits: vec![hit(nav, 2, 0)],
            switched: true,
            completed: None,
        }
    );
    assert_eq!(tester.shown(), nav);
    // The layer key on the base layer is taken to be held: outlined where
    // it sits, and its function seen.
    assert!(tester.key_view(1).outlined);
    assert!(tester.is_seen(&hit(base, 1, 0)));
    tester.key_up("left");
    assert!(tester.key_view(1).outlined);

    // A transparent key on Nav is the base key: pressing it stays on Nav.
    let outcome = tester.key_down("a", "A", Mods::NONE, now());
    assert!(matches!(outcome, Outcome::Keys { layer, switched: false, .. } if layer == nav));
    assert_eq!(state(&tester, 0, 0), FacetState::Pressed);
    tester.key_up("a");

    // A key only the base layer has brings the tester back, and lets go
    // of the layer key.
    let outcome = tester.key_down("b", "B", Mods::NONE, now());
    assert!(matches!(outcome, Outcome::Keys { layer, switched: true, .. } if layer == base));
    assert_eq!(tester.shown(), base);
    assert!(!tester.key_view(1).outlined);
    assert_eq!(state(&tester, 1, 0), FacetState::Seen);
}

#[test]
fn marks_are_kept_per_layer() {
    let (mut project, base, nav) = blank();
    project.set_binding(base, 0, kp("A")).unwrap();
    project.set_binding(nav, 0, kp("LEFT")).unwrap();
    project.set_binding(nav, 1, kp("RIGHT")).unwrap();

    let mut tester = Tester::new(project);
    tap(&mut tester, "a", now());
    assert_eq!(state(&tester, 0, 0), FacetState::Seen);
    assert_eq!(tester.layer_progress(base).functions_seen, 1);

    tester.show(nav);
    assert_eq!(tester.shown(), nav);
    assert_eq!(state(&tester, 0, 0), FacetState::Unseen);
    assert_eq!(tester.layer_progress(nav).functions_seen, 0);
    tap(&mut tester, "left", now());
    assert_eq!(state(&tester, 0, 0), FacetState::Seen);
    assert_eq!(state(&tester, 1, 0), FacetState::Unseen);

    tester.show(base);
    assert_eq!(state(&tester, 0, 0), FacetState::Seen);
    assert_eq!(tester.total_progress().functions_seen, 2);
    assert_eq!(tester.total_progress().functions, 3);
}

#[test]
fn held_modifiers_light_the_keys_that_hold_them() {
    let (mut project, base, _) = blank();
    project
        .set_binding(base, 0, Binding::new("mt", vec![key("LSHFT"), key("F")]))
        .unwrap();
    project.set_binding(base, 1, kp("LSHFT")).unwrap();
    project.set_binding(base, 2, kp("N1")).unwrap();

    let mut tester = Tester::new(project);
    let shift = Mods::NONE.with(ModClass::Shift);
    let at = now();
    let outcomes = tester.modifiers(shift, at);
    assert_eq!(outcomes.len(), 1);
    assert!(matches!(&outcomes[0], Outcome::Keys { hits, .. } if hits.len() == 2));
    assert_eq!(state(&tester, 0, 1), FacetState::Pressed);
    assert_eq!(state(&tester, 0, 0), FacetState::Unseen);
    assert_eq!(state(&tester, 1, 0), FacetState::Pressed);
    assert_eq!(tester.mods(), shift);

    // Shift held well before a key that does not need it: both stay.
    tester.key_down("1", "N1", shift, later(at, 500));
    assert_eq!(state(&tester, 0, 1), FacetState::Pressed);
    assert_eq!(state(&tester, 2, 0), FacetState::Pressed);
    tester.key_up("1");
    tester.modifiers(Mods::NONE, later(at, 600));
    assert_eq!(state(&tester, 0, 1), FacetState::Seen);
    assert_eq!(state(&tester, 1, 0), FacetState::Seen);
}

#[test]
fn a_shifted_symbol_takes_its_shift_with_it() {
    let (mut project, base, symbols) = blank();
    project
        .set_binding(base, 0, Binding::new("mt", vec![key("LSHFT"), key("F")]))
        .unwrap();
    project.set_binding(base, 1, kp("N1")).unwrap();
    project.set_binding(symbols, 1, kp("EXCL")).unwrap();

    let mut tester = Tester::new(project);
    let shift = Mods::NONE.with(ModClass::Shift);
    let at = now();
    // On the base layer, Shift and "1" is the "1" key with Shift held:
    // that is how the keyboard makes an exclamation mark there.
    tester.modifiers(shift, at);
    let outcome = tester.key_down("1", "N1", shift, later(at, 5));
    assert!(matches!(outcome, Outcome::Keys { hits, .. } if hits == [hit(base, 1, 0)]));
    assert_eq!(state(&tester, 0, 1), FacetState::Pressed);
    tester.key_up("1");
    tester.modifiers(Mods::NONE, later(at, 100));

    // On the symbol layer, the same report is its exclamation mark, whose
    // shift is built in: the shift key of the layer below is not lit.
    tester.show(symbols);
    assert_eq!(state(&tester, 0, 1), FacetState::Unseen);
    tester.modifiers(shift, later(at, 1000));
    assert_eq!(state(&tester, 0, 1), FacetState::Pressed);
    let outcome = tester.key_down("1", "N1", shift, later(at, 1005));
    assert!(matches!(outcome, Outcome::Keys { hits, .. } if hits == [hit(symbols, 1, 0)]));
    assert_eq!(state(&tester, 0, 1), FacetState::Unseen);
    assert_eq!(state(&tester, 1, 0), FacetState::Pressed);
    tester.key_up("1");
    tester.modifiers(Mods::NONE, later(at, 1100));
    assert_eq!(state(&tester, 1, 0), FacetState::Seen);
    assert_eq!(state(&tester, 0, 1), FacetState::Unseen);
}

#[test]
fn the_most_specific_key_wins() {
    let (mut project, base, _) = blank();
    project.set_binding(base, 0, kp("N1")).unwrap();
    project.set_binding(base, 1, kp("EXCL")).unwrap();
    let mut tester = Tester::new(project);
    let shift = Mods::NONE.with(ModClass::Shift);
    let at = now();
    assert!(matches!(
        tester.key_down("1", "N1", Mods::NONE, at),
        Outcome::Keys { hits, .. } if hits == [hit(base, 0, 0)]
    ));
    tester.key_up("1");
    assert!(matches!(
        tester.key_down("1", "N1", shift, later(at, 1000)),
        Outcome::Keys { hits, .. } if hits == [hit(base, 1, 0)]
    ));
}

#[test]
fn combos_are_matched_by_what_they_send() {
    let (mut project, base, _) = blank();
    project.set_binding(base, 0, kp("A")).unwrap();
    project.set_binding(base, 1, kp("S")).unwrap();
    project.set_binding(base, 5, kp("ESC")).unwrap();
    let escape = project.add_combo("escape", vec![0, 1], kp("ESC"));
    let tab = project.add_combo("tab", vec![1, 2], kp("TAB"));

    let mut tester = Tester::new(project);
    // A key sends Escape too, so the key wins unless combos come first.
    assert!(matches!(
        tap(&mut tester, "escape", now()),
        Outcome::Keys { .. }
    ));
    assert_eq!(tester.combo_progress(), (0, 2));
    tester.set_mode(Mode::CombosAndMacros);
    let at = now();
    assert_eq!(
        tester.key_down("escape", "ESC", Mods::NONE, at),
        Outcome::Combo { combo: escape }
    );
    assert_eq!(tester.combo_keys(), Some((escape, vec![0, 1])));
    tester.key_up("escape");
    assert_eq!(tester.combo_keys(), None);
    assert_eq!(tester.combo_progress(), (1, 2));

    // Nothing but the combo sends Tab: it is the combo in either mode.
    tester.set_mode(Mode::Keys);
    assert_eq!(
        tester.key_down("tab", "TAB", Mods::NONE, later(at, 10)),
        Outcome::Combo { combo: tab }
    );
    tester.key_up("tab");
    assert_eq!(tester.combo_progress(), (2, 2));
}

#[test]
fn a_macro_is_its_keys_arriving_in_a_burst() {
    let (mut project, base, _) = blank();
    project.set_binding(base, 0, kp("DOT")).unwrap();
    let dots = project
        .add_behavior(
            "dotdot",
            "Two dots",
            BehaviorKind::Macro(Macro {
                wait_ms: None,
                tap_ms: None,
                params: 0,
                steps: vec![MacroStep::Tap(vec![kp("DOT"), kp("DOT")])],
            }),
        )
        .unwrap();
    project
        .set_binding(base, 1, Binding::user(dots, vec![]))
        .unwrap();

    let mut tester = Tester::new(project);
    let facet = &tester.facets(base, 1)[0];
    assert_eq!(facet.label, "Two dots");
    assert!(matches!(&facet.signal, Signal::Sequence(keys) if keys.len() == 2));
    assert_eq!(tester.macro_progress(), (0, 1));

    // Two dots typed slowly are two dots.
    let at = now();
    tap(&mut tester, "dot", at);
    let outcome = tap(&mut tester, "dot", later(at, 800));
    assert!(matches!(
        outcome,
        Outcome::Keys {
            completed: None,
            ..
        }
    ));
    assert_eq!(tester.macro_progress(), (0, 1));

    // Two in a burst are the macro; the key wins, the macro is marked.
    tap(&mut tester, "dot", later(at, 2000));
    let outcome = tap(&mut tester, "dot", later(at, 2100));
    assert!(matches!(outcome, Outcome::Keys { completed: Some(h), .. } if h == hit(base, 1, 0)));
    assert_eq!(tester.macro_progress(), (1, 1));
    assert_eq!(state(&tester, 1, 0), FacetState::Seen);

    // With combos and macros first, the macro's key is what lights.
    tester.reset();
    tester.set_mode(Mode::CombosAndMacros);
    tap(&mut tester, "dot", later(at, 3000));
    let outcome = tester.key_down("dot", "DOT", Mods::NONE, later(at, 3100));
    assert_eq!(
        outcome,
        Outcome::Macro {
            hit: hit(base, 1, 0)
        }
    );
    assert_eq!(state(&tester, 1, 0), FacetState::Pressed);
    tester.key_up("dot");
    assert_eq!(state(&tester, 1, 0), FacetState::Seen);
}

#[test]
fn hold_taps_tap_dances_and_mod_morphs_have_a_facet_per_function() {
    let (mut project, base, _) = blank();
    // A home-row mod whose tap is a macro that types the key it is given,
    // as TailorKey's are.
    let tap_macro = project
        .add_behavior(
            "hrm_tap",
            "HRM tap",
            BehaviorKind::Macro(Macro {
                wait_ms: Some(0),
                tap_ms: Some(0),
                params: 1,
                steps: vec![
                    MacroStep::Release(vec![kp("LSHFT")]),
                    MacroStep::Param { from: 1, to: 1 },
                    MacroStep::Tap(vec![kp("A")]),
                ],
            }),
        )
        .unwrap();
    let hrm = project
        .add_behavior(
            "hrm",
            "Home-row mod",
            BehaviorKind::HoldTap(HoldTap::new(
                BehaviorRef::built_in("kp"),
                BehaviorRef::User { user: tap_macro },
            )),
        )
        .unwrap();
    project
        .set_binding(base, 0, Binding::user(hrm, vec![key("LCTRL"), key("F")]))
        .unwrap();
    let dance = project
        .add_behavior(
            "esc_caps",
            "Escape or Caps",
            BehaviorKind::TapDance(TapDance {
                tapping_term_ms: 200,
                bindings: vec![kp("ESC"), kp("CAPS")],
            }),
        )
        .unwrap();
    project
        .set_binding(base, 1, Binding::user(dance, vec![]))
        .unwrap();
    let morph = project
        .add_behavior(
            "comma_semi",
            "Comma or semicolon",
            BehaviorKind::ModMorph(ModMorph {
                normal: kp("COMMA"),
                morphed: kp("SEMI"),
                mods: vec![Modifier::LShift, Modifier::RShift],
                keep_mods: vec![],
            }),
        )
        .unwrap();
    project
        .set_binding(base, 2, Binding::user(morph, vec![]))
        .unwrap();

    let mut tester = Tester::new(project);
    let f = tester.facets(base, 0);
    assert_eq!((f[0].role.as_str(), f[0].label.as_str()), ("tap", "F"));
    assert_eq!(f[1].role, "hold");
    assert_eq!(f[1].signal, Signal::Modifier(ModClass::Control));
    let td = tester.facets(base, 1);
    assert_eq!(
        (td[0].role.as_str(), td[1].role.as_str()),
        ("1 tap", "2 taps")
    );
    let mm = tester.facets(base, 2);
    assert_eq!(mm[1].role, "with ⇧");

    let at = now();
    tap(&mut tester, "f", at);
    assert_eq!(state(&tester, 0, 0), FacetState::Seen);
    assert_eq!(state(&tester, 0, 1), FacetState::Unseen);
    tester.modifiers(Mods::NONE.with(ModClass::Control), later(at, 100));
    assert_eq!(state(&tester, 0, 1), FacetState::Pressed);
    tester.modifiers(Mods::NONE, later(at, 400));
    assert_eq!(state(&tester, 0, 1), FacetState::Seen);
    assert_eq!(tester.layer_progress(base).keys_seen, 1);

    tap(&mut tester, "escape", later(at, 1000));
    assert_eq!(state(&tester, 1, 0), FacetState::Seen);
    assert_eq!(state(&tester, 1, 1), FacetState::Unseen);
    tester.key_down("capslock", "CAPS", Mods::NONE, later(at, 2000));
    tester.key_up("capslock");
    assert_eq!(state(&tester, 1, 1), FacetState::Seen);

    tap(&mut tester, "comma", later(at, 3000));
    assert_eq!(state(&tester, 2, 0), FacetState::Seen);
    let shift = Mods::NONE.with(ModClass::Shift);
    tester.modifiers(shift, later(at, 4000));
    tester.key_down(";", "SEMI", shift, later(at, 4005));
    assert_eq!(state(&tester, 2, 1), FacetState::Pressed);
    tester.key_up(";");
    tester.modifiers(Mods::NONE, later(at, 4100));
    assert_eq!(tester.layer_progress(base).keys_seen, 3);
}

#[test]
fn mouse_functions_are_matched_on_the_shown_layer_only() {
    let (mut project, base, mouse) = blank();
    project.set_binding(base, 0, kp("A")).unwrap();
    project
        .set_binding(
            mouse,
            1,
            Binding::new("mmv", vec![Param::Constant("MOVE_LEFT".into())]),
        )
        .unwrap();
    project
        .set_binding(
            mouse,
            2,
            Binding::new("msc", vec![Param::Constant("SCRL_DOWN".into())]),
        )
        .unwrap();
    project
        .set_binding(
            mouse,
            3,
            Binding::new("mkp", vec![Param::Constant("LCLK".into())]),
        )
        .unwrap();

    let mut tester = Tester::new(project);
    // The computer's own mouse moves too, so movement never changes layer.
    assert_eq!(tester.pointer(Direction::Left), Outcome::Unmatched);
    assert_eq!(tester.shown(), base);
    tester.show(mouse);
    assert!(matches!(
        tester.pointer(Direction::Left),
        Outcome::Keys { hits, .. } if hits == [hit(mouse, 1, 0)]
    ));
    assert_eq!(state(&tester, 1, 0), FacetState::Pressed);
    tester.pointer_stop();
    assert_eq!(state(&tester, 1, 0), FacetState::Seen);
    assert_eq!(tester.pointer(Direction::Right), Outcome::Unmatched);
    assert!(matches!(
        tester.scroll(Direction::Down),
        Outcome::Keys { .. }
    ));
    assert_eq!(state(&tester, 2, 0), FacetState::Pressed);
    assert!(matches!(tester.button(1, true), Outcome::Keys { .. }));
    assert_eq!(state(&tester, 3, 0), FacetState::Pressed);
    tester.button(1, false);
    assert_eq!(state(&tester, 3, 0), FacetState::Seen);
    assert_eq!(tester.button(2, true), Outcome::Unmatched);
}

#[test]
fn keys_that_send_nothing_cannot_be_tested_and_progress_counts_the_rest() {
    let (mut project, base, nav) = blank();
    project.set_binding(base, 0, kp("A")).unwrap();
    project
        .set_binding(base, 1, Binding::layer("mo", nav))
        .unwrap();
    project
        .set_binding(base, 2, Binding::new("bootloader", vec![]))
        .unwrap();
    project
        .set_binding(
            base,
            3,
            Binding::Raw {
                raw: "&custom".into(),
            },
        )
        .unwrap();
    project.set_binding(nav, 4, kp("F5")).unwrap();

    let mut tester = Tester::new(project);
    assert_eq!(state(&tester, 2, 0), FacetState::Untestable);
    assert_eq!(state(&tester, 3, 0), FacetState::Untestable);
    assert_eq!(tester.untestable_keys(base), 2);
    let progress = tester.layer_progress(base);
    assert_eq!((progress.keys, progress.functions), (2, 2));
    // The layer key is confirmed by a key only Nav sends.
    tap(&mut tester, "f5", now());
    assert_eq!(tester.shown(), nav);
    assert_eq!(tester.layer_progress(base).keys_seen, 1);
    assert_eq!(tester.layer_progress(nav).keys_seen, 1);
    assert_eq!(tester.total_progress().functions_seen, 2);

    tester.reset();
    assert_eq!(tester.total_progress().functions_seen, 0);
    assert_eq!(tester.shown(), nav);
}

#[test]
fn letting_go_of_everything_keeps_what_was_seen() {
    let (mut project, base, _) = blank();
    project.set_binding(base, 0, kp("A")).unwrap();
    project.set_binding(base, 1, kp("B")).unwrap();
    let mut tester = Tester::new(project);
    tester.key_down("a", "A", Mods::NONE, now());
    tester.key_down("b", "B", Mods::NONE, now());
    assert_eq!(state(&tester, 0, 0), FacetState::Pressed);
    tester.release_all();
    assert_eq!(state(&tester, 0, 0), FacetState::Seen);
    assert_eq!(state(&tester, 1, 0), FacetState::Seen);
    // Let go, a key can be pressed again.
    tester.key_down("a", "A", Mods::NONE, now());
    assert_eq!(state(&tester, 0, 0), FacetState::Pressed);
}

#[test]
fn keys_get_a_column_and_a_row_from_where_they_sit() {
    for id in ["moergo-go60", "cyboard-imprint", "dygma-defy"] {
        let board = board(id);
        let keys = &board.layout(&board.default_layout).unwrap().keys;
        let cells = grid(keys);
        assert_eq!(cells.len(), keys.len(), "{id}");
        // Columns run from the left edge and rows from the bottom.
        assert_eq!(cells.iter().map(|c| c.0).min(), Some(0), "{id}");
        assert_eq!(cells.iter().map(|c| c.1).min(), Some(0), "{id}");
        assert!(cells.iter().all(|c| c.0 >= 0 && c.1 >= 0), "{id}");
        // A board is wider than it is tall, and has several of each.
        let columns = cells.iter().map(|c| c.0).max().unwrap();
        let rows = cells.iter().map(|c| c.1).max().unwrap();
        assert!(columns > rows && rows >= 3, "{id}: {columns} x {rows}");
    }
    // On the Go60 the first two keys are neighbors on the top row.
    let go60 = board("moergo-go60");
    let cells = grid(&go60.layout(&go60.default_layout).unwrap().keys);
    assert_eq!(cells[1].0 - cells[0].0, 1);
    assert!(cells[0].1 >= 3);
}
