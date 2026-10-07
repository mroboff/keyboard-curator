//! Layouts as moergo-rmk's runtime configuration, and back.
//!
//! moergo-rmk keeps everything a host may change in one runtime
//! configuration: the keymap as a grid per layer, named timing profiles
//! for hold-taps, morse (tap-dance) keys, combos, macros, forks
//! (mod-morphs), the touchpads, and lighting as colored cells per layer
//! with conditional rules. Its own tools read and write that configuration
//! as a TOML file and over Rynk. This module turns a layout and its board's
//! settings into that configuration, and a configuration read from a file
//! or a keyboard into a layout, saying at each key what could not be
//! carried across.

pub mod tokens;

use std::collections::{BTreeMap, HashMap, HashSet};

use kc_boards::board::{FirmwareProfile, RmkMatrix, RmkProfile};
use kc_boards::Board;
use kc_model::behavior::{
    BehaviorKind, Flavor, HoldTap, Macro, MacroStep, ModMorph, StickyKey, TapDance,
};
use kc_model::features::{
    Combo, InputProcessor, KeyLight, LockKind, PointingConfig as DevicePointing, PointingOverride,
    PointingProfile, Rgb, SettingValue,
};
use kc_model::project::Layer;
use kc_model::{
    BehaviorId, BehaviorRef, Binding, Carried, FirmwareConfig, LayerId, Location, Param, Problem,
    Project, Severity,
};
use kc_zmk::Modifier;
use moergo_config::{
    AutoMouseLayerConfig, BackgroundConfig, BackgroundModeConfig, BatteryConditionConfig,
    BehaviorConfig, ChargeConditionConfig, ComboConfig, ConditionalSceneConfig, EffectKind,
    EffectsConfig, ForkConfig, IndicatorsConditionConfig, KeyConditionConfig, KeyTargetConfig,
    LayerConfig, LayerKeyConfig, LightRuleConfig, LightingConfig, MacroConfig,
    MacroOperationConfig, MorseBehaviorConfig, MorseConfig, MorseProfileConfig, OutputModeConfig,
    PointingConfig, PointingDeviceConfig, PointingLayerOverride, PointingModeConfig, RuntimeConfig,
    ScenePolicyConfig, Snapshot,
};
use rynk::rmk_types::action::{Action, KeyAction};
use rynk::rmk_types::protocol::rynk::LayerMetadata;

use crate::settings::{self, Values};
use tokens::{Context, Read, Reverse};

/// Hold timings the firmware stores in thirteen bits.
const TIMEOUT_MAX_MS: u32 = 0x1FFF;

/// Which parts of the keyboard's configuration a layout and its board's
/// settings speak for. What they do not claim is left as the keyboard
/// holds it when the layout is applied, and written at the firmware's
/// defaults in an exported file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Claims {
    pub bluetooth_name: bool,
    /// Whether the configuration has a lighting section at all.
    pub lighting: bool,
    pub brightness: bool,
    pub output_mode: bool,
    pub background: bool,
    pub effect: bool,
    pub palette: bool,
    pub effect_value: bool,
    pub effect_speed: bool,
    pub wake_layers: bool,
    pub combo_timeout: bool,
    pub combo_prior_idle: bool,
    pub oneshot_timeout: bool,
    pub oneshot_quick_release: bool,
    pub tap_interval: bool,
    /// Any of the default hold-tap profile's values.
    pub hold_profile: bool,
    pub flow_tap: bool,
    pub prior_idle: bool,
    /// The pointing devices the layout configures, by the firmware's
    /// device number.
    pub pointing: Vec<u8>,
}

/// A layout as the firmware's runtime configuration.
#[derive(Debug, Clone)]
pub struct Translation {
    pub config: RuntimeConfig,
    pub claims: Claims,
    /// What was changed on the way, without stopping the translation.
    pub warnings: Vec<Problem>,
}

struct Translator {
    problems: Vec<Problem>,
}

impl Translator {
    fn error(&mut self, location: Location, message: impl Into<String>) {
        self.problems.push(Problem {
            severity: Severity::Error,
            location,
            message: message.into(),
        });
    }

    fn warning(&mut self, location: Location, message: impl Into<String>) {
        self.problems.push(Problem {
            severity: Severity::Warning,
            location,
            message: message.into(),
        });
    }
}

fn project_problem(message: impl Into<String>) -> Vec<Problem> {
    vec![Problem {
        severity: Severity::Error,
        location: Location::Project,
        message: message.into(),
    }]
}

/// The runtime profile a firmware profile must have for any of this.
fn rmk_of(profile: &FirmwareProfile) -> Result<(&RmkProfile, &RmkMatrix), Vec<Problem>> {
    let rmk = profile
        .rmk
        .as_ref()
        .ok_or_else(|| project_problem("this firmware has no RMK configuration described"))?;
    let matrix = rmk
        .matrix
        .as_ref()
        .ok_or_else(|| project_problem("this firmware's key matrix is not described"))?;
    Ok((rmk, matrix))
}

/// The layout as moergo-rmk's runtime configuration, or every problem that
/// stops it.
pub fn translate(
    project: &Project,
    board: &Board,
    profile: &FirmwareProfile,
    firmware: &FirmwareConfig,
) -> Result<Translation, Vec<Problem>> {
    let (translation, problems) = build(project, board, profile, firmware);
    match translation {
        Some(mut translation) if problems.iter().all(|p| p.severity != Severity::Error) => {
            translation.warnings = problems;
            Ok(translation)
        }
        _ => Err(problems),
    }
}

/// Every problem with writing the layout as the firmware's configuration.
pub fn check(
    project: &Project,
    board: &Board,
    profile: &FirmwareProfile,
    firmware: &FirmwareConfig,
) -> Vec<Problem> {
    build(project, board, profile, firmware).1
}

/// The configuration as the TOML file moergo-rmk's own tools read.
pub fn toml(
    project: &Project,
    board: &Board,
    profile: &FirmwareProfile,
    firmware: &FirmwareConfig,
) -> Result<String, Vec<Problem>> {
    let translation = translate(project, board, profile, firmware)?;
    translation
        .config
        .to_toml()
        .map_err(|e| project_problem(format!("the configuration could not be written: {e:#}")))
}

/// Whether a binding can be written into the firmware's configuration.
pub fn expressible(binding: &Binding, project: &Project, rmk: &RmkProfile) -> bool {
    let cx = Context::new(project, rmk.ble_profiles);
    tokens::token(binding, &cx).is_ok()
}

/// A layer's name as the firmware stores it: at most 32 bytes.
fn layer_name(name: &str) -> String {
    let mut name = name.trim().to_string();
    if name.is_empty() {
        name = "Layer".into();
    }
    while name.len() > 32 {
        name.pop();
    }
    name
}

/// An identifier made from a name: lowercase letters, digits and
/// underscores, not starting with a digit.
fn slug(name: &str) -> String {
    let mut out: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').to_string();
    match out.chars().next() {
        None => "layer".into(),
        Some(c) if c.is_ascii_digit() => format!("l_{out}"),
        Some(_) => out,
    }
}

/// Whether a key sits on the half that is not the central one, by the
/// board's LED chains.
fn on_peripheral(board: &Board, position: usize) -> bool {
    board.halves.iter().any(|half| {
        !half.central
            && half
                .leds
                .as_ref()
                .is_some_and(|leds| leds.chain.contains(&position))
    })
}

fn unique(base: String, taken: &mut HashSet<String>) -> String {
    let mut candidate = base.clone();
    let mut n = 2;
    while !taken.insert(candidate.clone()) {
        candidate = format!("{base}_{n}");
        n += 1;
    }
    candidate
}

fn grid(cells: &[String], cols: u8) -> String {
    let mut text = String::from("\n");
    for row in cells.chunks(usize::from(cols)) {
        text.push_str(&row.join(" "));
        text.push('\n');
    }
    text
}

fn hex(color: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", color.0, color.1, color.2)
}

fn mode_name(flavor: Flavor) -> &'static str {
    match flavor {
        Flavor::HoldPreferred => "hold-on-other-press",
        Flavor::Balanced => "permissive-hold",
        Flavor::TapPreferred => "normal",
        Flavor::TapUnlessInterrupted => "tap-unless-interrupted",
    }
}

fn flavor_of(mode: Option<&str>) -> Flavor {
    match mode {
        Some("hold-on-other-press") => Flavor::HoldPreferred,
        Some("permissive-hold") => Flavor::Balanced,
        Some("tap-unless-interrupted") => Flavor::TapUnlessInterrupted,
        _ => Flavor::TapPreferred,
    }
}

fn output_mode(text: Option<&str>) -> OutputModeConfig {
    match text {
        Some("always-off") => OutputModeConfig::AlwaysOff,
        Some("powered-only") => OutputModeConfig::PoweredOnly,
        _ => OutputModeConfig::AlwaysOn,
    }
}

fn output_mode_name(mode: OutputModeConfig) -> &'static str {
    match mode {
        OutputModeConfig::AlwaysOn => "always-on",
        OutputModeConfig::AlwaysOff => "always-off",
        OutputModeConfig::PoweredOnly => "powered-only",
    }
}

fn background_mode(text: Option<&str>) -> BackgroundModeConfig {
    match text {
        Some("breathe") => BackgroundModeConfig::Breathe,
        _ => BackgroundModeConfig::Solid,
    }
}

/// The split node a key's light belongs to: the central half is node 0,
/// the peripheral node 1. Known from the LED chains.
fn node_of(board: &Board, key: usize) -> Option<u8> {
    board
        .halves
        .iter()
        .find(|half| {
            half.leds
                .as_ref()
                .is_some_and(|leds| leds.chain.contains(&key))
        })
        .map(|half| u8::from(!half.central))
}

/// A timing in milliseconds as the firmware stores it, with a warning
/// when it had to be shortened.
fn timeout(t: &mut Translator, location: &Location, what: &str, ms: u32) -> u16 {
    if ms > TIMEOUT_MAX_MS {
        t.warning(
            location.clone(),
            format!("{what} of {ms} ms was shortened to {TIMEOUT_MAX_MS} ms, the most RMK stores"),
        );
    }
    u16::try_from(ms.min(TIMEOUT_MAX_MS)).expect("clamped")
}

/// A hold-tap's timing as a named profile.
fn profile_config(
    t: &mut Translator,
    location: &Location,
    hold_tap: &HoldTap,
    matrix: &RmkMatrix,
) -> MorseProfileConfig {
    if hold_tap.hold_while_undecided {
        t.warning(
            location.clone(),
            "RMK has no hold while undecided; the key decides as its flavor says",
        );
    }
    let mut positions = Vec::new();
    for key in &hold_tap.hold_trigger_key_positions {
        match matrix.position(*key) {
            Some(position) => positions.push(position),
            None => t.warning(
                location.clone(),
                format!("hold-trigger key {key} is not a key of this board and was left out"),
            ),
        }
    }
    MorseProfileConfig {
        index: None,
        enable_flow_tap: hold_tap.require_prior_idle_ms.map(|_| true),
        hold_timeout_ms: Some(timeout(
            t,
            location,
            "the tapping term",
            hold_tap.tapping_term_ms,
        )),
        gap_timeout_ms: None,
        quick_tap_ms: hold_tap
            .quick_tap_ms
            .map(|ms| timeout(t, location, "quick tap", ms)),
        prior_idle_ms: hold_tap
            .require_prior_idle_ms
            .map(|ms| timeout(t, location, "the prior idle time", ms)),
        unilateral_tap: None,
        opposite_hand_hold: hold_tap.opposite_hand_hold.then_some(true),
        retro_tap: hold_tap.retro_tap.then_some(true),
        hold_trigger_on_release: hold_tap.hold_trigger_on_release.then_some(true),
        hold_trigger_key_positions: positions,
        mode: Some(mode_name(hold_tap.flavor).to_string()),
    }
}

fn morse_config(
    t: &mut Translator,
    location: &Location,
    name: &str,
    tap_dance: &TapDance,
    cx: &Context,
) -> Result<MorseConfig, String> {
    if tokens::bluetooth_tap_dance(tap_dance, cx.project).is_some() {
        return Err(tokens::too_few_profiles(cx.ble_profiles));
    }
    let mut steps = tap_dance.bindings.iter();
    let tap = steps
        .next()
        .map(|b| tokens::single(b, cx))
        .transpose()?
        .ok_or("a tap-dance needs at least one binding")?;
    let double_tap = steps.next().map(|b| tokens::single(b, cx)).transpose()?;
    if steps.next().is_some() {
        return Err("RMK's tap-dances hold one and two taps, not more".into());
    }
    let term = timeout(t, location, "the tapping term", tap_dance.tapping_term_ms);
    Ok(MorseConfig {
        name: name.to_string(),
        tap: Some(tap),
        hold: None,
        double_tap,
        hold_after_tap: None,
        hold_timeout_ms: Some(term),
        gap_timeout_ms: Some(term),
        quick_tap_ms: None,
        prior_idle_ms: None,
        unilateral_tap: None,
        opposite_hand_hold: None,
        retro_tap: None,
        hold_trigger_on_release: None,
        mode: None,
    })
}

/// A key a macro step sends, as Vial spells it: a key, possibly with
/// modifiers, and nothing else.
fn macro_key(binding: &Binding, cx: &Context) -> Result<String, String> {
    let code = tokens::via(binding, cx)?;
    if code >= 0x2000 {
        return Err("RMK's macros send keys, with or without modifiers".into());
    }
    Ok(moergo_config::keycodes::format_keycode(code))
}

fn macro_config(
    t: &mut Translator,
    location: &Location,
    name: &str,
    macro_def: &Macro,
    cx: &Context,
) -> Result<MacroConfig, String> {
    if tokens::bluetooth_macro(macro_def).is_some() {
        return Err(tokens::too_few_profiles(cx.ble_profiles));
    }
    if macro_def.params > 0 {
        return Err("RMK's macros take no parameters".into());
    }
    let mut operations = Vec::new();
    let wait = macro_def.wait_ms.filter(|ms| *ms > 0);
    for step in &macro_def.steps {
        let (bindings, make): (&[Binding], fn(String) -> MacroOperationConfig) = match step {
            MacroStep::Tap(b) => (b, |keycode| MacroOperationConfig::Tap { keycode }),
            MacroStep::Press(b) => (b, |keycode| MacroOperationConfig::Down { keycode }),
            MacroStep::Release(b) => (b, |keycode| MacroOperationConfig::Up { keycode }),
            MacroStep::WaitTime(ms) => {
                t.warning(
                    location.clone(),
                    "a change of wait time was written as one pause; RMK's macros pause only where told to",
                );
                operations.push(MacroOperationConfig::Delay {
                    ms: u16::try_from(*ms).unwrap_or(u16::MAX),
                });
                continue;
            }
            MacroStep::TapTime(_) => {
                t.warning(
                    location.clone(),
                    "RMK's macros tap at the keyboard's tap length; the tap time was left out",
                );
                continue;
            }
            MacroStep::PauseForRelease => {
                return Err("RMK's macros cannot wait for the key to be released".into());
            }
            MacroStep::Param { .. } => return Err("RMK's macros take no parameters".into()),
        };
        for binding in bindings {
            if let (Some(ms), false) = (wait, operations.is_empty()) {
                operations.push(MacroOperationConfig::Delay {
                    ms: u16::try_from(ms).unwrap_or(u16::MAX),
                });
            }
            operations.push(make(macro_key(binding, cx)?));
        }
    }
    if operations.is_empty() {
        return Err("a macro needs at least one step".into());
    }
    Ok(MacroConfig {
        name: name.to_string(),
        operations,
    })
}

fn fork_config(name: &str, mod_morph: &ModMorph, cx: &Context) -> Result<ForkConfig, String> {
    let names = |mods: &[Modifier]| -> Vec<String> {
        mods.iter()
            .map(|m| tokens::modifier_name(*m).to_string())
            .collect()
    };
    if mod_morph.mods.is_empty() {
        return Err("a mod-morph needs at least one modifier to morph on".into());
    }
    Ok(ForkConfig {
        name: name.to_string(),
        trigger: tokens::single(&mod_morph.normal, cx)?,
        output: tokens::single(&mod_morph.morphed, cx)?,
        mods: names(&mod_morph.mods),
        keep_mods: names(&mod_morph.keep_mods),
    })
}

fn combo_config(combo: &Combo, cx: &Context, matrix: &RmkMatrix) -> Result<ComboConfig, String> {
    if combo.key_positions.len() < 2 {
        return Err("a combo needs at least two keys".into());
    }
    let mut positions = Vec::new();
    for key in &combo.key_positions {
        positions.push(
            matrix
                .position(*key)
                .ok_or("the combo uses a key the layout does not have")?,
        );
    }
    let layer = match combo.layers.as_slice() {
        [] => None,
        [only] => Some(u8::try_from(cx_layer(cx, *only)?).expect("layers fit a byte")),
        _ => return Err("in RMK a combo applies to one layer or to all of them".into()),
    };
    Ok(ComboConfig {
        name: combo.name.clone(),
        keys: Vec::new(),
        positions,
        output: tokens::single(&combo.binding, cx)?,
        layer,
    })
}

fn cx_layer(cx: &Context, layer: LayerId) -> Result<usize, String> {
    cx.layers
        .iter()
        .position(|l| *l == layer)
        .ok_or_else(|| "the layer no longer exists".to_string())
}

/// What a pointing device does, in the firmware's terms.
struct Mode {
    mode: PointingModeConfig,
    auto_layer: Option<(LayerId, u32)>,
    warnings: Vec<&'static str>,
}

/// The pointing editor's settings read from a processor list in any
/// order: RMK's modes have no order, so the vendors' arrangements all
/// mean the same thing here.
fn simple_profile(processors: &[InputProcessor]) -> Result<PointingProfile, String> {
    let mut profile = PointingProfile::default();
    for processor in processors {
        match processor {
            InputProcessor::Scale {
                multiplier,
                divisor,
            }
            | InputProcessor::ScrollScale {
                multiplier,
                divisor,
            } => profile.speed = (*multiplier, (*divisor).max(1)),
            InputProcessor::ToScroll => profile.scroll = true,
            InputProcessor::Transform {
                invert_x,
                invert_y,
                swap_xy,
                ..
            } => {
                profile.invert_x |= invert_x;
                profile.invert_y |= invert_y;
                profile.swap_xy |= swap_xy;
            }
            InputProcessor::RightClick => profile.right_click = true,
            InputProcessor::TempLayer { layer, timeout_ms } => {
                profile.auto_layer = Some((*layer, *timeout_ms));
            }
            InputProcessor::Raw(text) => {
                return Err(format!(
                    "the pointing processor `{text}` means nothing to RMK"
                ));
            }
        }
    }
    Ok(profile)
}

fn pointing_mode(processors: &[InputProcessor]) -> Result<Mode, String> {
    let profile = simple_profile(processors)?;
    let (multiplier, divisor) = profile.speed;
    let small = |n: u32, what: &str| {
        u8::try_from(n).map_err(|_| format!("the {what} must be at most 255 for RMK"))
    };
    let mut warnings = Vec::new();
    if profile.swap_xy {
        warnings.push("RMK cannot swap a touchpad's axes; they were left as they are");
    }
    let mode = if profile.scroll {
        if profile.right_click {
            warnings.push("a scrolling touchpad keeps a left click in RMK");
        }
        PointingModeConfig::Scroll {
            multiplier_x: small(multiplier, "speed")?,
            divisor_x: small(divisor, "speed divisor")?,
            multiplier_y: small(multiplier, "speed")?,
            divisor_y: small(divisor, "speed divisor")?,
            invert_x: profile.invert_x,
            invert_y: profile.invert_y,
        }
    } else if divisor > 1 {
        if profile.right_click {
            warnings.push("a slowed touchpad keeps a left click in RMK");
        }
        PointingModeConfig::Sniper {
            multiplier: small(multiplier, "speed")?,
            divisor: small(divisor, "speed divisor")?,
            invert_x: profile.invert_x,
            invert_y: profile.invert_y,
        }
    } else {
        PointingModeConfig::Cursor {
            multiplier_x: small(multiplier, "speed")?,
            multiplier_y: small(multiplier, "speed")?,
            invert_x: profile.invert_x,
            invert_y: profile.invert_y,
            primary_button: profile.right_click.then_some(2),
        }
    };
    Ok(Mode {
        mode,
        auto_layer: profile.auto_layer,
        warnings,
    })
}

/// The colors of one layer as entries beside its keys.
fn light_entries(
    t: &mut Translator,
    board: &Board,
    matrix: &RmkMatrix,
    layer: &Layer,
    lights: &[KeyLight],
) -> Vec<LayerKeyConfig> {
    let solid = |color: String, rules: Vec<LightRuleConfig>, key: [u8; 2]| LayerKeyConfig {
        target: KeyTargetConfig::matrix_key(key[0], key[1]),
        action: None,
        color: Some(color),
        effect: EffectKind::Solid,
        period_ms: None,
        phase_ms: None,
        duty: None,
        step_ms: None,
        rules,
    };
    let rule = |color: String, when: KeyConditionConfig| LightRuleConfig {
        color,
        effect: EffectKind::Solid,
        period_ms: None,
        phase_ms: None,
        duty: None,
        step_ms: None,
        when: Some(when),
    };
    let mut entries = Vec::new();
    for (position, light) in lights.iter().enumerate() {
        let Some(key) = matrix.position(position) else {
            continue;
        };
        let location = Location::Lighting(layer.id);
        match light {
            KeyLight::Inherit => {}
            KeyLight::Off => entries.push(solid(hex(Rgb(0, 0, 0)), Vec::new(), key)),
            KeyLight::Color(color) => entries.push(solid(hex(*color), Vec::new(), key)),
            KeyLight::Lock { lock, off, on } => {
                let indicators = IndicatorsConditionConfig {
                    caps_lock: (*lock == LockKind::Caps).then_some(true),
                    num_lock: (*lock == LockKind::Num).then_some(true),
                    scroll_lock: (*lock == LockKind::Scroll).then_some(true),
                };
                let when = KeyConditionConfig {
                    indicators: Some(indicators),
                    ..KeyConditionConfig::default()
                };
                entries.push(solid(hex(*off), vec![rule(hex(*on), when)], key));
            }
            KeyLight::Battery {
                percent,
                below,
                above,
            } => {
                let Some(node) = node_of(board, position) else {
                    t.error(
                        location,
                        format!("key {position} is on no half this board describes, so its battery light has no battery to show"),
                    );
                    continue;
                };
                let when = KeyConditionConfig {
                    battery: Some(BatteryConditionConfig {
                        node,
                        min_level: Some(*percent),
                        max_level: None,
                        charge: ChargeConditionConfig::Any,
                    }),
                    ..KeyConditionConfig::default()
                };
                entries.push(solid(hex(*below), vec![rule(hex(*above), when)], key));
            }
        }
    }
    entries
}

fn status_light(light: &KeyLight) -> bool {
    matches!(light, KeyLight::Lock { .. } | KeyLight::Battery { .. })
}

fn build(
    project: &Project,
    board: &Board,
    profile: &FirmwareProfile,
    firmware: &FirmwareConfig,
) -> (Option<Translation>, Vec<Problem>) {
    let (rmk, matrix) = match rmk_of(profile) {
        Ok(found) => found,
        Err(problems) => return (None, problems),
    };
    let mut t = Translator {
        problems: Vec::new(),
    };
    let mut claims = Claims::default();
    let values = Values(firmware);

    let holds = if rmk.layers > 0 {
        rmk.layers.min(tokens::MAX_LAYERS)
    } else {
        tokens::MAX_LAYERS
    };
    if project.layers.len() > holds {
        t.error(
            Location::Project,
            format!(
                "the layout has {} layers, and this firmware holds {holds}",
                project.layers.len()
            ),
        );
    }
    let cx = Context::new(project, rmk.ble_profiles);
    let cells = usize::from(matrix.rows) * usize::from(matrix.cols);

    // Layers: the grid, and the colors beside it.
    let mut layers = Vec::new();
    let mut ids = HashSet::new();
    let mut wake_layers = Vec::new();
    for (index, layer) in project.layers.iter().enumerate() {
        let mut grid_cells = vec!["--".to_string(); cells];
        for (position, binding) in layer.bindings.iter().enumerate() {
            let Some([row, col]) = matrix.position(position) else {
                t.error(
                    Location::Key {
                        layer: layer.id,
                        position,
                    },
                    "this key has no place in the firmware's matrix",
                );
                continue;
            };
            match tokens::token(binding, &cx) {
                Ok(token) => {
                    // ZMK's `&bootloader` on a key of the other half restarts
                    // that half; moergo-rmk has a key of its own for that.
                    let token = if token == "QK_BOOT" && on_peripheral(board, position) {
                        format!("USER({})", tokens::USER_PERIPHERAL_BOOTLOADER)
                    } else {
                        token
                    };
                    grid_cells[usize::from(row) * usize::from(matrix.cols) + usize::from(col)] =
                        token;
                    if let Some(note) = tokens::degraded(binding, &cx) {
                        t.warning(
                            Location::Key {
                                layer: layer.id,
                                position,
                            },
                            note,
                        );
                    }
                }
                Err(reason) => t.error(
                    Location::Key {
                        layer: layer.id,
                        position,
                    },
                    reason,
                ),
            }
        }
        let key_entries = project
            .lighting(layer.id)
            .map(|lighting| light_entries(&mut t, board, matrix, layer, &lighting.keys))
            .unwrap_or_default();
        if project
            .lighting(layer.id)
            .is_some_and(|l| l.keys.iter().any(status_light))
        {
            wake_layers.push(u8::try_from(index).unwrap_or(u8::MAX));
        }
        layers.push(LayerConfig {
            id: unique(slug(&layer.name), &mut ids),
            name: layer_name(&layer.name),
            keys: grid(&grid_cells, matrix.cols),
            key_entries,
            light_entries: Vec::new(),
        });
    }

    // Behaviors the layout defines, in the order the keys refer to them.
    let mut profiles = BTreeMap::new();
    let mut morses = Vec::new();
    let mut macros = Vec::new();
    let mut forks = Vec::new();
    let mut sticky_timeout = None;
    let mut sticky_quick_release = None;
    for def in &project.behaviors {
        let location = Location::Behavior(def.id);
        match &def.kind {
            BehaviorKind::HoldTap(h) => {
                let config = profile_config(&mut t, &location, h, matrix);
                profiles.insert(def.label.clone(), config);
            }
            BehaviorKind::TapDance(_) | BehaviorKind::Macro(_) if !cx.has_slot(def.id) => {
                // Stands for one of the firmware's own keys; see the note
                // at the keys that use it.
            }
            BehaviorKind::TapDance(td) => match morse_config(&mut t, &location, &def.name, td, &cx)
            {
                Ok(morse) => morses.push(morse),
                Err(reason) => {
                    t.error(location, reason);
                    morses.push(MorseConfig::default());
                }
            },
            BehaviorKind::Macro(m) => match macro_config(&mut t, &location, &def.name, m, &cx) {
                Ok(macro_config) => macros.push(macro_config),
                Err(reason) => {
                    t.error(location, reason);
                    macros.push(MacroConfig {
                        name: def.name.clone(),
                        operations: Vec::new(),
                    });
                }
            },
            BehaviorKind::ModMorph(m) => match fork_config(&def.name, m, &cx) {
                Ok(fork) => forks.push(fork),
                Err(reason) => t.error(location, reason),
            },
            BehaviorKind::StickyKey(StickyKey {
                release_after_ms,
                quick_release,
                ..
            }) => {
                sticky_timeout.get_or_insert(*release_after_ms);
                sticky_quick_release.get_or_insert(*quick_release);
            }
        }
    }

    // Combos.
    let mut combos = Vec::new();
    let mut combo_timeout = None;
    let mut combo_prior_idle = None;
    for combo in &project.combos {
        match combo_config(combo, &cx, matrix) {
            Ok(config) => combos.push(config),
            Err(reason) => t.error(Location::Combo(combo.id), reason),
        }
        if let Some(ms) = combo.timeout_ms {
            combo_timeout = Some(combo_timeout.unwrap_or(0).max(ms));
        }
        if let Some(ms) = combo.require_prior_idle_ms {
            combo_prior_idle = Some(combo_prior_idle.unwrap_or(0).max(ms));
        }
        if combo.slow_release {
            t.warning(
                Location::Combo(combo.id),
                "RMK's combos release with the first key; slow release was left out",
            );
        }
    }

    // Pointing.
    let mut devices = Vec::new();
    let mut overrides = Vec::new();
    let mut auto_mouse_layers = Vec::new();
    for pointing in &project.pointing {
        let location = Location::Pointing(pointing.listener.clone());
        let Some(device) = rmk
            .pointing
            .iter()
            .find(|p| p.listener == pointing.listener)
        else {
            t.warning(
                location,
                "this firmware has no such pointing device; its settings were left out",
            );
            continue;
        };
        match pointing_mode(&pointing.processors) {
            Ok(mode) => {
                for warning in mode.warnings {
                    t.warning(location.clone(), warning);
                }
                devices.push(PointingDeviceConfig {
                    device_id: device.device,
                    mode: mode.mode,
                });
                claims.pointing.push(device.device);
                if let Some((layer, timeout_ms)) = mode.auto_layer {
                    match cx_layer(&cx, layer) {
                        Ok(index) => auto_mouse_layers.push(AutoMouseLayerConfig {
                            device_id: Some(device.device),
                            target_layer: u8::try_from(index).unwrap_or(u8::MAX),
                            timeout_ms: timeout_ms.max(1),
                            threshold: 1,
                            deactivate_on_key: false,
                            extra_mouse_keys: Vec::new(),
                            reset_timeout_on_key: false,
                        }),
                        Err(reason) => t.error(location.clone(), reason),
                    }
                }
            }
            Err(reason) => t.error(location.clone(), reason),
        }
        for over in &pointing.overrides {
            let mode = match pointing_mode(&over.processors) {
                Ok(mode) => mode,
                Err(reason) => {
                    t.error(location.clone(), reason);
                    continue;
                }
            };
            if mode.auto_layer.is_some() {
                t.warning(
                    location.clone(),
                    "a layer switch inside a layer's own pointing settings was left out",
                );
            }
            for layer in &over.layers {
                match cx_layer(&cx, *layer) {
                    Ok(index) => overrides.push(PointingLayerOverride {
                        layer: u8::try_from(index).unwrap_or(u8::MAX),
                        device_id: device.device,
                        mode: mode.mode.clone(),
                    }),
                    Err(reason) => t.error(location.clone(), reason),
                }
            }
        }
    }
    let pointing = (!devices.is_empty()).then_some(PointingConfig { devices, overrides });

    // Global behavior: the board's settings, with what the layout implies
    // where the board says nothing.
    claims.combo_timeout = values.is_set(settings::COMBO_TIMEOUT) || combo_timeout.is_some();
    claims.combo_prior_idle = combo_prior_idle.is_some();
    claims.oneshot_timeout = values.is_set(settings::ONESHOT_TIMEOUT) || sticky_timeout.is_some();
    claims.oneshot_quick_release =
        values.is_set(settings::ONESHOT_QUICK_RELEASE) || sticky_quick_release.is_some();
    claims.tap_interval = values.is_set(settings::TAP_INTERVAL);
    claims.hold_profile = [
        settings::HOLD_TIMEOUT,
        settings::HOLD_MODE,
        settings::QUICK_TAP,
        settings::UNILATERAL_TAP,
        settings::OPPOSITE_HAND_HOLD,
    ]
    .iter()
    .any(|key| values.is_set(key));
    claims.flow_tap = values.is_set(settings::FLOW_TAP);
    claims.prior_idle = values.is_set(settings::PRIOR_IDLE);
    let mut unilateral = values.bool(settings::UNILATERAL_TAP).unwrap_or(false);
    let opposite = values.bool(settings::OPPOSITE_HAND_HOLD).unwrap_or(false);
    if unilateral && opposite {
        t.warning(
            Location::Setting(settings::UNILATERAL_TAP.into()),
            "tap for the same hand and hold only for the other hand exclude each other; the hold wins",
        );
        unilateral = false;
    }
    let hold_timeout = values
        .u16(settings::HOLD_TIMEOUT)
        .unwrap_or(settings::defaults::HOLD_TIMEOUT_MS);
    let tap_interval = values
        .u16(settings::TAP_INTERVAL)
        .unwrap_or(settings::defaults::TAP_INTERVAL_MS);
    let behavior = BehaviorConfig {
        combo_timeout_ms: values
            .u16(settings::COMBO_TIMEOUT)
            .or(combo_timeout.map(|ms| u16::try_from(ms).unwrap_or(u16::MAX)))
            .unwrap_or(settings::defaults::COMBO_TIMEOUT_MS),
        oneshot_timeout_ms: values
            .u16(settings::ONESHOT_TIMEOUT)
            .or(sticky_timeout.map(|ms| u16::try_from(ms).unwrap_or(u16::MAX)))
            .unwrap_or(settings::defaults::ONESHOT_TIMEOUT_MS),
        tap_interval_ms: tap_interval,
        tap_capslock_interval_ms: tap_interval,
        tri_layer: None,
        combo_prior_idle_ms: combo_prior_idle.map(|ms| u16::try_from(ms).unwrap_or(u16::MAX)),
        oneshot_activate_on_keypress: false,
        oneshot_quick_release: values
            .bool(settings::ONESHOT_QUICK_RELEASE)
            .or(sticky_quick_release)
            .unwrap_or(false),
        morse: MorseBehaviorConfig {
            enable_flow_tap: values.bool(settings::FLOW_TAP).unwrap_or(false),
            prior_idle_ms: values
                .u16(settings::PRIOR_IDLE)
                .unwrap_or(settings::defaults::PRIOR_IDLE_MS),
            default_profile: MorseProfileConfig {
                index: None,
                enable_flow_tap: None,
                hold_timeout_ms: Some(hold_timeout),
                gap_timeout_ms: Some(hold_timeout),
                quick_tap_ms: values.u16(settings::QUICK_TAP).filter(|ms| *ms > 0),
                prior_idle_ms: None,
                unilateral_tap: Some(unilateral),
                opposite_hand_hold: opposite.then_some(true),
                retro_tap: None,
                hold_trigger_on_release: None,
                hold_trigger_key_positions: Vec::new(),
                mode: Some(
                    values
                        .text(settings::HOLD_MODE)
                        .unwrap_or(settings::defaults::HOLD_MODE)
                        .to_string(),
                ),
            },
            hold_trigger_key_positions: Vec::new(),
            profiles,
        },
        auto_mouse_layers,
    };

    // Lighting: the colors are the layout's, the controls the board's.
    let lit = layers.iter().any(|l| !l.key_entries.is_empty());
    claims.lighting = lit || values.any_lighting();
    claims.brightness = values.is_set(settings::BRIGHTNESS);
    claims.output_mode = values.is_set(settings::OUTPUT_MODE);
    claims.background = values
        .0
        .settings
        .keys()
        .any(|key| key.starts_with("rmk.lighting.background."));
    claims.effect = values.is_set(settings::EFFECT);
    claims.palette = values.is_set(settings::PALETTE);
    claims.effect_value = values.is_set(settings::EFFECT_VALUE);
    claims.effect_speed = values.is_set(settings::EFFECT_SPEED);
    claims.wake_layers = !wake_layers.is_empty();
    let effects = (claims.effect || claims.palette || claims.effect_value || claims.effect_speed)
        .then(|| EffectsConfig {
            effect: values
                .text(settings::EFFECT)
                .map(str::to_string)
                .or_else(|| rmk.effects.first().cloned())
                .unwrap_or_default(),
            overlay: None,
            palette: values
                .text(settings::PALETTE)
                .map(str::to_string)
                .or_else(|| rmk.palettes.first().cloned())
                .unwrap_or_default(),
            value: values.byte(settings::EFFECT_VALUE).unwrap_or(255),
            speed: values.byte(settings::EFFECT_SPEED).unwrap_or(128),
            params: BTreeMap::new(),
        });
    if let Some(effects) = &effects {
        if !rmk.effects.is_empty() && !rmk.effects.contains(&effects.effect) {
            t.error(
                Location::Setting(settings::EFFECT.into()),
                format!("this firmware has no animation named “{}”", effects.effect),
            );
        }
        if !rmk.palettes.is_empty() && !rmk.palettes.contains(&effects.palette) {
            t.error(
                Location::Setting(settings::PALETTE.into()),
                format!("this firmware has no palette named “{}”", effects.palette),
            );
        }
    }
    let lighting = claims.lighting.then(|| LightingConfig {
        brightness: values
            .int(settings::BRIGHTNESS)
            .map_or(settings::defaults::BRIGHTNESS, settings::brightness_byte),
        output_mode: output_mode(values.text(settings::OUTPUT_MODE)),
        wake_layers: wake_layers.clone(),
        scene_policy: ScenePolicyConfig::ActiveStack,
        background: BackgroundConfig {
            enabled: values.bool(settings::BACKGROUND_ENABLED).unwrap_or(false),
            hue: values.byte(settings::BACKGROUND_HUE).unwrap_or(0),
            saturation: values.byte(settings::BACKGROUND_SATURATION).unwrap_or(0),
            value: values.byte(settings::BACKGROUND_VALUE).unwrap_or(0),
            speed: values
                .byte(settings::BACKGROUND_SPEED)
                .unwrap_or(settings::defaults::BACKGROUND_SPEED),
            mode: background_mode(values.text(settings::BACKGROUND_MODE)),
        },
        effects,
        scenes: Vec::new(),
        conditional_scenes: Vec::new(),
    });

    claims.bluetooth_name = values.text(settings::BLUETOOTH_NAME).is_some();
    let config = RuntimeConfig {
        rows: matrix.rows,
        cols: matrix.cols,
        bluetooth_name: values.text(settings::BLUETOOTH_NAME).map(str::to_string),
        default_layer: 0,
        layers,
        morses,
        combos,
        macros,
        forks,
        behavior: Some(behavior),
        pointing,
        lighting,
    };

    // The firmware's own validation has the last word on spelling.
    if t.problems.iter().all(|p| p.severity != Severity::Error) {
        if let Err(error) = config.snapshot() {
            t.error(
                Location::Project,
                format!("the firmware's configuration model rejects this layout: {error:#}"),
            );
        }
    }
    let translation = Translation {
        config,
        claims,
        warnings: Vec::new(),
    };
    (Some(translation), t.problems)
}

/// Fills in what the layout does not claim from what the keyboard holds,
/// so that applying a layout changes nothing the board did not set, and
/// clears the layers above the layout's so that none of an earlier
/// configuration lingers.
pub fn settle(desired: &mut Snapshot, before: &Snapshot, claims: &Claims) {
    if !claims.bluetooth_name {
        desired.bluetooth_name = None;
    }
    let cells = usize::from(desired.rows) * usize::from(desired.cols);
    while desired.layers.len() < before.layers.len() {
        desired.layers.push(vec![KeyAction::Transparent; cells]);
    }
    if let (Some(names), Some(held)) = (&mut desired.layer_names, &before.layer_names) {
        while names.len() < held.len() {
            names.push(LayerMetadata::vacant());
        }
    }

    if let (Some(wanted), Some(held)) = (&mut desired.behaviors.config, &before.behaviors.config) {
        let keep_profile = !claims.hold_profile;
        let ours = *wanted;
        *wanted = *held;
        if claims.combo_timeout {
            wanted.combo_timeout_ms = ours.combo_timeout_ms;
        }
        if claims.oneshot_timeout {
            wanted.oneshot_timeout_ms = ours.oneshot_timeout_ms;
        }
        if claims.tap_interval {
            wanted.tap_interval_ms = ours.tap_interval_ms;
            wanted.tap_capslock_interval_ms = ours.tap_capslock_interval_ms;
        }
        if !keep_profile {
            wanted.morse_default_profile = ours.morse_default_profile;
        }
        if claims.prior_idle {
            wanted.morse_prior_idle_time_ms = ours.morse_prior_idle_time_ms;
        }
    }
    if let (Some(wanted), Some(held)) = (&mut desired.behaviors.options, &before.behaviors.options)
    {
        let ours = *wanted;
        *wanted = *held;
        if claims.combo_prior_idle {
            wanted.combo_prior_idle_ms = ours.combo_prior_idle_ms;
        }
        if claims.oneshot_quick_release {
            wanted.oneshot_quick_release = ours.oneshot_quick_release;
        }
        if claims.flow_tap {
            wanted.morse_enable_flow_tap = ours.morse_enable_flow_tap;
        }
        if claims.prior_idle {
            wanted.morse_prior_idle_ms = ours.morse_prior_idle_ms;
        }
        if claims.hold_profile {
            wanted.morse_default_profile = ours.morse_default_profile;
        }
    }

    if let (Some(wanted), Some(held)) = (&mut desired.lighting, &before.lighting) {
        if !claims.brightness {
            wanted.brightness = held.brightness;
        }
        if !claims.output_mode {
            wanted.output_mode = held.output_mode;
        }
        if !claims.wake_layers {
            wanted.wake_layers = held.wake_layers.clone();
        }
        if !claims.background {
            wanted.background = held.background.clone();
        }
        match (&mut wanted.effects, &held.effects) {
            (Some(ours), Some(theirs)) => {
                if !claims.effect {
                    ours.effect = theirs.effect.clone();
                }
                if !claims.palette {
                    ours.palette = theirs.palette.clone();
                }
                if !claims.effect_value {
                    ours.value = theirs.value;
                }
                if !claims.effect_speed {
                    ours.speed = theirs.speed;
                }
                ours.overlay = theirs.overlay.clone();
                ours.params = BTreeMap::new();
            }
            (ours @ None, Some(theirs)) => *ours = Some(theirs.clone()),
            _ => {}
        }
    }

    // Pointing devices the layout says nothing about stay as they are.
    if let (Some(wanted), Some(held)) = (&mut desired.pointing, &before.pointing) {
        let mut merged = *held;
        merged.revision = wanted.revision;
        let ours: Vec<_> = wanted.devices().to_vec();
        let mut devices: Vec<_> = held
            .devices()
            .iter()
            .filter(|d| !claims.pointing.contains(&d.device_id))
            .copied()
            .collect();
        devices.extend(ours);
        let mut overs: Vec<_> = held
            .overrides()
            .iter()
            .filter(|o| !claims.pointing.contains(&o.device_id))
            .copied()
            .collect();
        overs.extend(wanted.overrides().iter().copied());
        merged.device_count = 0;
        for (slot, device) in devices.into_iter().enumerate() {
            if let Some(place) = merged.devices.get_mut(slot) {
                *place = device;
                merged.device_count = u8::try_from(slot + 1).unwrap_or(u8::MAX);
            }
        }
        merged.override_count = 0;
        for (slot, over) in overs.into_iter().enumerate() {
            if let Some(place) = merged.overrides.get_mut(slot) {
                *place = over;
                merged.override_count = u8::try_from(slot + 1).unwrap_or(u8::MAX);
            }
        }
        *wanted = merged;
    }
}

/// A configuration read from a file or a keyboard, as a layout.
#[derive(Debug, Clone)]
pub struct Imported {
    pub project: Project,
    /// The board's settings the configuration carried.
    pub carried: Carried,
    /// What could not be kept as it was, in the user's terms.
    pub notes: Vec<String>,
}

/// How a key of a morse table entry is bound: as a tap-dance, or as a
/// hold-tap of its own when the entry holds.
enum MorseUse {
    TapDance(BehaviorId),
    HoldTap {
        behavior: BehaviorId,
        hold: Param,
        tap: Param,
    },
    Unbound,
}

struct Importer<'a> {
    project: Project,
    matrix: &'a RmkMatrix,
    notes: Vec<String>,
    labels: HashSet<String>,
}

impl Importer<'_> {
    fn note(&mut self, note: impl Into<String>) {
        let note = note.into();
        if !self.notes.contains(&note) {
            self.notes.push(note);
        }
    }

    /// A label for a behavior made from a name, unused so far.
    fn label(&mut self, name: &str, suffix: &str) -> String {
        let mut base = slug(name);
        if !suffix.is_empty() {
            base = format!("{base}_{suffix}");
        }
        if kc_zmk::behaviors::built_in(&base).is_some() {
            base = format!("{base}_key");
        }
        unique(base, &mut self.labels)
    }

    fn read(&mut self, action: Action, rx: &Reverse) -> Binding {
        let Read { binding, note } = tokens::single_binding(action, rx);
        if let Some(note) = note {
            self.note(note);
        }
        binding
    }

    fn key_of(&self, row: u8, col: u8) -> Option<usize> {
        self.matrix.key_at(row, col)
    }
}

/// One key action's text parsed the way the firmware's model parses the
/// grid, for combo keys and morse steps.
fn parse_token(text: &str, profile_names: &[String]) -> Result<KeyAction, String> {
    moergo_config::parse_key_actions_for_matrix(text, profile_names, 1, 1)
        .map(|actions| actions[0])
        .map_err(|e| format!("{e:#}"))
}

/// Turns a configuration into a layout for `board`. `led_to_key` says
/// which key of the layout an LED of the firmware's numbering lights, for
/// lighting cells addressed by LED.
pub fn import(
    config: &RuntimeConfig,
    board: &Board,
    profile: &FirmwareProfile,
    name: &str,
    led_to_key: &dyn Fn(u16) -> Option<usize>,
) -> Result<Imported, String> {
    let (rmk, matrix) = rmk_of(profile).map_err(|p| p[0].message.clone())?;
    if config.rows != matrix.rows || config.cols != matrix.cols {
        return Err(format!(
            "this configuration is for a keyboard with a {}×{} matrix, and the {} has {}×{}",
            config.rows, config.cols, board.name, matrix.rows, matrix.cols
        ));
    }
    let snapshot = config
        .snapshot()
        .map_err(|e| format!("the configuration is not one the firmware would take: {e:#}"))?;
    if snapshot.layers.len() > kc_model::project::MAX_LAYERS {
        return Err(format!(
            "the configuration has {} layers, more than a layout holds",
            snapshot.layers.len()
        ));
    }

    let mut project = Project::new(name, board);
    for (index, layer) in config.layers.iter().enumerate() {
        if index == 0 {
            project.layers[0].name = layer.name.clone();
        } else {
            project
                .add_layer(layer.name.clone())
                .map_err(|e| e.to_string())?;
        }
    }
    let layers: Vec<LayerId> = project.layers.iter().map(|l| l.id).collect();
    let mut im = Importer {
        project,
        matrix,
        notes: Vec::new(),
        labels: HashSet::new(),
    };
    for deferred in config.deferred_bindings() {
        im.note(format!(
            "{deferred} is bound through the keyboard's own key names, which only the keyboard can resolve; it was left as the grid has it"
        ));
    }
    let rx = Reverse {
        layers: &layers,
        ble_profiles: rmk.ble_profiles,
    };

    // Timing profiles, by slot.
    let entries = snapshot
        .behaviors
        .morse_profiles
        .clone()
        .unwrap_or_default();
    let mut profile_names = vec![
        String::new();
        entries
            .iter()
            .map(|e| usize::from(e.index) + 1)
            .max()
            .unwrap_or(0)
    ];
    for entry in &entries {
        profile_names[usize::from(entry.index)] = entry.name.clone();
    }
    let default_profile = config
        .behavior
        .as_ref()
        .map(|b| b.morse.default_profile.clone())
        .unwrap_or_default();

    // Which hold-tap behaviors the keys need: one per profile and kind of
    // hold, plus one per morse entry that holds.
    let mut hold_taps: HashMap<(u8, String), BehaviorId> = HashMap::new();
    let mut uses_of_morse: HashMap<u8, MorseUse> = HashMap::new();
    let mut macro_ids: HashMap<u8, BehaviorId> = HashMap::new();

    let all_actions: Vec<KeyAction> = snapshot.layers.iter().flatten().copied().collect();
    for action in &all_actions {
        match *action {
            KeyAction::TapHold(_, hold, index) if !tokens::is_default_profile(index) => {
                let Some(entry) = entries.iter().find(|e| e.index == index) else {
                    continue;
                };
                let Some((behavior, _)) = tokens::hold_side(hold, &rx) else {
                    continue;
                };
                let BehaviorRef::BuiltIn(hold_label) = &behavior else {
                    continue;
                };
                let key = (index, hold_label.clone());
                if hold_taps.contains_key(&key) {
                    continue;
                }
                let profile_config = config
                    .behavior
                    .as_ref()
                    .and_then(|b| b.morse.profiles.get(&entry.name))
                    .cloned()
                    .unwrap_or_default();
                // Labeled as the profile is named, so that it goes back out
                // under that name; a profile used both for a modifier and
                // for a layer makes two behaviors, the second numbered.
                let label = im.label(&entry.name, "");
                let mut hold_tap = HoldTap::new(behavior.clone(), BehaviorRef::built_in("kp"));
                hold_tap.flavor = flavor_of(profile_config.mode.as_deref());
                hold_tap.tapping_term_ms = u32::from(
                    profile_config
                        .hold_timeout_ms
                        .or(default_profile.hold_timeout_ms)
                        .unwrap_or(settings::defaults::HOLD_TIMEOUT_MS),
                );
                hold_tap.quick_tap_ms = profile_config.quick_tap_ms.map(u32::from);
                hold_tap.require_prior_idle_ms =
                    profile_config.enable_flow_tap.unwrap_or(false).then(|| {
                        u32::from(
                            profile_config
                                .prior_idle_ms
                                .unwrap_or(settings::defaults::PRIOR_IDLE_MS),
                        )
                    });
                hold_tap.retro_tap = profile_config.retro_tap.unwrap_or(false);
                hold_tap.hold_trigger_on_release =
                    profile_config.hold_trigger_on_release.unwrap_or(false);
                hold_tap.opposite_hand_hold = profile_config.opposite_hand_hold.unwrap_or(false);
                hold_tap.hold_trigger_key_positions = profile_config
                    .hold_trigger_key_positions
                    .iter()
                    .filter_map(|[row, col]| im.key_of(*row, *col))
                    .collect();
                let id = im
                    .project
                    .add_behavior(label, entry.name.clone(), BehaviorKind::HoldTap(hold_tap))
                    .map_err(|e| e.to_string())?;
                hold_taps.insert(key, id);
            }
            KeyAction::Morse(index) if !uses_of_morse.contains_key(&index) => {
                let Some(morse) = config.morses.get(usize::from(index)) else {
                    im.note(format!("a key uses tap-dance {index}, which the configuration does not define; it was left unbound"));
                    uses_of_morse.insert(index, MorseUse::Unbound);
                    continue;
                };
                let step = |im: &mut Importer, text: &Option<String>| -> Option<Binding> {
                    let text = text.as_deref()?;
                    match parse_token(text, &profile_names) {
                        Ok(KeyAction::Single(action)) => Some(im.read(action, &rx)),
                        Ok(KeyAction::No) => Some(Binding::none()),
                        _ => {
                            im.note(format!("the tap-dance step “{text}” could not be read"));
                            None
                        }
                    }
                };
                let tap = step(&mut im, &morse.tap);
                let hold = step(&mut im, &morse.hold);
                let double = step(&mut im, &morse.double_tap);
                if morse.hold_after_tap.is_some() {
                    im.note(format!(
                        "“{}” acts on hold after tap, which a layout cannot hold; that step was left out",
                        morse.name
                    ));
                }
                let name = if morse.name.is_empty() {
                    format!("Tap-dance {index}")
                } else {
                    morse.name.clone()
                };
                let term = morse.hold_timeout_ms.map_or(200, u32::from);
                let used = match (tap, hold, double) {
                    (Some(tap), Some(hold), None) => {
                        // A tap and a hold: a hold-tap with its own timing.
                        let (hold_behavior, hold_param) = match hold {
                            Binding::Behavior {
                                behavior: behavior @ BehaviorRef::BuiltIn(_),
                                params,
                            } if params.len() == 1 => (behavior, params[0].clone()),
                            _ => {
                                im.note(format!("“{name}” holds something a hold-tap cannot; it was left unbound"));
                                uses_of_morse.insert(index, MorseUse::Unbound);
                                continue;
                            }
                        };
                        let Some(tap_param) =
                            kc_model::edit::tap_key(&tap).cloned().map(Param::Key)
                        else {
                            im.note(format!(
                                "“{name}” taps something a hold-tap cannot; it was left unbound"
                            ));
                            uses_of_morse.insert(index, MorseUse::Unbound);
                            continue;
                        };
                        let mut hold_tap = HoldTap::new(hold_behavior, BehaviorRef::built_in("kp"));
                        hold_tap.flavor = flavor_of(morse.mode.as_deref());
                        hold_tap.tapping_term_ms = term;
                        hold_tap.quick_tap_ms = morse.quick_tap_ms.map(u32::from);
                        hold_tap.require_prior_idle_ms = morse.prior_idle_ms.map(u32::from);
                        hold_tap.retro_tap = morse.retro_tap.unwrap_or(false);
                        hold_tap.hold_trigger_on_release =
                            morse.hold_trigger_on_release.unwrap_or(false);
                        hold_tap.opposite_hand_hold = morse.opposite_hand_hold.unwrap_or(false);
                        let label = im.label(&name, "");
                        let id = im
                            .project
                            .add_behavior(label, name, BehaviorKind::HoldTap(hold_tap))
                            .map_err(|e| e.to_string())?;
                        MorseUse::HoldTap {
                            behavior: id,
                            hold: hold_param,
                            tap: tap_param,
                        }
                    }
                    (tap, hold, double) => {
                        if hold.is_some() {
                            im.note(format!(
                                "“{name}” also acts when held, which a tap-dance cannot; the hold was left out"
                            ));
                        }
                        let bindings: Vec<Binding> = [tap, double].into_iter().flatten().collect();
                        if bindings.is_empty() {
                            uses_of_morse.insert(index, MorseUse::Unbound);
                            continue;
                        }
                        let label = im.label(&name, "");
                        let id = im
                            .project
                            .add_behavior(
                                label,
                                name,
                                BehaviorKind::TapDance(TapDance {
                                    tapping_term_ms: term,
                                    bindings,
                                }),
                            )
                            .map_err(|e| e.to_string())?;
                        MorseUse::TapDance(id)
                    }
                };
                uses_of_morse.insert(index, used);
            }
            KeyAction::Single(Action::TriggerMacro(index)) if !macro_ids.contains_key(&index) => {
                let Some(macro_config) = config.macros.get(usize::from(index)) else {
                    im.note(format!("a key uses macro {index}, which the configuration does not define; it was left unbound"));
                    continue;
                };
                let mut steps = Vec::new();
                for operation in &macro_config.operations {
                    let (text, make): (&str, fn(Vec<Binding>) -> MacroStep) = match operation {
                        MacroOperationConfig::Tap { keycode } => (keycode, MacroStep::Tap),
                        MacroOperationConfig::Down { keycode } => (keycode, MacroStep::Press),
                        MacroOperationConfig::Up { keycode } => (keycode, MacroStep::Release),
                        MacroOperationConfig::Delay { ms } => {
                            im.note("a pause inside a macro was read as a change of its wait time");
                            steps.push(MacroStep::WaitTime(u32::from(*ms)));
                            continue;
                        }
                    };
                    let code = moergo_config::keycodes::parse_keycode(text)
                        .map_err(|e| format!("macro key “{text}”: {e:#}"))?;
                    let binding = match moergo_config::rynk_keycode::from_via_keycode(code) {
                        KeyAction::Single(action) => im.read(action, &rx),
                        _ => Binding::none(),
                    };
                    steps.push(make(vec![binding]));
                }
                let name = if macro_config.name.is_empty() {
                    format!("Macro {index}")
                } else {
                    macro_config.name.clone()
                };
                let label = im.label(&name, "");
                let id = im
                    .project
                    .add_behavior(
                        label,
                        name,
                        BehaviorKind::Macro(Macro {
                            wait_ms: None,
                            tap_ms: None,
                            params: 0,
                            steps,
                        }),
                    )
                    .map_err(|e| e.to_string())?;
                macro_ids.insert(index, id);
            }
            _ => {}
        }
    }

    // The keys.
    for (index, actions) in snapshot.layers.iter().enumerate() {
        let layer = layers[index];
        for (cell, action) in actions.iter().enumerate() {
            let row = u8::try_from(cell / usize::from(matrix.cols)).unwrap_or(u8::MAX);
            let col = u8::try_from(cell % usize::from(matrix.cols)).unwrap_or(u8::MAX);
            let Some(position) = im.key_of(row, col) else {
                if !matches!(action, KeyAction::No | KeyAction::Transparent) {
                    im.note(format!(
                        "layer {} binds matrix position [{row}, {col}], which is no key of the {}",
                        config.layers.get(index).map_or("?", |l| l.name.as_str()),
                        board.name
                    ));
                }
                continue;
            };
            let binding = match *action {
                KeyAction::No => Binding::none(),
                KeyAction::Transparent => Binding::trans(),
                KeyAction::Single(action) | KeyAction::Tap(action) => {
                    if let Action::TriggerMacro(n) = action {
                        match macro_ids.get(&n) {
                            Some(id) => Binding::user(*id, vec![]),
                            None => Binding::none(),
                        }
                    } else {
                        im.read(action, &rx)
                    }
                }
                KeyAction::TapHold(tap, hold, profile) => {
                    let (hold_behavior, hold_param) = match tokens::hold_side(hold, &rx) {
                        Some(side) => side,
                        None => {
                            im.note(format!(
                                "a hold-tap holding {} has no equivalent in a layout and was left unbound",
                                tokens::describe(KeyAction::Single(hold))
                            ));
                            continue;
                        }
                    };
                    let Some(tap_param) = tokens::tap_side(tap) else {
                        im.note(format!(
                            "a hold-tap tapping {} has no equivalent in a layout and was left unbound",
                            tokens::describe(KeyAction::Single(tap))
                        ));
                        continue;
                    };
                    let hold_label = match &hold_behavior {
                        BehaviorRef::BuiltIn(label) => label.clone(),
                        BehaviorRef::User { .. } => unreachable!("hold sides are built in"),
                    };
                    if tokens::is_default_profile(profile) {
                        match (hold_label.as_str(), &hold_param) {
                            ("kp", Param::Key(_)) => {
                                Binding::new("mt", vec![hold_param, tap_param])
                            }
                            ("mo", Param::Layer(_)) => {
                                Binding::new("lt", vec![hold_param, tap_param])
                            }
                            _ => {
                                im.note(format!(
                                    "a hold-tap holding {} has no equivalent in a layout and was left unbound",
                                    tokens::describe(KeyAction::Single(hold))
                                ));
                                continue;
                            }
                        }
                    } else {
                        match hold_taps.get(&(profile, hold_label)) {
                            Some(id) => Binding::user(*id, vec![hold_param, tap_param]),
                            None => {
                                im.note(format!("a key uses timing profile {profile}, which the configuration does not define; it was left unbound"));
                                continue;
                            }
                        }
                    }
                }
                KeyAction::Morse(n) => match uses_of_morse.get(&n) {
                    Some(MorseUse::TapDance(id)) => Binding::user(*id, vec![]),
                    Some(MorseUse::HoldTap {
                        behavior,
                        hold,
                        tap,
                    }) => Binding::user(*behavior, vec![hold.clone(), tap.clone()]),
                    Some(MorseUse::Unbound) | None => Binding::none(),
                },
                KeyAction::LayerModTap(..) => {
                    im.note("a layer-and-modifier tap key has no equivalent in a layout and was left unbound");
                    Binding::none()
                }
                _ => {
                    im.note("a key of a kind this app does not know was left unbound");
                    Binding::none()
                }
            };
            im.project
                .set_binding(layer, position, binding)
                .map_err(|e| e.to_string())?;
        }
    }

    // Forks: a mod-morph at every key that has the fork's trigger.
    for fork in &config.forks {
        let parse = |im: &mut Importer, text: &str| -> Option<Binding> {
            match moergo_config::keycodes::parse_keycode(text)
                .ok()
                .map(moergo_config::rynk_keycode::from_via_keycode)
            {
                Some(KeyAction::Single(action)) => Some(im.read(action, &rx)),
                Some(KeyAction::No) => Some(Binding::none()),
                _ => None,
            }
        };
        let (Some(normal), Some(morphed)) =
            (parse(&mut im, &fork.trigger), parse(&mut im, &fork.output))
        else {
            im.note(format!(
                "the fork “{}” uses keys a layout cannot hold and was left out",
                fork.name
            ));
            continue;
        };
        let mods = |names: &[String]| -> Vec<Modifier> {
            names
                .iter()
                .filter_map(|name| {
                    Modifier::ALL
                        .into_iter()
                        .find(|m| tokens::modifier_name(*m).eq_ignore_ascii_case(name.trim()))
                })
                .collect()
        };
        let name = if fork.name.is_empty() {
            "Mod-morph".to_string()
        } else {
            fork.name.clone()
        };
        let label = im.label(&name, "");
        let id = im
            .project
            .add_behavior(
                label,
                name,
                BehaviorKind::ModMorph(ModMorph {
                    normal: normal.clone(),
                    morphed,
                    mods: mods(&fork.mods),
                    keep_mods: mods(&fork.keep_mods),
                }),
            )
            .map_err(|e| e.to_string())?;
        let mut placed = false;
        im.project.for_each_binding_mut(|location, binding| {
            if matches!(location, Location::Key { .. }) && *binding == normal {
                *binding = Binding::user(id, vec![]);
                placed = true;
            }
        });
        if !placed {
            im.note(format!(
                "the fork “{}” is triggered by a key no layer has, so it is defined but unused",
                fork.name
            ));
        }
    }

    // Combos, by position or by what their keys do on their layer.
    for combo in &config.combos {
        let layer_index = usize::from(combo.layer.unwrap_or(0));
        let mut positions: Vec<usize> = combo
            .positions
            .iter()
            .filter_map(|[row, col]| im.key_of(*row, *col))
            .collect();
        if positions.is_empty() {
            let Some(actions) = snapshot.layers.get(layer_index) else {
                continue;
            };
            let mut found = true;
            for key in &combo.keys {
                let Ok(wanted) = parse_token(key, &profile_names) else {
                    found = false;
                    break;
                };
                let position = actions
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| **a == wanted)
                    .filter_map(|(cell, _)| {
                        im.key_of(
                            u8::try_from(cell / usize::from(matrix.cols)).ok()?,
                            u8::try_from(cell % usize::from(matrix.cols)).ok()?,
                        )
                    })
                    .find(|p| !positions.contains(p));
                match position {
                    Some(position) => positions.push(position),
                    None => {
                        found = false;
                        break;
                    }
                }
            }
            if !found {
                im.note(format!(
                    "the combo “{}” names keys that are not on its layer, so it was left out",
                    combo.name
                ));
                continue;
            }
        }
        let binding = match moergo_config::keycodes::parse_keycode(&combo.output)
            .ok()
            .map(moergo_config::rynk_keycode::from_via_keycode)
        {
            Some(KeyAction::Single(action)) => im.read(action, &rx),
            Some(KeyAction::TapHold(tap, hold, _)) => {
                match (tokens::hold_side(hold, &rx), tokens::tap_side(tap)) {
                    (Some((BehaviorRef::BuiltIn(label), hold)), Some(tap)) if label == "kp" => {
                        Binding::new("mt", vec![hold, tap])
                    }
                    (Some((BehaviorRef::BuiltIn(label), hold)), Some(tap)) if label == "mo" => {
                        Binding::new("lt", vec![hold, tap])
                    }
                    _ => {
                        im.note(format!(
                            "the combo “{}” sends something a layout cannot hold and was left out",
                            combo.name
                        ));
                        continue;
                    }
                }
            }
            _ => {
                im.note(format!(
                    "the combo “{}” sends something a layout cannot hold and was left out",
                    combo.name
                ));
                continue;
            }
        };
        let id = im.project.add_combo(combo.name.clone(), positions, binding);
        if let Some(layer) = combo.layer.and_then(|l| layers.get(usize::from(l))) {
            if let Ok(c) = im.project.combo_mut(id) {
                c.layers = vec![*layer];
            }
        }
    }

    // Lighting: cells by LED or matrix key, and the rules a layout can
    // show as lock and battery lights.
    let mut carried = Carried::default();
    if let Some(lighting) = &snapshot.lighting {
        let key_of_target = |im: &mut Importer, target: &KeyTargetConfig| -> Option<usize> {
            match *target {
                KeyTargetConfig::Led { led } => led_to_key(led),
                KeyTargetConfig::MatrixKey { key: [row, col] } => im.key_of(row, col),
                KeyTargetConfig::KeyId { .. }
                | KeyTargetConfig::Zone { .. }
                | KeyTargetConfig::All { .. } => None,
            }
        };
        let color = |text: &str| -> Option<KeyLight> {
            let rgb: Rgb = text.parse().ok()?;
            Some(if rgb == Rgb(0, 0, 0) {
                KeyLight::Off
            } else {
                KeyLight::Color(rgb)
            })
        };
        let mut unplaced = 0usize;
        for scene in &lighting.scenes {
            let Some(layer) = layers.get(usize::from(scene.layer)).copied() else {
                unplaced += 1;
                continue;
            };
            let (Some(position), Some(light)) =
                (key_of_target(&mut im, &scene.target), color(&scene.color))
            else {
                unplaced += 1;
                continue;
            };
            if scene.effect != EffectKind::Solid {
                im.note("blinking and breathing colors were read as steady colors");
            }
            im.project
                .lighting_mut(layer)
                .map_err(|e| e.to_string())?
                .keys[position] = light;
        }
        if unplaced > 0 {
            im.note(format!(
                "{unplaced} colored light(s) could not be placed on a key and were left out"
            ));
        }
        let mut unkept = 0usize;
        for cell in lighting.conditional_scenes.iter().flatten() {
            let kept = import_rule(&mut im, cell, &layers, &key_of_target, &color);
            if !kept {
                unkept += 1;
            }
        }
        if unkept > 0 {
            im.note(format!(
                "{unkept} lighting rule(s) show states a layout has no light for and were left out; the keyboard keeps them until a layout with lighting is applied"
            ));
        }
        let mut set = |key: &str, value: SettingValue| {
            carried.settings.insert(key.to_string(), value);
        };
        set(
            settings::BRIGHTNESS,
            SettingValue::Int(settings::brightness_percent(lighting.brightness)),
        );
        set(
            settings::OUTPUT_MODE,
            SettingValue::Text(output_mode_name(lighting.output_mode).into()),
        );
        set(
            settings::BACKGROUND_ENABLED,
            SettingValue::Bool(lighting.background.enabled),
        );
        set(
            settings::BACKGROUND_HUE,
            SettingValue::Int(i64::from(lighting.background.hue)),
        );
        set(
            settings::BACKGROUND_SATURATION,
            SettingValue::Int(i64::from(lighting.background.saturation)),
        );
        set(
            settings::BACKGROUND_VALUE,
            SettingValue::Int(i64::from(lighting.background.value)),
        );
        set(
            settings::BACKGROUND_SPEED,
            SettingValue::Int(i64::from(lighting.background.speed)),
        );
        set(
            settings::BACKGROUND_MODE,
            SettingValue::Text(
                match lighting.background.mode {
                    BackgroundModeConfig::Solid => "solid",
                    BackgroundModeConfig::Breathe => "breathe",
                }
                .into(),
            ),
        );
        if let Some(effects) = &lighting.effects {
            set(settings::EFFECT, SettingValue::Text(effects.effect.clone()));
            set(
                settings::PALETTE,
                SettingValue::Text(effects.palette.clone()),
            );
            set(
                settings::EFFECT_VALUE,
                SettingValue::Int(i64::from(effects.value)),
            );
            set(
                settings::EFFECT_SPEED,
                SettingValue::Int(i64::from(effects.speed)),
            );
        }
    }

    // The touchpads.
    if let Some(pointing) = &config.pointing {
        let auto_layers = config
            .behavior
            .as_ref()
            .map(|b| b.auto_mouse_layers.as_slice())
            .unwrap_or_default();
        for device in &pointing.devices {
            let Some(listener) = rmk
                .pointing
                .iter()
                .find(|p| p.device == device.device_id)
                .map(|p| p.listener.clone())
            else {
                im.note(format!(
                    "pointing device {} is none the {} has; its settings were left out",
                    device.device_id, board.name
                ));
                continue;
            };
            let mut profile = match pointing_profile(&mut im, &device.mode) {
                Some(profile) => profile,
                None => continue,
            };
            if let Some(auto) = auto_layers
                .iter()
                .find(|a| a.device_id.is_none_or(|d| d == device.device_id))
            {
                if let Some(layer) = layers.get(usize::from(auto.target_layer)) {
                    profile.auto_layer = Some((*layer, auto.timeout_ms));
                }
            }
            let mut overrides = Vec::new();
            for over in pointing
                .overrides
                .iter()
                .filter(|o| o.device_id == device.device_id)
            {
                let (Some(layer), Some(profile)) = (
                    layers.get(usize::from(over.layer)).copied(),
                    pointing_profile(&mut im, &over.mode),
                ) else {
                    continue;
                };
                overrides.push(PointingOverride {
                    layers: vec![layer],
                    processors: profile.to_processors(false),
                });
            }
            im.project.pointing.push(DevicePointing {
                listener,
                processors: profile.to_processors(false),
                overrides,
            });
        }
    }

    // The board's settings the configuration carries.
    if let Some(name) = &config.bluetooth_name {
        carried.settings.insert(
            settings::BLUETOOTH_NAME.into(),
            SettingValue::Text(name.clone()),
        );
    }
    if let Some(behavior) = &config.behavior {
        let mut set = |key: &str, value: SettingValue| {
            carried.settings.insert(key.to_string(), value);
        };
        set(
            settings::COMBO_TIMEOUT,
            SettingValue::Int(i64::from(behavior.combo_timeout_ms)),
        );
        set(
            settings::ONESHOT_TIMEOUT,
            SettingValue::Int(i64::from(behavior.oneshot_timeout_ms)),
        );
        set(
            settings::ONESHOT_QUICK_RELEASE,
            SettingValue::Bool(behavior.oneshot_quick_release),
        );
        set(
            settings::TAP_INTERVAL,
            SettingValue::Int(i64::from(behavior.tap_interval_ms)),
        );
        set(
            settings::FLOW_TAP,
            SettingValue::Bool(behavior.morse.enable_flow_tap),
        );
        set(
            settings::PRIOR_IDLE,
            SettingValue::Int(i64::from(behavior.morse.prior_idle_ms)),
        );
        let default_profile = &behavior.morse.default_profile;
        if let Some(ms) = default_profile.hold_timeout_ms {
            set(settings::HOLD_TIMEOUT, SettingValue::Int(i64::from(ms)));
        }
        if let Some(mode) = &default_profile.mode {
            set(settings::HOLD_MODE, SettingValue::Text(mode.clone()));
        }
        if let Some(ms) = default_profile.quick_tap_ms {
            set(settings::QUICK_TAP, SettingValue::Int(i64::from(ms)));
        }
        if let Some(on) = default_profile.unilateral_tap {
            set(settings::UNILATERAL_TAP, SettingValue::Bool(on));
        }
        if let Some(on) = default_profile.opposite_hand_hold {
            set(settings::OPPOSITE_HAND_HOLD, SettingValue::Bool(on));
        }
    }

    let Importer { project, notes, .. } = im;
    Ok(Imported {
        project,
        carried,
        notes,
    })
}

/// A touchpad mode as the pointing editor's settings, when it is one of
/// the modes a layout describes.
fn pointing_profile(im: &mut Importer, mode: &PointingModeConfig) -> Option<PointingProfile> {
    let mut profile = PointingProfile::default();
    match *mode {
        PointingModeConfig::Cursor {
            multiplier_x,
            multiplier_y,
            invert_x,
            invert_y,
            primary_button,
        } => {
            if multiplier_x != multiplier_y {
                im.note("a touchpad moving at different speeds across and down was read at its speed across");
            }
            profile.speed = (u32::from(multiplier_x), 1);
            profile.invert_x = invert_x;
            profile.invert_y = invert_y;
            profile.right_click = primary_button == Some(2);
            if primary_button.is_some_and(|b| b != 2) {
                im.note("a touchpad clicking a button other than left or right was read as left-clicking");
            }
        }
        PointingModeConfig::Sniper {
            multiplier,
            divisor,
            invert_x,
            invert_y,
        } => {
            profile.speed = (u32::from(multiplier), u32::from(divisor.max(1)));
            profile.invert_x = invert_x;
            profile.invert_y = invert_y;
        }
        PointingModeConfig::Scroll {
            multiplier_x,
            divisor_x,
            multiplier_y,
            divisor_y,
            invert_x,
            invert_y,
        } => {
            if (multiplier_x, divisor_x) != (multiplier_y, divisor_y) {
                im.note("a touchpad scrolling at different speeds across and down was read at its speed down");
            }
            profile.scroll = true;
            profile.speed = (u32::from(multiplier_y), u32::from(divisor_y.max(1)));
            profile.invert_x = invert_x;
            profile.invert_y = invert_y;
        }
        PointingModeConfig::Drag { .. }
        | PointingModeConfig::Press { .. }
        | PointingModeConfig::Caret { .. }
        | PointingModeConfig::Keypad { .. } => {
            im.note("a touchpad in a drag, press, caret or keypad mode was read as a plain pointer; a layout has no setting for those modes");
            return Some(profile);
        }
    }
    Some(profile)
}

/// Keeps a conditional lighting rule as a lock or battery light when it
/// has that shape.
fn import_rule(
    im: &mut Importer,
    cell: &ConditionalSceneConfig,
    layers: &[LayerId],
    key_of_target: &dyn Fn(&mut Importer, &KeyTargetConfig) -> Option<usize>,
    color: &dyn Fn(&str) -> Option<KeyLight>,
) -> bool {
    let Some(layer) = cell
        .layer
        .filter(|l| l.active)
        .and_then(|l| layers.get(usize::from(l.layer)).copied())
    else {
        return false;
    };
    let plain = cell.layers.is_none()
        && cell.output_mode.is_none()
        && cell.connection.is_none()
        && cell.effects.is_none()
        && cell.maintenance.is_none()
        && cell.split_transport.is_none();
    if !plain {
        return false;
    }
    let (Some(position), Some(on)) = (key_of_target(im, &cell.target), color(&cell.color)) else {
        return false;
    };
    let on = match on {
        KeyLight::Color(color) => color,
        _ => Rgb(0, 0, 0),
    };
    let current = im
        .project
        .lighting(layer)
        .and_then(|l| l.keys.get(position).copied())
        .unwrap_or_default();
    let off = match current {
        KeyLight::Color(color) => color,
        KeyLight::Lock { off, .. } | KeyLight::Battery { below: off, .. } => off,
        KeyLight::Off | KeyLight::Inherit => Rgb(0, 0, 0),
    };
    let light = match (cell.indicators, cell.battery) {
        (Some(indicators), None) => {
            let lock = match (
                indicators.caps_lock,
                indicators.num_lock,
                indicators.scroll_lock,
            ) {
                (Some(true), None, None) => LockKind::Caps,
                (None, Some(true), None) => LockKind::Num,
                (None, None, Some(true)) => LockKind::Scroll,
                _ => return false,
            };
            KeyLight::Lock { lock, off, on }
        }
        (None, Some(battery)) => {
            if battery.max_level.is_some()
                || battery.charge != ChargeConditionConfig::Any
                || battery.min_level.is_none()
            {
                return false;
            }
            KeyLight::Battery {
                percent: battery.min_level.unwrap_or(0),
                below: off,
                above: on,
            }
        }
        _ => return false,
    };
    match im.project.lighting_mut(layer) {
        Ok(lighting) => {
            lighting.keys[position] = light;
            true
        }
        Err(_) => false,
    }
}
