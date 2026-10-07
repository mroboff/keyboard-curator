//! Writing a snapshot to the keyboard: only what differs from what it
//! holds, tables before the keys that point into them, keys in small pages
//! so that the firmware's flash queue is never flooded. The sequence
//! follows moergo-control's `config apply` (crates/moergo-control/src/
//! config.rs in moergo-rmk, MIT), which the firmware is tested against.
//!
//! The commands that write are exactly those in [`crate::WRITES`]; nothing
//! else in this crate changes anything on the keyboard.

use std::time::Duration;

use moergo_config::{
    background_to_wire, conditional_scene_predicate_mask, conditional_scene_to_advanced_wire,
    conditional_scene_to_rule, conditional_scene_to_wire, effects_to_wire, output_mode_to_wire,
    params_to_writes, scene_policy_to_wire, scene_to_wire, BehaviorSnapshot, EffectsConfig,
    LightingSnapshot, Snapshot,
};
use rynk::rmk_types::action::KeyAction;
use rynk::rmk_types::morse::MorseProfileName;
use rynk::rmk_types::protocol::rynk::{
    BleName, DeviceCapabilities, LightingExtensionNameKind, LightingFeatureFlags,
    LightingMutableState, LightingRuleStatus, MorseHoldTriggerPosition,
    MorseProfileEntry as WireMorseProfileEntry, RynkError, SetAutoMouseLayerConfigsRequest,
    SetKeymapBulkRequest, SetLightingExtensionLayersRequest, SetLightingExtensionParamRequest,
    SetLightingExtensionStateRequest, SetLightingLayerPolicyRequest, SetLightingOutputModeRequest,
    SetLightingStateRequest, SetLightingWakeLayersRequest, SetMorseHoldTriggerPositionsRequest,
    SetMorseProfileEntryRequest,
};
use rynk::{Client, RynkHostError};

use crate::read::{read_extension_names, unsupported};
use crate::session::host_error;
use crate::RynkError as Error;

/// Cells per keymap write. The firmware queues every written cell for its
/// flash task through a channel four deep and answers `Busy` when a page
/// does not fit, so four cells a page keep every reply instant.
const PERSIST_BATCH: usize = 4;
/// How long a page may keep drawing `Busy` before giving up: a storage
/// page migration can hold the queue for tens of seconds.
const PERSIST_BUSY_LIMIT: Duration = Duration::from_secs(300);
const PERSIST_BUSY_RETRY_DELAY: Duration = Duration::from_millis(100);

fn model(error: anyhow::Error) -> Error {
    Error::Model(format!("{error:#}"))
}

/// One past the last slot holding anything.
fn last_populated<T>(slots: &[T], populated: impl Fn(&T) -> bool) -> usize {
    slots.iter().rposition(populated).map_or(0, |i| i + 1)
}

async fn apply_behaviors(
    client: &Client,
    desired: &BehaviorSnapshot,
    before: &BehaviorSnapshot,
    macro_chunk: u16,
) -> Result<(), Error> {
    if let Some(config) = desired.config {
        if before.config != Some(config) {
            client.set_behavior(config).await.map_err(host_error)?;
        }
    }
    if let Some(options) = desired.options {
        if before.options != Some(options) {
            client
                .set_behavior_options(options)
                .await
                .map_err(host_error)?;
        }
    }
    if let Some(profiles) = &desired.morse_profiles {
        if before.morse_profiles.as_deref() != Some(profiles.as_slice()) {
            let held = before.morse_profiles.as_deref().unwrap_or_default();
            let mut writes = 0usize;
            let mut named = true;
            for entry in profiles {
                if held.iter().any(|old| old == entry) {
                    continue;
                }
                let name = MorseProfileName::try_from(entry.name.as_str()).map_err(|_| {
                    Error::Model(format!(
                        "the timing profile name “{}” is too long",
                        entry.name
                    ))
                })?;
                match client
                    .set_morse_profile_entry(SetMorseProfileEntryRequest {
                        entry: WireMorseProfileEntry {
                            index: entry.index,
                            name,
                            profile: entry.profile,
                        },
                    })
                    .await
                {
                    Ok(()) => writes += 1,
                    Err(error) if writes == 0 && unsupported(&error) => {
                        named = false;
                        break;
                    }
                    Err(error) => return Err(host_error(error)),
                }
            }
            if named {
                for old in held {
                    if profiles.iter().any(|e| e.index == old.index) {
                        continue;
                    }
                    match client.delete_morse_profile(old.index).await {
                        Ok(()) => writes += 1,
                        Err(error) if writes == 0 && unsupported(&error) => {
                            named = false;
                            break;
                        }
                        Err(error) => return Err(host_error(error)),
                    }
                }
            }
            if !named {
                // Older firmware keeps a dense, unnamed table.
                let default_profile = desired
                    .options
                    .or(before.options)
                    .map(|o| o.morse_default_profile)
                    .ok_or_else(|| {
                        Error::Protocol("the keyboard reports no default timing profile".into())
                    })?;
                let length = profiles
                    .iter()
                    .map(|e| usize::from(e.index) + 1)
                    .max()
                    .unwrap_or_default();
                let mut dense = vec![default_profile; length];
                for entry in profiles {
                    dense[usize::from(entry.index)] = entry.profile;
                }
                client
                    .write_all_morse_profiles(dense)
                    .await
                    .map_err(host_error)?;
            }
        }
    }
    if let Some(positions) = &desired.hold_trigger_positions {
        if before.hold_trigger_positions.as_ref() != Some(positions) {
            client
                .set_morse_hold_trigger_positions(SetMorseHoldTriggerPositionsRequest {
                    positions: positions
                        .iter()
                        .map(|p| MorseHoldTriggerPosition {
                            profile: p.profile,
                            row: p.row,
                            col: p.col,
                        })
                        .collect(),
                })
                .await
                .map_err(host_error)?;
        }
    }
    if let Some(configs) = &desired.auto_mouse_layers {
        if before.auto_mouse_layers.as_ref() != Some(configs) {
            client
                .set_auto_mouse_layer_configs(SetAutoMouseLayerConfigsRequest {
                    configs: configs.clone(),
                })
                .await
                .map_err(host_error)?;
        }
    }
    // The bulk writers write the slots they are given, so a table that
    // shrank is padded out to what the keyboard holds, which clears the
    // tail.
    if let Some(morses) = &desired.morses {
        let mut morses = morses.clone();
        let held = last_populated(before.morses.as_deref().unwrap_or_default(), |m| {
            !m.actions.is_empty()
        });
        morses.resize(morses.len().max(held), Default::default());
        if before.morses.as_deref() != Some(morses.as_slice()) {
            client.write_all_morses(morses).await.map_err(host_error)?;
        }
    }
    if let Some(combos) = &desired.combos {
        let mut combos = combos.clone();
        let held = last_populated(before.combos.as_deref().unwrap_or_default(), |c| {
            !c.is_empty()
        });
        let empty = rynk::rmk_types::combo::ComboDefinition::empty();
        combos.resize(combos.len().max(held), empty);
        if before.combos.as_deref() != Some(combos.as_slice()) {
            client
                .write_all_combo_definitions(combos)
                .await
                .map_err(host_error)?;
        }
    }
    if let Some(forks) = &desired.forks {
        let present = before.forks.as_deref().unwrap_or_default();
        let empty = rynk::rmk_types::fork::Fork::default();
        for index in 0..forks.len().max(present.len()) {
            let wanted = forks.get(index).unwrap_or(&empty);
            if present.get(index) == Some(wanted) {
                continue;
            }
            let slot = u8::try_from(index)
                .map_err(|_| Error::Model("more mod-morphs than the keyboard addresses".into()))?;
            client.set_fork(slot, *wanted).await.map_err(host_error)?;
        }
    }
    if let Some(macros) = &desired.macros {
        if before.macros.as_ref() != Some(macros) {
            write_macro_space(client, macros, macro_chunk).await?;
        }
    }
    Ok(())
}

async fn write_macro_space(client: &Client, space: &[u8], macro_chunk: u16) -> Result<(), Error> {
    let ceiling = rynk::rmk_types::constants::MACRO_DATA_SIZE;
    let chunk_size = usize::from(macro_chunk).clamp(1, ceiling);
    // One extra terminator so a shorter set does not leave the tail of a
    // longer one behind.
    let mut payload = space.to_vec();
    payload.push(0);
    for (index, chunk) in payload.chunks(chunk_size).enumerate() {
        let offset = u16::try_from(index * chunk_size).map_err(|_| {
            Error::Model("the macros are larger than the keyboard's macro space".into())
        })?;
        let data = rynk::rmk_types::protocol::rynk::MacroData {
            data: heapless::Vec::from_slice(chunk).map_err(|_| {
                Error::Model("a macro chunk exceeds the protocol's chunk size".into())
            })?,
        };
        client.set_macro(offset, data).await.map_err(host_error)?;
    }
    Ok(())
}

/// Contiguous runs of cells where `wanted` differs from `present`.
pub(crate) fn changed_runs(
    wanted: &[KeyAction],
    present: &[KeyAction],
) -> Vec<std::ops::Range<usize>> {
    if wanted.is_empty() {
        return Vec::new();
    }
    if present.len() != wanted.len() {
        return std::iter::once(0..wanted.len()).collect();
    }
    let mut runs: Vec<std::ops::Range<usize>> = Vec::new();
    for (index, (want, have)) in wanted.iter().zip(present).enumerate() {
        if want == have {
            continue;
        }
        match runs.last_mut() {
            Some(run) if run.end == index => run.end = index + 1,
            _ => runs.push(index..index + 1),
        }
    }
    runs
}

/// Runs split into pages of at most `batch` cells.
pub(crate) fn pages_of(
    runs: &[std::ops::Range<usize>],
    batch: usize,
) -> Vec<std::ops::Range<usize>> {
    let batch = batch.max(1);
    runs.iter()
        .flat_map(|run| {
            run.clone()
                .step_by(batch)
                .map(move |start| start..(start + batch).min(run.end))
        })
        .collect()
}

/// Runs one write, sending it again while the firmware answers `Busy`.
async fn write_until_accepted(
    mut write: impl AsyncFnMut() -> Result<(), RynkHostError>,
) -> Result<(), Error> {
    let started = std::time::Instant::now();
    loop {
        match write().await {
            Err(RynkHostError::Rejected(RynkError::Busy))
                if started.elapsed() < PERSIST_BUSY_LIMIT =>
            {
                tokio::time::sleep(PERSIST_BUSY_RETRY_DELAY).await;
            }
            result => return result.map_err(host_error),
        }
    }
}

/// Waits until every cell queued so far has reached flash: a layer
/// metadata read is served through the same queue as the keymap writes,
/// so it answers only once they are done.
async fn persist_barrier(
    client: &Client,
    layer: u8,
    barrier_works: &mut bool,
) -> Result<(), Error> {
    if !*barrier_works {
        return Ok(());
    }
    match client.get_layer_metadata(layer).await {
        Ok(_) => Ok(()),
        Err(error) if unsupported(&error) => {
            *barrier_works = false;
            Ok(())
        }
        Err(error) => Err(host_error(error)),
    }
}

async fn write_layer(
    client: &Client,
    capabilities: &DeviceCapabilities,
    layer: u8,
    wanted: &[KeyAction],
    present: &[KeyAction],
    barrier_works: &mut bool,
) -> Result<usize, Error> {
    let cols = usize::from(capabilities.num_cols).max(1);
    let batch = if capabilities.bulk_transfer_supported {
        PERSIST_BATCH.min(usize::from(capabilities.max_bulk_keys).max(1))
    } else {
        PERSIST_BATCH
    };
    let pages = pages_of(&changed_runs(wanted, present), batch);
    let mut cells = 0;
    for page in pages {
        let offset = page.start;
        let slice = &wanted[page.clone()];
        if capabilities.bulk_transfer_supported {
            let request = SetKeymapBulkRequest {
                layer,
                start_row: u8::try_from(offset / cols).unwrap_or(u8::MAX),
                start_col: u8::try_from(offset % cols).unwrap_or(u8::MAX),
                actions: slice.to_vec(),
            };
            write_until_accepted(async || client.set_keymap_bulk(request.clone()).await).await?;
        } else {
            for (index, action) in slice.iter().copied().enumerate() {
                let flat = offset + index;
                let row = u8::try_from(flat / cols).unwrap_or(u8::MAX);
                let col = u8::try_from(flat % cols).unwrap_or(u8::MAX);
                write_until_accepted(async || client.set_key(layer, row, col, action).await)
                    .await?;
            }
        }
        cells += page.len();
        persist_barrier(client, layer, barrier_works).await?;
    }
    Ok(cells)
}

fn require_conditions(
    lighting: &LightingSnapshot,
    features: LightingFeatureFlags,
    rule_status: Option<&LightingRuleStatus>,
) -> Result<(), Error> {
    let cells = lighting.conditional_scenes.as_deref().unwrap_or_default();
    if let Some(status) = rule_status {
        if let Some((index, missing)) = cells.iter().enumerate().find_map(|(index, cell)| {
            let missing = conditional_scene_predicate_mask(cell) & !status.predicates;
            (missing != 0).then_some((index, missing))
        }) {
            return Err(Error::Model(format!(
                "lighting rule {index} needs a condition (tag {}) this keyboard's firmware does not know",
                missing.trailing_zeros()
            )));
        }
        return Ok(());
    }
    if cells
        .iter()
        .any(|c| c.maintenance.is_some() || c.split_transport.is_some())
    {
        return Err(Error::Model(
            "the lighting rules use conditions this keyboard's firmware predates".into(),
        ));
    }
    if !features.contains(LightingFeatureFlags::RUNTIME_LAYER_INDICATOR_CONDITIONS)
        && cells
            .iter()
            .any(|c| c.layers.is_some() || c.indicators.is_some())
    {
        return Err(Error::Model(
            "lock lights need conditional lighting this keyboard's firmware does not advertise"
                .into(),
        ));
    }
    Ok(())
}

async fn apply_lighting(
    client: &Client,
    wanted: &LightingSnapshot,
    present: &LightingSnapshot,
) -> Result<(), Error> {
    let features = client
        .get_lighting_capabilities()
        .await
        .map_err(host_error)?
        .features;
    let rule_status = if features.contains(LightingFeatureFlags::RULES) {
        Some(
            client
                .get_lighting_rule_status()
                .await
                .map_err(host_error)?,
        )
    } else {
        None
    };
    require_conditions(wanted, features, rule_status.as_ref())?;
    if wanted.output_mode != present.output_mode {
        let revision = client
            .get_lighting_state()
            .await
            .map_err(host_error)?
            .revision;
        client
            .set_lighting_output_mode(SetLightingOutputModeRequest {
                expected_revision: revision,
                mode: output_mode_to_wire(wanted.output_mode),
            })
            .await
            .map_err(host_error)?;
    }
    if wanted.wake_layers != present.wake_layers {
        let layers = wanted
            .wake_layers
            .iter()
            .fold(0u64, |mask, layer| mask | (1u64 << layer));
        let revision = client
            .get_lighting_state()
            .await
            .map_err(host_error)?
            .revision;
        client
            .set_lighting_wake_layers(SetLightingWakeLayersRequest {
                expected_revision: revision,
                layers,
            })
            .await
            .map_err(host_error)?;
    }
    if wanted.brightness != present.brightness || wanted.background != present.background {
        let state = client.get_lighting_state().await.map_err(host_error)?;
        client
            .set_lighting_state(SetLightingStateRequest {
                expected_revision: state.revision,
                state: LightingMutableState {
                    output_enabled: state.output_enabled,
                    output_brightness: wanted.brightness,
                    background: background_to_wire(&wanted.background),
                },
            })
            .await
            .map_err(host_error)?;
    }
    let selection_differs = wanted.effects.as_ref().map(EffectsConfig::selection)
        != present.effects.as_ref().map(EffectsConfig::selection);
    if selection_differs {
        let effects = wanted.effects.as_ref().ok_or_else(|| {
            Error::Model("the keyboard's animation cannot be removed, only changed".into())
        })?;
        let effect_names = read_extension_names(client, LightingExtensionNameKind::Effects).await?;
        let palette_names =
            read_extension_names(client, LightingExtensionNameKind::Palettes).await?;
        let (state, overlay) =
            effects_to_wire(effects, &effect_names, &palette_names).map_err(model)?;
        let revision = client
            .get_lighting_state()
            .await
            .map_err(host_error)?
            .revision;
        client
            .set_lighting_extension_state(SetLightingExtensionStateRequest {
                expected_revision: revision,
                state,
            })
            .await
            .map_err(host_error)?;
        let had_overlay = present
            .effects
            .as_ref()
            .is_some_and(|e| e.overlay.is_some());
        if effects.overlay.is_some() || had_overlay {
            let revision = client
                .get_lighting_state()
                .await
                .map_err(host_error)?
                .revision;
            client
                .set_lighting_extension_layers(SetLightingExtensionLayersRequest {
                    expected_revision: revision,
                    overlay,
                })
                .await
                .map_err(host_error)?;
        }
    }
    if let Some(effects) = wanted.effects.as_ref().filter(|e| !e.params.is_empty()) {
        for write in params_to_writes(&effects.params, present.params.as_deref()).map_err(model)? {
            if write.value == write.current {
                continue;
            }
            let revision = client
                .get_lighting_state()
                .await
                .map_err(host_error)?
                .revision;
            client
                .set_lighting_extension_param(SetLightingExtensionParamRequest {
                    expected_revision: revision,
                    effect: write.effect,
                    index: write.index,
                    value: write.value,
                })
                .await
                .map_err(host_error)?;
        }
    }
    if wanted.scene_policy != present.scene_policy {
        let status = client
            .get_lighting_scene_status()
            .await
            .map_err(host_error)?;
        client
            .set_lighting_layer_policy(SetLightingLayerPolicyRequest {
                expected_revision: status.revision,
                policy: scene_policy_to_wire(wanted.scene_policy),
            })
            .await
            .map_err(host_error)?;
    }
    if let Some(rules) = &wanted.conditional_scenes {
        match &present.conditional_scenes {
            None if rules.is_empty() => {}
            None => {
                return Err(Error::Model(
                    "the layout has lock or battery lights, and this keyboard's firmware keeps no lighting rules".into(),
                ));
            }
            Some(live) if live == rules => {}
            Some(_) => {
                if features.contains(LightingFeatureFlags::RULES) {
                    let status = client
                        .get_lighting_rule_status()
                        .await
                        .map_err(host_error)?;
                    let wire = rules
                        .iter()
                        .map(conditional_scene_to_rule)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(model)?;
                    client
                        .replace_all_lighting_rules(status.revision, &wire)
                        .await
                        .map_err(host_error)?;
                } else if features
                    .contains(LightingFeatureFlags::RUNTIME_LAYER_INDICATOR_CONDITIONS)
                {
                    let status = client
                        .get_lighting_advanced_runtime_conditional_scene_status()
                        .await
                        .map_err(host_error)?;
                    let cells = rules
                        .iter()
                        .map(conditional_scene_to_advanced_wire)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(model)?;
                    client
                        .replace_all_lighting_advanced_runtime_conditional_scenes(
                            status.revision,
                            &cells,
                        )
                        .await
                        .map_err(host_error)?;
                } else if features.contains(LightingFeatureFlags::RUNTIME_EFFECTS_CONDITIONS) {
                    let status = client
                        .get_lighting_extended_runtime_conditional_scene_status()
                        .await
                        .map_err(host_error)?;
                    let cells = rules
                        .iter()
                        .map(conditional_scene_to_wire)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(model)?;
                    client
                        .replace_all_lighting_extended_runtime_conditional_scenes(
                            status.revision,
                            &cells,
                        )
                        .await
                        .map_err(host_error)?;
                } else {
                    let status = client
                        .get_lighting_runtime_conditional_scene_status()
                        .await
                        .map_err(host_error)?;
                    if rules
                        .iter()
                        .any(|c| c.connection.is_some() || c.effects.is_some())
                    {
                        return Err(Error::Model(
                            "a lighting rule uses a condition this keyboard's firmware predates"
                                .into(),
                        ));
                    }
                    let legacy = rules
                        .iter()
                        .map(|cell| conditional_scene_to_wire(cell).map(|c| c.cell))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(model)?;
                    client
                        .replace_all_lighting_runtime_conditional_scenes(status.revision, &legacy)
                        .await
                        .map_err(host_error)?;
                }
            }
        }
    }
    if wanted.scenes != present.scenes {
        let state = client.get_lighting_state().await.map_err(host_error)?;
        let cells = wanted
            .scenes
            .iter()
            .map(scene_to_wire)
            .collect::<Result<Vec<_>, _>>()
            .map_err(model)?;
        client
            .replace_all_lighting_scenes(state.revision, &cells)
            .await
            .map_err(host_error)?;
    }
    Ok(())
}

/// Writes what differs between `desired` and `before`, which must be what
/// the keyboard holds now. Returns how many keymap cells were written.
pub(crate) async fn apply_snapshot(
    client: &Client,
    capabilities: &DeviceCapabilities,
    desired: &Snapshot,
    before: &Snapshot,
) -> Result<usize, Error> {
    if desired.rows != capabilities.num_rows || desired.cols != capabilities.num_cols {
        return Err(Error::Matrix {
            rows: capabilities.num_rows,
            cols: capabilities.num_cols,
            expected_rows: desired.rows,
            expected_cols: desired.cols,
        });
    }
    if desired.layers.len() > usize::from(capabilities.num_layers) {
        return Err(Error::Model(format!(
            "the layout has {} layers and the keyboard holds {}",
            desired.layers.len(),
            capabilities.num_layers
        )));
    }
    if let Some(name) = &desired.bluetooth_name {
        if before.bluetooth_name.as_ref() != Some(name) {
            let template = heapless::String::try_from(name.as_str()).map_err(|_| {
                Error::Model("the keyboard name is longer than the firmware allows".into())
            })?;
            client
                .set_ble_name(&BleName { template })
                .await
                .map_err(host_error)?;
        }
    }
    // Tables first: a key that points into one needs it in place.
    apply_behaviors(
        client,
        &desired.behaviors,
        &before.behaviors,
        capabilities.macro_chunk_size,
    )
    .await?;

    let mut barrier_works = true;
    let mut cells = 0;
    for (index, wanted) in desired.layers.iter().enumerate() {
        let layer = u8::try_from(index).unwrap_or(u8::MAX);
        let present = before.layers.get(index).map_or(&[][..], Vec::as_slice);
        if wanted == present {
            continue;
        }
        cells += write_layer(
            client,
            capabilities,
            layer,
            wanted,
            present,
            &mut barrier_works,
        )
        .await?;
    }
    if desired.default_layer != before.default_layer {
        client
            .set_default_layer(desired.default_layer)
            .await
            .map_err(host_error)?;
    }
    if let Some(wanted) = &desired.layer_names {
        for (index, metadata) in wanted.iter().enumerate() {
            let present = before
                .layer_names
                .as_ref()
                .and_then(|names| names.get(index));
            if present == Some(metadata) {
                continue;
            }
            let layer = u8::try_from(index).unwrap_or(u8::MAX);
            client
                .set_layer_metadata(layer, metadata.clone())
                .await
                .map_err(host_error)?;
        }
    }
    if let Some(wanted) = desired.pointing {
        let differs = before.pointing.as_ref().is_none_or(|present| {
            wanted.devices() != present.devices() || wanted.overrides() != present.overrides()
        });
        if differs {
            let mut next = wanted;
            next.revision = before.pointing.map_or(0, |p| p.revision);
            client.set_pointing_config(next).await.map_err(host_error)?;
        }
    }
    if let Some(wanted) = &desired.lighting {
        let present = before
            .lighting
            .as_ref()
            .ok_or_else(|| Error::Model("the keyboard has no lighting to configure".into()))?;
        apply_lighting(client, wanted, present).await?;
    }
    Ok(cells)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rynk::rmk_types::action::Action;
    use rynk::rmk_types::keycode::{HidKeyCode, KeyCode};

    fn key(code: u8) -> KeyAction {
        KeyAction::Single(Action::Key(KeyCode::Hid(HidKeyCode::from(code))))
    }

    #[test]
    fn only_changed_cells_are_written_in_small_pages() {
        let present: Vec<KeyAction> = (4..10).map(key).collect();
        let mut wanted = present.clone();
        wanted[1] = key(20);
        wanted[2] = key(21);
        wanted[5] = key(22);
        assert_eq!(changed_runs(&wanted, &present), vec![1..3, 5..6]);
        assert!(changed_runs(&present, &present).is_empty());
        // A layer the keyboard could not report is written whole.
        assert_eq!(changed_runs(&wanted, &[]), vec![0..6]);
        assert_eq!(
            pages_of(&[0..10, 12..13], 4),
            vec![0..4, 4..8, 8..10, 12..13]
        );
        let one = 0..3;
        assert_eq!(
            pages_of(std::slice::from_ref(&one), 0),
            vec![0..1, 1..2, 2..3]
        );
    }
}
