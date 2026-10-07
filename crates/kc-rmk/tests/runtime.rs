//! Layouts as moergo-rmk's runtime configuration, and back.

use kc_boards::board::FirmwareProfile;
use kc_boards::Board;
use kc_model::behavior::{
    BehaviorKind, Flavor, HoldTap, Macro, MacroStep, ModMorph, StickyKey, TapDance,
};
use kc_model::features::{
    InputProcessor, KeyLight, LockKind, PointingConfig, PointingOverride, PointingProfile, Rgb,
    SettingValue,
};
use kc_model::{
    BehaviorRef, Binding, FirmwareConfig, KeyExpr, LayerId, Location, Param, Project, Severity,
};
use kc_rmk::moergo_config::{OutputModeConfig, RuntimeConfig};
use kc_rmk::runtime::{import, settle, translate};
use kc_rmk::settings;
use kc_zmk::Modifier;

fn go60() -> (Board, FirmwareProfile) {
    let board = kc_boards::built_in()
        .unwrap()
        .into_iter()
        .find(|b| b.id == "moergo-go60")
        .unwrap();
    let profile = board.profile("moergo-rmk").unwrap().clone();
    (board, profile)
}

fn firmware() -> FirmwareConfig {
    FirmwareConfig::new("moergo-rmk")
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

/// A key as what it sends: usage page, usage and every modifier, built in
/// or wrapped around it. `EXCL` and `LS(N1)` are the same key.
fn key_shape(expr: &KeyExpr) -> String {
    let code = kc_zmk::keycodes::keycodes().get(&expr.key).unwrap();
    let mut mods: Vec<Modifier> = expr
        .mods
        .iter()
        .chain(&code.implicit_mods)
        .copied()
        .collect();
    mods.sort();
    mods.dedup();
    format!("{}:{:#x} {mods:?}", code.page.id(), code.usage)
}

/// A binding as text that does not depend on behavior IDs or key
/// spellings, for comparing a layout with the one read back from its
/// configuration.
fn shape(project: &Project, binding: &Binding) -> String {
    let params = |params: &[Param]| -> String {
        params
            .iter()
            .map(|p| match p {
                Param::Layer(l) => format!("layer#{}", project.layer_index(*l).unwrap()),
                Param::Key(expr) => key_shape(expr),
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    match binding {
        Binding::Behavior {
            behavior: BehaviorRef::User { user },
            params: ps,
        } => {
            let def = project.behavior(*user).unwrap();
            format!("{} {}", def.kind.name(), params(ps))
        }
        Binding::Behavior {
            behavior,
            params: ps,
        } => format!("{behavior:?} {}", params(ps)),
        Binding::Raw { raw } => raw.clone(),
    }
}

#[test]
fn the_factory_layout_is_flagged_at_its_fourth_profile_and_otherwise_translated() {
    let (go60, profile) = go60();
    let mut project = Project::from_template("Factory", &go60);
    // moergo-rmk keeps three Bluetooth profiles where MoErgo's ZMK keeps
    // four: the keys for the fourth are flagged, and nothing else is.
    let problems = kc_rmk::runtime::check(&project, &go60, &profile, &firmware());
    let errors: Vec<_> = problems
        .iter()
        .filter(|p| p.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty());
    assert!(
        errors
            .iter()
            .all(|p| p.message.contains("3 Bluetooth profiles")),
        "{errors:?}"
    );
    // Unbinding the flagged keys and dropping the flagged behaviors is
    // what the user would do.
    for problem in &errors {
        match problem.location {
            Location::Key { layer, position } => {
                project
                    .set_binding(layer, position, Binding::none())
                    .unwrap();
            }
            Location::Behavior(id) => {
                for location in project.behavior_references(id) {
                    if let Location::Key { layer, position } = location {
                        project
                            .set_binding(layer, position, Binding::none())
                            .unwrap();
                    }
                }
            }
            _ => panic!("{problem:?}"),
        }
    }
    let flagged: Vec<_> = errors
        .iter()
        .filter_map(|p| match p.location {
            Location::Behavior(id) => Some(id),
            _ => None,
        })
        .collect();
    // A behavior used inside another goes once that other is gone.
    let mut remaining = flagged.clone();
    while !remaining.is_empty() {
        let before = remaining.len();
        remaining.retain(|id| project.remove_behavior(*id).is_err());
        assert!(remaining.len() < before, "{remaining:?}");
    }
    let translation = translate(&project, &go60, &profile, &firmware()).expect("translates");
    // What was changed on the way is said.
    let notes: Vec<&str> = translation
        .warnings
        .iter()
        .map(|w| w.message.as_str())
        .collect();
    assert!(notes.iter().any(|n| n.contains("profile key")), "{notes:?}");
    // The Magic key taps MoErgo's status macro, which does nothing here.
    assert!(
        notes.iter().any(|n| n.contains("taps nothing")),
        "{notes:?}"
    );
    let config = &translation.config;
    assert_eq!(config.layers.len(), 5);
    let base = &config.layers[0].keys;
    // The Magic key taps MoErgo's status macro, which does nothing here,
    // so it is written as the layer it holds.
    assert!(base.contains("MO(3)"), "{base}");
    // The profile keys are RMK's own.
    let magic = &config.layers[3].keys;
    assert!(
        magic.contains("USER(0)") && magic.contains("USER(2)"),
        "{magic}"
    );
    assert!(!magic.contains("TD("), "{magic}");
    // Only the timing profile the layout defines is written.
    let behavior = config.behavior.as_ref().unwrap();
    assert_eq!(behavior.morse.profiles.len(), 1);
    assert!(behavior.morse.profiles.contains_key("magic"));
    // The whole thing is a file the firmware's tools read.
    let text = config.to_toml().unwrap();
    RuntimeConfig::from_toml(&text).unwrap();
}

#[test]
fn every_kind_of_key_round_trips_through_the_configuration() {
    let (go60, profile) = go60();
    let mut project = Project::new("Mine", &go60);
    let base = project.layers[0].id;
    let nav = project.add_layer("Nav").unwrap();

    // Behaviors of the layout's own.
    let hrm = project
        .add_behavior(
            "hrm",
            "Home row mod",
            BehaviorKind::HoldTap(HoldTap {
                flavor: Flavor::Balanced,
                tapping_term_ms: 280,
                quick_tap_ms: Some(175),
                require_prior_idle_ms: Some(150),
                opposite_hand_hold: true,
                ..HoldTap::new(BehaviorRef::built_in("kp"), BehaviorRef::built_in("kp"))
            }),
        )
        .unwrap();
    let thumb = project
        .add_behavior(
            "thumb",
            "Thumb layer",
            BehaviorKind::HoldTap(HoldTap {
                flavor: Flavor::HoldPreferred,
                tapping_term_ms: 200,
                hold_trigger_key_positions: vec![30, 31, 32],
                ..HoldTap::new(BehaviorRef::built_in("mo"), BehaviorRef::built_in("kp"))
            }),
        )
        .unwrap();
    let odd = project
        .add_behavior(
            "odd",
            "Escape or Q",
            BehaviorKind::HoldTap(HoldTap {
                flavor: Flavor::TapUnlessInterrupted,
                ..HoldTap::new(BehaviorRef::built_in("kp"), BehaviorRef::built_in("kp"))
            }),
        )
        .unwrap();
    let dance = project
        .add_behavior(
            "td_xy",
            "X or Y",
            BehaviorKind::TapDance(TapDance {
                tapping_term_ms: 220,
                bindings: vec![
                    Binding::kp(KeyExpr::new("X")),
                    Binding::kp(KeyExpr::new("Y")),
                ],
            }),
        )
        .unwrap();
    let hi = project
        .add_behavior(
            "hi",
            "Hi",
            BehaviorKind::Macro(Macro {
                wait_ms: None,
                tap_ms: None,
                params: 0,
                steps: vec![
                    MacroStep::Tap(vec![Binding::kp(KeyExpr::new("H"))]),
                    MacroStep::Press(vec![Binding::kp(KeyExpr::new("LSHFT"))]),
                    MacroStep::Tap(vec![Binding::kp(KeyExpr::new("I"))]),
                    MacroStep::Release(vec![Binding::kp(KeyExpr::new("LSHFT"))]),
                ],
            }),
        )
        .unwrap();
    let morph = project
        .add_behavior(
            "comma_semi",
            "Comma or semicolon",
            BehaviorKind::ModMorph(ModMorph {
                normal: Binding::kp(KeyExpr::new("COMMA")),
                morphed: Binding::kp(KeyExpr::new("SEMI")),
                mods: vec![Modifier::LShift],
                keep_mods: vec![],
            }),
        )
        .unwrap();
    let sticky = project
        .add_behavior(
            "sk_slow",
            "Slow sticky",
            BehaviorKind::StickyKey(StickyKey {
                behavior: BehaviorRef::built_in("kp"),
                release_after_ms: 2000,
                quick_release: true,
                lazy: false,
                ignore_modifiers: false,
            }),
        )
        .unwrap();

    let keys: Vec<Binding> = vec![
        Binding::kp(KeyExpr::new("A")),
        Binding::kp(KeyExpr::new("EXCL")),
        Binding::kp(KeyExpr::new("C_VOL_UP")),
        Binding::kp(
            KeyExpr::new("TAB")
                .with(Modifier::LGui)
                .with(Modifier::LShift),
        ),
        Binding::new("mt", vec![key("LSHFT"), key("A")]),
        Binding::new("lt", vec![Param::Layer(nav), key("SPACE")]),
        Binding::new("sk", vec![key("LCTRL")]),
        Binding::layer("mo", nav),
        Binding::layer("to", nav),
        Binding::layer("tog", nav),
        Binding::layer("sl", nav),
        Binding::new("bootloader", vec![]),
        Binding::new("sys_reset", vec![]),
        Binding::new("caps_word", vec![]),
        Binding::new("key_repeat", vec![]),
        Binding::new("mkp", vec![Param::Constant("LCLK".into())]),
        Binding::new("mmv", vec![Param::Constant("MOVE_UP".into())]),
        Binding::new("msc", vec![Param::Constant("SCRL_DOWN".into())]),
        command("bt", "BT_SEL", &[1]),
        command("bt", "BT_NXT", &[]),
        command("bt", "BT_CLR", &[]),
        command("bt", "BT_CLR_ALL", &[]),
        command("out", "OUT_USB", &[]),
        command("out", "OUT_TOG", &[]),
        command("rgb_ug", "RGB_TOG", &[]),
        command("rgb_ug", "RGB_HUI", &[]),
        Binding::user(hrm, vec![key("LGUI"), key("S")]),
        Binding::user(thumb, vec![Param::Layer(nav), key("ENTER")]),
        Binding::user(odd, vec![key("ESC"), key("Q")]),
        Binding::user(dance, vec![]),
        Binding::user(hi, vec![]),
        Binding::user(morph, vec![]),
        Binding::user(sticky, vec![key("LALT")]),
        Binding::none(),
        Binding::trans(),
    ];
    for (position, binding) in keys.iter().enumerate() {
        project
            .set_binding(base, position, binding.clone())
            .unwrap();
    }
    project
        .set_binding(nav, 0, Binding::kp(KeyExpr::new("N1")))
        .unwrap();

    // Combos: one on a layer, one everywhere.
    let escape = project.add_combo("Escape", vec![0, 1], Binding::kp(KeyExpr::new("ESC")));
    project.combo_mut(escape).unwrap().layers = vec![nav];
    project.combo_mut(escape).unwrap().timeout_ms = Some(80);
    project.add_combo("Tab", vec![2, 3], Binding::kp(KeyExpr::new("TAB")));

    // Lighting: plain colors, off, a lock light and a battery light.
    let lights = project.lighting_mut(base).unwrap();
    lights.keys[0] = KeyLight::Color(Rgb(255, 0, 0));
    lights.keys[1] = KeyLight::Off;
    lights.keys[2] = KeyLight::Lock {
        lock: LockKind::Caps,
        off: Rgb(0, 0, 32),
        on: Rgb(255, 255, 255),
    };
    lights.keys[3] = KeyLight::Battery {
        percent: 40,
        below: Rgb(255, 0, 0),
        above: Rgb(0, 255, 0),
    };
    project.lighting_mut(nav).unwrap().keys[0] = KeyLight::Color(Rgb(0, 255, 0));

    // Pointing: a scrolling left pad and a fast right pad with a layer.
    // MoErgo's own order for the left pad: scrolling first, then the
    // speed, then a right click, which RMK's scroll mode has no room for.
    project.pointing.push(PointingConfig {
        listener: "cirque_lh_listener".into(),
        processors: vec![
            InputProcessor::ToScroll,
            InputProcessor::Scale {
                multiplier: 1,
                divisor: 8,
            },
            InputProcessor::Transform {
                invert_x: false,
                invert_y: true,
                swap_xy: false,
                scroll: true,
            },
            InputProcessor::RightClick,
        ],
        overrides: vec![],
    });
    project.pointing.push(PointingConfig {
        listener: "cirque_rh_listener".into(),
        processors: PointingProfile {
            speed: (3, 1),
            right_click: true,
            auto_layer: Some((nav, 300)),
            ..PointingProfile::default()
        }
        .to_processors(false),
        overrides: vec![PointingOverride {
            layers: vec![nav],
            processors: vec![InputProcessor::Scale {
                multiplier: 9,
                divisor: 1,
            }],
        }],
    });

    let translation = translate(&project, &go60, &profile, &firmware()).expect("translates");
    let config = &translation.config;
    let text = config.to_toml().unwrap();
    let parsed = RuntimeConfig::from_toml(&text).expect("the firmware's model takes it");
    let grid = &parsed.layers[0].keys;
    for expected in [
        "KC_A",
        "LSFT(KC_1)",
        "KC_VOLU",
        "LSFT(LGUI(KC_TAB))",
        "LSFT_T(KC_A)",
        "LT(1, KC_SPC)",
        "OSM(MOD_LCTL)",
        "MO(1)",
        "TO(1)",
        "TG(1)",
        "OSL(1)",
        // On the right half, so the firmware's key for that half's bootloader.
        "USER(12)",
        "QK_RBT",
        "CW_TOGG",
        "QK_REP",
        "KC_BTN1",
        "KC_MS_U",
        "KC_WH_D",
        "USER(1)",
        "USER(3)",
        "USER(10)",
        "USER(11)",
        "QK_OUTPUT_USB",
        "USER(6)",
        "BL_TOGG",
        "UG_HUEU",
        "MT(KC_S, LGui, hrm)",
        "LT(1, KC_ENT, thumb)",
        "TH(KC_Q, KC_ESC, odd)",
        "TD(0)",
        "MACRO(0)",
        "KC_COMM",
        "OSM(MOD_LALT)",
        "KC_NO",
        "KC_TRNS",
    ] {
        assert!(grid.contains(expected), "{expected} missing from\n{grid}");
    }
    // Lighting rides beside the keys as cells with rules, and the lit
    // layer with status lights wakes the lights.
    let lighting = parsed.lighting.as_ref().unwrap();
    assert_eq!(lighting.wake_layers, [0]);
    assert_eq!(parsed.layers[0].key_entries.len(), 4);
    let battery = &parsed.layers[0].key_entries[3];
    assert_eq!(battery.color.as_deref(), Some("#ff0000"));
    assert_eq!(battery.rules.len(), 1);
    // The behavior tables.
    assert_eq!(parsed.morses.len(), 1);
    assert_eq!(parsed.morses[0].double_tap.as_deref(), Some("KC_Y"));
    assert_eq!(parsed.macros.len(), 1);
    assert_eq!(parsed.macros[0].operations.len(), 4);
    assert_eq!(parsed.forks.len(), 1);
    assert_eq!(parsed.forks[0].trigger, "KC_COMM");
    assert_eq!(parsed.combos.len(), 2);
    assert_eq!(parsed.combos[0].layer, Some(1));
    assert_eq!(parsed.combos[0].positions, [[0, 0], [0, 1]]);
    let behavior = parsed.behavior.as_ref().unwrap();
    // The longest combo window in the layout, and the sticky key's timing,
    // set the globals the board left alone.
    assert_eq!(behavior.combo_timeout_ms, 80);
    assert_eq!(behavior.oneshot_timeout_ms, 2000);
    assert!(behavior.oneshot_quick_release);
    let hrm_profile = &behavior.morse.profiles["hrm"];
    assert_eq!(hrm_profile.mode.as_deref(), Some("permissive-hold"));
    assert_eq!(hrm_profile.opposite_hand_hold, Some(true));
    assert_eq!(hrm_profile.enable_flow_tap, Some(true));
    assert_eq!(hrm_profile.prior_idle_ms, Some(150));
    let thumb_profile = &behavior.morse.profiles["thumb"];
    assert_eq!(thumb_profile.mode.as_deref(), Some("hold-on-other-press"));
    assert_eq!(
        thumb_profile.hold_trigger_key_positions,
        [[2, 8], [2, 9], [2, 10]]
    );
    // Pointing.
    let pointing = parsed.pointing.as_ref().unwrap();
    assert_eq!(pointing.devices.len(), 2);
    assert_eq!(pointing.overrides.len(), 1);
    assert_eq!(behavior.auto_mouse_layers.len(), 1);
    assert_eq!(behavior.auto_mouse_layers[0].target_layer, 1);
    assert!(translation
        .warnings
        .iter()
        .any(|w| w.message.contains("keeps a left click")));

    // And back again.
    let led_to_key = |led: u16| profile.key_of_led(&go60, led);
    let back = import(&parsed, &go60, &profile, "Back", &led_to_key).expect("imports");
    assert!(back.notes.is_empty(), "{:?}", back.notes);
    let returned = &back.project;
    assert_eq!(returned.layers.len(), 2);
    assert_eq!(returned.layers[1].name, "Nav");
    for (position, binding) in keys.iter().enumerate() {
        // RMK has one timing for every sticky key, so the layout's own
        // sticky key comes back as the plain one.
        let expected = if binding.user_behavior() == Some(sticky) {
            Binding::new("sk", vec![key("LALT")])
        } else {
            binding.clone()
        };
        assert_eq!(
            shape(returned, &returned.layers[0].bindings[position]),
            shape(&project, &expected),
            "key {position}"
        );
    }
    // The timing profiles came back as hold-taps with their settings.
    let hrm_back = returned
        .behaviors
        .iter()
        .find_map(|b| match &b.kind {
            BehaviorKind::HoldTap(h) if b.name == "hrm" => Some(h.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(hrm_back.flavor, Flavor::Balanced);
    assert_eq!(hrm_back.tapping_term_ms, 280);
    assert_eq!(hrm_back.quick_tap_ms, Some(175));
    assert_eq!(hrm_back.require_prior_idle_ms, Some(150));
    assert!(hrm_back.opposite_hand_hold);
    let thumb_back = returned
        .behaviors
        .iter()
        .find_map(|b| match &b.kind {
            BehaviorKind::HoldTap(h) if b.name == "thumb" => Some(h.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(thumb_back.hold_trigger_key_positions, [30, 31, 32]);
    assert_eq!(thumb_back.flavor, Flavor::HoldPreferred);
    // Lighting and pointing too.
    let lights_back = returned.lighting(returned.layers[0].id).unwrap();
    assert_eq!(
        lights_back.keys[..4],
        project.lighting(base).unwrap().keys[..4]
    );
    assert_eq!(
        returned.lighting(returned.layers[1].id).unwrap().keys[0],
        KeyLight::Color(Rgb(0, 255, 0))
    );
    let left = returned
        .pointing
        .iter()
        .find(|p| p.listener == "cirque_lh_listener")
        .unwrap();
    let left_profile = PointingProfile::from_processors(&left.processors, false).unwrap();
    assert!(left_profile.scroll && left_profile.invert_y);
    assert_eq!(left_profile.speed, (1, 8));
    let right = returned
        .pointing
        .iter()
        .find(|p| p.listener == "cirque_rh_listener")
        .unwrap();
    let right_profile = PointingProfile::from_processors(&right.processors, false).unwrap();
    assert_eq!(right_profile.speed, (3, 1));
    assert!(right_profile.right_click);
    assert_eq!(right_profile.auto_layer, Some((returned.layers[1].id, 300)));
    assert_eq!(right.overrides.len(), 1);
    // Combos.
    assert_eq!(returned.combos.len(), 2);
    assert_eq!(returned.combos[0].key_positions, [0, 1]);
    assert_eq!(returned.combos[0].layers, [returned.layers[1].id]);
    // The board's settings came apart from the layout.
    assert_eq!(
        back.carried.settings.get(settings::COMBO_TIMEOUT),
        Some(&SettingValue::Int(80))
    );
}

#[test]
fn a_tailorkey_style_configuration_opens_as_a_layout() {
    let (go60, profile) = go60();
    let text = include_str!("data/tailorkey-style-go60.toml");
    let config = RuntimeConfig::from_toml(text).unwrap();
    let led_to_key = |led: u16| profile.key_of_led(&go60, led);
    let imported = import(&config, &go60, &profile, "TailorKey", &led_to_key).unwrap();
    let project = &imported.project;
    assert_eq!(project.layers.len(), 3);
    assert_eq!(project.layers[0].name, "Base (QWERTY)");
    // Home-row mods became hold-taps with their profiles' timing; the
    // same profile holding a modifier is one behavior however many keys
    // use it.
    let pinky = project
        .behaviors
        .iter()
        .find(|b| b.name == "hrm_pinky")
        .expect("the pinky profile");
    let BehaviorKind::HoldTap(hold_tap) = &pinky.kind else {
        panic!("a hold-tap");
    };
    assert_eq!(hold_tap.tapping_term_ms, 270);
    assert_eq!(hold_tap.quick_tap_ms, Some(300));
    assert_eq!(hold_tap.require_prior_idle_ms, Some(150));
    assert!(hold_tap.opposite_hand_hold);
    assert_eq!(hold_tap.flavor, Flavor::TapPreferred);
    assert_eq!(
        project
            .behaviors
            .iter()
            .filter(|b| b.name == "hrm_pinky")
            .count(),
        1
    );
    // The A key, at matrix [2, 1], holds LGui with that profile.
    let a = &project.layers[0].bindings[25];
    assert_eq!(a.user_behavior(), Some(pinky.id));
    assert!(
        matches!(a, Binding::Behavior { params, .. } if params[0] == key("LGUI") && params[1] == key("A"))
    );
    // Thumb layer-taps hold a layer.
    let thumb = project
        .behaviors
        .iter()
        .find(|b| b.name == "thumb_layer")
        .unwrap();
    let BehaviorKind::HoldTap(thumb_tap) = &thumb.kind else {
        panic!("a hold-tap");
    };
    assert_eq!(thumb_tap.hold, BehaviorRef::built_in("mo"));
    assert_eq!(thumb_tap.flavor, Flavor::Balanced);
    // Autoshift tap-holds tap a key and hold its shifted self.
    let autoshift = project
        .behaviors
        .iter()
        .find(|b| b.name == "autoshift")
        .unwrap();
    assert!(matches!(&autoshift.kind, BehaviorKind::HoldTap(h) if h.tapping_term_ms == 190));
    // The firmware's own keys read as the app's.
    let magic = &project.layers[2].bindings;
    assert_eq!(magic[12], Binding::new("bootloader", vec![]));
    assert_eq!(magic[11], command("bt", "BT_CLR_ALL", &[]));
    assert_eq!(magic[0], command("bt", "BT_CLR", &[]));
    assert_eq!(magic[36], command("bt", "BT_SEL", &[0]));
    assert_eq!(magic[50], command("out", "OUT_TOG", &[]));
    assert_eq!(magic[17], command("rgb_ug", "RGB_TOG", &[]));
    assert_eq!(magic[29], command("rgb_ug", "RGB_EFF", &[]));
    assert_eq!(magic[51], Binding::new("caps_word", vec![]));
    assert_eq!(magic[53], Binding::layer("to", project.layers[0].id));
    assert_eq!(magic[48], command("out", "OUT_USB", &[]));
    // The right half's bootloader key is a bootloader key, with a note;
    // the layer-and-modifier tap has no equivalent and is said so.
    assert_eq!(magic[23], Binding::new("bootloader", vec![]));
    assert!(imported
        .notes
        .iter()
        .any(|n| n.contains("layer-and-modifier")));
    assert_eq!(project.layers[1].bindings[30], Binding::none());
    // The macro, and the key that triggers it.
    let select = project
        .behaviors
        .iter()
        .find(|b| b.name == "Select line")
        .unwrap();
    assert!(matches!(&select.kind, BehaviorKind::Macro(m) if m.steps.len() == 4));
    assert_eq!(
        project.layers[1].bindings[31].user_behavior(),
        Some(select.id)
    );
    // Combos named by what their keys do found those keys.
    assert_eq!(project.combos.len(), 2);
    assert_eq!(project.combos[0].key_positions, [13, 14]);
    assert_eq!(project.combos[0].layers, [project.layers[0].id]);
    assert_eq!(project.combos[1].key_positions, [37, 38]);
    assert!(project.combos[1].layers.is_empty());
    // Lighting: cells by key and by LED, and the rules a layout can show.
    let magic_lights = project.lighting(project.layers[2].id).unwrap();
    assert_eq!(
        magic_lights.keys[0],
        KeyLight::Battery {
            percent: 40,
            below: Rgb(255, 0, 0),
            above: Rgb(0, 255, 0)
        }
    );
    assert_eq!(
        magic_lights.keys[12],
        KeyLight::Lock {
            lock: LockKind::Caps,
            off: Rgb(0x20, 0x20, 0x20),
            on: Rgb(255, 255, 255)
        }
    );
    let lower_lights = project.lighting(project.layers[1].id).unwrap();
    assert_eq!(lower_lights.keys[29], KeyLight::Color(Rgb(0, 0, 255)));
    // The touchpads.
    let left = project
        .pointing
        .iter()
        .find(|p| p.listener == "cirque_lh_listener")
        .unwrap();
    let left_profile = PointingProfile::from_processors(&left.processors, false).unwrap();
    assert!(left_profile.scroll && left_profile.invert_y);
    let right = project
        .pointing
        .iter()
        .find(|p| p.listener == "cirque_rh_listener")
        .unwrap();
    let right_profile = PointingProfile::from_processors(&right.processors, false).unwrap();
    assert!(right_profile.right_click);
    assert_eq!(right_profile.speed, (3, 1));
    // The board's settings.
    assert_eq!(
        imported.carried.settings.get(settings::BRIGHTNESS),
        Some(&SettingValue::Int(50))
    );
    assert_eq!(
        imported.carried.settings.get(settings::OUTPUT_MODE),
        Some(&SettingValue::Text("powered-only".into()))
    );
    assert_eq!(
        imported.carried.settings.get(settings::EFFECT),
        Some(&SettingValue::Text("Rain".into()))
    );
    assert_eq!(
        imported.carried.settings.get(settings::COMBO_TIMEOUT),
        Some(&SettingValue::Int(60))
    );
    // What came back goes out again as a configuration the firmware takes.
    let mut firmware = firmware();
    firmware.absorb(imported.carried.clone());
    let again = translate(project, &go60, &profile, &firmware).expect("translates again");
    let text = again.config.to_toml().unwrap();
    let parsed = RuntimeConfig::from_toml(&text).unwrap();
    assert_eq!(parsed.lighting.as_ref().unwrap().brightness, 128);
    assert!(parsed.layers[0].keys.contains("MT(KC_A, LGui, hrm_pinky)"));
}

#[test]
fn the_boards_settings_shape_the_lighting_and_typing_sections() {
    let (go60, profile) = go60();
    let project = Project::new("Mine", &go60);
    let mut firmware = firmware();
    let set = |firmware: &mut FirmwareConfig, key: &str, value: SettingValue| {
        firmware.settings.insert(key.into(), value);
    };
    set(&mut firmware, settings::BRIGHTNESS, SettingValue::Int(40));
    set(
        &mut firmware,
        settings::OUTPUT_MODE,
        SettingValue::Text("powered-only".into()),
    );
    set(
        &mut firmware,
        settings::EFFECT,
        SettingValue::Text("Rain".into()),
    );
    set(
        &mut firmware,
        settings::PALETTE,
        SettingValue::Text("Dracula".into()),
    );
    set(
        &mut firmware,
        settings::COMBO_TIMEOUT,
        SettingValue::Int(80),
    );
    set(
        &mut firmware,
        settings::HOLD_MODE,
        SettingValue::Text("permissive-hold".into()),
    );
    set(
        &mut firmware,
        settings::BLUETOOTH_NAME,
        SettingValue::Text("Go60 {slot}".into()),
    );
    let translation = translate(&project, &go60, &profile, &firmware).unwrap();
    let lighting = translation.config.lighting.as_ref().unwrap();
    assert_eq!(lighting.brightness, 102);
    assert_eq!(lighting.output_mode, OutputModeConfig::PoweredOnly);
    let effects = lighting.effects.as_ref().unwrap();
    assert_eq!(
        (effects.effect.as_str(), effects.palette.as_str()),
        ("Rain", "Dracula")
    );
    let behavior = translation.config.behavior.as_ref().unwrap();
    assert_eq!(behavior.combo_timeout_ms, 80);
    assert_eq!(
        behavior.morse.default_profile.mode.as_deref(),
        Some("permissive-hold")
    );
    assert_eq!(
        translation.config.bluetooth_name.as_deref(),
        Some("Go60 {slot}")
    );
    let claims = &translation.claims;
    assert!(claims.lighting && claims.brightness && claims.output_mode && claims.effect);
    assert!(!claims.background && !claims.effect_value);
    assert!(claims.combo_timeout && claims.hold_profile && !claims.tap_interval);

    // An animation the firmware does not have is refused by name.
    set(
        &mut firmware,
        settings::EFFECT,
        SettingValue::Text("Lava".into()),
    );
    let problems = kc_rmk::runtime::check(&project, &go60, &profile, &firmware);
    assert!(problems.iter().any(|p| p.message.contains("Lava")));

    // A plain layout with no lighting settings has no lighting section:
    // the keyboard's lights are left alone.
    let plain = translate(
        &project,
        &go60,
        &profile,
        &FirmwareConfig::new("moergo-rmk"),
    )
    .unwrap();
    assert!(plain.config.lighting.is_none());
    assert!(!plain.claims.lighting);
}

#[test]
fn settling_takes_what_the_board_does_not_claim_from_the_keyboard() {
    let (go60, profile) = go60();
    let mut project = Project::new("Mine", &go60);
    let base = project.layers[0].id;
    project.lighting_mut(base).unwrap().keys[0] = KeyLight::Color(Rgb(255, 0, 0));
    let translation = translate(&project, &go60, &profile, &firmware()).unwrap();
    let mut desired = translation.config.snapshot().unwrap();

    // What the keyboard holds: three layers, its own brightness and timing.
    let mut held = translation.config.clone();
    for n in 2..=3 {
        held.layers.push(kc_rmk::moergo_config::LayerConfig {
            id: format!("l{n}"),
            name: format!("Layer {n}"),
            keys: String::new(),
            key_entries: Vec::new(),
            light_entries: Vec::new(),
        });
    }
    let lighting = held.lighting.as_mut().unwrap();
    lighting.brightness = 77;
    lighting.output_mode = OutputModeConfig::AlwaysOff;
    lighting.wake_layers = vec![2];
    held.behavior.as_mut().unwrap().combo_timeout_ms = 99;
    let before = held.snapshot().unwrap();

    settle(&mut desired, &before, &translation.claims);
    let lighting = desired.lighting.as_ref().unwrap();
    assert_eq!(lighting.brightness, 77);
    assert_eq!(lighting.output_mode, OutputModeConfig::AlwaysOff);
    // No status light in the layout, so the keyboard's wake layers stay.
    assert_eq!(lighting.wake_layers, [2]);
    assert_eq!(desired.behaviors.config.unwrap().combo_timeout_ms, 99);
    // The layers above the layout's are cleared, not left behind.
    assert_eq!(desired.layers.len(), 3);
    assert!(desired.layers[2]
        .iter()
        .all(|a| *a == kc_rmk::moergo_config::rynk_keycode::from_via_keycode(1)));
    assert_eq!(desired.layer_names.as_ref().unwrap().len(), 3);
    assert!(!desired.layer_names.as_ref().unwrap()[2].occupied);
    // The keyboard's name is left alone when the board does not set it.
    assert_eq!(desired.bluetooth_name, None);

    // A claimed value is kept.
    let mut firmware = firmware();
    firmware
        .settings
        .insert(settings::BRIGHTNESS.into(), SettingValue::Int(100));
    let claimed = translate(&project, &go60, &profile, &firmware).unwrap();
    let mut desired = claimed.config.snapshot().unwrap();
    settle(&mut desired, &before, &claimed.claims);
    assert_eq!(desired.lighting.as_ref().unwrap().brightness, 255);
}

#[test]
fn what_rmk_lacks_is_refused_at_the_key_with_a_reason() {
    let (go60, profile) = go60();
    let mut project = Project::new("Mine", &go60);
    let base = project.layers[0].id;
    let rmk = profile.rmk.as_ref().unwrap();
    let mixed = Binding::kp(
        KeyExpr::new("A")
            .with(Modifier::LCtrl)
            .with(Modifier::RShift),
    );
    assert!(!kc_rmk::runtime::expressible(&mixed, &project, rmk));
    assert!(kc_rmk::runtime::expressible(
        &Binding::kp(KeyExpr::new("A").with(Modifier::LCtrl)),
        &project,
        rmk
    ));
    assert!(!kc_rmk::runtime::expressible(
        &command("bt", "BT_SEL", &[3]),
        &project,
        rmk
    ));
    assert!(!kc_rmk::runtime::expressible(
        &Binding::new("sk", vec![key("A")]),
        &project,
        rmk
    ));
    project.set_binding(base, 0, mixed).unwrap();
    project
        .set_binding(
            base,
            1,
            Binding::Raw {
                raw: "&weird".into(),
            },
        )
        .unwrap();
    let problems = kc_rmk::runtime::check(&project, &go60, &profile, &firmware());
    let at = |position: usize| {
        problems
            .iter()
            .find(|p| {
                p.location
                    == Location::Key {
                        layer: base,
                        position,
                    }
            })
            .map(|p| p.message.clone())
            .unwrap_or_default()
    };
    assert!(at(0).contains("one hand"), "{}", at(0));
    assert!(at(1).contains("devicetree"), "{}", at(1));
    assert!(translate(&project, &go60, &profile, &firmware()).is_err());
    // A tap-dance of three steps is more than a morse key holds.
    let mut project = Project::new("Mine", &go60);
    let three = project
        .add_behavior(
            "three",
            "Three",
            BehaviorKind::TapDance(TapDance {
                tapping_term_ms: 200,
                bindings: vec![
                    Binding::kp(KeyExpr::new("A")),
                    Binding::kp(KeyExpr::new("B")),
                    Binding::kp(KeyExpr::new("C")),
                ],
            }),
        )
        .unwrap();
    project
        .set_binding(base, 0, Binding::user(three, vec![]))
        .unwrap();
    let problems = kc_rmk::runtime::check(&project, &go60, &profile, &firmware());
    assert!(
        problems
            .iter()
            .any(|p| p.location == Location::Behavior(three)
                && p.message.contains("one and two taps"))
    );
    let _ = LayerId(0);
}

fn glove80() -> (Board, FirmwareProfile) {
    let board = kc_boards::built_in()
        .unwrap()
        .into_iter()
        .find(|b| b.id == "moergo-glove80")
        .unwrap();
    let profile = board.profile("moergo-rmk").unwrap().clone();
    (board, profile)
}

/// Moosy Research's TailorKey for RMK, as the Glove80 build of moergo-rmk
/// takes it, opens as a layout and goes out again with every key as it
/// was, up to the firmware's own spellings.
#[test]
fn tailorkey_for_rmk_on_the_glove80_opens_as_a_layout_and_goes_back() {
    let (glove80, profile) = glove80();
    let text = include_str!("data/tailorkey-v52-bilateral-glove80.toml");
    let config = RuntimeConfig::from_toml(text).unwrap();
    let led_to_key = |led: u16| profile.key_of_led(&glove80, led);
    let imported = import(&config, &glove80, &profile, "TailorKey", &led_to_key).unwrap();
    let project = &imported.project;
    let names: Vec<_> = project.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Base (QWERTY)",
            "Typing",
            "Autoshift",
            "Gaming",
            "Cursor",
            "Symbol",
            "Mouse",
            "MouseSlow",
            "MouseFast",
            "MouseWarp",
            "Lower",
            "Magic"
        ]
    );
    // Four bilateral home-row profiles, two thumb profiles and autoshift
    // became hold-taps; the six macros came whole.
    let count =
        |f: &dyn Fn(&BehaviorKind) -> bool| project.behaviors.iter().filter(|b| f(&b.kind)).count();
    assert_eq!(count(&|k| matches!(k, BehaviorKind::HoldTap(_))), 7);
    assert_eq!(count(&|k| matches!(k, BehaviorKind::Macro(_))), 6);
    let pinky = project
        .behaviors
        .iter()
        .find(|b| b.name == "hrm_pinky")
        .expect("the pinky profile");
    let BehaviorKind::HoldTap(hold_tap) = &pinky.kind else {
        panic!("a hold-tap");
    };
    assert!(hold_tap.opposite_hand_hold);
    // Twenty combo slots; the six that send a layer-and-modifier hold,
    // which a layout cannot hold, are reported rather than dropped.
    assert_eq!(project.combos.len(), 14);
    assert_eq!(
        imported
            .notes
            .iter()
            .filter(|n| n.contains("-Tab") && n.contains("left out"))
            .count(),
        6
    );
    assert_eq!(
        imported.carried.settings.get(settings::COMBO_TIMEOUT),
        Some(&SettingValue::Int(50))
    );
    assert_eq!(
        imported.carried.settings.get(settings::HOLD_TIMEOUT),
        Some(&SettingValue::Int(250))
    );

    let mut firmware = firmware();
    firmware.absorb(imported.carried.clone());
    assert_eq!(
        kc_rmk::runtime::check(project, &glove80, &profile, &firmware),
        []
    );
    let again = translate(project, &glove80, &profile, &firmware).expect("translates again");
    let parsed = RuntimeConfig::from_toml(&again.config.to_toml().unwrap()).unwrap();
    assert_eq!(parsed.layers.len(), config.layers.len());
    // Macros are numbered in the order the layout holds them, so compare
    // them by name; `--` is the firmware's other spelling of no key.
    let normal = |cfg: &RuntimeConfig, cell: &str| -> String {
        if cell == "--" {
            return "KC_NO".to_string();
        }
        match cell
            .strip_prefix("MACRO(")
            .and_then(|rest| rest.strip_suffix(')'))
            .and_then(|n| n.parse::<usize>().ok())
        {
            Some(n) => format!("MACRO({})", cfg.macros[n].name),
            None => cell.to_string(),
        }
    };
    let mut differences = Vec::new();
    for (mine, theirs) in parsed.layers.iter().zip(&config.layers) {
        let a: Vec<String> = mine
            .keys
            .split_whitespace()
            .map(|c| normal(&parsed, c))
            .collect();
        let b: Vec<String> = theirs
            .keys
            .split_whitespace()
            .map(|c| normal(&config, c))
            .collect();
        assert_eq!(a.len(), b.len(), "{}", theirs.id);
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            if x != y {
                differences.push((theirs.id.clone(), i, x.clone(), y.clone()));
            }
        }
    }
    // The one key that changes: TailorKey's `UG_TOGG` toggles the
    // background animation, which a layout has no word for, so it was read
    // as the lighting toggle and said so.
    assert_eq!(
        differences,
        [(
            "magic".to_string(),
            33,
            "BL_TOGG".to_string(),
            "UG_TOGG".to_string()
        )]
    );
    assert!(imported
        .notes
        .iter()
        .any(|n| n.contains("animation toggle")));
    assert_eq!(parsed.macros.len(), 6);
    assert_eq!(
        parsed.behavior.as_ref().unwrap().morse.profiles.len(),
        config.behavior.as_ref().unwrap().morse.profiles.len()
    );
}

fn imprint() -> (Board, FirmwareProfile) {
    let board = kc_boards::built_in()
        .unwrap()
        .into_iter()
        .find(|b| b.id == "cyboard-imprint")
        .unwrap();
    let profile = board.profile("imprint-rmk").unwrap().clone();
    (board, profile)
}

/// The Imprint's factory layout translates for the Imprint build of the
/// moergo-rmk fork: 82 keys in a 14x8 grid with the holes marked, the
/// right half's bootloader key as the peripheral bootloader, five
/// Bluetooth profiles, and both trackballs' settings.
#[test]
fn the_imprint_factory_layout_translates_for_its_rmk_fork() {
    let (imprint, profile) = imprint();
    let project = Project::from_template("Factory", &imprint);
    let firmware = FirmwareConfig::new("imprint-rmk");
    let problems = kc_rmk::runtime::check(&project, &imprint, &profile, &firmware);
    assert!(
        problems.iter().all(|p| p.severity != Severity::Error),
        "{problems:#?}"
    );
    let translation = translate(&project, &imprint, &profile, &firmware).expect("translates");
    let text = translation.config.to_toml().unwrap();
    let parsed = RuntimeConfig::from_toml(&text).unwrap();
    assert_eq!(parsed.layers.len(), 5);
    let base: Vec<&str> = parsed.layers[0].keys.split_whitespace().collect();
    assert_eq!(base.len(), 14 * 8);
    assert_eq!(base.iter().filter(|c| **c == "--").count(), 14 * 8 - 82);
    // Row 6 is the left half's top finger row, read from the outer column
    // in: Escape sits at (6,5).
    assert_eq!(base[6 * 8 + 5], "KC_ESC");
    assert_eq!(base[13 * 8], "KC_F6");
    // The thumbs: (0,3) is the left cluster's inner top key.
    assert_eq!(base[3], "KC_ENT");
    let control: Vec<&str> = parsed.layers[2].keys.split_whitespace().collect();
    assert_eq!(control[3 * 8 + 5], "QK_BOOT", "left bootloader");
    assert_eq!(control[10 * 8 + 5], "USER(12)", "right half's bootloader");
    assert_eq!(control[5 * 8 + 5], "USER(10)", "clear the active profile");
    assert_eq!(control[5 * 8], "USER(4)", "the fifth profile exists");
    // What came out reads back as the same layout, trackballs included.
    let led_to_key = |led: u16| profile.key_of_led(&imprint, led);
    let back = import(&parsed, &imprint, &profile, "Back", &led_to_key).expect("imports");
    assert_eq!(back.project.layers.len(), 5);
    let pointing = &back.project.pointing;
    assert!(
        pointing
            .iter()
            .any(|p| p.listener == "trackball_central_listener"
                && p.processors.contains(&InputProcessor::ToScroll)),
        "{pointing:#?}"
    );
    for (mine, theirs) in back.project.layers.iter().zip(&project.layers) {
        assert_eq!(mine.bindings.len(), theirs.bindings.len());
    }
}
