//! Keeps the ZMK add-on catalog current between app releases.
//!
//! The catalog ships inside the app. A newer one, refreshed by a scheduled
//! job in the project's repository, is fetched at most once a day, checked,
//! kept in the config folder and used from then on. Nothing else is sent
//! or fetched, and a copy that fails its checks is ignored.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use gpui_kit::App;

const BASE: &str =
    "https://raw.githubusercontent.com/mroboff/keyboard-curator/main/crates/kc-zmk/catalog";
const FILES: [&str; 2] = ["addons.toml", "addons-meta.json"];
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

fn cache_dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("Keyboard Curator").join("catalog"))
}

/// Uses the copy kept from an earlier fetch, if it is newer than the one
/// the app shipped with, and reports when it was fetched.
fn use_cached() -> Option<SystemTime> {
    let dir = cache_dir()?;
    let catalog = std::fs::read_to_string(dir.join(FILES[0])).ok()?;
    let meta = std::fs::read_to_string(dir.join(FILES[1])).ok()?;
    let _ = kc_zmk::addons::install_if_newer(&catalog, &meta);
    std::fs::metadata(dir.join(FILES[1])).ok()?.modified().ok()
}

fn fetch() -> Option<()> {
    let catalog = kc_build::fetch_text(&format!("{BASE}/{}", FILES[0])).ok()?;
    let meta = kc_build::fetch_text(&format!("{BASE}/{}", FILES[1])).ok()?;
    // Checked before it is kept, so that a bad copy is never cached.
    kc_zmk::addons::parse(&catalog, Some(&meta)).ok()?;
    let dir = cache_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::write(dir.join(FILES[0]), &catalog).ok()?;
    std::fs::write(dir.join(FILES[1]), &meta).ok()?;
    let _ = kc_zmk::addons::install_if_newer(&catalog, &meta);
    Some(())
}

/// Loads the kept copy and, if a day has passed since the last fetch,
/// looks for a newer one in the background.
pub fn start(cx: &mut App) {
    let fetched = use_cached();
    let due = fetched
        .and_then(|at| at.elapsed().ok())
        .is_none_or(|age| age >= CHECK_EVERY);
    if due {
        cx.background_executor()
            .spawn(async {
                let _ = fetch();
            })
            .detach();
    }
}
