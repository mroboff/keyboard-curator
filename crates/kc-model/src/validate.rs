//! Checks a layout, and the firmware configuration it is built with,
//! against the board.

use std::collections::HashSet;

use kc_boards::Board;
use kc_zmk::behaviors::{self, ParamKind};
use kc_zmk::keycodes::keycodes;
use kc_zmk::Feature;

use crate::behavior::BehaviorKind;
use crate::binding::{BehaviorRef, Binding, Param};
use crate::features::{InputProcessor, KeyLight, SettingValue};
use crate::firmware::FirmwareConfig;
use crate::ids::LayerId;
use crate::project::{Location, Project, MAX_LAYERS};

/// The Kconfig option that sets the highest LED brightness.
pub const BRIGHTNESS_MAX_SETTING: &str = "CONFIG_ZMK_RGB_UNDERGLOW_BRT_MAX";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The firmware would fail to build or misbehave.
    Error,
    /// Worth a look, but the firmware may still build.
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub severity: Severity,
    pub location: Location,
    pub message: String,
}

struct Checker<'a> {
    project: &'a Project,
    features: &'a [Feature],
    problems: Vec<Problem>,
}

impl Checker<'_> {
    fn error(&mut self, location: &Location, message: impl Into<String>) {
        self.problems.push(Problem {
            severity: Severity::Error,
            location: location.clone(),
            message: message.into(),
        });
    }

    fn warning(&mut self, location: &Location, message: impl Into<String>) {
        self.problems.push(Problem {
            severity: Severity::Warning,
            location: location.clone(),
            message: message.into(),
        });
    }

    fn layer(&mut self, location: &Location, id: LayerId) {
        if self.project.layer(id).is_none() {
            self.error(location, "refers to a layer that no longer exists");
        }
    }

    fn feature(&mut self, location: &Location, what: &str, feature: Option<Feature>) {
        if let Some(feature) = feature.filter(|f| !self.features.contains(f)) {
            self.error(
                location,
                format!("{what} is not supported by this firmware (needs {feature:?})"),
            );
        }
    }

    fn behavior_ref(&mut self, location: &Location, behavior: &BehaviorRef) {
        match behavior {
            BehaviorRef::BuiltIn(label) => match behaviors::built_in(label) {
                Some(def) => self.feature(location, &format!("&{label}"), def.requires),
                None => self.error(location, format!("&{label} is not a ZMK behavior")),
            },
            BehaviorRef::User { user } => {
                if self.project.behavior(*user).is_none() {
                    self.error(location, "uses a behavior that no longer exists");
                }
            }
        }
    }

    fn binding(&mut self, location: &Location, binding: &Binding) {
        let Binding::Behavior { behavior, params } = binding else {
            return;
        };
        self.behavior_ref(location, behavior);
        for param in params {
            match param {
                Param::Layer(id) => self.layer(location, *id),
                // `MACRO_PLACEHOLDER` stands for a macro's own parameter.
                Param::Key(expr)
                    if keycodes().get(&expr.key).is_none() && expr.key != "MACRO_PLACEHOLDER" =>
                {
                    self.warning(location, format!("`{}` is not a known keycode", expr.key));
                }
                _ => {}
            }
        }
        match behavior {
            BehaviorRef::BuiltIn(label) => {
                if let Some(def) = behaviors::built_in(label) {
                    self.built_in_params(location, def, params);
                }
            }
            BehaviorRef::User { user } => {
                if let Some(def) = self.project.behavior(*user) {
                    let expected = def.kind.param_count();
                    if params.len() != expected {
                        self.error(
                            location,
                            format!(
                                "&{} takes {expected} parameter(s), found {}",
                                def.label,
                                params.len()
                            ),
                        );
                    }
                }
            }
        }
    }

    fn built_in_params(
        &mut self,
        location: &Location,
        def: &behaviors::Behavior,
        params: &[Param],
    ) {
        let label = def.label;
        if params.len() != def.params.len() {
            self.error(
                location,
                format!(
                    "&{label} takes {} parameter(s), found {}",
                    def.params.len(),
                    params.len()
                ),
            );
            return;
        }
        for (expected, param) in def.params.iter().zip(params) {
            match (expected.kind, param) {
                (ParamKind::Keycode, Param::Key(_)) | (ParamKind::Layer, Param::Layer(_)) => {}
                (ParamKind::Constant(allowed), Param::Constant(name)) => {
                    if !allowed.iter().any(|c| c.name == name) {
                        self.error(location, format!("`{name}` is not valid for &{label}"));
                    }
                }
                (ParamKind::Command(commands), Param::Command { name, args }) => {
                    let Some(command) = commands.iter().find(|c| c.name == name) else {
                        self.error(location, format!("`{name}` is not a command of &{label}"));
                        continue;
                    };
                    self.feature(location, name, command.requires);
                    if args.len() != command.args.len() {
                        self.error(
                            location,
                            format!(
                                "{name} takes {} argument(s), found {}",
                                command.args.len(),
                                args.len()
                            ),
                        );
                        continue;
                    }
                    for (arg, value) in command.args.iter().zip(args) {
                        if !(arg.min..=arg.max).contains(value) {
                            self.error(
                                location,
                                format!(
                                    "{name} {} must be between {} and {}, found {value}",
                                    arg.name, arg.min, arg.max
                                ),
                            );
                        }
                    }
                }
                _ => self.error(
                    location,
                    format!(
                        "&{label} has the wrong kind of value for `{}`",
                        expected.name
                    ),
                ),
            }
        }
    }

    fn positions(&mut self, location: &Location, what: &str, positions: &[usize]) {
        if let Some(bad) = positions.iter().find(|p| **p >= self.project.key_count) {
            self.error(
                location,
                format!(
                    "{what} uses key position {bad}, but the layout has {} keys",
                    self.project.key_count
                ),
            );
        }
    }
}

/// The labels of the nodes custom devicetree defines, as in
/// `label: name {`.
fn raw_labels_in(text: &str) -> Vec<String> {
    let word = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    text.split('{')
        .filter_map(|head| {
            // The statement that opens the node is what follows the last
            // `;` or `}` before its brace.
            let head = head.rsplit([';', '}']).next()?.trim();
            let (label, name) = head.split_once(':')?;
            let (label, name) = (label.trim(), name.trim());
            (word(label) && !name.is_empty() && !name.contains(char::is_whitespace))
                .then(|| label.to_string())
        })
        .collect()
}

/// Every problem in `project`, checked against the board it targets.
pub fn validate(project: &Project, board: &Board, config: &FirmwareConfig) -> Vec<Problem> {
    let root = Location::Project;
    let mut c = Checker {
        project,
        features: &[],
        problems: Vec::new(),
    };

    if project.board != board.id {
        c.error(
            &root,
            format!(
                "the layout is for board `{}`, not `{}`",
                project.board, board.id
            ),
        );
        return c.problems;
    }
    match board.layout(&project.layout) {
        Some(layout) if layout.keys.len() != project.key_count => c.error(
            &root,
            format!(
                "the layout has {} keys per layer, but the physical layout `{}` has {}",
                project.key_count,
                layout.id,
                layout.keys.len()
            ),
        ),
        Some(_) => {}
        None => c.error(
            &root,
            format!("the board has no physical layout `{}`", project.layout),
        ),
    }
    let profile = board.profile(&config.profile);
    match profile {
        Some(profile) => c.features = &profile.capabilities,
        None => c.error(
            &root,
            format!("the board has no firmware profile `{}`", config.profile),
        ),
    }

    if project.layers.len() + project.reserved_layers > MAX_LAYERS {
        c.error(
            &root,
            format!(
                "{} layers plus {} reserved is more than ZMK's limit of {MAX_LAYERS}",
                project.layers.len(),
                project.reserved_layers
            ),
        );
    }
    let mut names = HashSet::new();
    for layer in &project.layers {
        let location = Location::Key {
            layer: layer.id,
            position: 0,
        };
        if layer.bindings.len() != project.key_count {
            c.error(
                &location,
                format!(
                    "layer `{}` has {} bindings, but the layout has {} keys",
                    layer.name,
                    layer.bindings.len(),
                    project.key_count
                ),
            );
        }
        if !names.insert(layer.name.to_lowercase()) {
            c.warning(
                &location,
                format!("more than one layer is named `{}`", layer.name),
            );
        }
    }

    for (location, binding) in project.bindings() {
        c.binding(&location, binding);
    }

    for def in &project.behaviors {
        let location = Location::Behavior(def.id);
        for behavior in def.kind.behavior_refs() {
            c.behavior_ref(&location, behavior);
        }
        match &def.kind {
            BehaviorKind::HoldTap(h) => {
                c.positions(&location, "hold-trigger", &h.hold_trigger_key_positions)
            }
            BehaviorKind::TapDance(t) if t.bindings.is_empty() => {
                c.error(&location, "a tap-dance needs at least one binding")
            }
            BehaviorKind::Macro(m) if m.params > 2 => {
                c.error(&location, "a macro takes at most two parameters")
            }
            _ => {}
        }
    }

    // Custom devicetree is not checked, except that it must not define a
    // label a second time: the firmware would not build.
    let mut raw_labels = HashSet::new();
    for label in raw_labels_in(&project.raw.behaviors)
        .into_iter()
        .chain(raw_labels_in(&project.raw.devicetree))
    {
        let clash = project.behaviors.iter().find(|b| b.label == label);
        if let Some(def) = clash {
            c.error(
                &Location::Behavior(def.id),
                format!("`&{label}` is also defined in the custom devicetree"),
            );
        } else if !raw_labels.insert(label.clone()) {
            c.error(
                &root,
                format!("the custom devicetree defines `{label}` more than once"),
            );
        }
    }

    for combo in &project.combos {
        let location = Location::Combo(combo.id);
        if combo.key_positions.len() < 2 {
            c.error(&location, "a combo needs at least two keys");
        }
        c.positions(&location, "the combo", &combo.key_positions);
        for layer in &combo.layers {
            c.layer(&location, *layer);
        }
    }

    for (index, rule) in project.conditional_layers.iter().enumerate() {
        let location = Location::ConditionalLayer(index);
        for layer in rule.if_layers.iter().chain([&rule.then_layer]) {
            c.layer(&location, *layer);
        }
    }

    let listeners: Vec<&str> = board.pointing.iter().map(|d| d.listener.as_str()).collect();
    for device in &project.pointing {
        // On firmware without pointing this is kept in the project but not
        // generated, so there is nothing to check.
        if !c.features.contains(&Feature::Pointing) {
            break;
        }
        let location = Location::Pointing(device.listener.clone());
        // The mouse keys have listeners on every firmware with pointing.
        let mouse_keys = kc_zmk::pointing::mouse_key_listener(&device.listener).is_some();
        if !mouse_keys && !listeners.contains(&device.listener.as_str()) {
            c.error(
                &location,
                format!("the board has no pointing device `{}`", device.listener),
            );
        }
        let processors = device.all_processors();
        let layers = device
            .overrides
            .iter()
            .flat_map(|o| o.layers.iter().copied());
        let temp = processors.filter_map(|p| match p {
            InputProcessor::TempLayer { layer, .. } => Some(*layer),
            _ => None,
        });
        for layer in layers.chain(temp).collect::<Vec<_>>() {
            c.layer(&location, layer);
        }
    }

    for lighting in &project.lighting {
        let location = Location::Lighting(lighting.layer);
        c.layer(&location, lighting.layer);
        // Colors for firmware without per-key lighting are kept in the
        // project but not generated.
        if !c.features.contains(&Feature::PerKeyLighting) {
            continue;
        }
        if lighting.keys.iter().any(
            |k| matches!(k, KeyLight::Battery { percent, .. } if ![20, 40, 60, 80].contains(percent)),
        ) {
            c.error(&location, "battery lights can switch at 20, 40, 60 or 80 percent");
        }
        if lighting.keys.len() != project.key_count {
            c.error(
                &location,
                format!(
                    "lighting has {} keys, but the layout has {}",
                    lighting.keys.len(),
                    project.key_count
                ),
            );
        }
    }

    for key in kc_zmk::settings::BRIGHTNESS_SETTINGS {
        if let Some(SettingValue::Int(value)) = config.settings.get(key) {
            if *value > i64::from(board.brightness_cap) {
                c.error(
                    &Location::Setting(key.into()),
                    format!(
                        "brightness {value} is above this board's limit of {}",
                        board.brightness_cap
                    ),
                );
            }
        }
    }

    // Per-key colors are one of the lighting effects. Unless the firmware
    // starts in it, a key has to cycle to it once.
    let lit = c.features.contains(&Feature::PerKeyLighting)
        && project
            .lighting
            .iter()
            .any(|l| l.keys.iter().any(|k| *k != KeyLight::Inherit));
    let starts_lit = profile
        .and_then(|p| p.lighting.as_ref())
        .is_some_and(|l| l.start_effect.is_some());
    let cycles = project.bindings().iter().any(|(_, b)| {
        matches!(b, Binding::Behavior { params, .. } if params.iter().any(
            |p| matches!(p, Param::Command { name, .. } if name == "RGB_EFF" || name == "RGB_EFR"),
        ))
    });
    let needs_map = profile
        .and_then(|p| p.lighting.as_ref())
        .is_some_and(|l| l.led_map_overlay);
    if lit && needs_map {
        let maps: Vec<_> = board
            .halves
            .iter()
            .filter_map(|h| h.leds.as_ref())
            .collect();
        if maps.len() != board.halves.len() || maps.iter().any(|m| m.layout != project.layout) {
            c.error(
                &root,
                "per-key lighting is not available for this key layout: the positions of its LEDs are not known",
            );
        } else if maps.iter().any(|m| !m.verified) {
            c.warning(
                &root,
                "the LED positions for this keyboard have not been confirmed on hardware, so colors may land on the wrong keys",
            );
        }
    }
    if lit && !starts_lit && !cycles {
        c.warning(
            &root,
            "per-key colors show once the lighting effect is switched to them, but no key changes the lighting effect",
        );
    }

    // Without a bootloader key, reflashing needs the hardware reset button.
    let reaches_bootloader = project.bindings().iter().any(|(_, b)| {
        matches!(b, Binding::Behavior { behavior: BehaviorRef::BuiltIn(label), .. } if label == "bootloader")
            || matches!(b, Binding::Raw { raw } if raw.contains("&bootloader"))
    });
    if !reaches_bootloader {
        c.warning(
            &root,
            "no key enters the bootloader, so flashing will need the reset button on each half",
        );
    }

    c.problems
}
