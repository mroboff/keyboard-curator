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

use kc_boards::board::{DygmaProfile, FirmwareProfile, Release, RmkProfile, ViaProfile};
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
        Family::Via => {
            problems.retain(|p| p.severity == Severity::Error);
            if let Some(via) = via(board, config) {
                problems.extend(kc_via::layout::check(project, via, None));
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

fn via<'a>(board: &'a Board, config: &FirmwareConfig) -> Option<&'a ViaProfile> {
    board.profile(&config.profile)?.via.as_ref()
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
        Family::Via => {
            via(board, config).is_none_or(|via| kc_via::layout::expressible(binding, project, via))
        }
    }
}

/// The settings a board's firmware offers as typed controls: ZMK's Kconfig
/// options, RMK's runtime settings, or Dygma's Focus settings.
pub fn settings(board: &Board, config: &FirmwareConfig) -> &'static [Setting] {
    match config.family(board) {
        // Firmware nobody builds has no settings to build in.
        Family::Zmk if config.delivery(board) == Delivery::Studio => &[],
        Family::Zmk => kc_zmk::settings::SETTINGS,
        Family::Rmk if config.delivery(board) == Delivery::Released => kc_rmk::settings::SETTINGS,
        Family::Rmk => &[],
        Family::Dygma => kc_dygma::settings::SETTINGS,
        Family::Via => kc_via::settings::SETTINGS,
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
        let info = kc_flash::check(board, profile, side, &file.bytes)
            .map_err(|e| format!("{}: {e}", file.name))?;
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

    use kc_boards::board::ViaProfile;

    use super::{dygma, via, Delivery, Family};

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

    /// What the board page says about a keyboard configured live: the
    /// family's own words for reading, applying and restoring.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Words {
        /// What to know before anything else, when there is something.
        pub intro: Option<String>,
        pub read: String,
        pub apply: String,
        /// The question asked before writing `name`'s layout.
        pub apply_prompt: String,
        /// The restore panel's text, when backups can be restored.
        pub restore: Option<String>,
    }

    /// The words for `board` running `config`'s firmware, for the saved
    /// board called `name`, with backups kept in `backups`.
    pub fn words(board: &Board, config: &FirmwareConfig, name: &str, backups: &str) -> Words {
        match (config.family(board), config.delivery(board)) {
            (Family::Zmk, Delivery::Studio) => Words {
                intro: Some("This board was made from what the keyboard reported over ZMK Studio, so the app knows its keys but not its firmware. Its layers and keys are edited on the keyboard as it is. Combos, new behaviors, settings and lighting would need firmware built for it, which needs a board file. Connect the keyboard by USB and unlock Studio on it (the firmware's unlock key, or ZMK's unlock combination) before reading or applying.".into()),
                read: "Opens what the keyboard holds now as a new layout: every layer, with every key the app can name. Keys bound to behaviors the app does not know are left transparent. Nothing is written to the keyboard.".into(),
                apply: "Applying writes it to the keyboard at once and saves it there, so it survives a restart. Only keys that differ are written, and only ones the keyboard's firmware can take; the rest are reported. What the keyboard held is saved to disk first, as a layout file.".into(),
                apply_prompt: format!("This writes it to “{name}” over ZMK Studio at once: the layers and keys that differ. What the keyboard holds now is saved first, as a layout file in {backups}."),
                restore: None,
            },
            (Family::Via, _) => Words {
                intro: Some("This board was made from the keyboard's VIA definition. The keyboard's own QMK firmware stays; keys, layers and macros are written to it over VIA's protocol. Connect the keyboard by its cable (VIA does not work over Bluetooth or a wireless dongle) and close VIA; only one program can talk to the keyboard at a time.".into()),
                read: "Opens what the keyboard holds now as a new layout: every layer's keys, and the macros its keys use. Keys the app cannot name are kept as they are. The keyboard's lighting values are offered to the board as settings. Nothing is written to the keyboard.".into(),
                apply: "Applying writes it to the keyboard at once, with no build. Only what differs is written: keys, the macros they use, and the lighting settings. The keyboard's configuration is saved to disk first.".into(),
                apply_prompt: format!("This changes “{name}” at once: its keys on every layer, the macros they use, and the lighting settings that differ from the board's. Nothing else is touched. What the keyboard holds now is saved first, in {backups}."),
                restore: Some("Every write saves what the keyboard held first: its keymap, macros, layout options and lighting values. Restoring writes a chosen file back to the keyboard it came from, everything in it that differs. The keyboard is saved again first.".into()),
            },
            _ => Words {
                intro: None,
                read: "Opens what the keyboard holds now as a new layout: every layer's keys, superkeys as key setups, macros, the colors under the keys and the underglow. The keyboard's settings are offered to the board. Nothing is written to the keyboard. Close Dygma's Bazecor first; only one program can talk to the keyboard at a time.".into(),
                apply: "Applying writes it to the keyboard at once, with no build. Only what differs is written: keys and colors, the superkeys and macros they need, and settings. The keyboard's configuration is saved to disk first.".into(),
                apply_prompt: format!("This changes “{name}” at once: its keys and colors on every layer, the superkeys and macros they need, and the settings that differ from the board's. Nothing else is touched. What the keyboard holds now is saved first, in {backups}."),
                restore: Some("Every write saves what the keyboard held first, as one file holding everything the keyboard lists. Restoring writes a chosen file back to the keyboard it came from: everything in it that differs, except the lights and the wireless side, which are left as they are. The keyboard is saved again first.".into()),
            },
        }
    }

    /// The VIA interface of the board's keyboard: the linked one, or the
    /// first with the board's identifiers.
    fn open_via(
        board: &Board,
        device: Option<&Device>,
    ) -> Result<kc_via::Via<kc_via::transport::HidTransport>, String> {
        let ids = board
            .usb
            .first()
            .map(|id| (id.vendor, id.product))
            .ok_or("the board has no USB identifiers")?;
        kc_via::open(ids, device.map(|d| d.serial.as_str())).map_err(|e| e.to_string())
    }

    /// Everything a VIA keyboard of `board` holds that this app reads.
    fn read_via(
        board: &Board,
        profile: &ViaProfile,
        device: Option<&Device>,
    ) -> Result<(kc_via::Via<kc_via::transport::HidTransport>, kc_via::Image), String> {
        let mut via = open_via(board, device)?;
        let channels: Vec<&kc_via::Channel> = kc_via::CHANNELS
            .iter()
            .filter(|c| profile.lighting.iter().any(|[channel, _]| *channel == c.id))
            .collect();
        let matrix = kc_via::Matrix {
            rows: profile.rows,
            cols: profile.cols,
        };
        let image = via
            .read_image(matrix, &channels)
            .map_err(|e| e.to_string())?;
        Ok((via, image))
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
                    carried: image.carried(),
                    notes: image.battery_summary().into_iter().collect(),
                })
            }
            (Family::Zmk, Delivery::Studio) => {
                let project =
                    kc_studio::session::read_board(board, &name).map_err(|e| e.to_string())?;
                Ok(Read {
                    project,
                    carried: Carried::default(),
                    notes: Vec::new(),
                })
            }
            (Family::Via, _) => {
                let profile = via(board, config).ok_or_else(|| not_live(board, config))?;
                let (_, image) = read_via(board, profile, device)?;
                let project = kc_via::layout::to_project(&image, board, profile, &name)
                    .map_err(|e| e.to_string())?;
                Ok(Read {
                    project,
                    carried: image.carried(),
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
                let applied = kc_dygma::apply(project, board, profile, config, device, |before| {
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
                    changes: applied.sent,
                })
            }
            (Family::Via, _) => {
                let profile = via(board, config).ok_or_else(|| not_live(board, config))?;
                let (mut via, before) = read_via(board, profile, device)?;
                let new = kc_via::layout::with_project(&before, project, profile)
                    .map_err(|e| e.to_string())?
                    .with_settings(config);
                if new == before {
                    return Ok(Applied {
                        changed: false,
                        backup: None,
                        changes: Vec::new(),
                    });
                }
                std::fs::create_dir_all(backups).map_err(|e| e.to_string())?;
                let serial = device.map_or("keyboard", |d| d.serial.as_str());
                let path = backups.join(format!("{}-{}-{}.json", board.id, serial, stamp()));
                std::fs::write(&path, before.to_json()).map_err(|e| e.to_string())?;
                let changes = via
                    .write_changes(&before, &new)
                    .map_err(|e| e.to_string())?;
                Ok(Applied {
                    changed: !changes.is_empty(),
                    backup: Some(path),
                    changes,
                })
            }
            (Family::Zmk, Delivery::Studio) => {
                // What the keyboard holds is kept as a layout file first.
                let held = kc_studio::session::read_board(board, &format!("{} before", board.name))
                    .map_err(|e| e.to_string())?;
                std::fs::create_dir_all(backups).map_err(|e| e.to_string())?;
                let serial = device.map_or("keyboard", |d| d.serial.as_str());
                let path = backups.join(format!("{}-{}-{}.kcproj", board.id, serial, stamp()));
                std::fs::write(&path, kc_model::file::to_json(&held)).map_err(|e| e.to_string())?;
                let changed = kc_studio::session::send_board(project).map_err(|e| e.to_string())?;
                Ok(Applied {
                    changed: changed > 0,
                    backup: Some(path).filter(|_| changed > 0),
                    changes: (changed > 0)
                        .then(|| format!("{changed} keys"))
                        .into_iter()
                        .collect(),
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

    /// Whether a backup this app took can be written back from here.
    /// Rynk backups are files Rynkbench and moergo-control restore.
    pub fn can_restore(board: &Board, config: &FirmwareConfig) -> bool {
        matches!(config.family(board), Family::Dygma | Family::Via)
    }

    /// Writes a backup file this app took back to the keyboard it came
    /// from: everything that differs, except the lights and the wireless
    /// side. What the keyboard holds now is saved into `backups` first.
    pub fn restore(
        board: &Board,
        config: &FirmwareConfig,
        device: Option<&Device>,
        file: &Path,
        backups: &Path,
    ) -> Result<Applied, String> {
        if !can_restore(board, config) {
            return Err(
                "this firmware's backups are restored with Rynkbench or moergo-control".into(),
            );
        }
        let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        if config.family(board) == Family::Via {
            let profile = via(board, config).ok_or_else(|| not_live(board, config))?;
            let saved = kc_via::Image::from_json(&text).map_err(|e| e.to_string())?;
            let (mut via, before) = read_via(board, profile, device)?;
            if saved == before {
                return Ok(Applied {
                    changed: false,
                    backup: None,
                    changes: Vec::new(),
                });
            }
            std::fs::create_dir_all(backups).map_err(|e| e.to_string())?;
            let serial = device.map_or("keyboard", |d| d.serial.as_str());
            let path = backups.join(format!("{}-{}-{}.json", board.id, serial, stamp()));
            std::fs::write(&path, before.to_json()).map_err(|e| e.to_string())?;
            let changes = via
                .write_changes(&before, &saved)
                .map_err(|e| e.to_string())?;
            return Ok(Applied {
                changed: !changes.is_empty(),
                backup: Some(path),
                changes,
            });
        }
        let saved = kc_dygma::Image::from_json(&text).map_err(|e| e.to_string())?;
        let mut taken = None;
        let applied = kc_dygma::restore(board, device, &saved, |before| {
            std::fs::create_dir_all(backups)?;
            let path = backups.join(format!(
                "{}-{}-{}.json",
                board.id,
                before.chip_id.trim(),
                stamp()
            ));
            std::fs::write(&path, before.to_json())?;
            taken = Some(path);
            Ok(())
        })
        .map_err(|e| e.to_string())?;
        Ok(Applied {
            changed: !applied.sent.is_empty(),
            backup: taken,
            changes: applied.sent,
        })
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
        // Firmware the app never builds has no files.
        Family::Zmk if config.delivery(board) == Delivery::Studio => {
            Err(FirmwareError::NoBuild("the keyboard"))
        }
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
        Family::Dygma | Family::Via => Err(FirmwareError::NoBuild(family.name())),
    }
}
