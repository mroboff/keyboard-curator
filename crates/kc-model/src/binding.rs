//! What a key does: a behavior plus its parameters.

use std::fmt;
use std::str::FromStr;

use kc_zmk::Modifier;
use serde::{Deserialize, Serialize};

use crate::ids::{BehaviorId, LayerId};

/// A keycode with optional modifier wrappers, written `LC(LS(K))` in ZMK.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyExpr {
    /// Outermost wrapper first.
    pub mods: Vec<Modifier>,
    /// A keycode name. Not checked here, so names from locale headers work;
    /// validation warns about names the catalog does not know.
    pub key: String,
}

impl KeyExpr {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            mods: Vec::new(),
            key: key.into(),
        }
    }

    pub fn with(mut self, modifier: Modifier) -> Self {
        self.mods.insert(0, modifier);
        self
    }
}

impl fmt::Display for KeyExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for m in &self.mods {
            write!(f, "{}(", m.function())?;
        }
        write!(f, "{}{}", self.key, ")".repeat(self.mods.len()))
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("`{0}` is not a key expression")]
pub struct KeyExprError(pub String);

impl FromStr for KeyExpr {
    type Err = KeyExprError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || KeyExprError(s.to_string());
        let mut rest = s.trim();
        let mut mods = Vec::new();
        while let Some((function, inner)) = rest.split_once('(') {
            let modifier = Modifier::from_function(function.trim()).ok_or_else(err)?;
            rest = inner.trim().strip_suffix(')').ok_or_else(err)?.trim();
            mods.push(modifier);
        }
        let is_name = !rest.is_empty() && rest.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !is_name {
            return Err(err());
        }
        Ok(Self {
            mods,
            key: rest.to_string(),
        })
    }
}

impl Serialize for KeyExpr {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for KeyExpr {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// The behavior a binding invokes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BehaviorRef {
    /// A ZMK built-in, by label (`kp`, `mo`, ...).
    BuiltIn(String),
    /// A behavior defined in this project.
    User { user: BehaviorId },
}

impl BehaviorRef {
    pub fn built_in(label: &str) -> Self {
        BehaviorRef::BuiltIn(label.to_string())
    }
}

/// One parameter of a binding.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Param {
    Key(KeyExpr),
    Layer(LayerId),
    /// A named constant such as `LCLK` or `MOVE_UP`.
    Constant(String),
    /// A command with its numeric arguments, such as `BT_SEL 2`.
    Command {
        name: String,
        args: Vec<u32>,
    },
    /// A bare number, for parameters that carry no meaning of their own.
    Number(i64),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Binding {
    Behavior {
        behavior: BehaviorRef,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        params: Vec<Param>,
    },
    /// Binding text the model does not understand, kept verbatim.
    Raw { raw: String },
}

impl Binding {
    pub fn new(label: &str, params: Vec<Param>) -> Self {
        Binding::Behavior {
            behavior: BehaviorRef::built_in(label),
            params,
        }
    }

    pub fn user(id: BehaviorId, params: Vec<Param>) -> Self {
        Binding::Behavior {
            behavior: BehaviorRef::User { user: id },
            params,
        }
    }

    pub fn trans() -> Self {
        Binding::new("trans", vec![])
    }

    pub fn none() -> Self {
        Binding::new("none", vec![])
    }

    /// `&kp KEY`
    pub fn kp(key: KeyExpr) -> Self {
        Binding::new("kp", vec![Param::Key(key)])
    }

    /// A single-layer behavior such as `&mo` or `&to`.
    pub fn layer(label: &str, layer: LayerId) -> Self {
        Binding::new(label, vec![Param::Layer(layer)])
    }

    /// Every layer this binding refers to.
    pub fn layers(&self) -> impl Iterator<Item = LayerId> + '_ {
        let params: &[Param] = match self {
            Binding::Behavior { params, .. } => params,
            Binding::Raw { .. } => &[],
        };
        params.iter().filter_map(|p| match p {
            Param::Layer(id) => Some(*id),
            _ => None,
        })
    }

    pub fn user_behavior(&self) -> Option<BehaviorId> {
        match self {
            Binding::Behavior {
                behavior: BehaviorRef::User { user },
                ..
            } => Some(*user),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_expressions_round_trip_through_text() {
        let expr: KeyExpr = "LC(LS(K))".parse().unwrap();
        assert_eq!(expr.mods, [Modifier::LCtrl, Modifier::LShift]);
        assert_eq!(expr.key, "K");
        assert_eq!(expr.to_string(), "LC(LS(K))");
        assert_eq!(KeyExpr::new("A").with(Modifier::RAlt).to_string(), "RA(A)");
        assert_eq!(" N1 ".parse::<KeyExpr>().unwrap(), KeyExpr::new("N1"));
    }

    #[test]
    fn malformed_key_expressions_are_rejected() {
        for bad in ["", "LC(A", "XX(A)", "LC()", "A B", "LC(A))"] {
            assert!(bad.parse::<KeyExpr>().is_err(), "{bad}");
        }
    }

    #[test]
    fn bindings_have_a_compact_json_form() {
        let kp = Binding::kp("LG(TAB)".parse().unwrap());
        assert_eq!(
            serde_json::to_string(&kp).unwrap(),
            r#"{"behavior":"kp","params":[{"key":"LG(TAB)"}]}"#
        );
        let cases = [
            kp,
            Binding::trans(),
            Binding::layer("mo", LayerId(3)),
            Binding::user(
                BehaviorId(9),
                vec![Param::Layer(LayerId(1)), Param::Number(0)],
            ),
            Binding::new(
                "bt",
                vec![Param::Command {
                    name: "BT_SEL".into(),
                    args: vec![2],
                }],
            ),
            Binding::Raw {
                raw: "&custom 1 2".into(),
            },
        ];
        for binding in cases {
            let json = serde_json::to_string(&binding).unwrap();
            assert_eq!(
                serde_json::from_str::<Binding>(&json).unwrap(),
                binding,
                "{json}"
            );
        }
    }
}
