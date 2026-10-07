//! RMK keyboards over Rynk, RMK's host protocol: what moergo-rmk's own
//! tools, moergo-control and Rynkbench, do, done from the app.
//!
//! A layout is written to the running keyboard; nothing is built. Reading
//! takes everything the firmware lets a host manage and makes a layout of
//! it, with the board's settings apart. Writing is strict: what the
//! keyboard holds is read and saved first, only what differs is written,
//! and the keyboard is read again afterward to see that it holds what was
//! sent. The protocol client is pinned to the same RMK commit as the
//! firmware the board definition offers, because Rynk can change in any
//! release.
//!
//! The commands that change anything on the keyboard are exactly those in
//! [`WRITES`]. Nothing here sends another.

mod apply;
mod read;
mod session;
pub mod transport;

use std::time::Duration;

use kc_boards::board::{FirmwareProfile, Side};
use kc_boards::Board;
use kc_model::{Device, FirmwareConfig, Project};
use kc_rmk::runtime::{self, Imported, Translation};
use moergo_config::{differences, RuntimeConfig};
use rynk::rmk_types::protocol::rynk::StorageResetMode;
use rynk::Client;

pub use session::Identity;

/// The Rynk commands this crate writes with, by the firmware's names for
/// them. Everything else it sends only reads.
pub const WRITES: &[&str] = &[
    "SetBleName",
    "SetBehavior",
    "SetBehaviorOptions",
    "SetMorseProfileEntry",
    "DeleteMorseProfile",
    "SetMorseProfileBulk",
    "SetMorseHoldTriggerPositions",
    "SetAutoMouseLayerConfigs",
    "SetMorseBulk",
    "SetComboDefinitionBulk",
    "SetFork",
    "SetMacro",
    "SetKeymapBulk",
    "SetKey",
    "SetDefaultLayer",
    "SetLayerMetadata",
    "SetPointingConfig",
    "SetLightingOutputMode",
    "SetLightingWakeLayers",
    "SetLightingState",
    "SetLightingExtensionState",
    "SetLightingExtensionLayers",
    "SetLightingExtensionParam",
    "SetLightingLayerPolicy",
    "BeginLightingRuleReplace",
    "PutLightingRuleChunk",
    "CommitLightingRuleReplace",
    "AbortLightingRuleReplace",
    "BeginLightingRuntimeConditionalSceneReplace",
    "PutLightingRuntimeConditionalSceneChunk",
    "CommitLightingRuntimeConditionalSceneReplace",
    "AbortLightingRuntimeConditionalSceneReplace",
    "BeginLightingSceneReplace",
    "PutLightingSceneChunk",
    "CommitLightingSceneReplace",
    "AbortLightingSceneReplace",
    "BootloaderJump",
    "PeripheralBootloaderJump",
    "StorageReset",
];

/// How long a whole read or write may take. A keymap write that lands on
/// a full flash page makes the firmware migrate the page, during which it
/// answers nothing for tens of seconds.
const READ_BUDGET: Duration = Duration::from_secs(120);
const APPLY_BUDGET: Duration = Duration::from_secs(600);
const RESET_BUDGET: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum RynkError {
    #[error("no {0} speaking Rynk was found on USB. Connect its left half by USB, and close Rynkbench or moergo-control if either is open: only one program can talk to the keyboard at a time")]
    NotFound(String),
    #[error("the keyboard's maintenance lock is on, so it refuses changes from a host. Unlock it from its Magic layer and try again")]
    Locked,
    #[error("the keyboard holds a {rows}×{cols} key matrix, and this board's firmware is described with {expected_rows}×{expected_cols}")]
    Matrix {
        rows: u8,
        cols: u8,
        expected_rows: u8,
        expected_cols: u8,
    },
    #[error("the keyboard stopped answering")]
    Disconnected,
    #[error("timed out {0}")]
    Timeout(&'static str),
    #[error("could not talk to the keyboard: {0}")]
    Transport(String),
    #[error("the keyboard answered in a way this app does not understand: {0}")]
    Protocol(String),
    #[error("{0}")]
    Model(String),
    #[error("could not save what the keyboard holds before writing: {0}")]
    Backup(std::io::Error),
    #[error("the keyboard does not hold what was written to it: {0}")]
    Verify(String),
}

fn rmk_profile(profile: &FirmwareProfile) -> Result<&kc_boards::board::RmkProfile, RynkError> {
    profile
        .rmk
        .as_ref()
        .filter(|rmk| rmk.matrix.is_some())
        .ok_or_else(|| RynkError::Model("this firmware is not configured over Rynk".into()))
}

/// Finds the keyboard and asks who it is. Nothing is written.
pub fn find(board: &Board, device: Option<&Device>) -> Result<Identity, RynkError> {
    session::run(board, device, READ_BUDGET, async |_, identity| {
        Ok(identity.clone())
    })
}

/// What reading a keyboard gave.
pub struct Reading {
    pub identity: Identity,
    /// The keyboard's configuration as a layout, with the board's settings
    /// and what could not be kept.
    pub imported: Imported,
    /// The configuration as moergo-rmk's own tools would write it.
    pub toml: String,
}

/// Reads the keyboard's configuration as a layout named `name`. Nothing
/// is written.
pub fn read(
    board: &Board,
    profile: &FirmwareProfile,
    device: Option<&Device>,
    name: &str,
) -> Result<Reading, RynkError> {
    let rmk = rmk_profile(profile)?;
    session::run(board, device, READ_BUDGET, async |client, identity| {
        check_matrix(identity, rmk)?;
        let reading = read::read_snapshot(client).await?;
        let config = RuntimeConfig::from_snapshot(&reading.snapshot, None);
        let toml = config
            .to_toml()
            .map_err(|e| RynkError::Model(format!("{e:#}")))?;
        let leds = reading.topology.as_ref().map(read::led_keys);
        let matrix = rmk.matrix.as_ref().expect("checked");
        let led_to_key = |led: u16| -> Option<usize> {
            match &leds {
                Some(leds) => {
                    let (row, col) = leds.get(&led)?;
                    matrix.key_at(*row, *col)
                }
                None => profile.key_of_led(board, led),
            }
        };
        let imported = runtime::import(&config, board, profile, name, &led_to_key)
            .map_err(RynkError::Model)?;
        Ok(Reading {
            identity: identity.clone(),
            imported,
            toml,
        })
    })
}

fn check_matrix(identity: &Identity, rmk: &kc_boards::board::RmkProfile) -> Result<(), RynkError> {
    let matrix = rmk.matrix.as_ref().expect("checked");
    if identity.rows != matrix.rows || identity.cols != matrix.cols {
        return Err(RynkError::Matrix {
            rows: identity.rows,
            cols: identity.cols,
            expected_rows: matrix.rows,
            expected_cols: matrix.cols,
        });
    }
    Ok(())
}

/// What applying a layout did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub identity: Identity,
    /// What differed before writing, in the firmware's own words. Empty
    /// when the keyboard already held the layout.
    pub changes: Vec<String>,
    /// How many keymap cells were written.
    pub cells: usize,
}

/// Writes a layout to the keyboard. `backup` is given what the keyboard
/// holds, as the TOML moergo-rmk's own tools read, before anything is
/// written; nothing is written unless it succeeds. Afterward the keyboard
/// is read again and must hold what was sent.
pub fn apply(
    project: &Project,
    board: &Board,
    profile: &FirmwareProfile,
    firmware: &FirmwareConfig,
    device: Option<&Device>,
    backup: impl FnOnce(&str) -> std::io::Result<()>,
) -> Result<Applied, RynkError> {
    let rmk = rmk_profile(profile)?;
    let translation: Translation =
        runtime::translate(project, board, profile, firmware).map_err(|problems| {
            RynkError::Model(format!(
                "the layout has {} problem(s) to fix first, such as: {}",
                problems.len(),
                problems.first().map_or("", |p| p.message.as_str())
            ))
        })?;
    session::run(board, device, APPLY_BUDGET, async |client, identity| {
        check_matrix(identity, rmk)?;
        if !identity.unlocked {
            return Err(RynkError::Locked);
        }
        let before = read::read_snapshot(client).await?;
        let held = RuntimeConfig::from_snapshot(&before.snapshot, None)
            .to_toml()
            .map_err(|e| RynkError::Model(format!("{e:#}")))?;
        backup(&held).map_err(RynkError::Backup)?;

        let mut desired = match &before.topology {
            Some(topology) => translation
                .config
                .snapshot_with_topology(topology)
                .map_err(|e| RynkError::Model(format!("{e:#}")))?,
            None => translation
                .config
                .snapshot()
                .map_err(|e| RynkError::Model(format!("{e:#}")))?,
        };
        runtime::settle(&mut desired, &before.snapshot, &translation.claims);
        let changes = differences(&desired, &before.snapshot);
        if changes.is_empty() {
            return Ok(Applied {
                identity: identity.clone(),
                changes,
                cells: 0,
            });
        }
        let cells =
            apply::apply_snapshot(client, &before.capabilities, &desired, &before.snapshot).await?;
        let after = read::read_snapshot(client).await?;
        let remaining = differences(&desired, &after.snapshot);
        if !remaining.is_empty() {
            return Err(RynkError::Verify(remaining.join("; ")));
        }
        Ok(Applied {
            identity: identity.clone(),
            changes,
            cells,
        })
    })
}

/// Restarts one half into its UF2 bootloader, for flashing. The left half
/// is asked directly; the right is asked through the left, over the split
/// link, so both must be on.
pub fn enter_bootloader(
    board: &Board,
    device: Option<&Device>,
    side: Side,
) -> Result<(), RynkError> {
    let central = board
        .halves
        .iter()
        .find(|h| h.central)
        .map(|h| h.side)
        .unwrap_or(Side::Left);
    session::run(board, device, RESET_BUDGET, async |client: &Client, _| {
        if side == central {
            // The half restarts as soon as it has the request and never
            // answers it: the link dying is the answer.
            match client.bootloader_jump().await {
                Ok(()) | Err(rynk::RynkHostError::Disconnected) => Ok(()),
                Err(error) => Err(session::host_error(error)),
            }
        } else {
            let status = client
                .get_peripheral_status(0)
                .await
                .map_err(session::host_error)?;
            if !status.connected {
                return Err(RynkError::Model(
                    "the other half is not connected to this one, so it cannot be asked to restart"
                        .into(),
                ));
            }
            client
                .peripheral_bootloader_jump(0)
                .await
                .map_err(session::host_error)?;
            // The half has left when the central says it is gone.
            for _ in 0..150 {
                let status = client
                    .get_peripheral_status(0)
                    .await
                    .map_err(session::host_error)?;
                if !status.connected {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(RynkError::Timeout("waiting for the other half to restart"))
        }
    })
}

/// Erases every setting the keyboard stores, including its layout and
/// Bluetooth pairings, and restarts it on the firmware's compiled
/// defaults. The clean slate for going back to another firmware.
pub fn reset_settings(board: &Board, device: Option<&Device>) -> Result<(), RynkError> {
    session::run(
        board,
        device,
        RESET_BUDGET,
        async |client: &Client, identity| {
            if !identity.unlocked {
                return Err(RynkError::Locked);
            }
            match client.storage_reset(StorageResetMode::Full).await {
                Ok(()) | Err(rynk::RynkHostError::Disconnected) => Ok(()),
                Err(error) => Err(session::host_error(error)),
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_write_list_is_fixed_and_has_no_repeats() {
        let mut sorted = WRITES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), WRITES.len());
        assert!(WRITES.contains(&"StorageReset"));
        assert!(!WRITES.iter().any(|w| w.starts_with("Get")));
    }
}
