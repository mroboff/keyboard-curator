//! UF2 bootloader volume detection and copying, with per-half sequencing for split keyboards.
//!
//! A keyboard half in its bootloader appears as a small USB drive holding
//! an `INFO_UF2.TXT` file. Flashing is copying a `.uf2` file onto it; the
//! half then restarts and the drive disappears.

pub mod uf2;

use std::path::{Path, PathBuf};

use kc_boards::board::{FirmwareProfile, Side};
use kc_boards::Board;

pub use uf2::{Uf2Error, Uf2Info};

/// The directories under which this computer mounts removable drives, each
/// holding one subdirectory per drive. macOS has one, `/Volumes`. Linux
/// has several, by distribution: `/run/media/<user>` (udisks on Fedora
/// and Arch), `/media/<user>` (Ubuntu and Debian), `/media` and `/mnt`.
pub fn volume_roots() -> Vec<PathBuf> {
    let mut roots = platform::volume_roots();
    roots.dedup();
    roots
}

/// Every UF2 bootloader drive mounted anywhere this computer puts them,
/// in name order: the drives under [`volume_roots`], and on Linux every
/// mounted filesystem too, for a drive mounted by hand somewhere else.
pub fn scan_all() -> Vec<Bootloader> {
    let mut found: Vec<Bootloader> = volume_roots().iter().flat_map(|root| scan(root)).collect();
    found.extend(
        platform::mount_points()
            .iter()
            .filter_map(|mount| bootloader_at(mount)),
    );
    found.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    found.dedup_by(|a, b| a.path == b.path);
    found
}

/// The mount points in a `/proc/mounts` (or `/etc/mtab`) listing: the
/// second field of each line, with the octal escapes the kernel writes for
/// spaces and other special characters (`\040`) decoded. Pseudo
/// filesystems without a device path are left out.
pub fn mount_points(mounts: &str) -> Vec<PathBuf> {
    mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let device = fields.next()?;
            let mount = fields.next()?;
            device
                .starts_with('/')
                .then(|| PathBuf::from(unescape_mount(mount)))
        })
        .collect()
}

fn unescape_mount(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let digits: String = chars.clone().take(3).collect();
        match u8::from_str_radix(&digits, 8) {
            Ok(byte) if digits.len() == 3 => {
                out.push(byte as char);
                for _ in 0..3 {
                    chars.next();
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::PathBuf;

    pub fn volume_roots() -> Vec<PathBuf> {
        vec![PathBuf::from("/Volumes")]
    }

    pub fn mount_points() -> Vec<PathBuf> {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::path::PathBuf;

    pub fn volume_roots() -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(user) = std::env::var_os("USER") {
            let user = user.to_string_lossy();
            roots.push(PathBuf::from(format!("/run/media/{user}")));
            roots.push(PathBuf::from(format!("/media/{user}")));
        }
        // Other users' drives, and a drive mounted before the user logged in.
        if let Ok(entries) = std::fs::read_dir("/run/media") {
            roots.extend(entries.flatten().map(|entry| entry.path()));
        }
        roots.push(PathBuf::from("/media"));
        roots.push(PathBuf::from("/mnt"));
        roots.sort();
        roots
    }

    pub fn mount_points() -> Vec<PathBuf> {
        std::fs::read_to_string("/proc/mounts")
            .map(|mounts| super::mount_points(&mounts))
            .unwrap_or_default()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use std::path::PathBuf;

    pub fn volume_roots() -> Vec<PathBuf> {
        Vec::new()
    }

    pub fn mount_points() -> Vec<PathBuf> {
        Vec::new()
    }
}

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

/// The bootloader drive mounted at `path`, if that is what is there.
fn bootloader_at(path: &Path) -> Option<Bootloader> {
    let info = std::fs::read_to_string(path.join("INFO_UF2.TXT")).ok()?;
    Some(Bootloader {
        name: path.file_name()?.to_string_lossy().into_owned(),
        model: info_field(&info, "Model"),
        board_id: info_field(&info, "Board-ID"),
        path: path.to_path_buf(),
    })
}

/// Every UF2 bootloader drive mounted under `volumes` (one of
/// [`volume_roots`]), in name order.
pub fn scan(volumes: &Path) -> Vec<Bootloader> {
    let Ok(entries) = std::fs::read_dir(volumes) else {
        return Vec::new();
    };
    let mut found: Vec<Bootloader> = entries
        .flatten()
        .filter_map(|entry| bootloader_at(&entry.path()))
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

pub fn identify(board: &Board, profile: &FirmwareProfile, bootloader: &Bootloader) -> Half {
    let matches: Vec<Side> = board
        .halves
        .iter()
        .filter(|h| {
            board
                .bootloader(profile, h.side)
                .is_some_and(|facts| facts.volumes.contains(&bootloader.name))
        })
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
pub fn check(
    board: &Board,
    profile: &FirmwareProfile,
    side: Side,
    firmware: &[u8],
) -> Result<Uf2Info, FlashError> {
    let info = uf2::inspect(firmware)?;
    let expected = board
        .bootloader(profile, side)
        .and_then(|facts| facts.uf2_family);
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
    /// The volume names to wait for; any one of them.
    pub volumes: Vec<String>,
}

/// The halves of `board` in flashing order, as flashed when running
/// `profile`.
pub fn plan(board: &Board, profile: &FirmwareProfile) -> Vec<Step> {
    board
        .flash
        .order
        .iter()
        .filter_map(|side| {
            Some(Step {
                side: *side,
                volumes: board.bootloader(profile, *side)?.volumes,
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
    fn mount_points_are_read_from_proc_mounts() {
        let mounts = "\
proc /proc proc rw,nosuid 0 0
/dev/nvme0n1p2 / ext4 rw,relatime 0 0
tmpfs /run tmpfs rw,nosuid,nodev 0 0
/dev/sda1 /run/media/mark/GO60LHBOOT vfat rw,nosuid,nodev,relatime,uid=1000 0 0
/dev/sdb1 /media/mark/My\\040Drive exfat rw 0 0
/dev/sdc1 /mnt/odd\\134name vfat rw 0 0
broken line
";
        let points = mount_points(mounts);
        assert_eq!(
            points,
            [
                PathBuf::from("/"),
                PathBuf::from("/run/media/mark/GO60LHBOOT"),
                PathBuf::from("/media/mark/My Drive"),
                PathBuf::from("/mnt/odd\\name"),
            ]
        );
        assert!(mount_points("").is_empty());
    }

    #[test]
    fn a_drive_mounted_anywhere_is_found_by_its_mount_point() {
        let volumes = Volumes::new("mount");
        let path = volumes.mount("ASSIMILATOR", Some(INFO));
        volumes.mount("Other", None);
        assert_eq!(
            bootloader_at(&path).map(|b| b.name).as_deref(),
            Some("ASSIMILATOR")
        );
        assert_eq!(bootloader_at(&volumes.0.join("Other")), None);
    }

    #[test]
    fn volume_roots_are_known_on_every_platform() {
        let roots = volume_roots();
        if cfg!(target_os = "macos") {
            assert_eq!(roots, [PathBuf::from("/Volumes")]);
        } else if cfg!(target_os = "linux") {
            assert!(roots.contains(&PathBuf::from("/media")));
            assert!(roots.contains(&PathBuf::from("/mnt")));
        }
        // No root is listed twice, and scanning them all does not fail.
        let mut unique = roots.clone();
        unique.dedup();
        assert_eq!(unique, roots);
        let _ = scan_all();
    }

    #[test]
    fn halves_are_identified_by_volume_name_where_possible() {
        let volumes = Volumes::new("identify");
        volumes.mount("GO60RHBOOT", Some(INFO));
        volumes.mount("ASSIMILATOR", Some(INFO));
        let found = scan(&volumes.0);
        let (assimilator, go60_right) = (&found[0], &found[1]);
        let (go60, imprint) = (board("moergo-go60"), board("cyboard-imprint"));
        let (stock, cyboard) = (&go60.firmware[0], &imprint.firmware[0]);
        assert_eq!(identify(&go60, stock, go60_right), Half::Known(Side::Right));
        assert_eq!(identify(&go60, stock, assimilator), Half::NotThisBoard);
        // Both Imprint halves mount under the same name.
        assert_eq!(identify(&imprint, cyboard, assimilator), Half::Either);
    }

    #[test]
    fn plans_follow_the_vendor_order() {
        let steps = |id: &str| {
            let board = board(id);
            plan(&board, &board.firmware[0])
        };
        let sides = |id: &str| steps(id).iter().map(|s| s.side).collect::<Vec<_>>();
        assert_eq!(sides("moergo-go60"), [Side::Right, Side::Left]);
        assert_eq!(sides("cyboard-imprint"), [Side::Left, Side::Right]);
        assert_eq!(steps("moergo-go60")[0].volumes, ["GO60RHBOOT"]);
    }

    #[test]
    fn firmware_for_the_wrong_half_is_caught() {
        let go60 = board("moergo-go60");
        let stock = &go60.firmware[0];
        let left = uf2::tests::image(&[0x9809_B007]);
        let both = uf2::tests::image(&[0x9809_B007, 0x980A_B007]);
        assert!(check(&go60, stock, Side::Left, &left).is_ok());
        assert!(matches!(
            check(&go60, stock, Side::Right, &left),
            Err(FlashError::WrongHalf)
        ));
        assert!(
            check(&go60, stock, Side::Right, &both).is_ok(),
            "a combined file suits either half"
        );
        // The Imprint declares no family, so any valid UF2 is accepted.
        let imprint = board("cyboard-imprint");
        assert!(check(&imprint, &imprint.firmware[0], Side::Right, &left).is_ok());
        assert!(matches!(
            check(&go60, stock, Side::Left, b"not firmware"),
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
