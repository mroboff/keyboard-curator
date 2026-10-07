//! Import of RMK runtime configuration files: moergo-rmk's TOML, as
//! Rynkbench and moergo-control read and write it and as Moosy Research
//! publishes TailorKey for RMK.
//!
//! The file is the keyboard's whole managed state, so the layout comes
//! with the board's settings apart, and with a note for each thing the
//! layout has no place for.

use kc_boards::Board;
use kc_model::{Carried, Project};
use kc_rmk::moergo_config::RuntimeConfig;

use crate::{ImportError, Report};

/// Imports a runtime configuration for `board`, named `name`.
pub fn import_rmk(
    name: &str,
    text: &str,
    board: &Board,
) -> Result<(Project, Carried, Report), ImportError> {
    let config =
        RuntimeConfig::from_toml(text).map_err(|e| ImportError::NotRmk(format!("{e:#}")))?;
    let profile = board
        .firmware
        .iter()
        .find(|p| p.rmk.as_ref().is_some_and(|r| r.matrix.is_some()))
        .ok_or_else(|| {
            ImportError::NotRmk(format!(
                "the {} has no firmware configured this way",
                board.name
            ))
        })?;
    let led_to_key = |led: u16| profile.key_of_led(board, led);
    let imported = kc_rmk::runtime::import(&config, board, profile, name, &led_to_key)
        .map_err(ImportError::NotRmk)?;
    let report = Report {
        layers: imported.project.layers.len(),
        behaviors: imported.project.behaviors.len(),
        combos: imported.project.combos.len(),
        raw_bindings: 0,
        raw_blocks: Vec::new(),
        notes: imported.notes,
    };
    Ok((imported.project, imported.carried, report))
}
