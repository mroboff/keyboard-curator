//! Bindings as ZMK keymap text, in both directions.

use kc_zmk::behaviors::{self, CallStyle, ParamKind};

use crate::binding::{BehaviorRef, Binding, KeyExpr, Param};
use crate::ids::LayerId;
use crate::project::Project;

/// How a layer reference is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerStyle {
    /// The layer's position in the keymap, as ZMK itself counts them.
    Index,
    /// A `LAYER_Name` constant, which generated keymaps define.
    Constant,
}

/// The name of the `#define` a generated keymap gives a layer.
pub fn layer_constant(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("LAYER_{cleaned}")
}

fn format_layer(project: &Project, id: LayerId, style: LayerStyle) -> String {
    match (style, project.layer(id), project.layer_index(id)) {
        (LayerStyle::Constant, Some(layer), _) => layer_constant(&layer.name),
        (_, _, Some(index)) => index.to_string(),
        // A dangling reference; validation reports it.
        _ => "0".to_string(),
    }
}

/// A binding as it is written in a `.keymap` file, such as `&kp LC(A)`.
pub fn format_binding(project: &Project, binding: &Binding, style: LayerStyle) -> String {
    let (behavior, params) = match binding {
        Binding::Raw { raw } => return raw.clone(),
        Binding::Behavior { behavior, params } => (behavior, params),
    };
    let (label, function_style) = match behavior {
        BehaviorRef::BuiltIn(label) => (label.as_str(), function_commands(label)),
        BehaviorRef::User { user } => (
            project.behavior(*user).map_or("none", |b| b.label.as_str()),
            Vec::new(),
        ),
    };
    let mut text = format!("&{label}");
    for param in params {
        text.push(' ');
        match param {
            Param::Key(expr) => text.push_str(&expr.to_string()),
            Param::Layer(id) => text.push_str(&format_layer(project, *id, style)),
            Param::Constant(name) => text.push_str(name),
            Param::Number(n) => text.push_str(&n.to_string()),
            Param::Command { name, args } => {
                let args: Vec<String> = args.iter().map(u32::to_string).collect();
                if function_style.contains(&name.as_str()) {
                    text.push_str(&format!("{name}({})", args.join(",")));
                } else {
                    text.push_str(name);
                    for arg in args {
                        text.push(' ');
                        text.push_str(&arg);
                    }
                }
            }
        }
    }
    text
}

/// The commands of a built-in behavior that are written `NAME(a,b)`.
fn function_commands(label: &str) -> Vec<&'static str> {
    let Some(def) = behaviors::built_in(label) else {
        return Vec::new();
    };
    def.params
        .iter()
        .filter_map(|p| match p.kind {
            ParamKind::Command(commands) => Some(commands),
            _ => None,
        })
        .flatten()
        .filter(|c| c.style == CallStyle::Function)
        .map(|c| c.name)
        .collect()
}

fn parse_layer(project: &Project, token: &str) -> Option<LayerId> {
    if let Ok(index) = token.parse::<usize>() {
        return project.layers.get(index).map(|l| l.id);
    }
    project
        .layers
        .iter()
        .find(|l| layer_constant(&l.name) == token || l.name == token)
        .map(|l| l.id)
}

/// Splits binding text into tokens, keeping `NAME(a, b)` together.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut current, mut depth) = (String::new(), 0usize);
    for c in text.chars() {
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

fn parse_built_in(
    project: &Project,
    def: &behaviors::Behavior,
    args: &[String],
) -> Option<Vec<Param>> {
    let mut args = args.iter();
    let mut params = Vec::new();
    for expected in def.params {
        let token = args.next()?;
        params.push(match expected.kind {
            ParamKind::Keycode => Param::Key(token.parse::<KeyExpr>().ok()?),
            ParamKind::Layer => Param::Layer(parse_layer(project, token)?),
            ParamKind::Constant(allowed) => {
                allowed.iter().find(|c| c.name == token)?;
                Param::Constant(token.clone())
            }
            ParamKind::Command(commands) => {
                let (name, inline) = match token.split_once('(') {
                    Some((name, rest)) => (name, Some(rest.strip_suffix(')')?)),
                    None => (token.as_str(), None),
                };
                let command = commands.iter().find(|c| c.name == name)?;
                let values: Vec<u32> = match inline {
                    Some(list) => list
                        .split(',')
                        .map(|v| v.trim().parse())
                        .collect::<Result<_, _>>()
                        .ok()?,
                    None => (0..command.args.len())
                        .map(|_| args.next()?.parse().ok())
                        .collect::<Option<_>>()?,
                };
                if values.len() != command.args.len() {
                    return None;
                }
                Param::Command {
                    name: name.to_string(),
                    args: values,
                }
            }
        });
    }
    args.next().is_none().then_some(params)
}

/// Reads binding text such as `&mt LSHFT A`. Text that names a behavior and
/// parameters the model understands becomes a structured binding; anything
/// else is kept verbatim as a raw binding, so nothing typed is ever lost.
pub fn parse_binding(project: &Project, text: &str) -> Binding {
    let raw = || Binding::Raw {
        raw: text.trim().to_string(),
    };
    let tokens = tokens(text);
    let Some(label) = tokens.first().and_then(|t| t.strip_prefix('&')) else {
        return raw();
    };
    let args = &tokens[1..];
    if let Some(def) = behaviors::built_in(label) {
        return match parse_built_in(project, def, args) {
            Some(params) => Binding::new(label, params),
            None => raw(),
        };
    }
    let Some(def) = project.behaviors.iter().find(|b| b.label == label) else {
        return raw();
    };
    if args.len() != def.kind.param_count() {
        return raw();
    }
    // A user behavior's parameters take their meaning from the behaviors
    // it wraps: a hold-tap's first parameter goes to its hold behavior, and
    // so on. Where that says nothing, read each as the most specific thing
    // it could be.
    let wrapped: Vec<&BehaviorRef> = match &def.kind {
        crate::behavior::BehaviorKind::HoldTap(h) => vec![&h.hold, &h.tap],
        crate::behavior::BehaviorKind::StickyKey(s) => vec![&s.behavior],
        _ => Vec::new(),
    };
    let expected = |index: usize| match wrapped.get(index) {
        Some(BehaviorRef::BuiltIn(label)) => behaviors::built_in(label)
            .and_then(|def| def.params.first())
            .map(|p| p.kind),
        _ => None,
    };
    let params = args
        .iter()
        .enumerate()
        .map(|(index, token)| {
            let layer = || parse_layer(project, token).map(Param::Layer);
            let key = || token.parse::<KeyExpr>().ok().map(Param::Key);
            let number = || token.parse::<i64>().ok().map(Param::Number);
            let read = match expected(index) {
                Some(ParamKind::Layer) => layer().or_else(number),
                Some(ParamKind::Keycode) => key().or_else(number),
                _ => number().or_else(layer).or_else(key),
            };
            read.unwrap_or_else(|| Param::Constant(token.clone()))
        })
        .collect();
    Binding::user(def.id, params)
}
