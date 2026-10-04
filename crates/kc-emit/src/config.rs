//! The files around the keymap: `.conf`, `west.yml`, `build.yaml` and the
//! build workflow.

use kc_boards::board::{FirmwareProfile, Side};
use kc_model::features::SettingValue;
use kc_model::Project;

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

pub fn conf(project: &Project, profile: &FirmwareProfile) -> String {
    let mut out = format!("# {NOTICE}\n");
    let mut settings = project.settings.clone();
    if crate::keymap::uses_lighting(project) {
        for (key, value) in lighting_settings(profile) {
            settings.entry(key).or_insert(value);
        }
    }
    for (key, value) in &settings {
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
    let raw = project.raw.conf.trim();
    if !raw.is_empty() {
        out.push_str(&format!("\n{raw}\n"));
    }
    out
}

pub fn west(profile: &FirmwareProfile) -> String {
    let mut out = format!("# {NOTICE}\nmanifest:\n  projects:\n");
    out.push_str(&format!(
        "    - name: zmk\n      url: {}\n      revision: {}\n      import: app/west.yml\n",
        profile.zmk.url, profile.zmk.revision
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
        if !target.cmake_args.is_empty() {
            out.push_str(&format!(
                "    cmake-args: {}\n",
                target.cmake_args.join(" ")
            ));
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
