//! Board definitions from a ZMK checkout: every shield that declares a
//! physical layout becomes a board file, with one firmware profile per
//! controller it fits. ZMK is MIT; each file names the revision it came
//! from. The `zmk_boards` example runs this over a checkout.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use kc_boards::board::{Controller, Side};
use kc_boards::dts::{parse_physical_layouts, DtsLayout};

use crate::dts;

/// Where ZMK's build workflow lives, pinned to the same release.
const WORKFLOW: &str = "zmkfirmware/zmk/.github/workflows/build-user-config.yml";
const ZMK_URL: &str = "https://github.com/zmkfirmware/zmk";
/// ZMK's shared USB identifiers (app/Kconfig: ZMK_USB_VID and ZMK_USB_PID).
const ZMK_USB: (u16, u16) = (0x1d50, 0x615e);
/// Shields that are test fixtures or demonstration boards, not keyboards.
const NOT_KEYBOARDS: &[&str] = &[
    "tester_pro_micro",
    "tester_xiao",
    "zmk_uno",
    "zmk_uno_split",
    "minivan_studio_tester",
];
/// Who makes each shield, where the metadata's URL does not say plainly.
const VENDORS: &[(&str, &str)] = &[
    ("a_dux", "tapioki"),
    ("boardsource5x12", "Boardsource"),
    ("contra", "ai03"),
    ("corne", "foostan"),
    ("crbn", "Polarity Works"),
    ("hummingbird", "PJE66"),
    ("jian", "KGOH"),
    ("jiran", "Ladniy"),
    ("jorne", "joric"),
    ("kyria", "splitkb"),
    ("splitkb_aurora", "splitkb"),
    ("sofle", "josefadamcik"),
    ("cradio", "davidphilipbarr"),
    ("lily58", "kata0510"),
    ("lotus58", "TweetyDaBird"),
    ("m60", "Makerdiary"),
    ("murphpad", "MechWild"),
    ("qaz", "CB Keyboards"),
    ("reviung41", "gtips"),
];

#[derive(Debug, thiserror::Error)]
pub enum ZmkTreeError {
    #[error("could not read {path}: {error}")]
    Read {
        path: PathBuf,
        error: std::io::Error,
    },
    #[error("{path}: {error}")]
    Layouts {
        path: PathBuf,
        error: kc_boards::dts::DtsError,
    },
    #[error("{0} is not a ZMK checkout: app/boards/shields is missing")]
    NotZmk(PathBuf),
}

/// A shield's `*.zmk.yml` metadata, the parts the generator reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub url: String,
    pub requires: Vec<String>,
    pub features: Vec<String>,
    pub siblings: Vec<String>,
}

/// Reads the flat YAML ZMK uses for its metadata: scalars, inline lists
/// and indented `- item` lists, nothing deeper.
pub fn read_meta(text: &str) -> Meta {
    let mut meta = Meta::default();
    let mut list: Option<&mut Vec<String>> = None;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if let Some(item) = line.trim().strip_prefix("- ") {
            if let Some(list) = list.as_deref_mut() {
                list.push(item.trim().to_string());
            }
            continue;
        }
        list = None;
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        let items = |value: &str| -> Vec<String> {
            value
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };
        match key.trim() {
            "id" => meta.id = value,
            "name" => meta.name = value,
            "type" => meta.kind = value,
            "url" => meta.url = value,
            "requires" => {
                meta.requires = items(&value);
                if value.is_empty() {
                    list = Some(&mut meta.requires);
                }
            }
            "features" => {
                meta.features = items(&value);
                if value.is_empty() {
                    list = Some(&mut meta.features);
                }
            }
            "siblings" => {
                meta.siblings = items(&value);
                if value.is_empty() {
                    list = Some(&mut meta.siblings);
                }
            }
            _ => {}
        }
    }
    meta
}

/// What `Kconfig.defconfig` says about a shield: the name it announces
/// over USB, and which half is central.
#[derive(Debug, Clone, PartialEq)]
pub struct Kconfig {
    pub keyboard_name: Option<String>,
    pub central: Side,
}

pub fn read_kconfig(text: &str) -> Kconfig {
    let mut keyboard_name = None;
    let mut central = Side::Left;
    let mut condition = String::new();
    let mut in_name = false;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("if ") {
            condition = rest.to_string();
        } else if line.starts_with("endif") {
            condition.clear();
        }
        if line.starts_with("config ") {
            in_name = line == "config ZMK_KEYBOARD_NAME";
            if line == "config ZMK_SPLIT_ROLE_CENTRAL" && condition.contains("_RIGHT") {
                central = Side::Right;
            }
        } else if in_name {
            if let Some(value) = line.strip_prefix("default ") {
                if keyboard_name.is_none() {
                    keyboard_name = Some(value.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    Kconfig {
        keyboard_name,
        central,
    }
}

/// The plain keys of a keymap's first layer, one per key: the keycode of
/// a `&kp`, or nothing for anything else.
pub fn starter_keys(text: &str) -> Result<Vec<String>, dts::DtsError> {
    let source = dts::parse(text)?;
    let first = source
        .nodes
        .iter()
        .filter(|n| n.name == "/")
        .flat_map(|root| root.children.iter())
        .filter(|n| n.name == "keymap")
        .flat_map(|keymap| keymap.children.iter())
        .find(|n| n.prop("bindings").is_some());
    let Some(first) = first else {
        return Ok(Vec::new());
    };
    let value = dts::expand(first.prop("bindings").unwrap_or_default(), &source.defines);
    let codes = kc_zmk::keycodes::keycodes();
    Ok(dts::bindings(&dts::cell_groups(&value).join(" "))
        .iter()
        .map(|binding| {
            let mut words = binding.split_whitespace();
            match (words.next(), words.next(), words.next()) {
                (Some("&kp"), Some(code), None) if codes.get(code).is_some() => code.to_string(),
                _ => String::new(),
            }
        })
        .collect())
}

/// One generated board file.
#[derive(Debug, Clone, PartialEq)]
pub struct Generated {
    pub id: String,
    pub toml: String,
}

/// What a run of the generator produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Output {
    pub boards: Vec<Generated>,
    /// What was left out or guessed, and why.
    pub warnings: Vec<String>,
}

/// Every keyboard shield in a ZMK checkout that declares a physical
/// layout, as board files. `revision` is the tag or commit the checkout
/// is at, which the firmware profiles pin.
pub fn boards(zmk: &Path, revision: &str) -> Result<Output, ZmkTreeError> {
    let shields = zmk.join("app/boards/shields");
    if !shields.is_dir() {
        return Err(ZmkTreeError::NotZmk(zmk.to_path_buf()));
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(&shields)
        .map_err(|error| ZmkTreeError::Read {
            path: shields.clone(),
            error,
        })?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let mut out = Output::default();
    for dir in dirs {
        let mut metas: Vec<PathBuf> = fs::read_dir(&dir)
            .map_err(|error| ZmkTreeError::Read {
                path: dir.clone(),
                error,
            })?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with(".zmk.yml"))
            .collect();
        metas.sort();
        for path in metas {
            let meta = read_meta(&read(&path)?);
            if meta.kind != "shield" || NOT_KEYBOARDS.contains(&meta.id.as_str()) {
                continue;
            }
            if let Some(board) = shield_board(zmk, &dir, &meta, revision, &mut out.warnings)? {
                out.boards.push(board);
            }
        }
    }
    Ok(out)
}

fn read(path: &Path) -> Result<String, ZmkTreeError> {
    fs::read_to_string(path).map_err(|error| ZmkTreeError::Read {
        path: path.to_path_buf(),
        error,
    })
}

/// The devicetree files a shield variant is made of: its own, and what
/// they include, locally or from ZMK's shared layouts.
fn devicetree_files(zmk: &Path, dir: &Path, id: &str) -> Result<Vec<PathBuf>, ZmkTreeError> {
    let layouts = zmk.join("app/dts/layouts");
    let mut queue: Vec<PathBuf> = [
        format!("{id}.dtsi"),
        format!("{id}.overlay"),
        format!("{id}_left.overlay"),
        format!("{id}_right.overlay"),
    ]
    .iter()
    .map(|name| dir.join(name))
    .filter(|p| p.is_file())
    .collect();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut files = Vec::new();
    while let Some(path) = queue.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let text = read(&path)?;
        for line in text.lines() {
            let Some(rest) = line.trim().strip_prefix("#include") else {
                continue;
            };
            let rest = rest.trim();
            let included = if let Some(local) = rest.strip_prefix('"') {
                local
                    .split('"')
                    .next()
                    .map(|name| path.parent().unwrap_or(dir).join(name))
            } else if let Some(shared) = rest.strip_prefix("<layouts/") {
                shared.split('>').next().map(|name| layouts.join(name))
            } else {
                None
            };
            if let Some(included) = included.filter(|p| p.is_file()) {
                queue.push(included);
            }
        }
        files.push(path);
    }
    files.sort();
    Ok(files)
}

fn vendor(meta: &Meta) -> String {
    if let Some((_, vendor)) = VENDORS.iter().find(|(id, _)| *id == meta.id) {
        return (*vendor).to_string();
    }
    if let Some((_, vendor)) = VENDORS
        .iter()
        .find(|(id, _)| meta.id.starts_with(&format!("{id}_")))
    {
        return (*vendor).to_string();
    }
    // The GitHub owner, or the site.
    let rest = meta
        .url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    let mut parts = rest.split('/');
    match (parts.next(), parts.next()) {
        (Some("github.com"), Some(owner)) => owner.to_string(),
        (Some(host), _) => host.to_string(),
        _ => "Unknown".to_string(),
    }
}

fn shield_board(
    zmk: &Path,
    dir: &Path,
    meta: &Meta,
    revision: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<Generated>, ZmkTreeError> {
    let files = devicetree_files(zmk, dir, &meta.id)?;
    let mut layouts: Vec<DtsLayout> = Vec::new();
    let mut chosen = None;
    for path in &files {
        let text = read(path)?;
        for layout in parse_physical_layouts(&text).map_err(|error| ZmkTreeError::Layouts {
            path: path.clone(),
            error,
        })? {
            if !layouts.iter().any(|l| l.label == layout.label) {
                layouts.push(layout);
            }
        }
        if chosen.is_none() {
            chosen = text
                .split("zmk,physical-layout")
                .nth(1)
                .and_then(|rest| rest.split(';').next())
                .and_then(|rest| rest.split('&').nth(1))
                .map(|label| label.trim().to_string());
        }
    }
    if layouts.is_empty() {
        return Ok(None);
    }
    let default = match chosen.filter(|c| layouts.iter().any(|l| &l.label == c)) {
        Some(label) => label,
        None => {
            warnings.push(format!(
                "{}: no chosen physical layout; the first is the default",
                meta.id
            ));
            layouts[0].label.clone()
        }
    };
    let keys = layouts
        .iter()
        .find(|l| l.label == default)
        .map_or(0, |l| l.keys.len());

    let kconfig = dir
        .join("Kconfig.defconfig")
        .is_file()
        .then(|| read(&dir.join("Kconfig.defconfig")))
        .transpose()?
        .map(|text| read_kconfig(&text))
        .unwrap_or(Kconfig {
            keyboard_name: None,
            central: Side::Left,
        });
    let usb_name = kconfig.keyboard_name.clone().unwrap_or(meta.name.clone());

    let keymap = dir.join(format!("{}.keymap", meta.id));
    let starters = if keymap.is_file() {
        match starter_keys(&read(&keymap)?) {
            Ok(keys) => keys,
            Err(error) => {
                warnings.push(format!("{}: keymap not read: {error}", meta.id));
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    let starters = if starters.len() == keys {
        starters
    } else {
        if !starters.is_empty() {
            warnings.push(format!(
                "{}: the keymap has {} keys, the layout {keys}; no starter keys",
                meta.id,
                starters.len()
            ));
        }
        Vec::new()
    };

    let split = !meta.siblings.is_empty();
    let sides: Vec<(Side, String)> = if split {
        vec![
            (Side::Left, format!("{}_left", meta.id)),
            (Side::Right, format!("{}_right", meta.id)),
        ]
    } else {
        vec![(Side::Whole, meta.id.clone())]
    };
    let central = if split { kconfig.central } else { Side::Whole };
    let connector = meta.requires.first().cloned().unwrap_or_default();
    let controllers: Vec<&Controller> = kc_boards::controllers()
        .iter()
        .filter(|c| c.connector == connector)
        .collect();
    if controllers.is_empty() {
        warnings.push(format!(
            "{}: no controller in the table exposes `{connector}`; skipped",
            meta.id
        ));
        return Ok(None);
    }
    let studio = meta.features.iter().any(|f| f == "studio");
    let underglow = meta.features.iter().any(|f| f == "underglow");

    let id = meta.id.replace('_', "-");
    // The vendor column says who makes it.
    let name = meta
        .name
        .strip_prefix("splitkb.com ")
        .unwrap_or(&meta.name)
        .to_string();
    let sources: Vec<String> = files
        .iter()
        .filter_map(|p| p.strip_prefix(zmk).ok())
        .map(|p| p.display().to_string())
        .collect();
    let mut t = String::new();
    t.push_str(&format!(
        "# {name}. Generated by the zmk_boards example in kc-import from\n# zmkfirmware/zmk at {revision} (MIT): {}.\n# Rerun the generator rather than editing this file. Not yet tried on a\n# real keyboard.\n\n",
        sources.join(",\n# ")
    ));
    t.push_str(&format!("id = {id:?}\nname = {name:?}\nvendor = {:?}\nverified = false\nbrightness_cap = 100\ndefault_layout = {default:?}\n", vendor(meta)));
    if !starters.is_empty() {
        t.push_str(
            "# The plain keys of the shield's own keymap, in binding order.\nstarter_keys = [\n",
        );
        for row in starters.chunks(12) {
            t.push_str("  ");
            t.push_str(
                &row.iter()
                    .map(|k| format!("{k:?}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            t.push_str(",\n");
        }
        t.push_str("]\n");
    }
    t.push_str(&format!(
        "\n[[usb]]\nvendor = {:#06x}\nproduct = {:#06x}\nname = {usb_name:?}\n",
        ZMK_USB.0, ZMK_USB.1
    ));
    t.push_str(&format!(
        "\n[flash]\norder = [{}]\n",
        sides
            .iter()
            .map(|(side, _)| format!("{:?}", side.name()))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    for (side, _) in &sides {
        t.push_str(&format!(
            "\n[[halves]]\nside = {:?}\ncentral = {}\n",
            side.name(),
            *side == central
        ));
    }
    for controller in controllers {
        let mut capabilities = Vec::new();
        if studio {
            capabilities.push("studio");
        }
        if underglow {
            capabilities.push("rgb-underglow");
        }
        // The nRF52840 controllers all switch their VCC pin; the RP2040
        // ones have no such switch.
        if controller.uf2_family == Some(0xADA5_2840) {
            capabilities.push("ext-power");
        }
        t.push_str(&format!(
            "\n[[firmware]]\nid = {:?}\nname = {:?}\nzephyr = \"3.5\"\nconfig_name = {:?}\nworkflow = {:?}\nzmk = {{ url = {ZMK_URL:?}, revision = {revision:?} }}\ncontroller = {:?}\ncapabilities = [{}]\n",
            format!("zmk-{}-{}", revision.trim_start_matches('v'), controller.id),
            format!("ZMK {revision} on {}", controller.name),
            meta.id,
            format!("{WORKFLOW}@{revision}"),
            controller.id,
            capabilities
                .iter()
                .map(|c| format!("{c:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        if !controller.notes.is_empty() {
            t.push_str(&format!(
                "notes = [{}]\n",
                controller
                    .notes
                    .iter()
                    .map(|n| format!("{n:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        for (side, shield) in &sides {
            t.push_str(&format!(
                "\n[[firmware.builds]]\nside = {:?}\nboard = {:?}\nshield = {shield:?}\n",
                side.name(),
                controller.board
            ));
            if studio && *side == central {
                t.push_str(
                    "snippets = [\"studio-rpc-usb-uart\"]\ncmake_args = [\"-DCONFIG_ZMK_STUDIO=y\"]\n",
                );
            }
        }
    }
    for layout in &layouts {
        t.push_str(&format!(
            "\n[[layouts]]\nid = {:?}\nname = {:?}\nkeys = [\n",
            layout.label,
            layout.display_name.clone().unwrap_or_else(|| name.clone())
        ));
        for key in &layout.keys {
            let [w, h, x, y, rot, rx, ry]: [i32; 7] = (*key).into();
            t.push_str(&format!(
                "  [{w}, {h}, {x:>4}, {y:>3}, {rot:>5}, {rx:>4}, {ry:>4}],\n"
            ));
        }
        t.push_str("]\n");
    }
    Ok(Some(Generated { id, toml: t }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_and_kconfig_are_read() {
        let meta = read_meta(
            "file_format: \"1\"\nid: corne\nname: Corne\ntype: shield\nurl: https://github.com/foostan/crkbd/\nrequires: [pro_micro]\nexposes: [i2c_oled]\nfeatures:\n  - keys\n  - underglow\n  - studio\nsiblings:\n  - corne_left\n  - corne_right\n",
        );
        assert_eq!(meta.id, "corne");
        assert_eq!(meta.requires, ["pro_micro"]);
        assert_eq!(meta.features, ["keys", "underglow", "studio"]);
        assert_eq!(meta.siblings, ["corne_left", "corne_right"]);

        let kconfig = read_kconfig(
            "if SHIELD_X_RIGHT\n\nconfig ZMK_KEYBOARD_NAME\n    default \"X Board\"\n\nconfig ZMK_SPLIT_ROLE_CENTRAL\n    default y\n\nendif\n",
        );
        assert_eq!(kconfig.keyboard_name.as_deref(), Some("X Board"));
        assert_eq!(kconfig.central, Side::Right);
    }

    /// A checkout with one split shield on a shared layout, as ZMK lays
    /// its tree out.
    fn checkout() -> PathBuf {
        let root = std::env::temp_dir().join(format!("kc-zmk-tree-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let shield = root.join("app/boards/shields/foo");
        let layouts = root.join("app/dts/layouts/bar");
        fs::create_dir_all(&shield).unwrap();
        fs::create_dir_all(&layouts).unwrap();
        fs::write(
            shield.join("foo.zmk.yml"),
            "file_format: \"1\"\nid: foo\nname: Foo\ntype: shield\nurl: https://github.com/bar/foo\nrequires: [pro_micro]\nfeatures:\n  - keys\n  - studio\nsiblings:\n  - foo_left\n  - foo_right\n",
        )
        .unwrap();
        fs::write(
            shield.join("Kconfig.defconfig"),
            "if SHIELD_FOO_LEFT\n\nconfig ZMK_KEYBOARD_NAME\n    default \"Foo Board\"\n\nconfig ZMK_SPLIT_ROLE_CENTRAL\n    default y\n\nendif\n",
        )
        .unwrap();
        fs::write(
            shield.join("foo.dtsi"),
            "#include <layouts/bar/foo.dtsi>\n\n/ {\n    chosen {\n        zmk,physical-layout = &bar_foo_layout;\n    };\n};\n",
        )
        .unwrap();
        fs::write(shield.join("foo_left.overlay"), "#include \"foo.dtsi\"\n").unwrap();
        fs::write(shield.join("foo_right.overlay"), "#include \"foo.dtsi\"\n").unwrap();
        fs::write(
            shield.join("foo.keymap"),
            "#include <behaviors.dtsi>\n#include <dt-bindings/zmk/keys.h>\n/ {\n keymap {\n  compatible = \"zmk,keymap\";\n  default_layer {\n   bindings = <&kp A &kp B &mo 1>;\n  };\n };\n};\n",
        )
        .unwrap();
        fs::write(
            layouts.join("foo.dtsi"),
            "#include <physical_layouts.dtsi>\n\n/ {\n    bar_foo_layout: bar_foo_layout {\n        compatible = \"zmk,physical-layout\";\n        display-name = \"Foo\";\n        keys\n            = <&key_physical_attrs 100 100    0    0       0     0     0>\n            , <&key_physical_attrs 100 100  100    0       0     0     0>\n            , <&key_physical_attrs 100 100  200    0       0     0     0>\n            ;\n    };\n};\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn a_shield_becomes_a_board_with_a_profile_per_controller() {
        let root = checkout();
        let output = boards(&root, "v0.3.0").unwrap();
        assert!(output.warnings.is_empty(), "{:?}", output.warnings);
        assert_eq!(output.boards.len(), 1);
        let generated = &output.boards[0];
        assert_eq!(generated.id, "foo");
        let board = kc_boards::Board::from_toml(&generated.toml).unwrap();
        assert_eq!(board.name, "Foo");
        assert_eq!(board.vendor, "bar");
        assert!(!board.verified);
        assert_eq!(board.usb[0].name, "Foo Board");
        assert_eq!(board.starter_keys, ["A", "B", ""]);
        assert_eq!(board.default_layout, "bar_foo_layout");
        assert_eq!(board.layouts[0].keys.len(), 3);
        assert_eq!(board.layouts[0].name, "Foo");
        assert!(board.half(Side::Left).unwrap().central);
        assert!(board.half(Side::Right).is_some());
        assert_eq!(board.flash.order, [Side::Left, Side::Right]);

        let pro_micro = kc_boards::controllers()
            .iter()
            .filter(|c| c.connector == "pro_micro")
            .count();
        assert_eq!(board.firmware.len(), pro_micro);
        let nano = board.profile("zmk-0.3.0-nice_nano_v2").unwrap();
        assert_eq!(nano.controller.as_deref(), Some("nice_nano_v2"));
        assert_eq!(nano.zmk.as_ref().unwrap().revision, "v0.3.0");
        assert_eq!(nano.config_name, "foo");
        assert_eq!(nano.builds[0].shield.as_deref(), Some("foo_left"));
        assert_eq!(nano.builds[0].board, "nice_nano_v2");
        assert_eq!(nano.builds[0].snippets, ["studio-rpc-usb-uart"]);
        assert!(nano.builds[1].snippets.is_empty());
        assert!(nano
            .capabilities
            .contains(&kc_boards::board::Capability::Studio));
        assert_eq!(
            board.bootloader(nano, Side::Left).unwrap().volumes,
            ["NICENANO"]
        );
        let rp2040 = board
            .profile("zmk-0.3.0-sparkfun_pro_micro_rp2040")
            .unwrap();
        assert!(!rp2040
            .capabilities
            .contains(&kc_boards::board::Capability::ExtPower));
        assert_eq!(
            board.bootloader(rp2040, Side::Right).unwrap().volumes,
            ["RPI-RP2"]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn starter_keys_are_the_plain_keys_of_the_first_layer() {
        let keys = starter_keys(
            "#include <behaviors.dtsi>\n#include <dt-bindings/zmk/keys.h>\n/ {\n keymap {\n  compatible = \"zmk,keymap\";\n  default_layer {\n   bindings = <&kp A &mo 1 &kp SPACE &trans &lt 2 B>;\n  };\n };\n};\n",
        )
        .unwrap();
        assert_eq!(keys, ["A", "", "SPACE", "", ""]);
    }
}
