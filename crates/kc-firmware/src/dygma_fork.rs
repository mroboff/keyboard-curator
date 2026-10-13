//! The Curator firmware for the Defy: fetching a release of the fork,
//! checking it against what the board definition pins, keeping it for
//! Bazecor's custom firmware folder, and installing the Neuron image from
//! here.
//!
//! The fork's releases carry `Wireless_neuron.hex` (the Neuron image),
//! Dygma's own `keyscanner.bin` for the sides, a `SHA256SUMS` list and a
//! `manifest.json` naming the commits the build was made from. A release
//! is offered only when every file matches the list and the manifest, and
//! the manifest names the commits the board definition expects.

use std::path::{Path, PathBuf};

use kc_boards::board::DygmaFork;
use kc_boards::Board;
use kc_build::release;
use kc_model::{Device, FirmwareConfig};
use serde::Deserialize;

/// What the fork's `manifest.json` says about a build.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ForkManifest {
    pub schema: u32,
    pub project: String,
    pub version: String,
    pub source: ForkCommit,
    #[serde(default)]
    pub libraries: Option<ForkCommit>,
    pub kaleidoscope: ForkCommit,
    #[serde(default)]
    pub base: Option<ForkBase>,
    #[serde(default)]
    pub focus: Vec<String>,
    pub artifacts: Vec<ForkArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ForkCommit {
    #[serde(default)]
    pub repository: String,
    pub commit: String,
    #[serde(default)]
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ForkBase {
    #[serde(default)]
    pub repository: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkArtifact {
    pub kind: String,
    pub file: String,
    pub sha256: String,
    #[serde(default)]
    pub address_start: Option<String>,
    #[serde(default)]
    pub address_end: Option<String>,
}

/// A release of the fork, fetched and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkFirmware {
    pub tag: String,
    /// The release's page on GitHub.
    pub url: String,
    /// The Neuron image, as Intel HEX text.
    pub neuron_hex: String,
    /// Dygma's keyscanner image for the sides, when the release carries it.
    pub keyscanner: Option<Vec<u8>>,
    pub manifest: ForkManifest,
    /// The file names as the release calls them.
    pub neuron_file: String,
    pub keyscanner_file: String,
    /// What the app calls the fork, from the board file.
    pub fork_name: String,
}

/// The fork a board's chosen firmware is taken from, when it is one.
pub fn fork<'a>(board: &'a Board, config: &FirmwareConfig) -> Option<&'a DygmaFork> {
    board
        .profile(&config.profile)?
        .dygma
        .as_ref()?
        .fork
        .as_ref()
}

/// Checks a file against the release's `SHA256SUMS` and its manifest entry.
fn verify(name: &str, bytes: &[u8], sums: &str, manifest: &ForkManifest) -> Result<(), String> {
    let actual = release::sha256_hex(bytes);
    let listed = sums
        .lines()
        .find_map(|line| {
            let (sum, file) = line.split_once(char::is_whitespace)?;
            (file.trim() == name).then(|| sum.trim().to_ascii_lowercase())
        })
        .ok_or_else(|| format!("{name} is not in the release's SHA256SUMS"))?;
    let in_manifest = manifest
        .artifacts
        .iter()
        .find(|a| a.file == name)
        .map(|a| a.sha256.to_ascii_lowercase())
        .ok_or_else(|| format!("{name} is not in the release's manifest"))?;
    for expected in [listed, in_manifest] {
        if expected != actual {
            return Err(format!(
                "{name} does not match its checksum: expected {expected}, got {actual}, so it was not kept"
            ));
        }
    }
    Ok(())
}

/// Checks that a release was built from the sources the board pins.
pub fn check_sources(manifest: &ForkManifest, fork: &DygmaFork) -> Result<(), String> {
    if manifest.project != "defy-curator" {
        return Err(format!(
            "the release is for {}, not the Curator firmware",
            manifest.project
        ));
    }
    if manifest.source.dirty {
        return Err(
            "the release was built from a modified tree, which cannot be matched to its sources"
                .into(),
        );
    }
    if !manifest
        .source
        .commit
        .eq_ignore_ascii_case(&fork.source_commit)
    {
        return Err(format!(
            "the release was built from commit {}, not the {} this app expects",
            manifest.source.commit, fork.source_commit
        ));
    }
    if !manifest
        .kaleidoscope
        .commit
        .eq_ignore_ascii_case(&fork.kaleidoscope_commit)
    {
        return Err(format!(
            "the release's plugins come from commit {}, not the {} this app speaks to",
            manifest.kaleidoscope.commit, fork.kaleidoscope_commit
        ));
    }
    if manifest.version != fork.tag {
        return Err(format!(
            "the release reports itself as {}, not {}",
            manifest.version, fork.tag
        ));
    }
    Ok(())
}

/// Fetches the fork's release named by the board's chosen firmware and
/// checks every file. A GitHub token only lifts the rate limit.
pub fn fetch(
    board: &Board,
    config: &FirmwareConfig,
    token: Option<&str>,
) -> Result<ForkFirmware, String> {
    let fork = fork(board, config).ok_or("this firmware is not taken from a release")?;
    let mut names = vec!["SHA256SUMS", "manifest.json", fork.neuron_file.as_str()];
    if !fork.keyscanner_file.is_empty() {
        names.push(fork.keyscanner_file.as_str());
    }
    let assets = release::fetch_assets(&fork.repository, &fork.tag, &names, token)
        .map_err(|e| e.to_string())?;
    let file = |name: &str| -> Result<&[u8], String> {
        assets
            .files
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.bytes.as_slice())
            .ok_or_else(|| format!("the release has no {name}"))
    };
    let sums = String::from_utf8(file("SHA256SUMS")?.to_vec())
        .map_err(|_| "the release's SHA256SUMS is not text".to_string())?;
    let manifest: ForkManifest = serde_json::from_slice(file("manifest.json")?)
        .map_err(|e| format!("the release's manifest could not be read: {e}"))?;
    check_sources(&manifest, fork)?;
    let neuron = file(&fork.neuron_file)?;
    verify(&fork.neuron_file, neuron, &sums, &manifest)?;
    let neuron_hex = String::from_utf8(neuron.to_vec())
        .map_err(|_| format!("{} is not a text file", fork.neuron_file))?;
    // The image must be one a Neuron takes before it is offered at all.
    kc_dygma::flash::parse_hex(&neuron_hex)
        .and_then(|image| image.check().map(|()| image))
        .map_err(|e| e.to_string())?;
    let keyscanner = if fork.keyscanner_file.is_empty() {
        None
    } else {
        let bytes = file(&fork.keyscanner_file)?;
        verify(&fork.keyscanner_file, bytes, &sums, &manifest)?;
        Some(bytes.to_vec())
    };
    Ok(ForkFirmware {
        tag: assets.tag,
        url: assets.url,
        neuron_hex,
        keyscanner,
        manifest,
        neuron_file: fork.neuron_file.clone(),
        keyscanner_file: fork.keyscanner_file.clone(),
        fork_name: fork.name.clone(),
    })
}

/// Keeps a fetched release on disk as Bazecor's custom firmware folder
/// wants it: the Neuron image and the keyscanner image side by side in
/// `<firmware dir>/<tag>/`. Returns that folder.
pub fn save(firmware: &ForkFirmware, firmware_dir: &Path) -> Result<PathBuf, String> {
    let folder = firmware_dir.join(&firmware.tag);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    std::fs::write(folder.join(&firmware.neuron_file), &firmware.neuron_hex)
        .map_err(|e| e.to_string())?;
    if let Some(keyscanner) = &firmware.keyscanner {
        std::fs::write(folder.join(&firmware.keyscanner_file), keyscanner)
            .map_err(|e| e.to_string())?;
    }
    Ok(folder)
}

pub use kc_dygma::flash::Progress;

/// Installs a fetched release's Neuron image on the connected keyboard of
/// `board`, which must be a wireless Defy running Dygma's or the Curator
/// firmware. Returns the version the keyboard reports afterwards.
pub fn install(
    board: &Board,
    device: Option<&Device>,
    firmware: &ForkFirmware,
    progress: &mut dyn FnMut(Progress),
) -> Result<String, String> {
    kc_dygma::flash::install(board, device, &firmware.neuron_hex, progress)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fork() -> DygmaFork {
        DygmaFork {
            name: "Curator firmware".into(),
            repository: "mroboff/NeuronWireless_defy".into(),
            tag: "v2.2.1-curator.1".into(),
            source_commit: "69d3ce27159ed39cafeadc79096aa7a2a63b0e68".into(),
            kaleidoscope_commit: "fc3e1f83ed8049d8e1c9ad1300646d98f950778c".into(),
            version_mark: "-curator.".into(),
            neuron_file: "Wireless_neuron.hex".into(),
            keyscanner_file: "keyscanner.bin".into(),
        }
    }

    fn manifest(json: &str) -> ForkManifest {
        serde_json::from_str(json).unwrap()
    }

    const GOOD: &str = r#"{
      "schema": 1, "project": "defy-curator", "version": "v2.2.1-curator.1",
      "source": {"repository": "mroboff/NeuronWireless_defy", "commit": "69d3ce27159ed39cafeadc79096aa7a2a63b0e68", "dirty": false},
      "kaleidoscope": {"repository": "mroboff/Kaleidoscope-Dygma", "commit": "fc3e1f83ed8049d8e1c9ad1300646d98f950778c"},
      "focus": ["chords.map"],
      "artifacts": [{"kind": "neuron", "file": "Wireless_neuron.hex", "sha256": "ab", "addressStart": "0x27000"}]
    }"#;

    #[test]
    fn a_release_from_the_pinned_sources_passes() {
        assert_eq!(check_sources(&manifest(GOOD), &fork()), Ok(()));
    }

    #[test]
    fn other_sources_are_refused() {
        let other = GOOD.replace("69d3ce27", "deadbeef");
        assert!(check_sources(&manifest(&other), &fork())
            .unwrap_err()
            .contains("built from commit"));
        let dirty = GOOD.replace("\"dirty\": false", "\"dirty\": true");
        assert!(check_sources(&manifest(&dirty), &fork())
            .unwrap_err()
            .contains("modified tree"));
        let version = GOOD.replace("v2.2.1-curator.1", "v2.2.1-curator.2");
        assert!(check_sources(&manifest(&version), &fork())
            .unwrap_err()
            .contains("reports itself"));
    }

    #[test]
    fn files_are_checked_against_both_lists() {
        let bytes = b"hello";
        let sum = release::sha256_hex(bytes);
        let json = GOOD.replace("\"ab\"", &format!("\"{sum}\""));
        let manifest = manifest(&json);
        let sums = format!("{sum}  Wireless_neuron.hex\n");
        assert_eq!(
            verify("Wireless_neuron.hex", bytes, &sums, &manifest),
            Ok(())
        );
        assert!(verify("Wireless_neuron.hex", b"other", &sums, &manifest).is_err());
        assert!(verify("keyscanner.bin", bytes, &sums, &manifest).is_err());
    }
}
