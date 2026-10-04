//! The `.keymap` file.

use std::collections::HashSet;

use kc_boards::geometry::Key;
use kc_boards::Board;
use kc_model::behavior::{BehaviorDef, BehaviorKind, Flavor, MacroStep};
use kc_model::features::InputProcessor;
use kc_model::text::{format_binding, layer_constant, LayerStyle};
use kc_model::{BehaviorRef, Binding, LayerId, Project};
use kc_zmk::Feature;

use crate::{profile, EmitError, NOTICE};

struct Writer<'a> {
    project: &'a Project,
    /// Layers are written as `LAYER_Name` constants unless two layers would
    /// get the same constant, in which case plain indices are used.
    style: LayerStyle,
    out: String,
}

impl Writer<'_> {
    fn line(&mut self, depth: usize, text: &str) {
        if !text.is_empty() {
            self.out.push_str(&"    ".repeat(depth));
            self.out.push_str(text);
        }
        self.out.push('\n');
    }

    fn binding(&self, binding: &Binding) -> String {
        format_binding(self.project, binding, self.style)
    }

    fn behavior_ref(&self, behavior: &BehaviorRef) -> String {
        match behavior {
            BehaviorRef::BuiltIn(label) => format!("&{label}"),
            BehaviorRef::User { user } => {
                let label = self
                    .project
                    .behavior(*user)
                    .map_or("none", |b| b.label.as_str());
                format!("&{label}")
            }
        }
    }

    fn layer(&self, id: LayerId) -> String {
        match (self.style, self.project.layer(id)) {
            (LayerStyle::Constant, Some(layer)) => layer_constant(&layer.name),
            _ => self.project.layer_index(id).unwrap_or(0).to_string(),
        }
    }

    fn layers(&self, ids: &[LayerId]) -> String {
        ids.iter()
            .map(|id| self.layer(*id))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `<a>, <b>` for a list of bindings, as devicetree phandle arrays.
    fn binding_list(&self, bindings: &[&Binding]) -> String {
        bindings
            .iter()
            .map(|b| format!("<{}>", self.binding(b)))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn node_name(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn mods(mods: &[kc_zmk::Modifier]) -> String {
    let flags: Vec<&str> = mods.iter().map(|m| m.flag()).collect();
    format!("<({})>", flags.join("|"))
}

fn behavior(w: &mut Writer, def: &BehaviorDef) {
    let cells = def.kind.param_count();
    w.line(2, &format!("{}: {} {{", def.label, def.label));
    let compatible = match &def.kind {
        BehaviorKind::HoldTap(_) => "zmk,behavior-hold-tap",
        BehaviorKind::TapDance(_) => "zmk,behavior-tap-dance",
        BehaviorKind::ModMorph(_) => "zmk,behavior-mod-morph",
        BehaviorKind::StickyKey(_) => "zmk,behavior-sticky-key",
        BehaviorKind::Macro(m) => match m.params {
            0 => "zmk,behavior-macro",
            1 => "zmk,behavior-macro-one-param",
            _ => "zmk,behavior-macro-two-param",
        },
    };
    w.line(3, &format!("compatible = \"{compatible}\";"));
    w.line(3, &format!("#binding-cells = <{cells}>;"));
    match &def.kind {
        BehaviorKind::HoldTap(h) => {
            let flavor = match h.flavor {
                Flavor::HoldPreferred => "hold-preferred",
                Flavor::Balanced => "balanced",
                Flavor::TapPreferred => "tap-preferred",
                Flavor::TapUnlessInterrupted => "tap-unless-interrupted",
            };
            w.line(3, &format!("flavor = \"{flavor}\";"));
            w.line(3, &format!("tapping-term-ms = <{}>;", h.tapping_term_ms));
            if let Some(ms) = h.quick_tap_ms {
                w.line(3, &format!("quick-tap-ms = <{ms}>;"));
            }
            if let Some(ms) = h.require_prior_idle_ms {
                w.line(3, &format!("require-prior-idle-ms = <{ms}>;"));
            }
            if h.retro_tap {
                w.line(3, "retro-tap;");
            }
            if h.hold_while_undecided {
                w.line(3, "hold-while-undecided;");
            }
            if !h.hold_trigger_key_positions.is_empty() {
                let positions: Vec<String> = h
                    .hold_trigger_key_positions
                    .iter()
                    .map(usize::to_string)
                    .collect();
                w.line(
                    3,
                    &format!("hold-trigger-key-positions = <{}>;", positions.join(" ")),
                );
            }
            if h.hold_trigger_on_release {
                w.line(3, "hold-trigger-on-release;");
            }
            let (hold, tap) = (w.behavior_ref(&h.hold), w.behavior_ref(&h.tap));
            w.line(3, &format!("bindings = <{hold}>, <{tap}>;"));
        }
        BehaviorKind::TapDance(t) => {
            w.line(3, &format!("tapping-term-ms = <{}>;", t.tapping_term_ms));
            let list = w.binding_list(&t.bindings.iter().collect::<Vec<_>>());
            w.line(3, &format!("bindings = {list};"));
        }
        BehaviorKind::ModMorph(m) => {
            let list = w.binding_list(&[&m.normal, &m.morphed]);
            w.line(3, &format!("bindings = {list};"));
            w.line(3, &format!("mods = {};", mods(&m.mods)));
            if !m.keep_mods.is_empty() {
                w.line(3, &format!("keep-mods = {};", mods(&m.keep_mods)));
            }
        }
        BehaviorKind::StickyKey(s) => {
            let inner = w.behavior_ref(&s.behavior);
            w.line(3, &format!("bindings = <{inner}>;"));
            w.line(3, &format!("release-after-ms = <{}>;", s.release_after_ms));
            for (on, property) in [
                (s.quick_release, "quick-release;"),
                (s.lazy, "lazy;"),
                (s.ignore_modifiers, "ignore-modifiers;"),
            ] {
                if on {
                    w.line(3, property);
                }
            }
        }
        BehaviorKind::Macro(m) => {
            if let Some(ms) = m.wait_ms {
                w.line(3, &format!("wait-ms = <{ms}>;"));
            }
            if let Some(ms) = m.tap_ms {
                w.line(3, &format!("tap-ms = <{ms}>;"));
            }
            let steps: Vec<String> = m
                .steps
                .iter()
                .map(|step| {
                    let group = |control: &str, bindings: &[Binding]| {
                        let list: Vec<String> = bindings.iter().map(|b| w.binding(b)).collect();
                        format!("<{control} {}>", list.join(" "))
                    };
                    match step {
                        MacroStep::Tap(b) => group("&macro_tap", b),
                        MacroStep::Press(b) => group("&macro_press", b),
                        MacroStep::Release(b) => group("&macro_release", b),
                        MacroStep::PauseForRelease => "<&macro_pause_for_release>".to_string(),
                        MacroStep::WaitTime(ms) => format!("<&macro_wait_time {ms}>"),
                        MacroStep::TapTime(ms) => format!("<&macro_tap_time {ms}>"),
                        MacroStep::Param { from, to } => format!("<&macro_param_{from}to{to}>"),
                    }
                })
                .collect();
            w.line(3, "bindings");
            for (index, step) in steps.iter().enumerate() {
                let lead = if index == 0 { "= " } else { ", " };
                w.line(4, &format!("{lead}{step}"));
            }
            w.line(4, ";");
        }
    }
    w.line(2, "};");
}

/// Lays a layer's bindings out as a grid that mirrors the keyboard: a new
/// row starts wherever the layout steps back to the left, and bindings in
/// the same physical column line up.
fn grid(keys: &[Key], cells: &[String]) -> Vec<String> {
    // Assign each key a column slot from its x position; keys that share a
    // position (stacked thumb arcs) take the next free slot.
    let mut rows: Vec<Vec<(usize, &str)>> = Vec::new();
    let mut last_x = i32::MAX;
    for (key, cell) in keys.iter().zip(cells) {
        if key.x < last_x || rows.is_empty() {
            rows.push(Vec::new());
        }
        last_x = key.x;
        let row = rows.last_mut().expect("a row was just pushed");
        let wanted = (key.x.max(0) as usize + 50) / 100;
        let slot = row.last().map_or(wanted, |(prev, _)| wanted.max(prev + 1));
        row.push((slot, cell));
    }
    let slots = rows
        .iter()
        .flatten()
        .map(|(slot, _)| slot + 1)
        .max()
        .unwrap_or(0);
    let mut widths = vec![0; slots];
    for (slot, cell) in rows.iter().flatten() {
        widths[*slot] = widths[*slot].max(cell.chars().count());
    }
    rows.iter()
        .map(|row| {
            let mut line = String::new();
            let mut next = 0;
            for (slot, cell) in row {
                for width in &widths[next..*slot] {
                    if *width > 0 {
                        line.push_str(&" ".repeat(width + 2));
                    }
                }
                line.push_str(&format!("{cell:<width$}  ", width = widths[*slot]));
                next = slot + 1;
            }
            line.trim_end().to_string()
        })
        .collect()
}

fn processor(w: &Writer, processor: &InputProcessor) -> String {
    match processor {
        InputProcessor::Scale {
            multiplier,
            divisor,
        } => {
            format!("<&zip_xy_scaler {multiplier} {divisor}>")
        }
        InputProcessor::ToScroll => "<&zip_xy_to_scroll_mapper>".to_string(),
        InputProcessor::Transform {
            invert_x,
            invert_y,
            swap_xy,
            scroll,
        } => {
            let flags: Vec<&str> = [
                (*swap_xy, "INPUT_TRANSFORM_XY_SWAP"),
                (*invert_x, "INPUT_TRANSFORM_X_INVERT"),
                (*invert_y, "INPUT_TRANSFORM_Y_INVERT"),
            ]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, flag)| *flag)
            .collect();
            let node = if *scroll {
                "zip_scroll_transform"
            } else {
                "zip_xy_transform"
            };
            let flags = if flags.is_empty() {
                "0".to_string()
            } else {
                flags.join(" | ")
            };
            format!("<&{node} ({flags})>")
        }
        InputProcessor::TempLayer { layer, timeout_ms } => {
            format!("<&zip_temp_layer {} {timeout_ms}>", w.layer(*layer))
        }
        InputProcessor::Raw(text) => text.clone(),
    }
}

/// The `.keymap` file for `project`.
pub fn keymap(project: &Project, board: &Board) -> Result<String, EmitError> {
    let features = &profile(project, board)?.capabilities;
    let layout = board
        .layout(&project.layout)
        .ok_or_else(|| EmitError::NoLayout(project.layout.clone()))?;

    let constants: Vec<String> = project
        .layers
        .iter()
        .map(|l| layer_constant(&l.name))
        .collect();
    let unique = constants.iter().collect::<HashSet<_>>().len() == constants.len();
    let style = if unique {
        LayerStyle::Constant
    } else {
        LayerStyle::Index
    };
    let mut w = Writer {
        project,
        style,
        out: String::new(),
    };

    w.line(0, &format!("/* {NOTICE} */"));
    w.line(0, "");
    let mut includes = vec![
        "behaviors.dtsi",
        "dt-bindings/zmk/keys.h",
        "dt-bindings/zmk/bt.h",
        "dt-bindings/zmk/outputs.h",
    ];
    if features.contains(&Feature::RgbUnderglow) {
        includes.push("dt-bindings/zmk/rgb.h");
    }
    if features.contains(&Feature::ExtPower) {
        includes.push("dt-bindings/zmk/ext_power.h");
    }
    if features.contains(&Feature::Backlight) {
        includes.push("dt-bindings/zmk/backlight.h");
    }
    if features.contains(&Feature::Pointing) {
        includes.push("dt-bindings/zmk/pointing.h");
        includes.push("input/processors.dtsi");
        includes.push("dt-bindings/zmk/input_transform.h");
    }
    for include in includes {
        w.line(0, &format!("#include <{include}>"));
    }
    w.line(0, "");
    if unique {
        for (index, constant) in constants.iter().enumerate() {
            w.line(0, &format!("#define {constant} {index}"));
        }
        w.line(0, "");
    }

    w.line(0, "/ {");
    if board.layouts.len() > 1 {
        w.line(1, "chosen {");
        w.line(2, &format!("zmk,physical-layout = &{};", layout.id));
        w.line(1, "};");
        w.line(0, "");
    }

    let (macros, others): (Vec<&BehaviorDef>, Vec<&BehaviorDef>) = project
        .behaviors
        .iter()
        .partition(|b| matches!(b.kind, BehaviorKind::Macro(_)));
    let raw_behaviors = project.raw.behaviors.trim();
    if !others.is_empty() || !raw_behaviors.is_empty() {
        w.line(1, "behaviors {");
        for def in others {
            behavior(&mut w, def);
        }
        for line in raw_behaviors.lines() {
            w.line(2, line);
        }
        w.line(1, "};");
        w.line(0, "");
    }
    if !macros.is_empty() {
        w.line(1, "macros {");
        for def in macros {
            behavior(&mut w, def);
        }
        w.line(1, "};");
        w.line(0, "");
    }

    if !project.combos.is_empty() {
        w.line(1, "combos {");
        w.line(2, "compatible = \"zmk,combos\";");
        for combo in &project.combos {
            w.line(
                2,
                &format!(
                    "combo_{}_{} {{",
                    node_name(&combo.name).to_lowercase(),
                    combo.id.0
                ),
            );
            if let Some(ms) = combo.timeout_ms {
                w.line(3, &format!("timeout-ms = <{ms}>;"));
            }
            let positions: Vec<String> = combo.key_positions.iter().map(usize::to_string).collect();
            w.line(3, &format!("key-positions = <{}>;", positions.join(" ")));
            let binding = w.binding(&combo.binding);
            w.line(3, &format!("bindings = <{binding}>;"));
            if !combo.layers.is_empty() {
                let layers = w.layers(&combo.layers);
                w.line(3, &format!("layers = <{layers}>;"));
            }
            if let Some(ms) = combo.require_prior_idle_ms {
                w.line(3, &format!("require-prior-idle-ms = <{ms}>;"));
            }
            if combo.slow_release {
                w.line(3, "slow-release;");
            }
            w.line(2, "};");
        }
        w.line(1, "};");
        w.line(0, "");
    }

    if !project.conditional_layers.is_empty() {
        w.line(1, "conditional_layers {");
        w.line(2, "compatible = \"zmk,conditional-layers\";");
        for (index, rule) in project.conditional_layers.iter().enumerate() {
            w.line(2, &format!("rule_{index} {{"));
            let (if_layers, then) = (w.layers(&rule.if_layers), w.layer(rule.then_layer));
            w.line(3, &format!("if-layers = <{if_layers}>;"));
            w.line(3, &format!("then-layer = <{then}>;"));
            w.line(2, "};");
        }
        w.line(1, "};");
        w.line(0, "");
    }

    w.line(1, "keymap {");
    w.line(2, "compatible = \"zmk,keymap\";");
    for (index, layer) in project.layers.iter().enumerate() {
        w.line(0, "");
        w.line(2, &format!("layer_{index}_{} {{", node_name(&layer.name)));
        let name = layer.name.replace('\\', "").replace('"', "'");
        w.line(3, &format!("display-name = \"{name}\";"));
        w.line(3, "bindings = <");
        let cells: Vec<String> = layer.bindings.iter().map(|b| w.binding(b)).collect();
        for row in grid(&layout.keys, &cells) {
            w.line(4, &row);
        }
        w.line(3, ">;");
        w.line(2, "};");
    }
    // Empty slots that ZMK Studio can turn into layers must come last.
    for index in 0..project.reserved_layers {
        w.line(0, "");
        w.line(2, &format!("reserved_{index} {{"));
        w.line(3, "status = \"reserved\";");
        w.line(2, "};");
    }
    w.line(1, "};");
    w.line(0, "};");

    for device in &project.pointing {
        w.line(0, "");
        w.line(0, &format!("&{} {{", device.listener));
        let list = |w: &Writer, processors: &[InputProcessor]| {
            processors
                .iter()
                .map(|p| processor(w, p))
                .collect::<Vec<_>>()
                .join(", ")
        };
        if !device.processors.is_empty() {
            let processors = list(&w, &device.processors);
            w.line(1, &format!("input-processors = {processors};"));
        }
        for (index, layer_override) in device.overrides.iter().enumerate() {
            w.line(1, &format!("override_{index} {{"));
            let layers = w.layers(&layer_override.layers);
            w.line(2, &format!("layers = <{layers}>;"));
            let processors = list(&w, &layer_override.processors);
            w.line(2, &format!("input-processors = {processors};"));
            w.line(1, "};");
        }
        w.line(0, "};");
    }

    let raw = project.raw.devicetree.trim();
    if !raw.is_empty() {
        w.line(0, "");
        for line in raw.lines() {
            w.line(0, line);
        }
    }
    Ok(w.out)
}
