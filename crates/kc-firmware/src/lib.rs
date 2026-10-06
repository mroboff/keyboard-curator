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

use kc_boards::Board;
use kc_model::{FirmwareConfig, Problem, Project, Severity};

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
    let problems = kc_model::validate(project, board, config);
    match config.family(board) {
        Family::Zmk | Family::Rmk | Family::Dygma => problems,
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
