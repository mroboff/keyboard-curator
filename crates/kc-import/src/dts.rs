//! A small reader for devicetree source, enough for keymap files: nodes,
//! labels and properties, with the original text of each node kept so that
//! anything not understood can be carried over verbatim.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Prop {
    pub name: String,
    /// The text after `=`, without the closing `;`. `None` for a flag.
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Labels placed before the node name, as in `label: name { ... }`.
    pub labels: Vec<String>,
    /// The node name, `/` for the root, or `&label` for a reference.
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<Node>,
    /// The node as written, from its labels to its closing `};`.
    pub text: String,
}

impl Node {
    pub fn prop(&self, name: &str) -> Option<&str> {
        self.props
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| p.value.as_deref())
    }

    pub fn has(&self, name: &str) -> bool {
        self.props.iter().any(|p| p.name == name)
    }

    /// The first string in a property, as in `compatible = "zmk,keymap"`.
    pub fn string(&self, name: &str) -> Option<&str> {
        self.prop(name)?.split('"').nth(1)
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    /// The label a binding would use to refer to this node.
    pub fn label(&self) -> Option<&str> {
        self.labels.first().map(String::as_str)
    }
}

/// What a keymap file holds once comments and preprocessor lines are out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Source {
    /// Every top-level node, in order: `/ { ... }` blocks and `&label`
    /// overrides.
    pub nodes: Vec<Node>,
    /// Object-like `#define`s: name to replacement text.
    pub defines: HashMap<String, String>,
    /// Function-like macros that were defined, by name. They are not
    /// expanded.
    pub macros: Vec<String>,
    /// `#include` lines, as written.
    pub includes: Vec<String>,
    /// Whether the file has `#if`-style blocks, which are not evaluated.
    pub conditionals: bool,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DtsError {
    #[error("line {0}: a `{{` is never closed")]
    Unclosed(usize),
    #[error("line {0}: expected `;`, `=` or `{{` after `{1}`")]
    Unexpected(usize, String),
}

/// Removes comments, keeping line breaks so line numbers stay right.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_string = !in_string;
                out.push(c);
            }
            '/' if !in_string && chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if !in_string && chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = ' ';
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                    }
                    if previous == '*' && c == '/' {
                        break;
                    }
                    previous = c;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn line(&self) -> usize {
        self.src[..self.pos].matches('\n').count() + 1
    }

    fn skip_space(&mut self) {
        let rest = &self.src[self.pos..];
        self.pos += rest.len() - rest.trim_start().len();
    }

    /// Reads up to the next `{`, `=` or `;` outside any bracket or string.
    fn head(&mut self) -> Option<(String, char)> {
        let start = self.pos;
        let mut in_string = false;
        for (offset, c) in self.src[start..].char_indices() {
            match c {
                '"' => in_string = !in_string,
                '{' | '=' | ';' | '}' if !in_string => {
                    self.pos = start + offset + 1;
                    return Some((self.src[start..start + offset].trim().to_string(), c));
                }
                _ => {}
            }
        }
        None
    }

    /// Reads a property value up to its closing `;`.
    fn value(&mut self) -> String {
        let start = self.pos;
        let (mut depth, mut in_string) = (0i32, false);
        for (offset, c) in self.src[start..].char_indices() {
            match c {
                '"' => in_string = !in_string,
                '<' | '(' | '[' if !in_string => depth += 1,
                '>' | ')' | ']' if !in_string => depth -= 1,
                ';' if !in_string && depth <= 0 => {
                    self.pos = start + offset + 1;
                    return self.src[start..start + offset].trim().to_string();
                }
                _ => {}
            }
        }
        self.pos = self.src.len();
        self.src[start..].trim().to_string()
    }

    /// Parses the items inside a node, or at the top level.
    fn items(&mut self, top: bool) -> Result<(Vec<Prop>, Vec<Node>), DtsError> {
        let (mut props, mut children) = (Vec::new(), Vec::new());
        loop {
            self.skip_space();
            let start = self.pos;
            let line = self.line();
            let Some((head, end)) = self.head() else {
                return if top {
                    Ok((props, children))
                } else {
                    Err(DtsError::Unclosed(line))
                };
            };
            match end {
                '}' if !top && head.is_empty() => {
                    // Consume the `;` that closes the node.
                    self.skip_space();
                    if self.src[self.pos..].starts_with(';') {
                        self.pos += 1;
                    }
                    return Ok((props, children));
                }
                '{' => {
                    let mut parts: Vec<&str> = head.split(':').map(str::trim).collect();
                    let name = parts.pop().unwrap_or_default().to_string();
                    let labels = parts.into_iter().map(str::to_string).collect();
                    let (node_props, node_children) = self.items(false)?;
                    children.push(Node {
                        labels,
                        name,
                        props: node_props,
                        children: node_children,
                        text: self.src[start..self.pos].to_string(),
                    });
                }
                '=' => {
                    let value = self.value();
                    props.push(Prop {
                        name: head,
                        value: Some(value),
                    });
                }
                // `/dts-v1/;`, `/delete-node/ x;` and the like are skipped.
                ';' if head.starts_with('/') => {}
                ';' if !head.is_empty() => props.push(Prop {
                    name: head,
                    value: None,
                }),
                ';' => {}
                _ => return Err(DtsError::Unexpected(line, head)),
            }
        }
    }
}

pub fn parse(src: &str) -> Result<Source, DtsError> {
    let mut source = Source::default();
    let mut body = String::new();
    let joined = strip_comments(src).replace("\\\n", " ");
    for line in joined.lines() {
        let trimmed = line.trim_start();
        let Some(directive) = trimmed.strip_prefix('#') else {
            body.push_str(line);
            body.push('\n');
            continue;
        };
        // `#binding-cells` and friends are properties, not directives.
        let directive = directive.trim_start();
        let word = directive.split_whitespace().next().unwrap_or_default();
        match word {
            "define" => {
                let rest = directive["define".len()..].trim();
                let name_end = rest
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .unwrap_or(rest.len());
                let (name, value) = rest.split_at(name_end);
                if value.starts_with('(') {
                    source.macros.push(name.to_string());
                } else if !name.is_empty() {
                    source
                        .defines
                        .insert(name.to_string(), value.trim().to_string());
                }
            }
            "include" => source.includes.push(trimmed.to_string()),
            "if" | "ifdef" | "ifndef" | "else" | "elif" | "endif" => source.conditionals = true,
            "undef" | "pragma" | "error" | "warning" => {}
            _ => {
                body.push_str(line);
                body.push('\n');
                continue;
            }
        }
        body.push('\n');
    }
    let mut parser = Parser { src: &body, pos: 0 };
    let (_, nodes) = parser.items(true)?;
    source.nodes = nodes;
    Ok(source)
}

/// Replaces every identifier that is an object-like `#define` with its
/// value, repeatedly, so that aliases of aliases resolve.
pub fn expand(text: &str, defines: &HashMap<String, String>) -> String {
    let mut current = text.to_string();
    for _ in 0..8 {
        let mut out = String::with_capacity(current.len());
        let mut word = String::new();
        let mut changed = false;
        for c in current.chars().chain([' ']) {
            if c.is_alphanumeric() || c == '_' {
                word.push(c);
                continue;
            }
            match defines.get(&word) {
                Some(value) if !word.is_empty() => {
                    out.push_str(value);
                    changed = true;
                }
                _ => out.push_str(&word),
            }
            word.clear();
            out.push(c);
        }
        out.pop();
        current = out;
        if !changed {
            break;
        }
    }
    current
}

/// The contents of each `<...>` group in a property value.
pub fn cell_groups(value: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let (mut depth, mut start) = (0, 0);
    for (index, c) in value.char_indices() {
        match c {
            '<' => {
                if depth == 0 {
                    start = index + 1;
                }
                depth += 1;
            }
            '>' => {
                depth -= 1;
                if depth == 0 {
                    groups.push(value[start..index].trim().to_string());
                }
            }
            _ => {}
        }
    }
    groups
}

/// Whitespace-separated tokens, keeping `NAME(a, b)` and `(A | B)` whole.
pub fn tokens(cells: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut current, mut depth) = (String::new(), 0usize);
    for c in cells.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        } else if !c.is_whitespace() {
            current.push(c);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Splits a flat run of cells into bindings, each starting at a `&`.
pub fn bindings(cells: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in tokens(cells) {
        match out.last_mut() {
            Some(last) if !token.starts_with('&') => {
                last.push(' ');
                last.push_str(&token);
            }
            _ => out.push(token),
        }
    }
    out
}

/// The numbers in a property such as `<40 43>`.
pub fn numbers(value: &str) -> Vec<i64> {
    cell_groups(value)
        .iter()
        .flat_map(|g| tokens(g))
        .filter_map(|t| {
            let t = t.trim_matches(|c| c == '(' || c == ')');
            match t.strip_prefix("0x") {
                Some(hex) => i64::from_str_radix(hex, 16).ok(),
                None => t.parse().ok(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
/* A keymap. */
#include <behaviors.dtsi>
#define LAYER_Nav 1
#define HYPER LC(LS(LG(LALT)))
#define ROW(a, b) a b
#ifdef SOMETHING
#endif

/ {
    chosen { zmk,physical-layout = &layout_a; };
    behaviors {
        hm: homerow_mods {
            compatible = "zmk,behavior-hold-tap";
            #binding-cells = <2>;
            flavor = "balanced"; // why not
            retro-tap;
            bindings = <&kp>, <&kp>;
        };
    };
    keymap {
        compatible = "zmk,keymap";
        base {
            display-name = "Base; with {braces}";
            bindings = <&kp A &mo LAYER_Nav
                        &kp HYPER &rgb_ug RGB_COLOR_HSB(1, 2, 3)>;
        };
    };
};

&listener {
    input-processors = <&zip_xy_scaler 1 3>, <&zip_xy_to_scroll_mapper>;
};
"#;

    #[test]
    fn reads_nodes_labels_properties_and_directives() {
        let source = parse(SRC).unwrap();
        assert_eq!(source.nodes.len(), 2);
        assert_eq!(source.defines["LAYER_Nav"], "1");
        assert_eq!(source.macros, ["ROW"]);
        assert_eq!(source.includes, ["#include <behaviors.dtsi>"]);
        assert!(source.conditionals);

        let root = &source.nodes[0];
        assert_eq!(root.name, "/");
        let hold_tap = &root.child("behaviors").unwrap().children[0];
        assert_eq!(
            (hold_tap.label(), hold_tap.name.as_str()),
            (Some("hm"), "homerow_mods")
        );
        assert_eq!(hold_tap.string("compatible"), Some("zmk,behavior-hold-tap"));
        assert_eq!(hold_tap.prop("#binding-cells"), Some("<2>"));
        assert!(hold_tap.has("retro-tap") && !hold_tap.has("lazy"));
        assert!(hold_tap.text.starts_with("hm: homerow_mods {") && hold_tap.text.ends_with("};"));

        let layer = &root.child("keymap").unwrap().children[0];
        assert_eq!(layer.string("display-name"), Some("Base; with {braces}"));
        let cells = expand(
            &cell_groups(layer.prop("bindings").unwrap())[0],
            &source.defines,
        );
        assert_eq!(
            bindings(&cells),
            [
                "&kp A",
                "&mo 1",
                "&kp LC(LS(LG(LALT)))",
                "&rgb_ug RGB_COLOR_HSB(1,2,3)"
            ]
        );

        let listener = &source.nodes[1];
        assert_eq!(listener.name, "&listener");
        assert_eq!(
            cell_groups(listener.prop("input-processors").unwrap()).len(),
            2
        );
        assert_eq!(
            root.child("chosen").unwrap().prop("zmk,physical-layout"),
            Some("&layout_a")
        );
    }

    #[test]
    fn numbers_and_errors() {
        assert_eq!(numbers("<40 43>, <0x10>"), [40, 43, 16]);
        assert!(matches!(
            parse("/ { a { b = <1>; };"),
            Err(DtsError::Unclosed(_))
        ));
        assert!(parse("").unwrap().nodes.is_empty());
    }
}
