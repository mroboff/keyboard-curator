//! One front door to the firmware families.
//!
//! A family is one way of putting a layout on a keyboard: ZMK and RMK
//! generate files that are built into firmware and flashed; Dygma's
//! firmware is configured live, with no build. The app asks here what a
//! board's firmware can check and generate, and never branches on the
//! family itself. Firmwares within a family differ only by data, in the
//! board definitions.

pub use kc_boards::{Delivery, Family};
pub use kc_emit::GeneratedFile;

use kc_boards::board::DygmaProfile;
use kc_boards::Board;
use kc_model::{Binding, FirmwareConfig, Problem, Project, Severity};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FirmwareError {
    /// The layout has problems the firmware would reject. Nothing is
    /// generated until they are fixed.
    #[error("the layout has {} problem(s) that must be fixed first", .0.len())]
    Invalid(Vec<Problem>),
    #[error("{0} is configured on the keyboard itself, so there are no files to generate")]
    NoBuild(&'static str),
    #[error("{0}")]
    Other(String),
}

/// Every problem with building `project` for a board's firmware: those any
/// firmware would have, and those that come from what this family cannot
/// express.
pub fn check(project: &Project, board: &Board, config: &FirmwareConfig) -> Vec<Problem> {
    let mut problems = kc_model::validate(project, board, config);
    match config.family(board) {
        Family::Zmk | Family::Rmk => {}
        Family::Dygma => {
            // The general check's advice is about ZMK builds; only what is
            // wrong with the layout itself carries over.
            problems.retain(|p| p.severity == Severity::Error);
            if let Some(dygma) = dygma(board, config) {
                problems.extend(kc_dygma::check(project, dygma));
            }
        }
    }
    problems
}

fn dygma<'a>(board: &'a Board, config: &FirmwareConfig) -> Option<&'a DygmaProfile> {
    board.profile(&config.profile)?.dygma.as_ref()
}

/// Whether a binding can be used with a board's firmware at all, so that
/// the editor offers only what works.
pub fn expressible(
    binding: &Binding,
    project: &Project,
    board: &Board,
    config: &FirmwareConfig,
) -> bool {
    match config.family(board) {
        Family::Zmk | Family::Rmk => true,
        Family::Dygma => kc_dygma::expressible(binding, project),
    }
}

/// Firmware configured on the running keyboard: reading a layout from it
/// and writing one to it.
pub mod live {
    use std::path::{Path, PathBuf};

    use kc_boards::Board;
    use kc_model::{Device, FirmwareConfig, Project};

    use super::{dygma, Family};

    /// What applying a layout did.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Applied {
        /// Whether anything had to be written.
        pub changed: bool,
        /// Where the keyboard's earlier configuration was saved, when
        /// something was written.
        pub backup: Option<PathBuf>,
    }

    fn not_live(board: &Board, config: &FirmwareConfig) -> String {
        format!(
            "{} firmware is built and flashed, not configured on the keyboard",
            config.family(board).name()
        )
    }

    /// Reads the connected keyboard's configuration as a new layout.
    /// Nothing is written to the keyboard.
    pub fn read(
        board: &Board,
        config: &FirmwareConfig,
        device: Option<&Device>,
    ) -> Result<Project, String> {
        match (config.family(board), dygma(board, config)) {
            (Family::Dygma, Some(profile)) => {
                let image = kc_dygma::read(board, device).map_err(|e| e.to_string())?;
                image
                    .to_project(&format!("{} Layout", board.name), board, profile)
                    .map_err(|e| e.to_string())
            }
            _ => Err(not_live(board, config)),
        }
    }

    /// Writes a layout to the connected keyboard. What the keyboard held
    /// before is saved into `backups` first, and nothing is written unless
    /// that succeeds.
    pub fn apply(
        project: &Project,
        board: &Board,
        config: &FirmwareConfig,
        device: Option<&Device>,
        backups: &Path,
    ) -> Result<Applied, String> {
        match (config.family(board), dygma(board, config)) {
            (Family::Dygma, Some(profile)) => {
                let mut saved = None;
                let applied = kc_dygma::apply(project, board, profile, device, |before| {
                    std::fs::create_dir_all(backups)?;
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs());
                    let path = backups.join(format!(
                        "{}-{}-{stamp}.json",
                        board.id,
                        before.chip_id.trim()
                    ));
                    std::fs::write(&path, before.to_json())?;
                    saved = Some(path);
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
                Ok(Applied {
                    changed: !applied.sent.is_empty(),
                    backup: saved,
                })
            }
            _ => Err(not_live(board, config)),
        }
    }
}

/// The problems that stop a build or an apply.
pub fn errors(project: &Project, board: &Board, config: &FirmwareConfig) -> Vec<Problem> {
    check(project, board, config)
        .into_iter()
        .filter(|p| p.severity == Severity::Error)
        .collect()
}

/// The files of the firmware repository for a layout and a board's
/// firmware configuration, for families that build.
pub fn generate(
    project: &Project,
    board: &Board,
    config: &FirmwareConfig,
) -> Result<Vec<GeneratedFile>, FirmwareError> {
    let family = config.family(board);
    match family {
        Family::Zmk => kc_emit::generate(project, board, config).map_err(|error| match error {
            kc_emit::EmitError::Invalid(problems) => FirmwareError::Invalid(problems),
            other => FirmwareError::Other(other.to_string()),
        }),
        Family::Rmk => Err(FirmwareError::Other(
            "RMK firmware generation is not available yet".into(),
        )),
        Family::Dygma => Err(FirmwareError::NoBuild(family.name())),
    }
}
