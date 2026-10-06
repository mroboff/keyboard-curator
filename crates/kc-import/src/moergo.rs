//! Import of MoErgo Layout Editor exports (the JSON from "Local Backup").
//!
//! The export is turned into keymap and `.conf` text and passed through the
//! keymap importer, so both paths understand exactly the same things. The
//! editor has a few shorthands of its own, which are written out as the
//! behaviors they stand for.

use kc_boards::Board;
use kc_model::{Carried, Project};
use serde_json::Value;

use crate::dts;
use crate::keymap::{import_conf, import_keymap};
use crate::{ImportError, Report};

/// Behaviors the editor provides without listing them in an export, as
/// MoErgo's factory keymap defines them.
const BUILT_IN_BEHAVIORS: &str = r#"
magic: magic {
    compatible = "zmk,behavior-hold-tap";
    #binding-cells = <2>;
    flavor = "tap-preferred";
    tapping-term-ms = <200>;
    bindings = <&mo>, <&rgb_ug_status_macro>;
};
rgb_ug_status_macro: rgb_ug_status_macro {
    compatible = "zmk,behavior-macro";
    #binding-cells = <0>;
    bindings = <&rgb_ug RGB_STATUS>;
};
bt_0: bt_0 { compatible = "zmk,behavior-tap-dance"; #binding-cells = <0>; tapping-term-ms = <200>; bindings = <&bt_select_0>, <&bt BT_DISC 0>; };
bt_1: bt_1 { compatible = "zmk,behavior-tap-dance"; #binding-cells = <0>; tapping-term-ms = <200>; bindings = <&bt_select_1>, <&bt BT_DISC 1>; };
bt_2: bt_2 { compatible = "zmk,behavior-tap-dance"; #binding-cells = <0>; tapping-term-ms = <200>; bindings = <&bt_select_2>, <&bt BT_DISC 2>; };
bt_3: bt_3 { compatible = "zmk,behavior-tap-dance"; #binding-cells = <0>; tapping-term-ms = <200>; bindings = <&bt_select_3>, <&bt BT_DISC 3>; };
bt_select_0: bt_select_0 { compatible = "zmk,behavior-macro"; #binding-cells = <0>; bindings = <&out OUT_BLE>, <&bt BT_SEL 0>; };
bt_select_1: bt_select_1 { compatible = "zmk,behavior-macro"; #binding-cells = <0>; bindings = <&out OUT_BLE>, <&bt BT_SEL 1>; };
bt_select_2: bt_select_2 { compatible = "zmk,behavior-macro"; #binding-cells = <0>; bindings = <&out OUT_BLE>, <&bt BT_SEL 2>; };
bt_select_3: bt_select_3 { compatible = "zmk,behavior-macro"; #binding-cells = <0>; bindings = <&out OUT_BLE>, <&bt BT_SEL 3>; };
"#;

/// The input processor the editor's touchpad settings refer to.
const RIGHT_CLICK_MAPPER: &str = r#"#include <zephyr/dt-bindings/input/input-event-codes.h>

/ {
    input_processors {
        zip_click_to_right_click_mapper: zip_click_to_right_click_mapper {
            compatible = "zmk,input-processor-code-mapper";
            #input-processor-cells = <0>;
            type = <INPUT_EV_KEY>;
            map = <INPUT_BTN_0 INPUT_BTN_1>;
        };
    };
};
"#;

/// The editor's names for settings, and the Kconfig options they set.
const CONFIG_NAMES: [(&str, &str); 6] = [
    ("DEEP_SLEEP", "CONFIG_ZMK_SLEEP"),
    ("DEEP_SLEEP_TIMEOUT_MS", "CONFIG_ZMK_IDLE_SLEEP_TIMEOUT"),
    ("IDLE_TIMEOUT_MS", "CONFIG_ZMK_IDLE_TIMEOUT"),
    ("HID_POINTING", "CONFIG_ZMK_POINTING"),
    (
        "HID_POINTING_SMOOTH_SCROLLING",
        "CONFIG_ZMK_POINTING_SMOOTH_SCROLLING",
    ),
    ("BLE_SUPPORT", "CONFIG_ZMK_BLE"),
];

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A parameter with its own parameters is a call, such as `LG(LA(K))`.
fn param(value: &Value) -> String {
    let name = text(&value["value"]);
    match value["params"].as_array() {
        Some(inner) if !inner.is_empty() => {
            let inner: Vec<String> = inner.iter().map(param).collect();
            format!("{name}({})", inner.join(","))
        }
        _ => name,
    }
}

struct Writer {
    magic_layer: Option<usize>,
    /// Layers that `&layer N` shorthands refer to.
    layer_keys: Vec<i64>,
}

impl Writer {
    /// One binding as keymap text, with the editor's shorthands written out.
    fn binding(&mut self, value: &Value) -> String {
        let name = text(&value["value"]);
        let params: Vec<String> = value["params"]
            .as_array()
            .map(|p| p.iter().map(param).collect())
            .unwrap_or_default();
        match (name.as_str(), self.magic_layer) {
            ("&magic", Some(layer)) if params.is_empty() => format!("&magic {layer} 0"),
            ("&reset", _) => "&sys_reset".to_string(),
            ("&layer", _) => match params.first().and_then(|p| p.parse::<i64>().ok()) {
                Some(layer) => {
                    if !self.layer_keys.contains(&layer) {
                        self.layer_keys.push(layer);
                    }
                    format!("&layer_td_{layer}")
                }
                None => format!("&layer {}", params.join(" ")),
            },
            _ if params.is_empty() => name,
            _ => format!("{name} {}", params.join(" ")),
        }
    }

    fn bindings(&mut self, values: &Value) -> String {
        values
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|b| self.binding(b))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default()
    }
}

fn number_prop(out: &mut String, object: &Value, key: &str, prop: &str) {
    if let Some(n) = object[key].as_i64() {
        out.push_str(&format!("            {prop} = <{n}>;\n"));
    }
}

fn numbers(value: &Value) -> String {
    value
        .as_array()
        .map(|list| list.iter().map(text).collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

fn processors(list: &Value) -> String {
    list.as_array()
        .map(|processors| {
            processors
                .iter()
                .map(|p| {
                    let params: Vec<String> = p["params"]
                        .as_array()
                        .map(|params| {
                            params
                                .iter()
                                .map(|v| match v.as_array() {
                                    // A list of flags is combined with `|`.
                                    Some(flags) => format!(
                                        "({})",
                                        flags.iter().map(text).collect::<Vec<_>>().join("|")
                                    ),
                                    None => text(v),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    format!("<{} {}>", text(&p["code"]), params.join(" ")).replace(" >", ">")
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// The keymap and `.conf` text an export describes.
fn convert(export: &Value) -> (String, String, Vec<String>) {
    let mut notes = Vec::new();
    let names: Vec<String> = export["layer_names"]
        .as_array()
        .map(|n| n.iter().map(text).collect())
        .unwrap_or_default();
    let mut writer = Writer {
        magic_layer: names.iter().position(|n| n == "Magic"),
        layer_keys: Vec::new(),
    };

    let mut layers = String::new();
    for (index, layer) in export["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let name = names
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("Layer {index}"));
        layers.push_str(&format!(
            "        layer_{index} {{\n            display-name = \"{}\";\n            bindings = <{}>;\n        }};\n",
            name.replace('"', "'"),
            writer.bindings(layer)
        ));
    }

    let mut behaviors = String::new();
    for hold_tap in export["holdTaps"].as_array().into_iter().flatten() {
        let label = text(&hold_tap["name"]).trim_start_matches('&').to_string();
        let sides = hold_tap["bindings"]
            .as_array()
            .map(|b| {
                b.iter()
                    .map(|s| format!("<{}>", text(s)))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        behaviors.push_str(&format!(
            "        {label}: {label} {{\n            compatible = \"zmk,behavior-hold-tap\";\n            #binding-cells = <2>;\n"
        ));
        if let Some(flavor) = hold_tap["flavor"].as_str() {
            behaviors.push_str(&format!("            flavor = \"{flavor}\";\n"));
        }
        number_prop(&mut behaviors, hold_tap, "tappingTermMs", "tapping-term-ms");
        number_prop(&mut behaviors, hold_tap, "quickTapMs", "quick-tap-ms");
        number_prop(
            &mut behaviors,
            hold_tap,
            "requirePriorIdleMs",
            "require-prior-idle-ms",
        );
        if hold_tap["holdTriggerKeyPositions"].is_array() {
            behaviors.push_str(&format!(
                "            hold-trigger-key-positions = <{}>;\n",
                numbers(&hold_tap["holdTriggerKeyPositions"])
            ));
        }
        for (key, prop) in [
            ("holdTriggerOnRelease", "hold-trigger-on-release"),
            ("retroTap", "retro-tap"),
            ("holdWhileUndecided", "hold-while-undecided"),
        ] {
            if hold_tap[key].as_bool() == Some(true) {
                behaviors.push_str(&format!("            {prop};\n"));
            }
        }
        behaviors.push_str(&format!("            bindings = {sides};\n        }};\n"));
    }
    for macro_ in export["macros"].as_array().into_iter().flatten() {
        let label = text(&macro_["name"]).trim_start_matches('&').to_string();
        let params = macro_["params"].as_array().map_or(0, Vec::len);
        let compatible = match params {
            0 => "zmk,behavior-macro",
            1 => "zmk,behavior-macro-one-param",
            _ => "zmk,behavior-macro-two-param",
        };
        behaviors.push_str(&format!(
            "        {label}: {label} {{\n            compatible = \"{compatible}\";\n            #binding-cells = <{}>;\n",
            params.min(2)
        ));
        number_prop(&mut behaviors, macro_, "waitMs", "wait-ms");
        number_prop(&mut behaviors, macro_, "tapMs", "tap-ms");
        behaviors.push_str(&format!(
            "            bindings = <{}>;\n        }};\n",
            writer.bindings(&macro_["bindings"])
        ));
    }

    let mut combos = String::new();
    for combo in export["combos"].as_array().into_iter().flatten() {
        let name: String = text(&combo["name"])
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        combos.push_str(&format!(
            "        {name} {{\n            key-positions = <{}>;\n            bindings = <{}>;\n",
            numbers(&combo["keyPositions"]),
            writer.binding(&combo["binding"])
        ));
        number_prop(&mut combos, combo, "timeoutMs", "timeout-ms");
        if combo["layers"].as_array().is_some_and(|l| !l.is_empty()) {
            // The editor writes -1 for "every layer".
            let layers: Vec<String> = combo["layers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|l| l.as_i64().is_some_and(|n| n >= 0))
                .map(text)
                .collect();
            if !layers.is_empty() {
                combos.push_str(&format!("            layers = <{}>;\n", layers.join(" ")));
            }
        }
        combos.push_str("        };\n");
    }

    let mut listeners = String::new();
    for listener in export["inputListeners"].as_array().into_iter().flatten() {
        listeners.push_str(&format!("{} {{\n", text(&listener["code"])));
        let own = processors(&listener["inputProcessors"]);
        if !own.is_empty() {
            listeners.push_str(&format!("    input-processors = {own};\n"));
        }
        for (index, node) in listener["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            listeners.push_str(&format!(
                "    override_{index} {{\n        layers = <{}>;\n        input-processors = {};\n    }};\n",
                numbers(&node["layers"]),
                processors(&node["inputProcessors"])
            ));
        }
        listeners.push_str("};\n");
    }

    // `&layer N` is the editor's key that holds a layer on one tap and
    // switches to it on two.
    for layer in &writer.layer_keys {
        behaviors.push_str(&format!(
            "        layer_td_{layer}: layer_td_{layer} {{\n            compatible = \"zmk,behavior-tap-dance\";\n            #binding-cells = <0>;\n            tapping-term-ms = <200>;\n            bindings = <&mo {layer}>, <&to {layer}>;\n        }};\n"
        ));
    }

    let custom_behaviors = export["custom_defined_behaviors"]
        .as_str()
        .unwrap_or_default();
    let custom_devicetree = export["custom_devicetree"].as_str().unwrap_or_default();
    let mut keymap = format!(
        "/ {{\n    behaviors {{\n{behaviors}    }};\n    combos {{\n        compatible = \"zmk,combos\";\n{combos}    }};\n    keymap {{\n        compatible = \"zmk,keymap\";\n{layers}    }};\n}};\n{listeners}\n{custom_behaviors}\n{custom_devicetree}\n"
    );

    // Add the editor's own behaviors that the layout uses but does not
    // define, and anything those build on.
    if let Ok(built_in) = dts::parse(&format!("/ {{ {BUILT_IN_BEHAVIORS} }};")) {
        let nodes = &built_in.nodes[0].children;
        let mut added: Vec<&str> = Vec::new();
        loop {
            let wanted: Vec<&dts::Node> = nodes
                .iter()
                .filter(|n| {
                    let label = n.label().unwrap_or_default();
                    let used = keymap.contains(&format!("&{label} "))
                        || keymap.contains(&format!("&{label}>"));
                    used && !keymap.contains(&format!("{label}: ")) && !added.contains(&label)
                })
                .collect();
            if wanted.is_empty() {
                break;
            }
            let mut block = String::from("/ {\n    behaviors {\n");
            for node in wanted {
                added.push(node.label().unwrap_or_default());
                block.push_str(&format!("        {}\n", node.text));
            }
            block.push_str("    };\n};\n");
            keymap.push_str(&block);
        }
    }
    if keymap.contains("&zip_click_to_right_click_mapper")
        && !keymap.contains("zip_click_to_right_click_mapper:")
    {
        keymap.push_str(RIGHT_CLICK_MAPPER);
    }

    let mut conf = String::new();
    for parameter in export["config_parameters"].as_array().into_iter().flatten() {
        let (name, value) = (text(&parameter["paramName"]), text(&parameter["value"]));
        match CONFIG_NAMES.iter().find(|(editor, _)| *editor == name) {
            Some((_, option)) => conf.push_str(&format!("{option}={value}\n")),
            None => notes.push(format!(
                "The setting `{name}` has no known equivalent and was not imported."
            )),
        }
    }
    let sensitivity = export["layout_parameters"]["cirque_touch_sensitivity"].as_str();
    if sensitivity.is_some_and(|s| !s.is_empty()) {
        notes.push("The touchpad sensitivity setting was not imported.".to_string());
    }
    (keymap, conf, notes)
}

/// Imports a MoErgo Layout Editor export as a layout for `board`, with the
/// firmware settings the export also holds.
pub fn import_moergo(json: &str, board: &Board) -> Result<(Project, Carried, Report), ImportError> {
    let export: Value =
        serde_json::from_str(json).map_err(|e| ImportError::NotAnExport(e.to_string()))?;
    if !export["layers"].is_array() {
        return Err(ImportError::NotAnExport("it has no layers".into()));
    }
    let title = export["title"]
        .as_str()
        .filter(|t| !t.is_empty())
        .unwrap_or("Imported layout");
    let (keymap, conf, notes) = convert(&export);
    let (project, mut report) = import_keymap(title, &keymap, board)?;
    report.notes.splice(0..0, notes);
    Ok((project, import_conf(&conf), report))
}
