//! UI-thread settings storage and persistence. Effective settings include
//! environment overrides; only the saved settings are written to disk.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::VisualsConfig;

/// The saved settings and the ones in effect (with the overrides).
#[derive(Default)]
struct Store {
    saved: VisualsConfig,
    effective: Rc<VisualsConfig>,
    path: Option<PathBuf>,
    /// Counts changes, so the effects can tell a still picture is stale.
    revision: u64,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store {
        effective: Rc::new(VisualsConfig::default().overridden()),
        ..Store::default()
    });
}

/// Reads `visuals.json` from the config directory `dir`, once.
pub fn load(dir: &Path) {
    let path = dir.join("visuals.json");
    let saved: VisualsConfig = std::fs::read(&path)
        .ok()
        .and_then(
            |bytes| match serde_json::from_slice::<VisualsConfig>(&bytes) {
                Ok(c) => Some(c),
                Err(e) => {
                    log::warn!(
                        "visuals: {} unreadable, using defaults: {e}",
                        path.display()
                    );
                    None
                }
            },
        )
        .unwrap_or_default()
        .clamped();
    STORE.with_borrow_mut(|s| {
        s.effective = Rc::new(saved.clone().overridden());
        s.saved = saved;
        s.path = Some(path);
        s.revision += 1;
    });
}

/// The settings in effect.
pub fn get() -> Rc<VisualsConfig> {
    STORE.with_borrow(|s| s.effective.clone())
}

/// The settings as saved (what Settings shows).
pub fn saved() -> VisualsConfig {
    STORE.with_borrow(|s| s.saved.clone())
}

/// Bumped on every change.
pub fn revision() -> u64 {
    STORE.with_borrow(|s| s.revision)
}

/// New settings, in effect at once; written to the file when `save` (a
/// slider saves when it is let go).
pub fn set(config: VisualsConfig, save: bool) {
    let config = config.clamped();
    let path = STORE.with_borrow_mut(|s| {
        s.effective = Rc::new(config.clone().overridden());
        s.saved = config.clone();
        s.revision += 1;
        s.path.clone()
    });
    if save && let Some(path) = path {
        write(&path, &config);
    }
}

fn write(path: &Path, config: &VisualsConfig) {
    let written = serde_json::to_vec_pretty(config)
        .map_err(std::io::Error::other)
        .and_then(|bytes| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            encore_core::paths::write_atomic(path, &bytes)
        });
    if let Err(e) = written {
        log::warn!("visuals: couldn't save {}: {e}", path.display());
    }
}
