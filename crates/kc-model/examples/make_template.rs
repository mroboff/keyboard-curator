//! Builds a board's factory template from the vendor's keymap file.
//!
//! The templates under `templates/` are what new projects start from. This
//! tool regenerates one when a vendor changes their factory keymap:
//!
//! `cargo run -p kc-model --example make_template -- <board-id> <vendor.keymap> <out.kcproj>`
//!
//! Layers are read from the vendor file. The behaviours and pointing setup
//! around them are written out below per board, because reading those from
//! devicetree is the importer's job.

use kc_boards::Board;
use kc_model::behavior::{BehaviorKind, Flavor, HoldTap, Macro, MacroStep, TapDance};
use kc_model::features::{InputProcessor, PointingConfig, PointingOverride};
use kc_model::text::parse_binding;
use kc_model::{file, validate, BehaviorRef, Binding, Project, Severity};

type Error = Box<dyn std::error::Error>;

fn strip_comments(src: &str) -> String {
    let mut out = String::new();
    let mut rest = src;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start..]
            .find("*/")
            .map_or("", |end| &rest[start + end + 2..]);
    }
    out.push_str(rest);
    out.lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The layers of a keymap file: node name and binding texts, in order.
fn layers(src: &str) -> Result<Vec<(String, Vec<String>)>, Error> {
    let src = strip_comments(src);
    let start = src.find("zmk,keymap").ok_or("no keymap node")?;
    let body = &src[start..];
    let body = &body[..body.find("\n};").unwrap_or(body.len())];
    let mut found = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find("bindings") {
        let open = rest[..at].rfind('{').ok_or("bindings outside a node")?;
        let name = rest[..open]
            .split_whitespace()
            .last()
            .ok_or("unnamed layer")?;
        let list = &rest[at..];
        let (from, to) = (
            list.find('<').ok_or("no binding list")?,
            list.find(">;").ok_or("unterminated binding list")?,
        );
        let bindings = list[from + 1..to]
            .split('&')
            .skip(1)
            .map(|b| format!("&{}", b.split_whitespace().collect::<Vec<_>>().join(" ")))
            .collect();
        found.push((name.to_string(), bindings));
        rest = &list[to..];
    }
    Ok(found)
}

fn layer_name(board: &str, node: &str) -> String {
    let imprint = [
        ("default_layer", "Base"),
        ("Numpad_Nav_Layer", "Numpad and Nav"),
        ("Keyboard_Control_Layer", "Keyboard Control"),
        ("Auto_Mouse_Layer", "Auto Mouse"),
        ("factory_test", "Factory Test"),
    ];
    if board == "cyboard-imprint" {
        if let Some((_, name)) = imprint.iter().find(|(n, _)| *n == node) {
            return (*name).to_string();
        }
    }
    node.strip_prefix("layer_").unwrap_or(node).to_string()
}

fn command(label: &str, name: &str, args: &[u32]) -> Binding {
    Binding::new(
        label,
        vec![kc_model::Param::Command {
            name: name.into(),
            args: args.to_vec(),
        }],
    )
}

/// The Go60's factory behaviours. Layers must exist first, by name.
fn go60_behaviors(p: &mut Project) -> Result<(), Error> {
    let plain = |steps| {
        BehaviorKind::Macro(Macro {
            wait_ms: None,
            tap_ms: None,
            params: 0,
            steps,
        })
    };
    let status = p.add_behavior(
        "rgb_ug_status_macro",
        "Show status lights",
        plain(vec![MacroStep::Tap(vec![command(
            "rgb_ug",
            "RGB_STATUS",
            &[],
        )])]),
    )?;
    p.add_behavior(
        "magic",
        "Magic",
        BehaviorKind::HoldTap(HoldTap {
            flavor: Flavor::TapPreferred,
            ..HoldTap::new(
                BehaviorRef::built_in("mo"),
                BehaviorRef::User { user: status },
            )
        }),
    )?;
    for (label, name, layer) in [
        ("keypad_td", "Keypad layer", "Keypad"),
        ("symbol_nav_td", "Symbol and Nav layer", "SymbolNav"),
    ] {
        let bindings = ["mo", "to"]
            .iter()
            .map(|b| parse_binding(p, &format!("&{b} {layer}")))
            .collect();
        p.add_behavior(
            label,
            name,
            BehaviorKind::TapDance(TapDance {
                tapping_term_ms: 200,
                bindings,
            }),
        )?;
    }
    for profile in 0..4 {
        let select = p.add_behavior(
            format!("bt_select_{profile}"),
            format!("Select Bluetooth {}", profile + 1),
            plain(vec![MacroStep::Tap(vec![
                command("out", "OUT_BLE", &[]),
                command("bt", "BT_SEL", &[profile]),
            ])]),
        )?;
        p.add_behavior(
            format!("bt_{profile}"),
            format!("Bluetooth {}", profile + 1),
            BehaviorKind::TapDance(TapDance {
                tapping_term_ms: 200,
                bindings: vec![
                    Binding::user(select, vec![]),
                    command("bt", "BT_DISC", &[profile]),
                ],
            }),
        )?;
    }
    Ok(())
}

fn pointing(p: &mut Project, board: &str) {
    let scale = |multiplier, divisor| InputProcessor::Scale {
        multiplier,
        divisor,
    };
    if board == "cyboard-imprint" {
        // The left trackball scrolls; the right one keeps the board default.
        p.pointing.push(PointingConfig {
            listener: "trackball_central_listener".into(),
            processors: vec![
                scale(1, 3),
                InputProcessor::ToScroll,
                InputProcessor::Transform {
                    invert_x: false,
                    invert_y: true,
                    swap_xy: false,
                    scroll: true,
                },
            ],
            overrides: vec![],
        });
        return;
    }
    let (symbol_nav, factory) = (p.layers[2].id, p.layers[4].id);
    p.pointing.push(PointingConfig {
        listener: "cirque_rh_listener".into(),
        processors: vec![scale(3, 1)],
        overrides: vec![PointingOverride {
            layers: vec![symbol_nav],
            processors: vec![scale(9, 1)],
        }],
    });
    p.pointing.push(PointingConfig {
        listener: "cirque_lh_listener".into(),
        processors: vec![
            InputProcessor::ToScroll,
            scale(1, 8),
            InputProcessor::Raw("<&zip_click_to_right_click_mapper>".into()),
        ],
        overrides: vec![PointingOverride {
            layers: vec![factory],
            processors: vec![scale(3, 1)],
        }],
    });
    // The left pad's tap is turned into a right click by a processor the
    // editor does not model, so it is carried as devicetree.
    p.raw.devicetree = "#include <zephyr/dt-bindings/input/input-event-codes.h>\n\n/ {\n    input_processors {\n        zip_click_to_right_click_mapper: zip_click_to_right_click_mapper {\n            compatible = \"zmk,input-processor-code-mapper\";\n            #input-processor-cells = <0>;\n            type = <INPUT_EV_KEY>;\n            map = <INPUT_BTN_0 INPUT_BTN_1>;\n        };\n    };\n};".into();
}

fn build(board: &Board, vendor: &str) -> Result<Project, Error> {
    let layers = layers(vendor)?;
    let mut p = Project::new(format!("{} factory layout", board.name), board);
    p.layers[0].name = layer_name(&board.id, &layers[0].0);
    for (node, _) in &layers[1..] {
        p.add_layer(layer_name(&board.id, node))?;
    }
    if board.id == "moergo-go60" {
        go60_behaviors(&mut p)?;
    }
    for (index, (node, bindings)) in layers.iter().enumerate() {
        if bindings.len() != p.key_count {
            return Err(format!(
                "{node} has {} bindings, expected {}",
                bindings.len(),
                p.key_count
            )
            .into());
        }
        let id = p.layers[index].id;
        for (position, text) in bindings.iter().enumerate() {
            let binding = parse_binding(&p, text);
            if matches!(binding, Binding::Raw { .. }) {
                return Err(format!("{node} key {position}: could not read `{text}`").into());
            }
            p.set_binding(id, position, binding)?;
        }
    }
    pointing(&mut p, &board.id);
    if board.id == "cyboard-imprint" {
        // Cyboard's own keymap keeps every spare slot free for ZMK Studio.
        p.set_reserved_layers(kc_model::project::MAX_LAYERS - p.layers.len())?;
    }
    let errors: Vec<_> = validate(&p, board)
        .into_iter()
        .filter(|problem| problem.severity == Severity::Error)
        .collect();
    if !errors.is_empty() {
        return Err(format!("the template is not valid: {errors:?}").into());
    }
    Ok(p)
}

fn main() -> Result<(), Error> {
    let mut args = std::env::args().skip(1);
    let (Some(board_id), Some(vendor), Some(out)) = (args.next(), args.next(), args.next()) else {
        return Err("usage: make_template <board-id> <vendor.keymap> <out.kcproj>".into());
    };
    let boards = kc_boards::built_in()?;
    let board = boards
        .iter()
        .find(|b| b.id == board_id)
        .ok_or_else(|| format!("unknown board `{board_id}`"))?;
    let project = build(board, &std::fs::read_to_string(vendor)?)?;
    file::save(&project, out.as_ref())?;
    println!(
        "{}: {} layers, {} behaviours",
        out,
        project.layers.len(),
        project.behaviors.len()
    );
    Ok(())
}
