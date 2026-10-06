//! Firmware build back ends. GitHub Actions first; the trait leaves room for Docker and native builds.
//!
//! The GitHub back end works on a local clone of the user's zmk-config
//! repository: it writes the generated files, commits and pushes, follows
//! the workflow run that push starts, and downloads the firmware it builds.

pub mod github;
pub mod repo;

use std::io::Read;

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
