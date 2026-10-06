//! Turns a parsed `.keymap` file into a project.

use std::collections::HashMap;

use kc_boards::Board;
use kc_model::behavior::{
    BehaviorKind, Flavor, HoldTap, Macro, MacroStep, ModMorph, StickyKey, TapDance,
};
use kc_model::features::{
    ConditionalLayer, InputProcessor, KeyLight, LockKind, PointingConfig, PointingOverride, Rgb,
    SettingValue,
};
use kc_model::text::parse_binding;
use kc_model::{BehaviorRef, Binding, LayerId, Project};
use kc_zmk::settings::{setting_for, SettingKind};
use kc_zmk::Modifier;

use crate::dts::{self, Node, Source};
use crate::{ImportError, Report};

/// Root nodes the importer turns into model data, rather than raw text.
const UNDERSTOOD: [&str; 7] = [
    "chosen",
    "behaviors",
    "macros",
    "combos",
    "conditional_layers",
    "keymap",
    "underglow-layer",
];

struct Importer<'a> {
    source: &'a Source,
    project: Project,
    report: Report,
    /// Labels of input processors the file defines that make a click a
    /// right click.
    right_click: Vec<String>,
}

/// Whether an input processor node is a code mapper that turns the first
/// button into the second: a click into a right click.
fn is_right_click_mapper(node: &Node) -> bool {
    let cells = |prop: &str| node.prop(prop).map(dts::tokens_in).unwrap_or_default();
    node.string("compatible") == Some("zmk,input-processor-code-mapper")
        && cells("#input-processor-cells") == ["0"]
        && cells("type") == ["INPUT_EV_KEY"]
        && cells("map") == ["INPUT_BTN_0", "INPUT_BTN_1"]
}

impl Importer<'_> {
    fn expand(&self, text: &str) -> String {
        dts::expand(text, &self.source.defines)
    }

    /// The bindings in a property, each read into the model.
    fn bindings(&mut self, value: &str) -> Vec<Binding> {
        let cells = dts::cell_groups(&self.expand(value)).join(" ");
        dts::bindings(&cells)
            .iter()
            .map(|text| {
                let binding = parse_binding(&self.project, text);
                if matches!(binding, Binding::Raw { .. }) {
                    self.report.raw_bindings += 1;
                }
                binding
            })
            .collect()
    }

    fn number(&self, node: &Node, prop: &str) -> Option<u32> {
        let value = self.expand(node.prop(prop)?);
        dts::numbers(&value)
            .first()
            .and_then(|n| u32::try_from(*n).ok())
    }

    fn layers(&self, value: &str) -> Vec<LayerId> {
        dts::numbers(&self.expand(value))
            .iter()
            .filter_map(|index| self.project.layers.get(usize::try_from(*index).ok()?))
            .map(|layer| layer.id)
            .collect()
    }

    fn behavior_ref(&self, text: &str) -> Option<BehaviorRef> {
        let label = text.trim().strip_prefix('&')?;
        if kc_zmk::behaviors::built_in(label).is_some() {
            return Some(BehaviorRef::built_in(label));
        }
        self.project
            .behaviors
            .iter()
            .find(|b| b.label == label)
            .map(|b| BehaviorRef::User { user: b.id })
    }

    fn modifiers(&self, node: &Node, prop: &str) -> Vec<Modifier> {
        let text = node.prop(prop).map(|v| self.expand(v)).unwrap_or_default();
        text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter_map(Modifier::from_flag)
            .collect()
    }

    /// Reads a behavior node, or `None` if it uses something not yet
    /// defined or not understood.
    fn behavior_kind(&mut self, node: &Node) -> Option<BehaviorKind> {
        let compatible = node.string("compatible")?;
        let groups = dts::cell_groups(&self.expand(node.prop("bindings").unwrap_or_default()));
        match compatible {
            "zmk,behavior-hold-tap" => {
                let [hold, tap] = groups.as_slice() else {
                    return None;
                };
                let flavor = match node.string("flavor") {
                    Some("balanced") => Flavor::Balanced,
                    Some("tap-preferred") => Flavor::TapPreferred,
                    Some("tap-unless-interrupted") => Flavor::TapUnlessInterrupted,
                    _ => Flavor::HoldPreferred,
                };
                let positions = node
                    .prop("hold-trigger-key-positions")
                    .map(|v| dts::numbers(&self.expand(v)))
                    .unwrap_or_default();
                Some(BehaviorKind::HoldTap(HoldTap {
                    hold: self.behavior_ref(hold)?,
                    tap: self.behavior_ref(tap)?,
                    flavor,
                    tapping_term_ms: self
                        .number(node, "tapping-term-ms")
                        .or_else(|| self.number(node, "tapping_term_ms"))
                        .unwrap_or(200),
                    quick_tap_ms: self
                        .number(node, "quick-tap-ms")
                        .or_else(|| self.number(node, "quick_tap_ms")),
                    require_prior_idle_ms: self.number(node, "require-prior-idle-ms"),
                    retro_tap: node.has("retro-tap"),
                    hold_while_undecided: node.has("hold-while-undecided"),
                    hold_trigger_key_positions: positions
                        .into_iter()
                        .filter_map(|p| usize::try_from(p).ok())
                        .collect(),
                    hold_trigger_on_release: node.has("hold-trigger-on-release"),
                }))
            }
            "zmk,behavior-tap-dance" => {
                let before = self.report.raw_bindings;
                let bindings = self.bindings(node.prop("bindings")?);
                // A tap-dance that refers to something not yet defined is
                // retried once that exists.
                if self.report.raw_bindings > before {
                    self.report.raw_bindings = before;
                    return None;
                }
                Some(BehaviorKind::TapDance(TapDance {
                    tapping_term_ms: self.number(node, "tapping-term-ms").unwrap_or(200),
                    bindings,
                }))
            }
            "zmk,behavior-mod-morph" => {
                let before = self.report.raw_bindings;
                let mut bindings = self.bindings(node.prop("bindings")?).into_iter();
                if self.report.raw_bindings > before {
                    self.report.raw_bindings = before;
                    return None;
                }
                Some(BehaviorKind::ModMorph(ModMorph {
                    normal: bindings.next()?,
                    morphed: bindings.next()?,
                    mods: self.modifiers(node, "mods"),
                    keep_mods: self.modifiers(node, "keep-mods"),
                }))
            }
            "zmk,behavior-sticky-key" => Some(BehaviorKind::StickyKey(StickyKey {
                behavior: self.behavior_ref(groups.first()?)?,
                release_after_ms: self.number(node, "release-after-ms").unwrap_or(1000),
                quick_release: node.has("quick-release"),
                lazy: node.has("lazy"),
                ignore_modifiers: node.has("ignore-modifiers"),
            })),
            "zmk,behavior-macro"
            | "zmk,behavior-macro-one-param"
            | "zmk,behavior-macro-two-param" => {
                let params = match compatible {
                    "zmk,behavior-macro" => 0,
                    "zmk,behavior-macro-one-param" => 1,
                    _ => 2,
                };
                let steps = self.macro_steps(&groups.join(" "))?;
                Some(BehaviorKind::Macro(Macro {
                    wait_ms: self.number(node, "wait-ms"),
                    tap_ms: self.number(node, "tap-ms"),
                    params,
                    steps,
                }))
            }
            _ => None,
        }
    }

    /// Reads a macro's flat binding list into steps. Bindings follow the
    /// most recent `&macro_tap`, `&macro_press` or `&macro_release`.
    fn macro_steps(&mut self, cells: &str) -> Option<Vec<MacroStep>> {
        #[derive(Clone, Copy, PartialEq)]
        enum Mode {
            Tap,
            Press,
            Release,
        }
        let mut steps: Vec<MacroStep> = Vec::new();
        let mut mode = Mode::Tap;
        // Whether the next binding starts a new step.
        let mut fresh = true;
        for text in dts::bindings(cells) {
            let mut parts = text.split_whitespace();
            let control = parts.next()?;
            let argument = parts.next().and_then(|n| n.parse::<u32>().ok());
            let new_mode = match control {
                "&macro_tap" => Some(Mode::Tap),
                "&macro_press" => Some(Mode::Press),
                "&macro_release" => Some(Mode::Release),
                _ => None,
            };
            if let Some(new_mode) = new_mode {
                (mode, fresh) = (new_mode, true);
                continue;
            }
            match control {
                "&macro_pause_for_release" => steps.push(MacroStep::PauseForRelease),
                "&macro_wait_time" => steps.push(MacroStep::WaitTime(argument?)),
                "&macro_tap_time" => steps.push(MacroStep::TapTime(argument?)),
                _ if control.starts_with("&macro_param_") => {
                    let digits: Vec<u8> = control
                        .chars()
                        .filter_map(|c| c.to_digit(10).map(|d| d as u8))
                        .collect();
                    let [from, to] = digits.as_slice() else {
                        return None;
                    };
                    steps.push(MacroStep::Param {
                        from: *from,
                        to: *to,
                    });
                }
                _ => {
                    let binding = parse_binding(&self.project, &text);
                    // `&kp MACRO_PLACEHOLDER` after a parameter step is
                    // expected to stay as text.
                    if matches!(binding, Binding::Raw { .. }) && !text.contains("MACRO_PLACEHOLDER")
                    {
                        return None;
                    }
                    let current = match (fresh, steps.last_mut(), mode) {
                        (false, Some(MacroStep::Tap(b)), Mode::Tap)
                        | (false, Some(MacroStep::Press(b)), Mode::Press)
                        | (false, Some(MacroStep::Release(b)), Mode::Release) => b,
                        _ => {
                            steps.push(match mode {
                                Mode::Tap => MacroStep::Tap(Vec::new()),
                                Mode::Press => MacroStep::Press(Vec::new()),
                                Mode::Release => MacroStep::Release(Vec::new()),
                            });
                            match steps.last_mut() {
                                Some(
                                    MacroStep::Tap(b) | MacroStep::Press(b) | MacroStep::Release(b),
                                ) => b,
                                _ => return None,
                            }
                        }
                    };
                    current.push(binding);
                    fresh = false;
                    continue;
                }
            }
            fresh = true;
        }
        Some(steps)
    }

    /// Imports behavior nodes, in whatever order lets each find the ones
    /// it builds on. Nodes that never resolve are kept as raw devicetree.
    fn behaviors(&mut self, nodes: Vec<&Node>) {
        let mut pending = nodes;
        loop {
            let mut deferred = Vec::new();
            let before = pending.len();
            for node in pending {
                let label = node.label().unwrap_or(&node.name).to_string();
                match self.behavior_kind(node) {
                    Some(kind) => match self.project.add_behavior(label.clone(), label, kind) {
                        Ok(id) => {
                            self.report.behaviors += 1;
                            if let Ok(def) = self.project.behavior_mut(id) {
                                def.description = node.comment.clone();
                            }
                        }
                        Err(_) => deferred.push(node),
                    },
                    None => deferred.push(node),
                }
            }
            if deferred.is_empty() || deferred.len() == before {
                for node in deferred {
                    let label = node.label().unwrap_or(&node.name);
                    self.report.raw_blocks.push(format!("behavior &{label}"));
                    self.project.raw.behaviors.push_str(&node.text);
                    self.project.raw.behaviors.push('\n');
                }
                return;
            }
            pending = deferred;
        }
    }

    fn combos(&mut self, node: &Node) {
        for combo in &node.children {
            let positions = combo
                .prop("key-positions")
                .map(|v| dts::numbers(&self.expand(v)))
                .unwrap_or_default()
                .into_iter()
                .filter_map(|p| usize::try_from(p).ok())
                .collect();
            let binding = combo
                .prop("bindings")
                .and_then(|v| self.bindings(v).into_iter().next())
                .unwrap_or_else(Binding::none);
            // Generated combos are named `combo_<name>_<id>`.
            let name = combo.name.strip_prefix("combo_").unwrap_or(&combo.name);
            let name = name
                .trim_end_matches(|c: char| c.is_ascii_digit())
                .trim_end_matches('_');
            let id = self.project.add_combo(name, positions, binding);
            let timeout = self.number(combo, "timeout-ms");
            let idle = self.number(combo, "require-prior-idle-ms");
            let layers = combo
                .prop("layers")
                .map(|v| self.layers(v))
                .unwrap_or_default();
            if let Ok(model) = self.project.combo_mut(id) {
                model.timeout_ms = timeout;
                model.require_prior_idle_ms = idle;
                model.slow_release = combo.has("slow-release");
                model.layers = layers;
            }
            self.report.combos += 1;
        }
    }

    fn processor(&self, group: &str) -> InputProcessor {
        let raw = || InputProcessor::Raw(format!("<{group}>"));
        let tokens = dts::tokens(group);
        let number = |index: usize| tokens.get(index).and_then(|t| t.parse::<u32>().ok());
        match tokens.first().map(String::as_str) {
            Some("&zip_xy_scaler") => match (number(1), number(2)) {
                (Some(multiplier), Some(divisor)) => InputProcessor::Scale {
                    multiplier,
                    divisor,
                },
                _ => raw(),
            },
            Some("&zip_scroll_scaler") => match (number(1), number(2)) {
                (Some(multiplier), Some(divisor)) => InputProcessor::ScrollScale {
                    multiplier,
                    divisor,
                },
                _ => raw(),
            },
            Some("&zip_xy_to_scroll_mapper") => InputProcessor::ToScroll,
            Some(label)
                if tokens.len() == 1
                    && self
                        .right_click
                        .iter()
                        .any(|l| label.strip_prefix('&') == Some(l)) =>
            {
                InputProcessor::RightClick
            }
            Some(node @ ("&zip_xy_transform" | "&zip_scroll_transform")) => {
                let flags = tokens.get(1).map(String::as_str).unwrap_or_default();
                InputProcessor::Transform {
                    invert_x: flags.contains("INPUT_TRANSFORM_X_INVERT"),
                    invert_y: flags.contains("INPUT_TRANSFORM_Y_INVERT"),
                    swap_xy: flags.contains("INPUT_TRANSFORM_XY_SWAP"),
                    scroll: node == "&zip_scroll_transform",
                }
            }
            Some("&zip_temp_layer") => {
                let layer = number(1).and_then(|i| self.project.layers.get(i as usize));
                match (layer, number(2)) {
                    (Some(layer), Some(timeout_ms)) => InputProcessor::TempLayer {
                        layer: layer.id,
                        timeout_ms,
                    },
                    _ => raw(),
                }
            }
            _ => raw(),
        }
    }

    fn processors(&self, value: &str) -> Vec<InputProcessor> {
        dts::cell_groups(&self.expand(value))
            .iter()
            .map(|group| self.processor(group))
            .collect()
    }

    fn pointing(&mut self, node: &Node) {
        let listener = node.name.trim_start_matches('&').to_string();
        let processors = node
            .prop("input-processors")
            .map(|v| self.processors(v))
            .unwrap_or_default();
        let overrides = node
            .children
            .iter()
            .map(|child| PointingOverride {
                layers: child
                    .prop("layers")
                    .map(|v| self.layers(v))
                    .unwrap_or_default(),
                processors: child
                    .prop("input-processors")
                    .map(|v| self.processors(v))
                    .unwrap_or_default(),
            })
            .collect();
        self.project.pointing.push(PointingConfig {
            listener,
            processors,
            overrides,
        });
    }

    fn color(token: &str) -> Option<Rgb> {
        const NAMED: [(&str, u32); 12] = [
            ("GREEN", 0x00ff00),
            ("RED", 0xff0000),
            ("BLUE", 0x0000ff),
            ("TEAL", 0x008080),
            ("ORANGE", 0xffa500),
            ("YELLOW", 0xffff00),
            ("GOLD", 0xffd700),
            ("PURPLE", 0x800080),
            ("PINK", 0xffc0cb),
            ("WHITE", 0xffffff),
            ("BLACK", 0x000000),
            ("___", 0x000000),
        ];
        let value = match token
            .strip_prefix("0x")
            .or_else(|| token.strip_prefix("0X"))
        {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => NAMED.iter().find(|(name, _)| *name == token)?.1,
        };
        Some(Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8))
    }

    fn light(text: &str) -> Option<KeyLight> {
        let tokens = dts::tokens(text);
        let color = |index: usize| tokens.get(index).and_then(|t| Self::color(t));
        Some(match tokens.first()?.as_str() {
            "&trans" => KeyLight::Inherit,
            "&ug" => match color(1)? {
                Rgb(0, 0, 0) => KeyLight::Off,
                color => KeyLight::Color(color),
            },
            lock @ ("&ug_cl" | "&ug_nl" | "&ug_sl") => KeyLight::Lock {
                lock: match lock {
                    "&ug_cl" => LockKind::Caps,
                    "&ug_nl" => LockKind::Num,
                    _ => LockKind::Scroll,
                },
                off: color(1)?,
                on: color(2)?,
            },
            battery if battery.starts_with("&ug_b") => KeyLight::Battery {
                percent: battery["&ug_b".len()..].parse::<u8>().ok()? * 10,
                below: color(1)?,
                above: color(2)?,
            },
            _ => return None,
        })
    }

    fn lighting(&mut self, node: &Node) {
        for child in &node.children {
            let layer = child
                .prop("layer-id")
                .and_then(|v| self.layers(v).into_iter().next());
            let Some((layer, value)) = layer.zip(child.prop("bindings")) else {
                continue;
            };
            let cells = dts::cell_groups(&self.expand(value)).join(" ");
            let lights: Option<Vec<KeyLight>> = dts::bindings(&cells)
                .iter()
                .map(|b| Self::light(b))
                .collect();
            let fade = self.number(child, "fade-delay");
            match lights {
                Some(lights) if lights.len() == self.project.key_count => {
                    if let Ok(lighting) = self.project.lighting_mut(layer) {
                        lighting.keys = lights;
                        lighting.fade_delay = fade;
                    }
                }
                _ => {
                    self.report
                        .raw_blocks
                        .push(format!("lighting for {}", child.name));
                    self.project.raw.devicetree.push_str(&format!(
                        "/ {{\n    underglow-layer {{\n        {}\n    }};\n}};\n",
                        child.text
                    ));
                }
            }
        }
    }
}

/// Imports a `.keymap` file as a project for `board`.
pub fn import_keymap(
    name: &str,
    text: &str,
    board: &Board,
) -> Result<(Project, Report), ImportError> {
    let source = dts::parse(text)?;
    // Several `/ { ... }` blocks are one root.
    let roots: Vec<&Node> = source.nodes.iter().filter(|n| n.name == "/").collect();
    let section = |name: &str| -> Vec<&Node> {
        roots
            .iter()
            .flat_map(|root| root.children.iter())
            .filter(|n| n.name == name)
            .collect()
    };
    let keymap = section("keymap")
        .into_iter()
        .next()
        .ok_or(ImportError::NoKeymap)?;
    let layers: Vec<&Node> = keymap
        .children
        .iter()
        .filter(|n| n.prop("bindings").is_some())
        .collect();
    let first = layers.first().ok_or(ImportError::NoKeymap)?;
    let count = |node: &Node| {
        let value = dts::expand(node.prop("bindings").unwrap_or_default(), &source.defines);
        dts::bindings(&dts::cell_groups(&value).join(" ")).len()
    };
    let keys = count(first);
    if let Some(other) = layers.iter().map(|l| count(l)).find(|n| *n != keys) {
        return Err(ImportError::UnevenLayers(keys, other));
    }

    // The layout is the one the file chooses, or else one with as many keys.
    let chosen = section("chosen")
        .iter()
        .find_map(|n| n.prop("zmk,physical-layout"))
        .map(|v| v.trim().trim_start_matches('&').to_string());
    let layout = chosen
        .and_then(|id| board.layout(&id))
        .filter(|l| l.keys.len() == keys)
        .or_else(|| {
            board
                .layout(&board.default_layout)
                .filter(|l| l.keys.len() == keys)
        })
        .or_else(|| board.layouts.iter().find(|l| l.keys.len() == keys))
        .ok_or_else(|| ImportError::KeyCount {
            found: keys,
            expected: board
                .layouts
                .iter()
                .map(|l| l.keys.len().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        })?;

    let mut project = Project::new(name, board);
    project.layout = layout.id.clone();
    project.key_count = keys;
    project.layers[0].bindings = vec![Binding::trans(); keys];
    let layer_name = |node: &Node| {
        node.string("display-name")
            .or_else(|| node.string("label"))
            .map_or_else(
                || node.name.trim_start_matches("layer_").to_string(),
                str::to_string,
            )
    };
    project.layers[0].name = layer_name(first);
    for node in &layers[1..] {
        project.add_layer(layer_name(node))?;
    }
    let reserved = keymap
        .children
        .iter()
        .filter(|n| n.string("status") == Some("reserved"))
        .count();
    project.set_reserved_layers(reserved)?;

    let mut importer = Importer {
        source: &source,
        project,
        report: Report {
            layers: layers.len(),
            ..Report::default()
        },
        right_click: section("input_processors")
            .iter()
            .flat_map(|n| n.children.iter())
            .filter(|n| is_right_click_mapper(n))
            .filter_map(|n| n.label().map(str::to_string))
            .collect(),
    };

    let behavior_nodes: Vec<&Node> = section("behaviors")
        .into_iter()
        .chain(section("macros"))
        .flat_map(|n| n.children.iter())
        .collect();
    // Behaviors are created in dependency order, then put back in the
    // order the file has them.
    let order: Vec<String> = behavior_nodes
        .iter()
        .map(|n| n.label().unwrap_or(&n.name).to_string())
        .collect();
    importer.behaviors(behavior_nodes);
    importer
        .project
        .behaviors
        .sort_by_key(|b| order.iter().position(|label| *label == b.label));

    for (index, node) in layers.iter().enumerate() {
        let bindings = importer.bindings(node.prop("bindings").unwrap_or_default());
        importer.project.layers[index].bindings = bindings;
    }
    for node in section("combos") {
        importer.combos(node);
    }
    for node in section("conditional_layers") {
        for rule in &node.children {
            let if_layers = rule
                .prop("if-layers")
                .map(|v| importer.layers(v))
                .unwrap_or_default();
            let then = rule
                .prop("then-layer")
                .and_then(|v| importer.layers(v).into_iter().next());
            if let Some(then_layer) = then {
                importer.project.conditional_layers.push(ConditionalLayer {
                    if_layers,
                    then_layer,
                });
            }
        }
    }
    for node in section("underglow-layer") {
        importer.lighting(node);
    }

    // Everything else is kept as written.
    // The mouse keys have listeners on every firmware, whatever the board.
    let listeners: Vec<&str> = board
        .pointing
        .iter()
        .map(|d| d.listener.as_str())
        .chain(kc_zmk::pointing::MOUSE_KEY_LISTENERS.iter().map(|l| l.0))
        .collect();
    for node in &source.nodes {
        if node.name == "/" {
            for child in node
                .children
                .iter()
                .filter(|c| !UNDERSTOOD.contains(&c.name.as_str()))
            {
                // Right-click mappers are part of the pointing settings;
                // any other input processor is kept as written.
                if child.name == "input_processors" {
                    let other: Vec<&str> = child
                        .children
                        .iter()
                        .filter(|n| !is_right_click_mapper(n) || n.label().is_none())
                        .map(|n| n.text.as_str())
                        .collect();
                    if !other.is_empty() {
                        importer.report.raw_blocks.push(child.name.clone());
                        importer.project.raw.devicetree.push_str(&format!(
                            "/ {{\n    input_processors {{\n        {}\n    }};\n}};\n",
                            other.join("\n        ")
                        ));
                    }
                    continue;
                }
                importer.report.raw_blocks.push(child.name.clone());
                importer
                    .project
                    .raw
                    .devicetree
                    .push_str(&format!("/ {{\n    {}\n}};\n", child.text));
            }
        } else if listeners.contains(&node.name.trim_start_matches('&')) {
            importer.pointing(node);
        } else {
            importer.report.raw_blocks.push(node.name.clone());
            importer.project.raw.devicetree.push_str(&node.text);
            importer.project.raw.devicetree.push('\n');
        }
    }
    // Raw devicetree may use headers the generated keymap does not include.
    let extra: Vec<&String> = source
        .includes
        .iter()
        .filter(|line| {
            !line.contains("dt-bindings/zmk/")
                && !line.contains("behaviors.dtsi")
                && !line.contains("input/processors.dtsi")
        })
        .collect();
    if !importer.project.raw.devicetree.is_empty() && !extra.is_empty() {
        let includes: String = extra.iter().map(|l| format!("{l}\n")).collect();
        importer.project.raw.devicetree =
            format!("{includes}\n{}", importer.project.raw.devicetree);
    }

    let Importer {
        mut project,
        mut report,
        ..
    } = importer;
    project.raw.behaviors = project.raw.behaviors.trim_end().to_string();
    project.raw.devicetree = project.raw.devicetree.trim_end().to_string();
    if !source.unknown_tests.is_empty() {
        report.notes.push(format!(
            "The file chooses what to include by whether {} is defined. Nothing the app knows defines it, so it was taken as not defined.",
            source.unknown_tests.join(", ")
        ));
    }
    if report.raw_bindings > 0 {
        report.notes.push(format!(
            "{} binding(s) could not be read and are kept as text.",
            report.raw_bindings
        ));
    }
    // Per-key lighting needs a firmware that has it.
    let lit = project
        .lighting
        .iter()
        .any(|l| l.keys.iter().any(|k| *k != KeyLight::Inherit));
    if let (true, Some(profile)) = (lit, board.firmware.iter().find(|f| f.lighting.is_some())) {
        project.firmware = profile.id.clone();
    }
    Ok((project, report))
}

/// Reads a `.conf` file into a project's settings. Options the app has
/// controls for become settings; the rest are kept as extra lines.
pub fn import_conf(project: &mut Project, text: &str) {
    let mut extra: HashMap<usize, String> = HashMap::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let known = trimmed.split_once('=').and_then(|(key, value)| {
            let setting = setting_for(key.trim())?;
            let value = value.trim();
            let parsed = match setting.kind {
                SettingKind::Bool => match value {
                    "y" => SettingValue::Bool(true),
                    "n" => SettingValue::Bool(false),
                    _ => return None,
                },
                SettingKind::Int { .. } => SettingValue::Int(value.parse().ok()?),
                SettingKind::Text { .. } => SettingValue::Text(value.trim_matches('"').to_string()),
            };
            Some((setting.key, parsed))
        });
        match known {
            Some((key, value)) => {
                project.settings.insert(key.to_string(), value);
            }
            None => {
                extra.insert(index, trimmed.to_string());
            }
        }
    }
    let mut lines: Vec<(usize, String)> = extra.into_iter().collect();
    lines.sort();
    project.raw.conf = lines
        .into_iter()
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n");
}
