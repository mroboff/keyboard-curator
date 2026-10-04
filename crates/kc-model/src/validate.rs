//! Checks a project against its board and firmware profile.

use std::collections::HashSet;

use kc_boards::Board;
use kc_zmk::behaviors::{self, ParamKind};
use kc_zmk::keycodes::keycodes;
use kc_zmk::Feature;

use crate::behavior::BehaviorKind;
use crate::binding::{BehaviorRef, Binding, Param};
use crate::features::{InputProcessor, KeyLight, SettingValue};
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
                None => self.error(location, format!("&{label} is not a ZMK behaviour")),
            },
            BehaviorRef::User { user } => {
                if self.project.behavior(*user).is_none() {
                    self.error(location, "uses a behaviour that no longer exists");
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
                Param::Key(expr) if keycodes().get(&expr.key).is_none() => {
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

/// Every problem in `project`, checked against the board it targets.
pub fn validate(project: &Project, board: &Board) -> Vec<Problem> {
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
                "the project is for board `{}`, not `{}`",
                project.board, board.id
            ),
        );
        return c.problems;
    }
    match board.layout(&project.layout) {
        Some(layout) if layout.keys.len() != project.key_count => c.error(
            &root,
            format!(
                "the project has {} keys per layer, but layout `{}` has {}",
                project.key_count,
                layout.id,
                layout.keys.len()
            ),
        ),
        Some(_) => {}
        None => c.error(
            &root,
            format!("the board has no layout `{}`", project.layout),
        ),
    }
    match board.profile(&project.firmware) {
        Some(profile) => c.features = &profile.capabilities,
        None => c.error(
            &root,
            format!("the board has no firmware profile `{}`", project.firmware),
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
        let location = Location::Pointing(device.listener.clone());
        c.feature(
            &location,
            "pointing device configuration",
            Some(Feature::Pointing),
        );
        if !listeners.contains(&device.listener.as_str()) {
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
        if lighting.keys.iter().any(|k| *k != KeyLight::Inherit) {
            c.feature(&location, "per-key lighting", Some(Feature::PerKeyLighting));
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
        if let Some(SettingValue::Int(value)) = project.settings.get(key) {
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
