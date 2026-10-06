//! ZMK add-ons: west modules a board's ZMK build can take on.
//!
//! The catalog is data. `catalog/addons.toml` is curated by hand, and
//! `catalog/addons-meta.json` is refreshed by a scheduled job with each
//! add-on's pinned commit and activity. Both ship inside the app, and a
//! newer pair fetched from the project's repository can replace them while
//! the app runs.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, RwLock};

use serde::Deserialize;

use crate::feature::Feature;

/// The catalog format this build reads.
pub const FORMAT: u32 = 1;

const EMBEDDED: &str = include_str!("../catalog/addons.toml");
const EMBEDDED_META: &str = include_str!("../catalog/addons-meta.json");

/// One add-on, as offered to the user.
#[derive(Debug, Clone, PartialEq)]
pub struct Addon {
    pub id: String,
    pub name: String,
    pub author: String,
    pub url: String,
    /// The name of the west project.
    pub module: String,
    /// The tag or branch the add-on is taken from.
    pub git_ref: String,
    /// The commit `git_ref` was last resolved to, which builds use so that
    /// a moved tag cannot change a firmware. Falls back to `git_ref`.
    pub revision: String,
    /// The Zephyr generations of ZMK this pin is for.
    pub zephyr: Vec<String>,
    pub category: String,
    /// Features the app offers once the add-on is in the build.
    pub provides: Vec<Feature>,
    /// Include lines for the top of the keymap, with their delimiters.
    pub includes: Vec<String>,
    pub summary: String,
    /// What people use it for.
    pub uses: String,
    /// Usage and compatibility notes.
    pub notes: Vec<String>,
    /// A starter for Custom Behaviors, for add-ons whose behaviors the
    /// user defines.
    pub snippet: Option<String>,
    /// Boards the add-on must not be offered for.
    pub exclude_boards: Vec<String>,
    pub stars: Option<u32>,
    /// The date of the repository's last push.
    pub pushed: Option<String>,
    pub archived: bool,
}

impl Addon {
    /// Whether the add-on can go into a build of ZMK on `zephyr` for
    /// `board`.
    pub fn fits(&self, zephyr: &str, board: &str) -> bool {
        self.zephyr.iter().any(|z| z == zephyr) && !self.exclude_boards.iter().any(|b| b == board)
    }
}

/// A repository the refresh job found that is not in the catalog yet.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Candidate {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub stars: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Catalog {
    pub addons: Vec<Addon>,
    /// When the metadata was last refreshed.
    pub checked: Option<String>,
    pub candidates: Vec<Candidate>,
}

impl Catalog {
    pub fn addon(&self, id: &str) -> Option<&Addon> {
        self.addons.iter().find(|a| a.id == id)
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CatalogError {
    #[error("the add-on catalog is not valid: {0}")]
    Parse(String),
    #[error("the add-on catalog is format {found}, and this version reads up to {supported}")]
    Newer { found: u32, supported: u32 },
    #[error("the add-on `{0}` is listed more than once")]
    Duplicate(String),
    #[error("the add-on `{id}` has an unusable {field}")]
    Unsafe { id: String, field: &'static str },
}

#[derive(Deserialize)]
struct RawCatalog {
    format: u32,
    /// Repositories the refresh job no longer suggests. Not used here.
    #[serde(default)]
    #[allow(dead_code)]
    ignore: Vec<String>,
    #[serde(default)]
    addon: Vec<RawAddon>,
}

#[derive(Deserialize)]
struct RawAddon {
    id: String,
    name: String,
    author: String,
    url: String,
    module: String,
    #[serde(rename = "ref")]
    git_ref: String,
    zephyr: Vec<String>,
    category: String,
    #[serde(default)]
    provides: Vec<String>,
    #[serde(default)]
    includes: Vec<String>,
    summary: String,
    uses: String,
    #[serde(default)]
    notes: Vec<String>,
    snippet: Option<String>,
    #[serde(default)]
    exclude_boards: Vec<String>,
}

#[derive(Deserialize, Default)]
struct RawMeta {
    #[serde(default)]
    checked: Option<String>,
    #[serde(default)]
    addons: BTreeMap<String, RawAddonMeta>,
    #[serde(default)]
    candidates: Vec<Candidate>,
}

#[derive(Deserialize)]
struct RawAddonMeta {
    revision: Option<String>,
    stars: Option<u32>,
    pushed: Option<String>,
    #[serde(default)]
    archived: bool,
}

/// Whether text is safe to write into a generated YAML or keymap line: a
/// catalog fetched from the network must not be able to smuggle anything
/// else into the build files.
fn plain(text: &str, extra: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

fn safe_include(include: &str) -> bool {
    let inner = include
        .strip_prefix('<')
        .and_then(|i| i.strip_suffix('>'))
        .or_else(|| include.strip_prefix('"').and_then(|i| i.strip_suffix('"')));
    inner.is_some_and(|path| plain(path, "/._-") && !path.contains(".."))
}

/// Reads a catalog and its metadata. The metadata may be absent or behind;
/// an add-on without it builds from its `ref`.
pub fn parse(catalog: &str, meta: Option<&str>) -> Result<Catalog, CatalogError> {
    let raw: RawCatalog =
        toml::from_str(catalog).map_err(|e| CatalogError::Parse(e.to_string()))?;
    if raw.format > FORMAT {
        return Err(CatalogError::Newer {
            found: raw.format,
            supported: FORMAT,
        });
    }
    let mut meta: RawMeta = match meta {
        Some(text) => serde_json::from_str(text).map_err(|e| CatalogError::Parse(e.to_string()))?,
        None => RawMeta::default(),
    };
    let mut addons: Vec<Addon> = Vec::new();
    for raw in raw.addon {
        if addons.iter().any(|a| a.id == raw.id) {
            return Err(CatalogError::Duplicate(raw.id));
        }
        let extra = meta.addons.remove(&raw.id);
        let revision = extra
            .as_ref()
            .and_then(|m| m.revision.clone())
            .unwrap_or_else(|| raw.git_ref.clone());
        let unsafe_field = |field| CatalogError::Unsafe {
            id: raw.id.clone(),
            field,
        };
        if !plain(&raw.id, "-_") {
            return Err(unsafe_field("id"));
        }
        if !plain(&raw.module, "-_.") {
            return Err(unsafe_field("module name"));
        }
        if !raw.url.starts_with("https://github.com/") || !plain(&raw.url, ":/._-") {
            return Err(unsafe_field("URL"));
        }
        if !plain(&raw.git_ref, "/._-+") || !plain(&revision, "/._-+") {
            return Err(unsafe_field("revision"));
        }
        if !raw.includes.iter().all(|i| safe_include(i)) {
            return Err(unsafe_field("include"));
        }
        // Features a newer catalog names that this version does not know
        // are left out; the add-on still builds.
        let provides = raw
            .provides
            .iter()
            .filter_map(|name| serde_json::from_value(serde_json::Value::String(name.clone())).ok())
            .collect();
        addons.push(Addon {
            id: raw.id,
            name: raw.name,
            author: raw.author,
            url: raw.url,
            module: raw.module,
            git_ref: raw.git_ref,
            revision,
            zephyr: raw.zephyr,
            category: raw.category,
            provides,
            includes: raw.includes,
            summary: raw.summary,
            uses: raw.uses,
            notes: raw.notes,
            snippet: raw.snippet.map(|s| s.trim().to_string()),
            exclude_boards: raw.exclude_boards,
            stars: extra.as_ref().and_then(|m| m.stars),
            pushed: extra.as_ref().and_then(|m| m.pushed.clone()),
            archived: extra.as_ref().is_some_and(|m| m.archived),
        });
    }
    Ok(Catalog {
        addons,
        checked: meta.checked,
        candidates: meta.candidates,
    })
}

fn current() -> &'static RwLock<Arc<Catalog>> {
    static CURRENT: OnceLock<RwLock<Arc<Catalog>>> = OnceLock::new();
    CURRENT.get_or_init(|| {
        let embedded = parse(EMBEDDED, Some(EMBEDDED_META))
            .expect("the catalog that ships with the app is valid");
        RwLock::new(Arc::new(embedded))
    })
}

/// The catalog in use: the one that ships with the app, or a newer one
/// installed since.
pub fn catalog() -> Arc<Catalog> {
    current()
        .read()
        .map_or_else(|poisoned| poisoned.into_inner().clone(), |c| c.clone())
}

/// Replaces the catalog in use with another, if that one checks out and
/// was refreshed more recently. A copy fetched or cached before the app
/// was updated must not push out the newer one the update shipped with.
/// Returns whether it was installed.
pub fn install_if_newer(catalog: &str, meta: &str) -> Result<bool, CatalogError> {
    let parsed = parse(catalog, Some(meta))?;
    // Dates are ISO, so they order as text.
    if parsed.checked <= self::catalog().checked {
        return Ok(false);
    }
    let parsed = Arc::new(parsed);
    match current().write() {
        Ok(mut slot) => *slot = parsed,
        Err(poisoned) => *poisoned.into_inner() = parsed,
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_catalog_loads_with_pinned_commits() {
        let catalog = parse(EMBEDDED, Some(EMBEDDED_META)).unwrap();
        assert!(catalog.addons.len() >= 7);
        for addon in &catalog.addons {
            assert_eq!(
                addon.revision.len(),
                40,
                "{} is pinned to a commit",
                addon.id
            );
            assert!(addon.url.starts_with("https://github.com/"), "{}", addon.id);
            assert!(
                !addon.summary.is_empty() && !addon.uses.is_empty(),
                "{}",
                addon.id
            );
            assert!(addon.fits("3.5", "moergo-go60"), "{}", addon.id);
            assert!(!addon.fits("4.1", "moergo-go60"), "{}", addon.id);
        }
        let auto = catalog.addon("zmk-auto-layer").unwrap();
        assert_eq!(auto.provides, [Feature::AutoLayer]);
        assert_eq!(auto.includes, ["<behaviors/num_word.dtsi>"]);
        assert!(auto
            .snippet
            .as_deref()
            .unwrap()
            .starts_with("nav_word: nav_word {"));
        assert!(catalog.checked.is_some());
    }

    const ONE: &str = r#"
format = 1
[[addon]]
id = "x"
name = "X"
author = "a"
url = "https://github.com/a/x"
module = "x"
ref = "v0.3"
zephyr = ["3.5"]
category = "Behaviors"
provides = ["auto-layer", "something-newer"]
summary = "s"
uses = "u"
exclude_boards = ["cyboard-imprint"]
"#;

    #[test]
    fn a_catalog_without_metadata_builds_from_the_ref() {
        let catalog = parse(ONE, None).unwrap();
        let addon = catalog.addon("x").unwrap();
        assert_eq!(addon.revision, "v0.3");
        // Features this version does not know are skipped, not fatal.
        assert_eq!(addon.provides, [Feature::AutoLayer]);
        assert!(addon.fits("3.5", "moergo-go60"));
        assert!(!addon.fits("3.5", "cyboard-imprint"));
        assert_eq!(addon.stars, None);

        let meta = r#"{"checked":"2026-10-06","addons":{"x":{"revision":"abc123","stars":5,"pushed":"2026-01-01","archived":true}},
            "candidates":[{"name":"new/one","url":"https://github.com/new/one","stars":3}]}"#;
        let catalog = parse(ONE, Some(meta)).unwrap();
        let addon = catalog.addon("x").unwrap();
        assert_eq!(
            (addon.revision.as_str(), addon.stars, addon.archived),
            ("abc123", Some(5), true)
        );
        assert_eq!(catalog.candidates[0].name, "new/one");
    }

    #[test]
    fn a_catalog_cannot_smuggle_text_into_build_files() {
        let bad_url = ONE.replace("https://github.com/a/x", "https://evil.example/x");
        assert!(matches!(
            parse(&bad_url, None),
            Err(CatalogError::Unsafe { field: "URL", .. })
        ));
        let bad_module = ONE.replace("module = \"x\"", "module = \"x\\n    - name: other\"");
        assert!(matches!(
            parse(&bad_module, None),
            Err(CatalogError::Unsafe {
                field: "module name",
                ..
            })
        ));
        let bad_include = format!("{ONE}includes = [\"<a.h>\\n#include <b.h>\"]\n");
        assert!(matches!(
            parse(&bad_include, None),
            Err(CatalogError::Unsafe {
                field: "include",
                ..
            })
        ));
        let bad_revision = r#"{"addons":{"x":{"revision":"abc\n  import: evil"}}}"#;
        assert!(matches!(
            parse(ONE, Some(bad_revision)),
            Err(CatalogError::Unsafe {
                field: "revision",
                ..
            })
        ));
        assert!(matches!(
            parse(&ONE.replace("format = 1", "format = 9"), None),
            Err(CatalogError::Newer { found: 9, .. })
        ));
        assert!(matches!(
            parse(
                &format!("{ONE}{}", &ONE[ONE.find("[[addon]]").unwrap()..]),
                None
            ),
            Err(CatalogError::Duplicate(_))
        ));
    }
}

#[cfg(test)]
mod install_tests {
    use super::*;

    #[test]
    fn only_a_more_recently_refreshed_catalog_replaces_the_one_in_use() {
        let shipped = catalog().checked.clone().unwrap();
        let older = r#"{"checked":"2000-01-01","addons":{}}"#;
        assert_eq!(install_if_newer(EMBEDDED, older), Ok(false));
        assert_eq!(catalog().checked.as_deref(), Some(shipped.as_str()));
        // One that fails its checks is refused whatever its date.
        assert!(install_if_newer("format = 99", r#"{"checked":"2999-01-01"}"#).is_err());
        assert_eq!(catalog().checked.as_deref(), Some(shipped.as_str()));
    }
}
