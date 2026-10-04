//! Combos, conditional layers, pointing devices, lighting and settings.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::binding::Binding;
use crate::ids::{ComboId, LayerId};

/// Keys pressed together that produce a different binding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Combo {
    pub id: ComboId,
    pub name: String,
    pub key_positions: Vec<usize>,
    pub binding: Binding,
    pub timeout_ms: Option<u32>,
    pub require_prior_idle_ms: Option<u32>,
    pub slow_release: bool,
    /// Layers the combo is active on; empty means all.
    pub layers: Vec<LayerId>,
}

/// "When all of `if_layers` are active, activate `then_layer`."
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionalLayer {
    pub if_layers: Vec<LayerId>,
    pub then_layer: LayerId,
}

/// What reports from a pointing device are turned into.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputProcessor {
    /// Scale pointer movement by `multiplier / divisor`.
    Scale { multiplier: u32, divisor: u32 },
    /// Turn pointer movement into scrolling.
    ToScroll,
    /// Flip or swap axes. `scroll` applies it to scrolling instead of movement.
    Transform {
        invert_x: bool,
        invert_y: bool,
        swap_xy: bool,
        scroll: bool,
    },
    /// Activate a layer while the device is in use.
    TempLayer { layer: LayerId, timeout_ms: u32 },
    /// Processor text the model does not understand, kept verbatim.
    Raw(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointingOverride {
    pub layers: Vec<LayerId>,
    pub processors: Vec<InputProcessor>,
}

/// Configuration for one pointing device, by its input listener label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointingConfig {
    pub listener: String,
    pub processors: Vec<InputProcessor>,
    /// Different processors while particular layers are active.
    pub overrides: Vec<PointingOverride>,
}

impl PointingConfig {
    pub fn processors_mut(&mut self) -> impl Iterator<Item = &mut InputProcessor> {
        self.processors.iter_mut().chain(
            self.overrides
                .iter_mut()
                .flat_map(|o| o.processors.iter_mut()),
        )
    }

    pub fn all_processors(&self) -> impl Iterator<Item = &InputProcessor> {
        self.processors
            .iter()
            .chain(self.overrides.iter().flat_map(|o| o.processors.iter()))
    }
}

/// What a pointing device does, in the terms the pointing editor offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointingProfile {
    /// Scroll instead of moving the pointer.
    pub scroll: bool,
    /// Speed as `multiplier / divisor`.
    pub speed: (u32, u32),
    pub invert_x: bool,
    pub invert_y: bool,
    pub swap_xy: bool,
    /// A layer to activate while the device is in use, and for how long
    /// after it stops.
    pub auto_layer: Option<(LayerId, u32)>,
}

impl Default for PointingProfile {
    fn default() -> Self {
        Self {
            scroll: false,
            speed: (1, 1),
            invert_x: false,
            invert_y: false,
            swap_xy: false,
            auto_layer: None,
        }
    }
}

impl PointingProfile {
    /// The input processors that give this behaviour.
    pub fn to_processors(self) -> Vec<InputProcessor> {
        let mut out = Vec::new();
        if self.speed != (1, 1) {
            out.push(InputProcessor::Scale {
                multiplier: self.speed.0,
                divisor: self.speed.1,
            });
        }
        if self.scroll {
            out.push(InputProcessor::ToScroll);
        }
        if self.invert_x || self.invert_y || self.swap_xy {
            out.push(InputProcessor::Transform {
                invert_x: self.invert_x,
                invert_y: self.invert_y,
                swap_xy: self.swap_xy,
                scroll: self.scroll,
            });
        }
        if let Some((layer, timeout_ms)) = self.auto_layer {
            out.push(InputProcessor::TempLayer { layer, timeout_ms });
        }
        out
    }

    /// Reads a processor list back, when it is one the editor could have
    /// produced. Anything else (raw processors, repeats) is left to the
    /// raw view.
    pub fn from_processors(processors: &[InputProcessor]) -> Option<Self> {
        let mut profile = Self::default();
        let (mut scaled, mut transformed) = (false, None);
        for processor in processors {
            match processor {
                InputProcessor::Scale {
                    multiplier,
                    divisor,
                } if !scaled => {
                    profile.speed = (*multiplier, *divisor);
                    scaled = true;
                }
                InputProcessor::ToScroll if !profile.scroll => profile.scroll = true,
                InputProcessor::Transform {
                    invert_x,
                    invert_y,
                    swap_xy,
                    scroll,
                } if transformed.is_none() => {
                    (profile.invert_x, profile.invert_y, profile.swap_xy) =
                        (*invert_x, *invert_y, *swap_xy);
                    transformed = Some(*scroll);
                }
                InputProcessor::TempLayer { layer, timeout_ms } if profile.auto_layer.is_none() => {
                    profile.auto_layer = Some((*layer, *timeout_ms));
                }
                _ => return None,
            }
        }
        // A transform has to act on what the device produces.
        match transformed {
            Some(scroll) if scroll != profile.scroll => None,
            _ => Some(profile),
        }
    }
}

/// A colour at full range, written `#RRGGBB`. Brightness limits are applied
/// by firmware settings, never by altering stored colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("`{0}` is not a #RRGGBB colour")]
pub struct RgbError(pub String);

impl FromStr for Rgb {
    type Err = RgbError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || RgbError(s.to_string());
        let hex = s
            .strip_prefix('#')
            .filter(|h| h.len() == 6)
            .ok_or_else(err)?;
        let value = u32::from_str_radix(hex, 16).map_err(|_| err())?;
        Ok(Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8))
    }
}

impl Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockKind {
    Caps,
    Num,
    Scroll,
}

/// How one key is lit on one layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyLight {
    /// Show whatever the next active layer down shows.
    #[default]
    Inherit,
    Off,
    Color(Rgb),
    /// One colour while a lock is off, another while it is on.
    Lock {
        lock: LockKind,
        off: Rgb,
        on: Rgb,
    },
    /// One colour below a battery percentage, another at or above it.
    Battery {
        percent: u8,
        below: Rgb,
        above: Rgb,
    },
}

/// Per-key colours for one layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerLighting {
    pub layer: LayerId,
    pub fade_delay: Option<u32>,
    /// One entry per key position.
    pub keys: Vec<KeyLight>,
    /// Swatches offered when painting this layer.
    pub palette: Vec<Rgb>,
}

/// A value in the firmware's `.conf` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    Text(String),
}

/// Devicetree and Kconfig text carried verbatim for anything the model does
/// not represent.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RawBlocks {
    /// Behaviour definitions, emitted inside the `behaviors` node.
    pub behaviors: String,
    /// General devicetree, emitted at the end of the keymap file.
    pub devicetree: String,
    /// Extra lines for the `.conf` file.
    pub conf: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_round_trip_as_hex() {
        let c: Rgb = "#ff8000".parse().unwrap();
        assert_eq!(c, Rgb(255, 128, 0));
        assert_eq!(c.to_string(), "#FF8000");
        assert_eq!(
            serde_json::to_string(&KeyLight::Color(c)).unwrap(),
            r##"{"color":"#FF8000"}"##
        );
        for bad in ["ff8000", "#ff80", "#gg0000", "#ff800000"] {
            assert!(bad.parse::<Rgb>().is_err(), "{bad}");
        }
    }

    #[test]
    fn pointing_profiles_round_trip_through_processors() {
        let scroller = PointingProfile {
            scroll: true,
            speed: (1, 3),
            invert_y: true,
            auto_layer: Some((LayerId(4), 500)),
            ..PointingProfile::default()
        };
        let processors = scroller.to_processors();
        assert_eq!(processors.len(), 4);
        assert_eq!(
            PointingProfile::from_processors(&processors),
            Some(scroller)
        );
        assert_eq!(PointingProfile::default().to_processors(), []);
        assert_eq!(
            PointingProfile::from_processors(&[]),
            Some(PointingProfile::default())
        );
        // The Imprint's factory scroller, in the vendor's own order.
        let factory = [
            InputProcessor::Scale {
                multiplier: 1,
                divisor: 3,
            },
            InputProcessor::ToScroll,
            InputProcessor::Transform {
                invert_x: false,
                invert_y: true,
                swap_xy: false,
                scroll: true,
            },
        ];
        assert!(PointingProfile::from_processors(&factory).is_some_and(|p| p.scroll && p.invert_y));
        // Lists the editor could not have written are left alone.
        assert_eq!(
            PointingProfile::from_processors(&[InputProcessor::Raw("<&x>".into())]),
            None
        );
        assert_eq!(
            PointingProfile::from_processors(&[InputProcessor::ToScroll, InputProcessor::ToScroll]),
            None
        );
        let mismatched = [InputProcessor::Transform {
            invert_x: true,
            invert_y: false,
            swap_xy: false,
            scroll: true,
        }];
        assert_eq!(PointingProfile::from_processors(&mismatched), None);
    }

    #[test]
    fn settings_keep_their_types() {
        let json = r#"[true,40,"Imprint"]"#;
        let values: Vec<SettingValue> = serde_json::from_str(json).unwrap();
        assert_eq!(
            values,
            [
                SettingValue::Bool(true),
                SettingValue::Int(40),
                SettingValue::Text("Imprint".into())
            ]
        );
        assert_eq!(serde_json::to_string(&values).unwrap(), json);
    }
}
