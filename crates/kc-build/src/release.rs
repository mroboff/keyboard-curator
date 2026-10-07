//! Firmware taken ready-made from a project's GitHub releases, and checked
//! before it is offered: the archive against the digest GitHub publishes
//! for it, each firmware file against the `SHA256SUMS` list and the
//! `manifest.json` inside the archive, and the manifest's facts about what
//! the firmware was built from against what the board definition expects.
//!
//! The archive layout is moergo-rmk's (`just dist` in its README): one UF2
//! per half, `SHA256SUMS`, and a manifest of schema 2.

use std::io::Read;

use kc_boards::board::Release;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{BuildError, Firmware};

const API: &str = "https://api.github.com";
/// The most a release asset may be.
const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;

/// What a release manifest says about the firmware in it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub project: String,
    pub version: String,
    pub source: ManifestSource,
    pub rmk: ManifestRmk,
    pub application_range: AddressRange,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ManifestSource {
    pub commit: String,
    #[serde(default)]
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ManifestRmk {
    pub commit: String,
    #[serde(default)]
    pub version: String,
}

/// Addresses as the manifest writes them, `0x26000`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AddressRange {
    pub start: String,
    pub end: String,
}

impl AddressRange {
    fn parse(text: &str) -> Option<u32> {
        let hex = text
            .trim()
            .strip_prefix("0x")
            .or_else(|| text.strip_prefix("0X"))?;
        u32::from_str_radix(hex, 16).ok()
    }

    pub fn start(&self) -> Option<u32> {
        Self::parse(&self.start)
    }

    pub fn end(&self) -> Option<u32> {
        Self::parse(&self.end)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Artifact {
    pub half: String,
    pub uf2: ArtifactFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactFile {
    pub file: String,
    pub sha256: String,
    pub family_id: String,
    pub address_start: String,
    pub address_end: String,
}

impl ArtifactFile {
    pub fn family(&self) -> Option<u32> {
        AddressRange::parse(&self.family_id)
    }
}

/// Firmware from a release, checked and ready to flash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasedFirmware {
    pub tag: String,
    /// The release's title.
    pub name: String,
    /// The release's page on GitHub.
    pub url: String,
    /// The firmware files the board definition named, each verified.
    pub files: Vec<Firmware>,
    pub manifest: Manifest,
}

#[derive(Deserialize)]
struct AssetJson {
    name: String,
    browser_download_url: String,
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    #[serde(default)]
    name: String,
    html_url: String,
    assets: Vec<AssetJson>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn release_error(message: impl Into<String>) -> BuildError {
    BuildError::Release(message.into())
}

/// Fetches the release's asset and checks everything about it. A GitHub
/// token is optional: releases are public, and the token only lifts the
/// rate limit.
pub fn fetch(release: &Release, token: Option<&str>) -> Result<ReleasedFirmware, BuildError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(120))
        .build();
    let request = |url: &str| {
        let mut request = agent
            .get(url)
            .set("Accept", "application/vnd.github+json")
            .set("X-GitHub-Api-Version", "2022-11-28")
            .set("User-Agent", "keyboard-curator");
        if let Some(token) = token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        request
    };
    let fail = |error: ureq::Error| match error {
        ureq::Error::Status(status, response) => BuildError::Api {
            status,
            message: response
                .into_json::<serde_json::Value>()
                .ok()
                .and_then(|v| v["message"].as_str().map(str::to_string))
                .unwrap_or_else(|| "no details".to_string()),
        },
        other => BuildError::Network(other.to_string()),
    };
    let url = format!(
        "{API}/repos/{}/releases/tags/{}",
        release.repository, release.tag
    );
    let found: ReleaseJson = request(&url)
        .call()
        .map_err(fail)?
        .into_json()
        .map_err(|e| BuildError::Network(e.to_string()))?;
    let asset = found
        .assets
        .iter()
        .find(|a| a.name == release.asset)
        .ok_or_else(|| BuildError::NoAsset(release.asset.clone()))?;
    if asset.size == 0 || asset.size > MAX_ASSET_BYTES {
        return Err(release_error(format!(
            "the release asset {} is {} bytes, outside the size this app takes",
            asset.name, asset.size
        )));
    }
    let digest = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| {
            release_error(format!(
                "GitHub publishes no SHA-256 digest for {}, so the download cannot be checked",
                asset.name
            ))
        })?;

    let mut bytes = Vec::with_capacity(usize::try_from(asset.size).unwrap_or_default());
    agent
        .get(&asset.browser_download_url)
        .set("User-Agent", "keyboard-curator")
        .call()
        .map_err(fail)?
        .into_reader()
        .take(MAX_ASSET_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != asset.size {
        return Err(release_error(format!(
            "the download of {} is {} bytes, and GitHub says {}",
            asset.name,
            bytes.len(),
            asset.size
        )));
    }
    let actual = sha256(&bytes);
    if actual != digest {
        return Err(BuildError::Digest {
            name: asset.name.clone(),
            expected: digest,
            actual,
        });
    }
    let (files, manifest) = unpack(&bytes, release)?;
    Ok(ReleasedFirmware {
        tag: found.tag_name,
        name: found.name,
        url: found.html_url,
        files,
        manifest,
    })
}

/// Reads the firmware files named by the board out of a release archive
/// whose own digest has been checked, and checks each against the
/// archive's `SHA256SUMS` and `manifest.json`.
pub fn unpack(archive: &[u8], release: &Release) -> Result<(Vec<Firmware>, Manifest), BuildError> {
    let error = |e: zip::result::ZipError| BuildError::Archive(e.to_string());
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).map_err(error)?;
    let mut read = |name: &str| -> Result<Vec<u8>, BuildError> {
        let index = (0..zip.len())
            .find(|i| {
                zip.by_index(*i)
                    .ok()
                    .is_some_and(|entry| entry.name().rsplit('/').next() == Some(name))
            })
            .ok_or_else(|| release_error(format!("the release archive has no {name}")))?;
        let mut entry = zip.by_index(index).map_err(error)?;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    let sums = String::from_utf8(read("SHA256SUMS")?)
        .map_err(|_| release_error("the release's SHA256SUMS is not text"))?;
    let manifest: Manifest = serde_json::from_slice(&read("manifest.json")?)
        .map_err(|e| release_error(format!("the release's manifest could not be read: {e}")))?;
    let listed = |name: &str| -> Option<String> {
        sums.lines().find_map(|line| {
            let (sum, file) = line.split_once(char::is_whitespace)?;
            (file.trim() == name).then(|| sum.trim().to_ascii_lowercase())
        })
    };
    let mut files = Vec::new();
    for wanted in &release.files {
        let bytes = read(&wanted.name)?;
        let actual = sha256(&bytes);
        let in_sums = listed(&wanted.name).ok_or_else(|| {
            release_error(format!(
                "{} is not in the release's SHA256SUMS",
                wanted.name
            ))
        })?;
        let in_manifest = manifest
            .artifacts
            .iter()
            .find(|a| a.uf2.file == wanted.name)
            .map(|a| a.uf2.sha256.to_ascii_lowercase())
            .ok_or_else(|| {
                release_error(format!("{} is not in the release's manifest", wanted.name))
            })?;
        for expected in [in_sums, in_manifest] {
            if expected != actual {
                return Err(BuildError::Digest {
                    name: wanted.name.clone(),
                    expected,
                    actual,
                });
            }
        }
        files.push(Firmware {
            name: wanted.name.clone(),
            bytes,
        });
    }
    Ok((files, manifest))
}

/// Checks that a release was built from the sources the board definition
/// pins: the firmware project's commit, and the RMK commit whose protocol
/// the app speaks.
pub fn check_sources(manifest: &Manifest, source: &str, rmk: &str) -> Result<(), BuildError> {
    if manifest.source.dirty {
        return Err(release_error(
            "the release was built from a modified tree, which cannot be matched to its sources",
        ));
    }
    if !manifest.source.commit.eq_ignore_ascii_case(source) {
        return Err(release_error(format!(
            "the release was built from commit {}, not the {} this app expects",
            manifest.source.commit, source
        )));
    }
    if !manifest.rmk.commit.eq_ignore_ascii_case(rmk) {
        return Err(release_error(format!(
            "the release was built with RMK commit {}, not the {} this app speaks to",
            manifest.rmk.commit, rmk
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use kc_boards::board::{ReleaseFile, Side};

    use super::*;

    fn release() -> Release {
        Release {
            repository: "someone/firmware".into(),
            tag: "v1".into(),
            asset: "go60-rmk.zip".into(),
            files: vec![
                ReleaseFile {
                    side: Side::Left,
                    name: "go60-rmk-0.1.0-lh.uf2".into(),
                },
                ReleaseFile {
                    side: Side::Right,
                    name: "go60-rmk-0.1.0-rh.uf2".into(),
                },
            ],
        }
    }

    fn manifest_json(left: &str, right: &str) -> String {
        format!(
            r#"{{"applicationRange":{{"start":"0x26000","end":"0xdc000"}},"artifacts":[
            {{"half":"left","uf2":{{"file":"go60-rmk-0.1.0-lh.uf2","sha256":"{left}","familyId":"0x9809b007","addressStart":"0x26000","addressEnd":"0xda500","blocks":2885}}}},
            {{"half":"right","uf2":{{"file":"go60-rmk-0.1.0-rh.uf2","sha256":"{right}","familyId":"0x980ab007","addressStart":"0x26000","addressEnd":"0x8a700","blocks":1607}}}}],
            "project":"go60-rmk","rmk":{{"commit":"997b62f5","version":"997b62f5"}},"schemaVersion":2,
            "source":{{"commit":"179bd66b","dirty":false}},"version":"0.1.0"}}"#
        )
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buffer = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
            let options = zip::write::SimpleFileOptions::default();
            for (name, contents) in entries {
                zip.start_file(*name, options).unwrap();
                zip.write_all(contents).unwrap();
            }
            zip.finish().unwrap();
        }
        buffer
    }

    #[test]
    fn firmware_is_checked_against_the_sums_and_the_manifest() {
        let (left, right) = (b"left firmware".as_slice(), b"right firmware".as_slice());
        let (left_sum, right_sum) = (sha256(left), sha256(right));
        let sums =
            format!("{left_sum}  go60-rmk-0.1.0-lh.uf2\n{right_sum}  go60-rmk-0.1.0-rh.uf2\n");
        let good = archive(&[
            (
                "manifest.json",
                manifest_json(&left_sum, &right_sum).as_bytes(),
            ),
            ("go60-rmk-0.1.0-lh.uf2", left),
            ("go60-rmk-0.1.0-rh.uf2", right),
            ("SHA256SUMS", sums.as_bytes()),
        ]);
        let (files, manifest) = unpack(&good, &release()).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "go60-rmk-0.1.0-lh.uf2");
        assert_eq!(files[1].bytes, right);
        assert_eq!(manifest.application_range.start(), Some(0x26000));
        assert_eq!(manifest.application_range.end(), Some(0xdc000));
        assert_eq!(manifest.artifacts[1].uf2.family(), Some(0x980a_b007));
        assert!(check_sources(&manifest, "179bd66b", "997b62f5").is_ok());
        assert!(check_sources(&manifest, "179bd66b", "deadbeef")
            .unwrap_err()
            .to_string()
            .contains("RMK commit"));
        assert!(check_sources(&manifest, "0000000", "997b62f5").is_err());

        // A file that does not match the list is refused.
        let tampered = archive(&[
            (
                "manifest.json",
                manifest_json(&left_sum, &right_sum).as_bytes(),
            ),
            ("go60-rmk-0.1.0-lh.uf2", b"something else"),
            ("go60-rmk-0.1.0-rh.uf2", right),
            ("SHA256SUMS", sums.as_bytes()),
        ]);
        assert!(matches!(
            unpack(&tampered, &release()),
            Err(BuildError::Digest { name, .. }) if name == "go60-rmk-0.1.0-lh.uf2"
        ));
        // A file the manifest does not know is refused too.
        let unlisted = archive(&[
            (
                "manifest.json",
                manifest_json(&left_sum, &right_sum).as_bytes(),
            ),
            ("go60-rmk-0.1.0-lh.uf2", left),
            ("go60-rmk-0.1.0-rh.uf2", right),
            (
                "SHA256SUMS",
                format!("{left_sum}  go60-rmk-0.1.0-lh.uf2\n").as_bytes(),
            ),
        ]);
        assert!(unpack(&unlisted, &release())
            .unwrap_err()
            .to_string()
            .contains("SHA256SUMS"));
        assert!(unpack(b"not a zip", &release()).is_err());
    }
}
