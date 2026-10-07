//! Reading everything the firmware lets a host manage, as moergo-config's
//! snapshot: the keymap, layer names, behavior tables, pointing and
//! lighting. The sequence follows moergo-control's `config pull`
//! (crates/moergo-control/src/config.rs in moergo-rmk, MIT), so that what
//! this app reads is what that tool reads.

use std::collections::HashMap;

use moergo_config::{
    background_from_wire, conditional_scene_from_advanced_wire, conditional_scene_from_rule,
    conditional_scene_from_wire, effects_from_wire, live_param_tables, output_mode_from_wire,
    scene_from_wire, scene_policy_from_wire, BehaviorSnapshot, EffectParams, HoldTriggerPosition,
    LightingSnapshot, MorseProfileEntry, OutputModeConfig, ParamSpec, Snapshot,
};
use rynk::rmk_types::action::KeyAction;
use rynk::rmk_types::protocol::rynk::{
    DeviceCapabilities, LayerMetadata, LightingError, LightingExtendedConditionalSceneCell,
    LightingExtensionNameKind, LightingExtensionParamsRequest, LightingFeatureFlags, RynkError,
};
use rynk::{Client, KeyTopology, RynkHostError};

use crate::session::host_error;
use crate::RynkError as Error;

/// A firmware that does not have an endpoint answers `UnknownCmd`; one
/// whose service lacks a surface answers `Unsupported`. Both mean there is
/// nothing to read there.
pub(crate) fn unsupported(error: &RynkHostError) -> bool {
    matches!(
        error,
        RynkHostError::Rejected(RynkError::UnknownCmd | RynkError::Unimplemented)
            | RynkHostError::LightingRejected(LightingError::Unsupported)
            | RynkHostError::Unsupported(..)
    )
}

fn optional<T>(result: Result<T, RynkHostError>) -> Result<Option<T>, Error> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if unsupported(&error) => Ok(None),
        Err(error) => Err(host_error(error)),
    }
}

/// The whole keymap, layer after layer.
pub(crate) async fn read_all_actions(
    client: &Client,
    capabilities: &DeviceCapabilities,
) -> Result<Vec<KeyAction>, Error> {
    let cells = usize::from(capabilities.num_layers)
        * usize::from(capabilities.num_rows)
        * usize::from(capabilities.num_cols);
    if capabilities.bulk_transfer_supported {
        if let Ok(actions) = client.read_all_keymap().await {
            if actions.len() == cells {
                return Ok(actions);
            }
        }
    }
    let mut actions = Vec::with_capacity(cells);
    for layer in 0..capabilities.num_layers {
        for row in 0..capabilities.num_rows {
            for col in 0..capabilities.num_cols {
                actions.push(client.get_key(layer, row, col).await.map_err(host_error)?);
            }
        }
    }
    Ok(actions)
}

/// Every layer slot's name, or nothing when the firmware keeps none.
async fn read_layer_names(
    client: &Client,
    layers: u8,
) -> Result<Option<Vec<LayerMetadata>>, Error> {
    let mut slots = Vec::with_capacity(usize::from(layers));
    for layer in 0..layers {
        match optional(client.get_layer_metadata(layer).await)? {
            Some(metadata) => slots.push(metadata),
            None => return Ok(None),
        }
    }
    Ok(Some(slots))
}

/// Macro space, a chunk at a time, up to the last sequence.
async fn read_macro_space(client: &Client) -> Result<Vec<u8>, Error> {
    let mut space = Vec::new();
    let mut offset = 0u16;
    loop {
        let chunk = client.get_macro(offset).await.map_err(host_error)?;
        if chunk.data.is_empty() || chunk.data.iter().all(|byte| *byte == 0) {
            break;
        }
        space.extend_from_slice(&chunk.data);
        offset = offset
            .checked_add(u16::try_from(chunk.data.len()).unwrap_or(u16::MAX))
            .ok_or(Error::Protocol(
                "macro space is larger than it can be addressed".into(),
            ))?;
    }
    while space.ends_with(&[0, 0]) {
        space.pop();
    }
    Ok(space)
}

async fn read_behaviors(
    client: &Client,
    capabilities: &DeviceCapabilities,
) -> Result<BehaviorSnapshot, Error> {
    let options = optional(client.get_behavior_options().await)?;
    let hold_trigger_positions =
        optional(client.get_morse_hold_trigger_positions().await)?.map(|state| {
            state
                .positions
                .into_iter()
                .map(|p| HoldTriggerPosition {
                    profile: p.profile,
                    row: p.row,
                    col: p.col,
                })
                .collect::<Vec<_>>()
        });
    let morse_profiles = match client.read_morse_profile_state().await {
        Ok(state) => Some(
            state
                .entries
                .into_iter()
                .map(|entry| MorseProfileEntry {
                    index: entry.index,
                    name: entry.name.as_str().to_string(),
                    profile: entry.profile,
                })
                .collect(),
        ),
        Err(error) if unsupported(&error) => {
            // Older firmware: unnamed profiles, dense from slot 0.
            let mut profiles = optional(client.read_all_morse_profiles().await)?;
            if let (Some(profiles), Some(options)) = (&mut profiles, options) {
                let required = hold_trigger_positions
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .filter(|p| p.profile != u8::MAX)
                    .map(|p| usize::from(p.profile) + 1)
                    .max()
                    .unwrap_or_default();
                while profiles.len() > required
                    && profiles.last() == Some(&options.morse_default_profile)
                {
                    profiles.pop();
                }
            }
            profiles.map(|profiles| {
                profiles
                    .into_iter()
                    .enumerate()
                    .map(|(index, profile)| MorseProfileEntry {
                        index: u8::try_from(index).unwrap_or(u8::MAX),
                        name: format!("profile_{index:03}"),
                        profile,
                    })
                    .collect()
            })
        }
        Err(error) => return Err(host_error(error)),
    };
    Ok(BehaviorSnapshot {
        config: optional(client.get_behavior().await)?,
        options,
        morse_profiles,
        hold_trigger_positions,
        auto_mouse_layers: optional(client.get_auto_mouse_layer_configs().await)?
            .map(|s| s.configs),
        morses: if capabilities.max_morse > 0 {
            Some(client.read_all_morses().await.map_err(host_error)?)
        } else {
            None
        },
        combos: if capabilities.max_combos > 0 {
            Some(
                client
                    .read_all_combo_definitions()
                    .await
                    .map_err(host_error)?,
            )
        } else {
            None
        },
        macros: if capabilities.macro_space_size > 0 {
            Some(read_macro_space(client).await?)
        } else {
            None
        },
        forks: if capabilities.max_forks > 0 {
            let mut forks = Vec::new();
            for index in 0..capabilities.max_forks {
                forks.push(client.get_fork(index).await.map_err(host_error)?);
            }
            Some(forks)
        } else {
            None
        },
    })
}

pub(crate) async fn read_extension_names(
    client: &Client,
    kind: LightingExtensionNameKind,
) -> Result<Vec<String>, Error> {
    Ok(client
        .read_all_lighting_extension_names(kind)
        .await
        .map_err(host_error)?
        .iter()
        .map(|name| name.as_str().to_string())
        .collect())
}

async fn read_effect_params(client: &Client, effect: u8) -> Result<Vec<ParamSpec>, RynkHostError> {
    let mut params = Vec::new();
    let mut offset: u8 = 0;
    loop {
        let page = client
            .get_lighting_extension_params(LightingExtensionParamsRequest { effect, offset })
            .await?;
        if offset >= page.total {
            break;
        }
        if page.items.is_empty() || usize::from(offset) + page.items.len() > usize::from(page.total)
        {
            return Err(RynkHostError::InconsistentResponse {
                cmd: rynk::rmk_types::protocol::rynk::Cmd::GetLightingExtensionParams,
                reason: "parameter page is empty or extends beyond the advertised total",
            });
        }
        offset += u8::try_from(page.items.len()).unwrap_or(u8::MAX);
        params.extend(page.items.iter().map(|item| ParamSpec {
            name: item.name.as_str().to_string(),
            min: item.min,
            max: item.max,
            default: item.default,
            value: item.value,
        }));
    }
    Ok(params)
}

pub(crate) async fn read_extension_params(
    client: &Client,
    effect_names: &[String],
) -> Result<Option<Vec<EffectParams>>, Error> {
    let mut sets = Vec::new();
    for (index, effect) in effect_names.iter().enumerate() {
        let index = u8::try_from(index).unwrap_or(u8::MAX);
        let params = match read_effect_params(client, index).await {
            Ok(params) => params,
            Err(error) if unsupported(&error) => return Ok(None),
            Err(error) => return Err(host_error(error)),
        };
        if !params.is_empty() {
            sets.push(EffectParams {
                index,
                effect: effect.clone(),
                params,
            });
        }
    }
    Ok(Some(sets))
}

/// Which key each LED sits under, by the firmware's LED number.
pub type LedKeys = HashMap<u16, (u8, u8)>;

pub(crate) fn led_keys(topology: &KeyTopology) -> LedKeys {
    topology
        .leds
        .iter()
        .filter_map(|led| led.key.map(|key| (led.id.0, (key.row, key.col))))
        .collect()
}

/// Everything read from the keyboard.
pub struct Reading {
    pub snapshot: Snapshot,
    pub capabilities: DeviceCapabilities,
    pub topology: Option<KeyTopology>,
}

pub(crate) async fn read_snapshot(client: &Client) -> Result<Reading, Error> {
    let capabilities = client.get_capabilities().await.map_err(host_error)?;
    let (rows, cols) = (capabilities.num_rows, capabilities.num_cols);
    let layer_size = usize::from(rows) * usize::from(cols);
    let actions = read_all_actions(client, &capabilities).await?;
    let layers = actions
        .chunks(layer_size)
        .map(<[KeyAction]>::to_vec)
        .collect::<Vec<_>>();

    let lighting = if capabilities.lighting_enabled {
        Some(read_lighting(client).await?)
    } else {
        None
    };
    let topology = if capabilities.lighting_enabled {
        optional(client.read_lighting_key_topology().await)?
    } else {
        None
    };
    let snapshot = Snapshot {
        rows,
        cols,
        bluetooth_name: optional(client.get_ble_name().await)?
            .map(|name| name.template.as_str().to_string()),
        default_layer: client.get_default_layer().await.map_err(host_error)?,
        layers,
        layer_names: read_layer_names(client, capabilities.num_layers).await?,
        behaviors: read_behaviors(client, &capabilities).await?,
        pointing: optional(client.get_pointing_config().await)?,
        lighting,
    };
    Ok(Reading {
        snapshot,
        capabilities,
        topology,
    })
}

async fn read_lighting(client: &Client) -> Result<LightingSnapshot, Error> {
    let caps = client
        .get_lighting_capabilities()
        .await
        .map_err(host_error)?;
    let state = client.get_lighting_state().await.map_err(host_error)?;
    let output_mode_state = if caps.features.contains(LightingFeatureFlags::OUTPUT_MODE) {
        Some(
            client
                .get_lighting_output_mode()
                .await
                .map_err(host_error)?,
        )
    } else {
        None
    };
    let output_mode = output_mode_state
        .as_ref()
        .map_or(OutputModeConfig::AlwaysOn, |s| {
            output_mode_from_wire(s.mode)
        });
    let wake_layers = output_mode_state
        .as_ref()
        .map(|s| {
            (0..64u8)
                .filter(|layer| s.wake_layers & (1u64 << layer) != 0)
                .collect()
        })
        .unwrap_or_default();
    let scene_status = client
        .get_lighting_scene_status()
        .await
        .map_err(host_error)?;
    let (_, scene_cells) = client
        .read_all_lighting_scenes()
        .await
        .map_err(host_error)?;
    let model = |e: anyhow::Error| Error::Model(format!("{e:#}"));
    let conditional_scenes = if caps.features.contains(LightingFeatureFlags::RULES) {
        let (_, rules) = client.read_all_lighting_rules().await.map_err(host_error)?;
        Some(
            rules
                .into_iter()
                .map(conditional_scene_from_rule)
                .collect::<Result<Vec<_>, _>>()
                .map_err(model)?,
        )
    } else if caps
        .features
        .contains(LightingFeatureFlags::RUNTIME_LAYER_INDICATOR_CONDITIONS)
    {
        let (_, cells) = client
            .read_all_lighting_advanced_runtime_conditional_scenes()
            .await
            .map_err(host_error)?;
        Some(
            cells
                .into_iter()
                .map(conditional_scene_from_advanced_wire)
                .collect::<Result<Vec<_>, _>>()
                .map_err(model)?,
        )
    } else if caps
        .features
        .contains(LightingFeatureFlags::RUNTIME_EFFECTS_CONDITIONS)
    {
        let (_, cells) = client
            .read_all_lighting_extended_runtime_conditional_scenes()
            .await
            .map_err(host_error)?;
        Some(cells.into_iter().map(conditional_scene_from_wire).collect())
    } else if caps
        .features
        .contains(LightingFeatureFlags::RUNTIME_CONDITIONAL_SCENES)
    {
        let (_, cells) = client
            .read_all_lighting_runtime_conditional_scenes()
            .await
            .map_err(host_error)?;
        Some(
            cells
                .into_iter()
                .map(|cell| {
                    conditional_scene_from_wire(LightingExtendedConditionalSceneCell {
                        cell,
                        connection: None,
                        effects: None,
                    })
                })
                .collect(),
        )
    } else {
        None
    };
    let mut scenes = scene_cells
        .into_iter()
        .map(scene_from_wire)
        .collect::<Vec<_>>();
    scenes.sort();
    let (effects, params) = if caps
        .features
        .contains(LightingFeatureFlags::EXTENSION_EFFECTS)
    {
        let extension = client.get_lighting_extension().await.map_err(host_error)?;
        let effect_names = read_extension_names(client, LightingExtensionNameKind::Effects).await?;
        let palette_names =
            read_extension_names(client, LightingExtensionNameKind::Palettes).await?;
        let overlay = if caps
            .features
            .contains(LightingFeatureFlags::EXTENSION_LAYERING)
        {
            client
                .get_lighting_extension_layers()
                .await
                .map_err(host_error)?
                .overlay
        } else {
            None
        };
        let params = read_extension_params(client, &effect_names).await?;
        (
            Some(
                effects_from_wire(
                    extension.state,
                    overlay,
                    &effect_names,
                    &palette_names,
                    live_param_tables(params.as_deref()),
                )
                .map_err(model)?,
            ),
            params,
        )
    } else {
        (None, None)
    };
    Ok(LightingSnapshot {
        brightness: state.output_brightness,
        output_mode,
        wake_layers,
        scene_policy: scene_policy_from_wire(scene_status.policy),
        background: background_from_wire(state.background),
        effects,
        params,
        scenes,
        conditional_scenes,
    })
}
