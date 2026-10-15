//! Firmware build back ends, and firmware taken ready-made from releases.
//!
//! The GitHub back end works on a local clone of the user's zmk-config
//! repository: it writes the generated files, commits and pushes, follows
//! the workflow run that push starts, and downloads the firmware it builds.
//! [`release`] fetches a project's released firmware instead, and checks
//! it against the project's own checksums before offering it.

pub mod github;
pub mod release;
pub mod repo;

use std::io::Read;
use std::path::{Path, PathBuf};

pub use github::{GitHub, Run, RunState};
pub use repo::Repo;

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("git {command} failed: {message}")]
    Git { command: String, message: String },
    #[error("could not run git: {0}")]
    GitMissing(std::io::Error),
    #[error("{0} is not a git repository")]
    NotARepository(String),
    #[error("the repository's remote `{0}` is not on GitHub")]
    NotGitHub(String),
    #[error("GitHub refused the request ({status}): {message}")]
    Api { status: u16, message: String },
    #[error("could not reach GitHub: {0}")]
    Network(String),
    #[error("the firmware download could not be read: {0}")]
    Archive(String),
    #[error("the release has no asset named `{0}`")]
    NoAsset(String),
    #[error(
        "{name} does not match its checksum: expected {expected}, got {actual}, so it was not kept"
    )]
    Digest {
        name: String,
        expected: String,
        actual: String,
    },
    #[error("{0}")]
    Release(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Fetches a small text file over HTTPS, for data the app keeps current
/// between releases.
pub fn fetch_text(url: &str) -> Result<String, BuildError> {
    ureq::get(url)
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| BuildError::Network(e.to_string()))?
        .into_string()
        .map_err(BuildError::Io)
}

/// One built firmware image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Firmware {
    /// The file name, such as `imprint_left.uf2`.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Keeps a build's firmware on disk, under `root/<board>/<date> run <id>`,
/// and returns the folder. The date is the build's, in UTC, so the
/// folders of one board sort by time and name the GitHub run they came
/// from.
pub fn keep(root: &Path, board: &str, run: u64, files: &[Firmware]) -> Result<PathBuf, BuildError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let folder = root
        .join(board)
        .join(format!("{} run {run}", utc_stamp(now)));
    std::fs::create_dir_all(&folder).map_err(BuildError::Io)?;
    for file in files {
        std::fs::write(folder.join(&file.name), &file.bytes).map_err(BuildError::Io)?;
    }
    Ok(folder)
}

/// Seconds since the Unix epoch as `YYYY-MM-DD HH-MM UTC`, a folder name
/// every file system takes.
fn utc_stamp(secs: u64) -> String {
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (hour, minute) = ((secs % 86_400) / 3600, (secs % 3600) / 60);
    format!("{year:04}-{month:02}-{day:02} {hour:02}-{minute:02} UTC")
}

/// The `.uf2` files inside a GitHub artifact archive.
pub fn extract_uf2(archive: &[u8]) -> Result<Vec<Firmware>, BuildError> {
    let error = |e: zip::result::ZipError| BuildError::Archive(e.to_string());
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).map_err(error)?;
    let mut found = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(error)?;
        let name = entry
            .name()
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
        if name.to_lowercase().ends_with(".uf2") {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            found.push(Firmware { name, bytes });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn the_folder_stamp_is_the_utc_date_and_time() {
        assert_eq!(utc_stamp(0), "1970-01-01 00-00 UTC");
        // 2026-10-15T23:15:08Z, the Imprint build that went on hardware.
        assert_eq!(utc_stamp(1_792_106_108), "2026-10-15 23-15 UTC");
        // The leap day of a century year that is a leap year.
        assert_eq!(utc_stamp(951_782_400), "2000-02-29 00-00 UTC");
    }

    #[test]
    fn a_build_is_kept_under_its_board_and_run() {
        let root = std::env::temp_dir().join(format!("kc-build-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let files = vec![
            Firmware {
                name: "imprint_left.uf2".into(),
                bytes: b"left".to_vec(),
            },
            Firmware {
                name: "imprint_right.uf2".into(),
                bytes: b"right".to_vec(),
            },
        ];
        let folder = keep(&root, "cyboard-imprint", 42, &files).unwrap();
        assert!(folder.starts_with(root.join("cyboard-imprint")));
        let name = folder.file_name().unwrap().to_string_lossy();
        assert!(name.ends_with(" UTC run 42"), "{name}");
        assert_eq!(
            std::fs::read(folder.join("imprint_right.uf2")).unwrap(),
            b"right"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn firmware_is_pulled_out_of_an_artifact_archive() {
        let mut buffer = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
            let options = zip::write::SimpleFileOptions::default();
            for (name, contents) in [
                ("imprint_right.uf2", "right"),
                ("nested/imprint_left.UF2", "left"),
                ("notes.txt", "ignored"),
            ] {
                zip.start_file(name, options).unwrap();
                zip.write_all(contents.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        let firmware = extract_uf2(&buffer).unwrap();
        let names: Vec<&str> = firmware.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["imprint_left.UF2", "imprint_right.uf2"]);
        assert_eq!(firmware[0].bytes, b"left");
        assert!(matches!(
            extract_uf2(b"not a zip"),
            Err(BuildError::Archive(_))
        ));
    }
}
