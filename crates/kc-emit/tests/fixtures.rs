//! Generated configs kept in `fixtures/`, one per board.
//!
//! They serve two purposes: as snapshots, so any change to the emitter's
//! output shows up in review; and as the input to the firmware CI job,
//! which builds them with the real ZMK toolchain.
//!
//! Run with `UPDATE_FIXTURES=1` to rewrite them after an intended change.

use std::path::PathBuf;

use kc_boards::Board;
use kc_model::behavior::{BehaviorKind, HoldTap, Macro, MacroStep, ModMorph, StickyKey, TapDance};
use kc_model::features::{
    ConditionalLayer, InputProcessor, PointingConfig, PointingOverride, SettingValue,
};
use kc_model::{file, BehaviorRef, Binding, FirmwareConfig, KeyExpr, Param, Project};
use kc_zmk::{Feature, Modifier};

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

/// A project that uses every construct the emitter can write, limited to
/// what the board's firmware supports.
fn fixture(board: &Board) -> (Project, FirmwareConfig) {
    let features = &board.firmware[0].capabilities;
    let mut p = Project::new(format!("{} fixture", board.name), board);
    let base = p.layers[0].id;
    let nav = p.add_layer("Nav").unwrap();
    let system = p.add_layer("System").unwrap();
    let mouse = p.add_layer("Mouse").unwrap();
    p.set_reserved_layers(4).unwrap();
    let mut set = |layer, position, binding| p.set_binding(layer, position, binding).unwrap();

    set(base, 0, kp("LC(LS(ESC))"));
    set(
        base,
        12,
        Binding::new(
            "mt",
            vec![
                Param::Key(KeyExpr::new("LCTRL")),
                Param::Key(KeyExpr::new("TAB")),
            ],
        ),
    );
    set(
        base,
        13,
        Binding::new("lt", vec![Param::Layer(nav), Param::Key(KeyExpr::new("Q"))]),
    );
    set(base, 24, Binding::layer("mo", system));
    set(base, 35, Binding::layer("tog", mouse));
    set(
        base,
        36,
        Binding::new("sk", vec![Param::Key(KeyExpr::new("LSHFT"))]),
    );
    set(base, 47, Binding::layer("sl", nav));
    for (position, label) in ["caps_word", "key_repeat", "gresc", "none"]
        .iter()
        .enumerate()
    {
        set(nav, position, Binding::new(label, vec![]));
    }
    for (position, key) in ["LEFT", "DOWN", "UP", "RIGHT", "C_VOL_UP", "C_PP"]
        .iter()
        .enumerate()
    {
        set(nav, 18 + position, kp(key));
    }
    set(
        nav,
        30,
        Binding::new("kt", vec![Param::Key(KeyExpr::new("LALT"))]),
    );
    set(nav, 31, Binding::layer("to", base));

    for profile in 0..4 {
        set(
            system,
            profile as usize,
            command("bt", "BT_SEL", &[profile]),
        );
    }
    set(system, 4, command("bt", "BT_CLR", &[]));
    set(system, 5, command("bt", "BT_CLR_ALL", &[]));
    set(system, 6, command("bt", "BT_DISC", &[1]));
    set(system, 7, command("out", "OUT_TOG", &[]));
    set(system, 8, command("out", "OUT_USB", &[]));
    set(system, 12, Binding::new("bootloader", vec![]));
    set(system, 13, Binding::new("sys_reset", vec![]));
    set(system, 14, Binding::new("studio_unlock", vec![]));
    set(system, 15, command("ext_power", "EP_TOG", &[]));
    set(system, 24, command("rgb_ug", "RGB_TOG", &[]));
    set(system, 25, command("rgb_ug", "RGB_EFF", &[]));
    set(system, 26, command("rgb_ug", "RGB_BRI", &[]));
    set(
        system,
        27,
        command("rgb_ug", "RGB_COLOR_HSB", &[120, 100, 30]),
    );
    if features.contains(&Feature::RgbStatus) {
        set(system, 28, command("rgb_ug", "RGB_STATUS", &[]));
    }

    for (position, button) in ["LCLK", "MCLK", "RCLK"].iter().enumerate() {
        set(
            mouse,
            18 + position,
            Binding::new("mkp", vec![Param::Constant((*button).into())]),
        );
    }
    set(
        mouse,
        6,
        Binding::new("mmv", vec![Param::Constant("MOVE_UP".into())]),
    );
    set(
        mouse,
        7,
        Binding::new("msc", vec![Param::Constant("SCRL_DOWN".into())]),
    );

    let hold_tap = p
        .add_behavior(
            "hml",
            "Home-row mod",
            BehaviorKind::HoldTap(HoldTap {
                flavor: kc_model::behavior::Flavor::Balanced,
                tapping_term_ms: 280,
                quick_tap_ms: Some(175),
                require_prior_idle_ms: Some(150),
                hold_trigger_key_positions: vec![6, 7, 8, 9, 10, 11],
                hold_trigger_on_release: true,
                ..HoldTap::new(BehaviorRef::built_in("kp"), BehaviorRef::built_in("kp"))
            }),
        )
        .unwrap();
    p.set_binding(
        base,
        25,
        Binding::user(
            hold_tap,
            vec![
                Param::Key(KeyExpr::new("LGUI")),
                Param::Key(KeyExpr::new("A")),
            ],
        ),
    )
    .unwrap();
    let dance = p
        .add_behavior(
            "nav_td",
            "Nav tap-dance",
            BehaviorKind::TapDance(TapDance {
                tapping_term_ms: 200,
                bindings: vec![Binding::layer("mo", nav), Binding::layer("to", nav)],
            }),
        )
        .unwrap();
    p.set_binding(base, 37, Binding::user(dance, vec![]))
        .unwrap();
    let morph = p
        .add_behavior(
            "bspc_del",
            "Backspace or Delete",
            BehaviorKind::ModMorph(ModMorph {
                normal: kp("BSPC"),
                morphed: kp("DEL"),
                mods: vec![Modifier::LShift, Modifier::RShift],
                keep_mods: vec![],
            }),
        )
        .unwrap();
    p.set_binding(base, 38, Binding::user(morph, vec![]))
        .unwrap();
    let sticky = p
        .add_behavior(
            "skq",
            "Quick sticky key",
            BehaviorKind::StickyKey(StickyKey {
                behavior: BehaviorRef::built_in("kp"),
                release_after_ms: 800,
                quick_release: true,
                lazy: false,
                ignore_modifiers: true,
            }),
        )
        .unwrap();
    p.set_binding(
        base,
        39,
        Binding::user(sticky, vec![Param::Key(KeyExpr::new("RSHFT"))]),
    )
    .unwrap();
    let hello = p
        .add_behavior(
            "hello",
            "Type Hi",
            BehaviorKind::Macro(Macro {
                wait_ms: Some(30),
                tap_ms: Some(30),
                params: 0,
                steps: vec![
                    MacroStep::Press(vec![kp("LSHFT")]),
                    MacroStep::Tap(vec![kp("H")]),
                    MacroStep::Release(vec![kp("LSHFT")]),
                    MacroStep::WaitTime(50),
                    MacroStep::Tap(vec![kp("I")]),
                    MacroStep::PauseForRelease,
                    MacroStep::Tap(vec![kp("EXCL")]),
                ],
            }),
        )
        .unwrap();
    p.set_binding(nav, 32, Binding::user(hello, vec![]))
        .unwrap();
    let chord = p
        .add_behavior(
            "ctrl_with",
            "Ctrl with a key",
            BehaviorKind::Macro(Macro {
                wait_ms: None,
                tap_ms: None,
                params: 1,
                steps: vec![
                    MacroStep::Press(vec![kp("LCTRL")]),
                    MacroStep::Param { from: 1, to: 1 },
                    MacroStep::Tap(vec![Binding::Raw {
                        raw: "&kp MACRO_PLACEHOLDER".into(),
                    }]),
                    MacroStep::Release(vec![kp("LCTRL")]),
                ],
            }),
        )
        .unwrap();
    p.set_binding(
        nav,
        33,
        Binding::user(chord, vec![Param::Key(KeyExpr::new("C"))]),
    )
    .unwrap();

    let combo = p.add_combo("Caps Word", vec![40, 43], Binding::new("caps_word", vec![]));
    let combo = p.combo_mut(combo).unwrap();
    combo.timeout_ms = Some(40);
    combo.require_prior_idle_ms = Some(100);
    combo.slow_release = true;
    combo.layers = vec![base, nav];
    p.add_combo("Escape", vec![13, 14], kp("ESC"));
    p.conditional_layers.push(ConditionalLayer {
        if_layers: vec![nav, mouse],
        then_layer: system,
    });

    if let Some(device) = board.pointing.first() {
        p.pointing.push(PointingConfig {
            listener: device.listener.clone(),
            processors: vec![
                InputProcessor::Scale {
                    multiplier: 1,
                    divisor: 3,
                },
                InputProcessor::ToScroll,
                InputProcessor::Transform {
                    invert_x: false,
                    invert_y: true,
                    swap_xy: false,
                    scroll: true,
                },
            ],
            overrides: vec![PointingOverride {
                layers: vec![nav],
                processors: vec![InputProcessor::Scale {
                    multiplier: 2,
                    divisor: 1,
                }],
            }],
        });
    }
    if let Some(device) = board.pointing.get(1) {
        p.pointing.push(PointingConfig {
            listener: device.listener.clone(),
            processors: vec![
                InputProcessor::RightClick,
                InputProcessor::TempLayer {
                    layer: mouse,
                    timeout_ms: 500,
                },
            ],
            overrides: vec![],
        });
    }
    // The mouse keys, slower on one layer.
    if features.contains(&Feature::Pointing) {
        p.pointing.push(PointingConfig {
            listener: "mmv_input_listener".into(),
            processors: vec![],
            overrides: vec![PointingOverride {
                layers: vec![nav],
                processors: vec![InputProcessor::Scale {
                    multiplier: 1,
                    divisor: 4,
                }],
            }],
        });
        p.pointing.push(PointingConfig {
            listener: "msc_input_listener".into(),
            processors: vec![InputProcessor::ScrollScale {
                multiplier: 3,
                divisor: 2,
            }],
            overrides: vec![],
        });
    }

    let mut config = FirmwareConfig::stock(board);
    config
        .settings
        .insert("CONFIG_ZMK_SLEEP".into(), SettingValue::Bool(true));
    config.settings.insert(
        "CONFIG_ZMK_IDLE_SLEEP_TIMEOUT".into(),
        SettingValue::Int(900_000),
    );
    (p, config)
}

/// The factory layout on the board's per-key lighting firmware, with every
/// kind of key light across several layers.
fn lighting_fixture(board: &Board) -> (Project, FirmwareConfig) {
    use kc_model::features::{KeyLight, LockKind, Rgb};

    let mut p = Project::from_template(format!("{} lighting fixture", board.name), board);
    let profile = board
        .firmware
        .iter()
        .find(|f| f.lighting.is_some())
        .expect("a lighting profile");
    let config = FirmwareConfig::new(profile.id.clone());
    let (base, second, third) = (p.layers[0].id, p.layers[1].id, p.layers[2].id);
    let keys = p.key_count;
    // Until a board's LED positions are confirmed on hardware, its lighting
    // fixture is the pattern used to check them: rows on the base layer,
    // columns on the next.
    if board
        .halves
        .iter()
        .any(|h| h.leds.as_ref().is_some_and(|l| !l.verified))
    {
        p.name = format!("{} LED check", board.name);
        let layout = board.layout(&p.layout).unwrap();
        let (rows, columns) = kc_model::lighting::led_check(&layout.keys);
        p.lighting_mut(base).unwrap().keys = rows;
        p.lighting_mut(second).unwrap().keys = columns;
        return (p, config);
    }
    {
        let lights = &mut p.lighting_mut(base).unwrap().keys;
        for (position, light) in lights.iter_mut().enumerate() {
            *light = KeyLight::Color(Rgb((position * 4) as u8, 64, 200 - (position * 3) as u8));
        }
        lights[0] = KeyLight::Off;
        lights[1] = KeyLight::Lock {
            lock: LockKind::Caps,
            off: Rgb(0, 0, 0),
            on: Rgb(255, 0, 0),
        };
        lights[2] = KeyLight::Lock {
            lock: LockKind::Num,
            off: Rgb(0, 0, 32),
            on: Rgb(0, 0, 255),
        };
        lights[3] = KeyLight::Lock {
            lock: LockKind::Scroll,
            off: Rgb(0, 32, 0),
            on: Rgb(0, 255, 0),
        };
        for (index, percent) in [20u8, 40, 60, 80].into_iter().enumerate() {
            lights[4 + index] = KeyLight::Battery {
                percent,
                below: Rgb(255, 0, 0),
                above: Rgb(0, 255, 0),
            };
        }
    }
    let lighting = p.lighting_mut(second).unwrap();
    lighting.fade_delay = Some(15);
    for position in (0..keys).step_by(3) {
        lighting.keys[position] = KeyLight::Color(Rgb(255, 160, 0));
    }
    p.lighting_mut(third).unwrap().keys[keys - 1] = KeyLight::Color(Rgb(128, 0, 128));
    (p, config)
}

#[test]
fn fixtures_match_the_emitter() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let update = std::env::var_os("UPDATE_FIXTURES").is_some();
    let mut stale = Vec::new();
    let boards = kc_boards::built_in().unwrap();
    let lit = boards
        .iter()
        .filter(|board| board.firmware.iter().any(|f| f.lighting.is_some()))
        .map(|board| {
            (
                board,
                format!("{}-lighting", board.id),
                lighting_fixture(board),
            )
        });
    let projects = boards.iter().flat_map(|board| {
        [
            (board, board.id.clone(), fixture(board)),
            (
                board,
                format!("{}-factory", board.id),
                (
                    Project::from_template(format!("{} factory layout", board.name), board),
                    FirmwareConfig::stock(board),
                ),
            ),
        ]
    });
    for (board, directory, (project, config)) in projects.chain(lit) {
        let errors: Vec<_> = kc_model::validate(&project, board, &config)
            .into_iter()
            .filter(|p| p.severity == kc_model::Severity::Error)
            .collect();
        assert_eq!(errors, [], "{directory}");
        let mut files = kc_emit::generate(&project, board, &config).unwrap();
        files.push(kc_emit::GeneratedFile {
            path: format!("project.{}", file::EXTENSION),
            contents: file::to_json(&project),
        });
        for generated in files {
            let path = root.join(&directory).join(&generated.path);
            if update {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, &generated.contents).unwrap();
            } else if std::fs::read_to_string(&path).ok().as_deref() != Some(&generated.contents) {
                stale.push(format!("{directory}/{}", generated.path));
            }
        }
    }
    assert!(
        stale.is_empty(),
        "fixtures differ from the emitter's output: {stale:?}\nIf the change is intended, rerun with UPDATE_FIXTURES=1 and review the diff."
    );
}
