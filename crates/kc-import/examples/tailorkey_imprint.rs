//! Brings Moosy Research's TailorKey to the Cyboard Imprint, as the
//! template new Imprint layouts can start from.
//!
//! `cargo run -p kc-import --example tailorkey_imprint -- <tailorkey.toml> <rgb-scheme.dtsi> <out.kcproj>`
//!
//! The layout is TailorKey v5.2 Bilateral (QWERTY) in its RMK form (twelve
//! layers, named timing profiles), which this app's importer reads for the
//! Glove80. Each Glove80 key is moved to its counterpart on the Imprint;
//! the Imprint's two extra keys, the outer ends of its top row, take the
//! factory Escape and F11. The home-row mods become a left-hand and a
//! right-hand behavior each, triggered by the other hand's keys and the
//! thumbs as TailorKey's ZMK build does, which both ZMK and RMK take. The
//! trackballs stand in for the touchpads: the right one moves the pointer
//! and the left one scrolls, both scaled by TailorKey's mouse-speed layers.
//! Colors come from TailorKey's own RGB scheme for the Glove80, layer by
//! layer.

use std::collections::{BTreeSet, HashMap};
use std::error::Error;

use kc_model::behavior::BehaviorKind;
use kc_model::features::{
    InputProcessor, KeyLight, LockKind, PointingConfig, PointingOverride, Rgb,
};
use kc_model::{file, validate, BehaviorRef, Binding, KeyExpr, LayerId, Project, Severity};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Where each Glove80 key lands on the Imprint, in binding order.
fn glove80_to_imprint() -> Vec<Option<usize>> {
    let mut map = Vec::with_capacity(80);
    // The function row: five keys a hand, inside the Imprint's outer keys.
    map.extend((1..=5).map(Some));
    map.extend((6..=10).map(Some));
    // The number, tab and home rows: twelve keys each on both boards.
    for row in 0..3 {
        map.extend((12 + row * 12..24 + row * 12).map(Some));
    }
    // The Glove80's fifth row carries its upper thumb keys between the hands.
    map.extend((48..54).map(Some));
    map.extend((70..73).map(Some));
    map.extend((73..76).map(Some));
    map.extend((54..60).map(Some));
    // Its sixth row: five keys a hand around the lower thumb keys.
    map.extend((60..65).map(Some));
    map.extend((76..79).map(Some));
    map.extend((79..82).map(Some));
    map.extend((65..70).map(Some));
    assert_eq!(map.len(), 80);
    map
}

/// TailorKey's RGB scheme for the Glove80: per-layer color grids in the
/// Glove80's key order, with named colors.
struct Scheme {
    colors: HashMap<String, Rgb>,
    layers: Vec<(String, Option<u32>, Vec<String>)>,
}

fn parse_scheme(text: &str) -> Result<Scheme> {
    let mut colors = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("#define ") {
            let mut parts = rest.split_whitespace();
            if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
                if let (Some(short), Some(hex)) =
                    (name.strip_suffix("_RGB"), value.strip_prefix("0x"))
                {
                    let value = u32::from_str_radix(hex, 16)?;
                    colors.insert(
                        short.to_string(),
                        Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8),
                    );
                }
            }
        }
    }
    // The mouse-speed colors are aliases.
    for (alias, of) in [("FST", "GOL"), ("WRP", "CHU"), ("SLO", "COR")] {
        let color = *colors.get(of).ok_or(format!("no color {of}"))?;
        colors.insert(alias.to_string(), color);
    }
    let mut layers = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("layer-id = <LAYER_") {
        let before = &rest[..at];
        let open = before
            .rfind("bindings = <")
            .ok_or("bindings before layer-id")?;
        let grid = &before[open + "bindings = <".len()..];
        let grid = &grid[..grid.find('>').ok_or("unterminated bindings")?];
        let cells: Vec<String> = grid.split_whitespace().map(str::to_string).collect();
        let after = &rest[at + "layer-id = <LAYER_".len()..];
        let name = after[..after.find('>').ok_or("unterminated layer-id")?].to_string();
        let block_end = after.find("};").unwrap_or(after.len());
        let fade = after[..block_end].find("fade-delay = <").and_then(|i| {
            let v = &after[i + "fade-delay = <".len()..];
            v[..v.find('>')?].parse().ok()
        });
        layers.push((name, fade, cells));
        rest = after;
    }
    Ok(Scheme { colors, layers })
}

fn light(cell: &str, colors: &HashMap<String, Rgb>) -> Result<KeyLight> {
    let lock = |kind| {
        Ok(KeyLight::Lock {
            lock: kind,
            off: Rgb(0, 0, 0),
            on: colors["RED"],
        })
    };
    match cell {
        "___" => Ok(KeyLight::Off),
        "BCL" => lock(LockKind::Caps),
        "BNL" => lock(LockKind::Num),
        "BSL" => lock(LockKind::Scroll),
        name => colors
            .get(name)
            .map(|c| KeyLight::Color(*c))
            .ok_or_else(|| format!("unknown color {name}").into()),
    }
}

fn layer_named(project: &Project, name: &str) -> Option<LayerId> {
    project.layers.iter().find(|l| l.name == name).map(|l| l.id)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(toml), Some(scheme), Some(out)) = (args.next(), args.next(), args.next()) else {
        return Err(
            "usage: tailorkey_imprint <tailorkey.toml> <rgb-scheme.dtsi> <out.kcproj>".into(),
        );
    };
    let boards = kc_boards::built_in()?;
    let imprint = boards
        .iter()
        .find(|b| b.id == "cyboard-imprint")
        .ok_or("no Imprint")?;
    let text = std::fs::read_to_string(&toml)?;
    let imported = kc_import::import_file("tailorkey.toml", &text, None, &boards)?;
    if boards[imported.board].id != "moergo-glove80" {
        return Err("the layout is not for the Glove80".into());
    }
    let mut p = imported.project;
    for note in &imported.report.notes {
        eprintln!("note: {note}");
    }

    // Onto the Imprint.
    let dropped = p.remap_keys(imprint, &imprint.default_layout, &glove80_to_imprint())?;
    for name in dropped {
        eprintln!("combo dropped: {name}");
    }
    p.name = "TailorKey for the Imprint".into();
    let base = p.layers[0].id;
    p.layers[0].name = "Base".into();
    p.set_binding(base, 0, Binding::kp(KeyExpr::new("ESC")))?;
    p.set_binding(base, 11, Binding::kp(KeyExpr::new("F11")))?;

    // Which keys are on which hand, and the thumbs.
    let layout = imprint.layout(&imprint.default_layout).unwrap();
    let left: Vec<usize> = (0..layout.keys.len())
        .filter(|&i| layout.keys[i].x < 800)
        .collect();
    let right: Vec<usize> = (0..layout.keys.len())
        .filter(|&i| layout.keys[i].x >= 800)
        .collect();
    let thumbs: Vec<usize> = (70..82).collect();

    // Home-row mods: one behavior per hand, holding only for the other
    // hand's keys and the thumbs. Both firmwares take the positions.
    let hrm: Vec<_> = p
        .behaviors
        .iter()
        .filter(|d| matches!(&d.kind, BehaviorKind::HoldTap(h) if h.opposite_hand_hold))
        .map(|d| (d.id, d.label.clone(), d.name.clone(), d.kind.clone()))
        .collect();
    for (id, label, name, kind) in hrm {
        let BehaviorKind::HoldTap(mut hold_tap) = kind else {
            continue;
        };
        hold_tap.opposite_hand_hold = false;
        hold_tap.hold_trigger_on_release = true;
        let mut for_left = hold_tap.clone();
        for_left.hold_trigger_key_positions = right
            .iter()
            .chain(thumbs.iter().filter(|t| !right.contains(t)))
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut for_right = hold_tap;
        for_right.hold_trigger_key_positions = left
            .iter()
            .chain(thumbs.iter().filter(|t| !left.contains(t)))
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let right_id = p.add_behavior(
            format!("{label}_right"),
            format!("{name} (right hand)"),
            BehaviorKind::HoldTap(for_right),
        )?;
        {
            let def = p.behavior_mut(id)?;
            def.name = format!("{name} (left hand)");
            def.kind = BehaviorKind::HoldTap(for_left);
        }
        let layers: Vec<LayerId> = p.layers.iter().map(|l| l.id).collect();
        for layer in layers {
            for &position in &right {
                let binding = p.layer(layer).unwrap().bindings[position].clone();
                if let Binding::Behavior {
                    behavior: BehaviorRef::User { user },
                    params,
                } = binding
                {
                    if user == id {
                        p.set_binding(
                            layer,
                            position,
                            Binding::Behavior {
                                behavior: BehaviorRef::User { user: right_id },
                                params,
                            },
                        )?;
                    }
                }
            }
        }
    }

    // The trackballs stand in for the touchpads. TailorKey's mouse-speed
    // layers scale them as they scale its mouse keys.
    let speed =
        |layer: &str, multiplier: u32, divisor: u32, scroll: bool| -> Result<PointingOverride> {
            let layer = layer_named(&p, layer).ok_or(format!("no layer {layer}"))?;
            Ok(PointingOverride {
                layers: vec![layer],
                processors: vec![if scroll {
                    InputProcessor::ScrollScale {
                        multiplier,
                        divisor,
                    }
                } else {
                    InputProcessor::Scale {
                        multiplier,
                        divisor,
                    }
                }],
            })
        };
    p.pointing = vec![
        PointingConfig {
            listener: "trackball_peripheral_listener".into(),
            processors: Vec::new(),
            overrides: vec![
                speed("MouseSlow", 1, 9, false)?,
                speed("MouseFast", 3, 1, false)?,
                speed("MouseWarp", 12, 1, false)?,
            ],
        },
        PointingConfig {
            listener: "trackball_central_listener".into(),
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
            overrides: vec![
                speed("MouseSlow", 1, 9, true)?,
                speed("MouseFast", 3, 1, true)?,
                speed("MouseWarp", 12, 1, true)?,
            ],
        },
    ];

    // TailorKey's colors, layer by layer, moved to the Imprint's keys.
    let scheme = parse_scheme(&std::fs::read_to_string(&scheme)?)?;
    let map = glove80_to_imprint();
    let names = [
        ("HRM_WinLinx", "Base"),
        ("Autoshift", "Autoshift"),
        ("Gaming", "Gaming"),
        ("Cursor", "Cursor"),
        ("Symbol", "Symbol"),
        ("Mouse", "Mouse"),
        ("MouseSlow", "MouseSlow"),
        ("MouseFast", "MouseFast"),
        ("MouseWarp", "MouseWarp"),
        ("Lower", "Lower"),
    ];
    for (scheme_name, fade, cells) in &scheme.layers {
        let Some((_, ours)) = names.iter().find(|(s, _)| s == scheme_name) else {
            continue;
        };
        let layer = layer_named(&p, ours).ok_or(format!("no layer {ours}"))?;
        if cells.len() != 80 {
            return Err(format!("{scheme_name} has {} cells", cells.len()).into());
        }
        let mut keys = vec![KeyLight::Off; 82];
        let mut palette = BTreeSet::new();
        for (glove, cell) in cells.iter().enumerate() {
            let light = light(cell, &scheme.colors)?;
            if let KeyLight::Color(c) = light {
                palette.insert((c.0, c.1, c.2));
            }
            if let Some(ours) = map[glove] {
                keys[ours] = light;
            }
        }
        let lighting = p.lighting_mut(layer)?;
        lighting.keys = keys;
        lighting.fade_delay = *fade;
        lighting.palette = palette.into_iter().map(|(r, g, b)| Rgb(r, g, b)).collect();
    }

    let errors: Vec<_> = validate(&p, imprint, &kc_model::FirmwareConfig::stock(imprint))
        .into_iter()
        .filter(|problem| problem.severity == Severity::Error)
        .collect();
    if !errors.is_empty() {
        return Err(format!("the template is not valid: {errors:?}").into());
    }
    file::save(&p, std::path::Path::new(&out))?;
    println!(
        "{out}: {} layers, {} behaviors, {} combos, {} lit layers",
        p.layers.len(),
        p.behaviors.len(),
        p.combos.len(),
        p.lighting.len()
    );
    Ok(())
}
