//! Reads `zmk,physical-layout` key tables out of ZMK devicetree source, so a
//! vendor's layout file can be converted into a board definition.

use crate::geometry::Key;

/// One physical layout node found in devicetree source.
#[derive(Debug, Clone, PartialEq)]
pub struct DtsLayout {
    /// The node label, for example `physical_layout_imprint_function_row`.
    pub label: String,
    pub display_name: Option<String>,
    pub keys: Vec<Key>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DtsError {
    #[error("line {line}: expected 7 integers in key_physical_attrs, found {found}")]
    WrongFieldCount { line: usize, found: usize },
    #[error("line {line}: `{text}` is not an integer")]
    NotAnInteger { line: usize, text: String },
    #[error("line {line}: key_physical_attrs outside a labeled node")]
    KeyOutsideNode { line: usize },
}

/// Extracts every labeled node that carries `key_physical_attrs` entries.
///
/// This is a line-oriented scan, not a devicetree parser: it expects one
/// `<&key_physical_attrs w h x y rot rx ry>` per line, which is how ZMK and
/// its layout tooling format these tables.
pub fn parse_physical_layouts(src: &str) -> Result<Vec<DtsLayout>, DtsError> {
    let mut layouts: Vec<DtsLayout> = Vec::new();
    let mut in_layout = false;
    for (ix, raw) in src.lines().enumerate() {
        let line = ix + 1;
        let text = raw.split("//").next().unwrap_or("").trim();
        if let Some(label) = node_label(text) {
            layouts.push(DtsLayout {
                label: label.to_string(),
                display_name: None,
                keys: Vec::new(),
            });
            in_layout = true;
        } else if text.ends_with('{') {
            // An unlabeled node (such as the root) cannot be a layout.
            in_layout = false;
        }
        if let Some(rest) = text.strip_prefix("display-name") {
            if let (true, Some(layout)) = (in_layout, layouts.last_mut()) {
                layout.display_name = rest.split('"').nth(1).map(str::to_string);
            }
        }
        if let Some(start) = text.find("&key_physical_attrs") {
            let fields = text[start + "&key_physical_attrs".len()..]
                .split('>')
                .next()
                .unwrap_or("");
            let values = fields
                .split_whitespace()
                .map(|t| {
                    let digits = t.trim_matches(|c| c == '(' || c == ')');
                    digits.parse::<i32>().map_err(|_| DtsError::NotAnInteger {
                        line,
                        text: t.to_string(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let values: [i32; 7] =
                values
                    .try_into()
                    .map_err(|v: Vec<i32>| DtsError::WrongFieldCount {
                        line,
                        found: v.len(),
                    })?;
            match layouts.last_mut() {
                Some(layout) if in_layout => layout.keys.push(Key::from(values)),
                _ => return Err(DtsError::KeyOutsideNode { line }),
            }
        }
    }
    layouts.retain(|l| !l.keys.is_empty());
    Ok(layouts)
}

/// The label of a `label: name {` node header, if `text` is one.
fn node_label(text: &str) -> Option<&str> {
    let header = text.strip_suffix('{')?;
    let (label, _) = header.split_once(':')?;
    let label = label.trim();
    let is_ident = !label.is_empty() && label.chars().all(|c| c.is_alphanumeric() || c == '_');
    is_ident.then_some(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
/ {
    layout_a: physical_layout_0 {
        compatible = "zmk,physical-layout";
        display-name = "Layout A";
        keys  //                     w   h    x    y     rot    rx    ry
            = <&key_physical_attrs 100 100    0  125       0     0     0>
            , <&key_physical_attrs 100 100 1100  575 (-1600)  1150  1340>
            ;
    };
    position_map: position_map {
        compatible = "zmk,physical-layout-position-map";
    };
};
"#;

    #[test]
    fn reads_labels_names_and_negative_rotations() {
        let layouts = parse_physical_layouts(SRC).unwrap();
        assert_eq!(layouts.len(), 1);
        assert_eq!(layouts[0].label, "layout_a");
        assert_eq!(layouts[0].display_name.as_deref(), Some("Layout A"));
        assert_eq!(
            layouts[0].keys,
            vec![
                Key::from([100, 100, 0, 125, 0, 0, 0]),
                Key::from([100, 100, 1100, 575, -1600, 1150, 1340]),
            ]
        );
    }

    #[test]
    fn reports_malformed_entries_with_their_line() {
        let short = "a: b {\n = <&key_physical_attrs 100 100 0>\n};";
        assert_eq!(
            parse_physical_layouts(short),
            Err(DtsError::WrongFieldCount { line: 2, found: 3 })
        );
        let stray = "/ {\n = <&key_physical_attrs 1 2 3 4 5 6 7>\n};";
        assert_eq!(
            parse_physical_layouts(stray),
            Err(DtsError::KeyOutsideNode { line: 2 })
        );
    }
}
