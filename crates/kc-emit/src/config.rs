//! The files around the keymap: `.conf`, `west.yml`, `build.yaml` and the
//! build workflow.

use kc_boards::board::{FirmwareProfile, Side, Source};
use kc_model::features::SettingValue;
use kc_model::{FirmwareConfig, Project};
use kc_zmk::addons::Addon;

use crate::NOTICE;

/// Settings the firmware needs for per-key lighting, and to start in it
/// where the firmware allows, unless the project sets them itself.
fn lighting_settings(profile: &FirmwareProfile) -> Vec<(String, SettingValue)> {
    let Some(backend) = &profile.lighting else {
        return Vec::new();
    };
    let mut settings = vec![(
        "CONFIG_EXPERIMENTAL_RGB_LAYER".to_string(),
        SettingValue::Bool(true),
    )];
    if let Some(effect) = backend.start_effect {
        settings.push((
            "CONFIG_ZMK_RGB_UNDERGLOW_EFF_START".into(),
            SettingValue::Int(effect.into()),
        ));
        settings.push((
            "CONFIG_ZMK_RGB_UNDERGLOW_ON_START".into(),
            SettingValue::Bool(true),
        ));
    }
    settings
}

pub fn conf(
    project: &Project,
    config: &FirmwareConfig,
    profile: &FirmwareProfile,
    profile_features: &[kc_zmk::Feature],
) -> String {
    let mut out = format!("# {NOTICE}\n");
    let mut settings = config.settings.clone();
    let features = profile_features;
    if crate::keymap::uses_lighting(project) {
        for (key, value) in lighting_settings(profile) {
            settings.entry(key).or_insert(value);
        }
    }
    for (key, value) in &settings {
        // A setting for a feature this firmware lacks stays in the project
        // but is not generated.
        let unsupported = kc_zmk::settings::setting_for(key)
            .and_then(|setting| setting.requires)
            .is_some_and(|feature| !features.contains(&feature));
        if unsupported {
            continue;
        }
        let value = match value {
            SettingValue::Bool(true) => "y".to_string(),
            SettingValue::Bool(false) => "n".to_string(),
            SettingValue::Int(n) => n.to_string(),
            SettingValue::Text(text) => {
                format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
            }
        };
        out.push_str(&format!("{key}={value}\n"));
    }
    let raw = config.raw_conf.trim();
    if !raw.is_empty() {
        out.push_str(&format!("\n{raw}\n"));
    }
    out
}

/// Where a ZMK profile's ZMK comes from. Board definitions are checked to
/// have one for every ZMK profile.
fn source(profile: &FirmwareProfile) -> &Source {
    profile
        .zmk
        .as_ref()
        .expect("a ZMK firmware profile names its ZMK source")
}

pub fn west(profile: &FirmwareProfile, config: &FirmwareConfig, addons: &[Addon]) -> String {
    let mut out = format!("# {NOTICE}\nmanifest:\n  projects:\n");
    // A source the user chose for the board replaces the profile's own.
    let zmk = config.source.as_ref().unwrap_or_else(|| source(profile));
    out.push_str(&format!(
        "    - name: zmk\n      url: {}\n      revision: {}\n      import: app/west.yml\n",
        zmk.url, zmk.revision
    ));
    for module in &profile.modules {
        out.push_str(&format!(
            "    - name: {}\n      url: {}\n      revision: {}\n",
            module.name, module.url, module.revision
        ));
        if let Some(import) = &module.import {
            out.push_str(&format!("      import: {import}\n"));
        }
    }
    // Add-ons, each pinned to the commit the catalog resolved. One the
    // profile already carries as a module is not listed twice.
    for addon in addons {
        if profile.modules.iter().any(|m| m.name == addon.module) {
            continue;
        }
        out.push_str(&format!(
            "    - name: {}\n      url: {}\n      revision: {}\n",
            addon.module, addon.url, addon.revision
        ));
    }
    out.push_str("  self:\n    path: config\n");
    out
}

/// The name of the firmware artifact for one half, such as `imprint_left`.
pub fn artifact_name(profile: &FirmwareProfile, side: Side) -> String {
    let side = match side {
        Side::Left => "left",
        Side::Right => "right",
    };
    format!("{}_{side}", profile.config_name)
}

/// The name of the generated overlay that tells one half's firmware which
/// key each of its LEDs sits under.
pub fn led_overlay_name(profile: &FirmwareProfile, side: Side) -> String {
    format!("{}_leds.overlay", artifact_name(profile, side))
}

/// The LED overlay for one half: the key position under each LED, and the
/// real length of the LED chain.
pub fn led_overlay(chain: &[usize]) -> String {
    let lookup: Vec<String> = chain.iter().map(|p| format!("<{p}>")).collect();
    format!(
        "/* {NOTICE} */\n\n/ {{\n    underglow-layer {{\n        compatible = \"zmk,underglow-layer\";\n        pixel-lookup = {};\n    }};\n}};\n\n&led_strip {{\n    chain-length = <{}>;\n}};\n",
        lookup.join(", "),
        chain.len()
    )
}

pub fn build(profile: &FirmwareProfile) -> String {
    let mut out = format!("# {NOTICE}\n---\ninclude:\n");
    for target in &profile.builds {
        out.push_str(&format!("  - board: {}\n", target.board));
        if let Some(shield) = &target.shield {
            out.push_str(&format!("    shield: {shield}\n"));
        }
        if !target.snippets.is_empty() {
            out.push_str(&format!("    snippet: {}\n", target.snippets.join(";")));
        }
        let mut cmake_args = target.cmake_args.clone();
        if profile.lighting.as_ref().is_some_and(|l| l.led_map_overlay) {
            // Relative to ZMK's app directory, which sits two levels below
            // the config directory in every build layout.
            cmake_args.push(format!(
                "-DEXTRA_DTC_OVERLAY_FILE=../../config/{}",
                led_overlay_name(profile, target.side)
            ));
        }
        if !cmake_args.is_empty() {
            out.push_str(&format!("    cmake-args: {}\n", cmake_args.join(" ")));
        }
        out.push_str(&format!(
            "    artifact-name: {}\n",
            artifact_name(profile, target.side)
        ));
    }
    out
}

pub fn workflow(profile: &FirmwareProfile) -> String {
    format!(
        "# {NOTICE}\nname: Build ZMK firmware\non: [push, pull_request, workflow_dispatch]\n\njobs:\n  build:\n    uses: {}\n",
        profile.workflow
    )
}
