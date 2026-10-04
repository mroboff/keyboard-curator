//! UF2 bootloader volume detection and copying, with per-half sequencing for split keyboards.
//!
//! A keyboard half in its bootloader appears as a small USB drive holding
//! an `INFO_UF2.TXT` file. Flashing is copying a `.uf2` file onto it; the
//! half then restarts and the drive disappears.

pub mod uf2;

use std::path::{Path, PathBuf};

use kc_boards::board::Side;
use kc_boards::Board;

pub use uf2::{Uf2Error, Uf2Info};

/// Where macOS mounts removable drives.
pub const VOLUMES: &str = "/Volumes";

/// A mounted UF2 bootloader drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bootloader {
    pub path: PathBuf,
    /// The volume name, such as `GO60LHBOOT`.
    pub name: String,
    /// `Model` and `Board-ID` from `INFO_UF2.TXT`, when present.
    pub model: Option<String>,
    pub board_id: Option<String>,
}

fn info_field(info: &str, field: &str) -> Option<String> {
    info.lines()
        .find_map(|line| line.strip_prefix(field)?.strip_prefix(':'))
        .map(|value| value.trim().to_string())
}

/// Every UF2 bootloader drive mounted under `volumes` (normally
/// [`VOLUMES`]), in name order.
pub fn scan(volumes: &Path) -> Vec<Bootloader> {
    let Ok(entries) = std::fs::read_dir(volumes) else {
        return Vec::new();
    };
    let mut found: Vec<Bootloader> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let info = std::fs::read_to_string(path.join("INFO_UF2.TXT")).ok()?;
            Some(Bootloader {
                name: entry.file_name().to_string_lossy().into_owned(),
                model: info_field(&info, "Model"),
                board_id: info_field(&info, "Board-ID"),
                path,
            })
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// What is known about which half a bootloader drive belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    /// The volume name belongs to exactly one half.
    Known(Side),
    /// The volume belongs to this board, but both halves use the same
    /// name, so the user has to say which one is plugged in.
    Either,
    /// Not one of this board's bootloaders.
    NotThisBoard,
}

pub fn identify(board: &Board, bootloader: &Bootloader) -> Half {
    let matches: Vec<Side> = board
        .halves
        .iter()
        .filter(|h| h.bootloader_volume == bootloader.name)
        .map(|h| h.side)
        .collect();
    match matches.as_slice() {
        [] => Half::NotThisBoard,
        [side] => Half::Known(*side),
        _ => Half::Either,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FlashError {
    #[error("{0}")]
    Firmware(#[from] Uf2Error),
    #[error("this firmware is for the other half of the keyboard")]
    WrongHalf,
    #[error("could not read the firmware file: {0}")]
    Read(std::io::Error),
    #[error("could not copy the firmware to the keyboard: {0}")]
    Write(std::io::Error),
}

/// Checks that `firmware` is a UF2 file the `side` half of `board` accepts.
pub fn check(board: &Board, side: Side, firmware: &[u8]) -> Result<Uf2Info, FlashError> {
    let info = uf2::inspect(firmware)?;
    let expected = board.half(side).and_then(|h| h.uf2_family);
    // A combined file carries both halves; each bootloader takes its own.
    match expected {
        Some(family) if !info.families.is_empty() && !info.families.contains(&family) => {
            Err(FlashError::WrongHalf)
        }
        _ => Ok(info),
    }
}

/// Copies firmware onto a bootloader drive.
///
/// The half restarts as soon as it has the last block, which can pull the
/// drive out from under the copy. So a write error counts as success when
/// the drive has gone: that is what a completed flash looks like.
pub fn flash(bootloader: &Bootloader, firmware: &[u8]) -> Result<(), FlashError> {
    let target = bootloader.path.join("firmware.uf2");
    match std::fs::write(&target, firmware) {
        Ok(()) => Ok(()),
        Err(_) if !bootloader.path.join("INFO_UF2.TXT").exists() => Ok(()),
        Err(error) => Err(FlashError::Write(error)),
    }
}

/// One half to flash, in the order the vendor recommends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub side: Side,
    /// The volume to wait for.
    pub volume: String,
}

/// The halves of `board` in flashing order.
pub fn plan(board: &Board) -> Vec<Step> {
    board
        .flash
        .order
        .iter()
        .filter_map(|side| {
            Some(Step {
                side: *side,
                volume: board.half(*side)?.bootloader_volume.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(id: &str) -> Board {
        kc_boards::built_in()
            .unwrap()
            .into_iter()
            .find(|b| b.id == id)
            .unwrap()
    }

    /// A scratch directory standing in for `/Volumes`.
    struct Volumes(PathBuf);

    impl Volumes {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("kc-flash-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn mount(&self, name: &str, info: Option<&str>) -> PathBuf {
            let path = self.0.join(name);
            std::fs::create_dir_all(&path).unwrap();
            if let Some(info) = info {
                std::fs::write(path.join("INFO_UF2.TXT"), info).unwrap();
            }
            path
        }
    }

    impl Drop for Volumes {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const INFO: &str = "UF2 Bootloader 0.6.0\nModel: MoErgo Go60 LH\nBoard-ID: nRF52840-go60-lh\n";

    #[test]
    fn only_uf2_bootloaders_are_found() {
        let volumes = Volumes::new("scan");
        volumes.mount("Macintosh HD", None);
        volumes.mount("GO60LHBOOT", Some(INFO));
        volumes.mount("ASSIMILATOR", Some("UF2 Bootloader\n"));
        let found = scan(&volumes.0);
        let names: Vec<&str> = found.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["ASSIMILATOR", "GO60LHBOOT"]);
        assert_eq!(found[1].model.as_deref(), Some("MoErgo Go60 LH"));
        assert_eq!(found[1].board_id.as_deref(), Some("nRF52840-go60-lh"));
        assert_eq!(found[0].model, None);
        assert!(scan(&volumes.0.join("missing")).is_empty());
    }

    #[test]
    fn halves_are_identified_by_volume_name_where_possible() {
        let volumes = Volumes::new("identify");
        volumes.mount("GO60RHBOOT", Some(INFO));
        volumes.mount("ASSIMILATOR", Some(INFO));
        let found = scan(&volumes.0);
        let (assimilator, go60_right) = (&found[0], &found[1]);
        let (go60, imprint) = (board("moergo-go60"), board("cyboard-imprint"));
        assert_eq!(identify(&go60, go60_right), Half::Known(Side::Right));
        assert_eq!(identify(&go60, assimilator), Half::NotThisBoard);
        // Both Imprint halves mount under the same name.
        assert_eq!(identify(&imprint, assimilator), Half::Either);
    }

    #[test]
    fn plans_follow_the_vendor_order() {
        let sides = |id: &str| plan(&board(id)).iter().map(|s| s.side).collect::<Vec<_>>();
        assert_eq!(sides("moergo-go60"), [Side::Right, Side::Left]);
        assert_eq!(sides("cyboard-imprint"), [Side::Left, Side::Right]);
        assert_eq!(plan(&board("moergo-go60"))[0].volume, "GO60RHBOOT");
    }

    #[test]
    fn firmware_for_the_wrong_half_is_caught() {
        let go60 = board("moergo-go60");
        let left = uf2::tests::image(&[0x9809_B007]);
        let both = uf2::tests::image(&[0x9809_B007, 0x980A_B007]);
        assert!(check(&go60, Side::Left, &left).is_ok());
        assert!(matches!(
            check(&go60, Side::Right, &left),
            Err(FlashError::WrongHalf)
        ));
        assert!(
            check(&go60, Side::Right, &both).is_ok(),
            "a combined file suits either half"
        );
        // The Imprint declares no family, so any valid UF2 is accepted.
        assert!(check(&board("cyboard-imprint"), Side::Right, &left).is_ok());
        assert!(matches!(
            check(&go60, Side::Left, b"not firmware"),
            Err(FlashError::Firmware(_))
        ));
    }

    #[test]
    fn flashing_copies_the_file_and_tolerates_the_drive_leaving() {
        let volumes = Volumes::new("flash");
        let path = volumes.mount("GO60LHBOOT", Some(INFO));
        let bootloader = scan(&volumes.0).remove(0);
        let firmware = uf2::tests::image(&[0x9809_B007]);
        flash(&bootloader, &firmware).unwrap();
        assert_eq!(std::fs::read(path.join("firmware.uf2")).unwrap(), firmware);

        // The half restarted mid-copy and took the drive with it.
        std::fs::remove_dir_all(&path).unwrap();
        assert!(flash(&bootloader, &firmware).is_ok());

        // A drive that is still there but refuses the write is a failure.
        let stuck = volumes.mount("STUCK", Some(INFO));
        std::fs::create_dir(stuck.join("firmware.uf2")).unwrap();
        let stuck = scan(&volumes.0).remove(0);
        assert!(matches!(
            flash(&stuck, &firmware),
            Err(FlashError::Write(_))
        ));
    }
}
