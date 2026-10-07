//! One front door to the firmware families.
//!
//! A family is one way of putting a layout on a keyboard: ZMK and RMK
//! generate files that are built into firmware and flashed; Dygma's
//! firmware is configured live, with no build; moergo-rmk, within the RMK
//! family, is taken ready-made from its releases and then configured
//! live. The app asks here what a board's firmware can check, generate,
//! read, apply and fetch, and never branches on the family itself.
//! Firmwares within a family differ only by data, in the board
//! definitions.

pub use kc_boards::{Delivery, Family};
pub use kc_emit::GeneratedFile;

use kc_boards::board::{DygmaProfile, FirmwareProfile, Release, RmkProfile};
use kc_boards::Board;
use kc_model::{Binding, FirmwareConfig, Problem, Project, Severity};
use kc_zmk::settings::{Setting, SettingKind};

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
        Family::Zmk => {}
        Family::Rmk => {
            problems.retain(|p| p.severity == Severity::Error);
            if let Some(profile) = board.profile(&config.profile) {
                problems.extend(kc_rmk::check(project, board, profile, config));
            }
        }
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

fn rmk<'a>(board: &'a Board, config: &FirmwareConfig) -> Option<&'a RmkProfile> {
    board.profile(&config.profile)?.rmk.as_ref()
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
        Family::Zmk => true,
        Family::Rmk => {
            rmk(board, config).is_none_or(|rmk| kc_rmk::expressible(binding, project, rmk))
        }
        Family::Dygma => kc_dygma::expressible(binding, project),
    }
}

/// The settings a board's firmware offers as typed controls: ZMK's Kconfig
/// options, or RMK's runtime settings. Dygma has none.
pub fn settings(board: &Board, config: &FirmwareConfig) -> &'static [Setting] {
    match config.family(board) {
        Family::Zmk => kc_zmk::settings::SETTINGS,
        Family::Rmk if config.delivery(board) == Delivery::Released => kc_rmk::settings::SETTINGS,
        Family::Rmk | Family::Dygma => &[],
    }
}

/// The setting that names the keyboard, for the firmware that has one.
pub fn name_setting(board: &Board, config: &FirmwareConfig) -> Option<&'static str> {
    settings(board, config)
        .iter()
        .find(|s| matches!(s.kind, SettingKind::Text { .. }))
        .map(|s| s.key)
}

/// The names a setting chooses from: fixed ones for a choice, the
/// firmware's own for a named setting, nothing for the rest.
pub fn setting_options(board: &Board, config: &FirmwareConfig, setting: &Setting) -> Vec<String> {
    match setting.kind {
        SettingKind::Choice { options } => options.iter().map(|o| (*o).to_string()).collect(),
        SettingKind::Named => rmk(board, config)
            .map(|rmk| kc_rmk::settings::options(rmk, setting))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Firmware taken ready-made from a project's releases: where the chosen
/// firmware's comes from, when it does.
pub fn release<'a>(board: &'a Board, config: &FirmwareConfig) -> Option<&'a Release> {
    rmk(board, config)?.release.as_ref()
}

/// Fetches the chosen firmware's released files and checks them against
/// the release's checksums and against the sources the board definition
/// pins. A GitHub token only lifts the rate limit.
pub fn fetch_release(
    board: &Board,
    config: &FirmwareConfig,
    token: Option<&str>,
) -> Result<kc_build::release::ReleasedFirmware, String> {
    let profile = board
        .profile(&config.profile)
        .ok_or("the board has no such firmware")?;
    let rmk = profile
        .rmk
        .as_ref()
        .ok_or("this firmware is not taken from releases")?;
    let release = rmk
        .release
        .as_ref()
        .ok_or("this firmware is not taken from releases")?;
    let fetched = kc_build::release::fetch(release, token).map_err(|e| e.to_string())?;
    kc_build::release::check_sources(&fetched.manifest, &rmk.source.revision, &rmk.rmk_revision)
        .map_err(|e| e.to_string())?;
    // Each file is for the half the board definition says, and sits
    // where the vendor's own firmware does.
    for file in &fetched.files {
        let side = release
            .files
            .iter()
            .find(|f| f.name == file.name)
            .map(|f| f.side)
            .ok_or_else(|| format!("{} is not a file this board expects", file.name))?;
        let info =
            kc_flash::check(board, side, &file.bytes).map_err(|e| format!("{}: {e}", file.name))?;
        let (start, end) = (
            fetched.manifest.application_range.start(),
            fetched.manifest.application_range.end(),
        );
        if start.is_some_and(|s| info.address_start < s)
            || end.is_some_and(|e| info.address_end > e)
        {
            return Err(format!(
                "{} would be written outside the firmware's own range of flash ({:#x}-{:#x})",
                file.name, info.address_start, info.address_end
            ));
        }
    }
    Ok(fetched)
}

/// Firmware configured on the running keyboard: reading a layout from it
/// and writing one to it.
pub mod live {
    use std::path::{Path, PathBuf};

    use kc_boards::board::Side;
    use kc_boards::Board;
    use kc_model::{Carried, Device, FirmwareConfig, Project};

    use super::{dygma, Delivery, Family};

    /// What applying a layout did.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Applied {
        /// Whether anything had to be written.
        pub changed: bool,
        /// Where the keyboard's earlier configuration was saved, when
        /// something was written.
        pub backup: Option<PathBuf>,
        /// What changed, when the firmware says.
        pub changes: Vec<String>,
    }

    /// A layout read from a keyboard, with the firmware settings that
    /// belong to the board and what could not be kept.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Read {
        pub project: Project,
        pub carried: Carried,
        pub notes: Vec<String>,
    }

    fn not_live(board: &Board, config: &FirmwareConfig) -> String {
        format!(
            "{} firmware is built and flashed, not configured on the keyboard",
            config.family(board).name()
        )
    }

    fn stamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    /// Reads the connected keyboard's configuration as a new layout.
    /// Nothing is written to the keyboard.
    pub fn read(
        board: &Board,
        config: &FirmwareConfig,
        device: Option<&Device>,
    ) -> Result<Read, String> {
        let name = format!("{} Layout", board.name);
        match (config.family(board), config.delivery(board)) {
            (Family::Dygma, _) => {
                let profile = dygma(board, config).ok_or_else(|| not_live(board, config))?;
                let image = kc_dygma::read(board, device).map_err(|e| e.to_string())?;
                let project = image
                    .to_project(&name, board, profile)
                    .map_err(|e| e.to_string())?;
                Ok(Read {
                    project,
                    carried: Carried::default(),
                    notes: Vec::new(),
                })
            }
            (Family::Rmk, Delivery::Released) => {
                let profile = board
                    .profile(&config.profile)
                    .ok_or_else(|| not_live(board, config))?;
                let reading =
                    kc_rynk::read(board, profile, device, &name).map_err(|e| e.to_string())?;
                let imported = reading.imported;
                Ok(Read {
                    project: imported.project,
                    carried: imported.carried,
                    notes: imported.notes,
                })
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
        match (config.family(board), config.delivery(board)) {
            (Family::Dygma, _) => {
                let profile = dygma(board, config).ok_or_else(|| not_live(board, config))?;
                let mut saved = None;
                let applied = kc_dygma::apply(project, board, profile, device, |before| {
                    std::fs::create_dir_all(backups)?;
                    let path = backups.join(format!(
                        "{}-{}-{}.json",
                        board.id,
                        before.chip_id.trim(),
                        stamp()
                    ));
                    std::fs::write(&path, before.to_json())?;
                    saved = Some(path);
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
                Ok(Applied {
                    changed: !applied.sent.is_empty(),
                    backup: saved,
                    changes: applied.sent.iter().map(|s| (*s).to_string()).collect(),
                })
            }
            (Family::Rmk, Delivery::Released) => {
                let profile = board
                    .profile(&config.profile)
                    .ok_or_else(|| not_live(board, config))?;
                let mut saved = None;
                let serial = device.map_or("keyboard", |d| d.serial.as_str());
                let applied = kc_rynk::apply(project, board, profile, config, device, |held| {
                    std::fs::create_dir_all(backups)?;
                    let path = backups.join(format!("{}-{}-{}.toml", board.id, serial, stamp()));
                    std::fs::write(&path, held)?;
                    saved = Some(path);
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
                Ok(Applied {
                    changed: !applied.changes.is_empty(),
                    backup: saved.filter(|_| !applied.changes.is_empty()),
                    changes: applied.changes,
                })
            }
            _ => Err(not_live(board, config)),
        }
    }

    /// Whether the firmware can restart a half into its bootloader on
    /// request, and erase its stored settings.
    pub fn can_control(board: &Board, config: &FirmwareConfig) -> bool {
        config.family(board) == Family::Rmk && config.delivery(board) == Delivery::Released
    }

    /// Restarts one half of the connected keyboard into its bootloader,
    /// for flashing.
    pub fn enter_bootloader(
        board: &Board,
        config: &FirmwareConfig,
        device: Option<&Device>,
        side: Side,
    ) -> Result<(), String> {
        if !can_control(board, config) {
            return Err("this firmware cannot be asked to restart into its bootloader".into());
        }
        kc_rynk::enter_bootloader(board, device, side).map_err(|e| e.to_string())
    }

    /// Erases everything the connected keyboard stores and restarts it on
    /// its firmware's defaults.
    pub fn reset_settings(
        board: &Board,
        config: &FirmwareConfig,
        device: Option<&Device>,
    ) -> Result<(), String> {
        if !can_control(board, config) {
            return Err("this firmware cannot be asked to erase its settings".into());
        }
        kc_rynk::reset_settings(board, device).map_err(|e| e.to_string())
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
/// firmware configuration, for families that build; for firmware
/// configured live from a file, that file.
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
        Family::Rmk => {
            let problems = errors(project, board, config);
            if !problems.is_empty() {
                return Err(FirmwareError::Invalid(problems));
            }
            let profile: &FirmwareProfile = board
                .profile(&config.profile)
                .ok_or_else(|| FirmwareError::Other("the board has no such firmware".into()))?;
            let files =
                kc_rmk::generate(project, board, profile, config).map_err(FirmwareError::Other)?;
            Ok(files
                .into_iter()
                .map(|(path, contents)| GeneratedFile { path, contents })
                .collect())
        }
        Family::Dygma => Err(FirmwareError::NoBuild(family.name())),
    }
}
