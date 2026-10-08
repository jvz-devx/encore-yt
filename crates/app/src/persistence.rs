//! Coalesced preference writes. One pending value and one background writer
//! per file; dropping the app-owned writer flushes the final value at shutdown.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui_kit::BackgroundExecutor;
use serde::Serialize;

struct State {
    path: PathBuf,
    pending: Mutex<Option<Vec<u8>>>,
    writing: Mutex<()>,
}

impl State {
    fn flush(&self) {
        // These locks guard whole snapshot replacements, not user callbacks.
        let _writing = self
            .writing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let bytes = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(bytes) = bytes
            && let Err(error) = encore_core::paths::write_atomic(&self.path, &bytes)
        {
            log::warn!("couldn't save {}: {error}", self.path.display());
        }
    }
}

pub(crate) struct Writer {
    state: Arc<State>,
    wake: smol::channel::Sender<()>,
}

impl Writer {
    /// Finish pending writes before the process exits, even when Drop won't run.
    pub fn flush(&self) {
        self.state.flush();
    }

    pub fn new(path: PathBuf, executor: &BackgroundExecutor) -> Self {
        let state = Arc::new(State {
            path,
            pending: Mutex::new(None),
            writing: Mutex::new(()),
        });
        let (wake, work) = smol::channel::bounded(1);
        let background = state.clone();
        executor
            .spawn(async move {
                while work.recv().await.is_ok() {
                    background.flush();
                }
            })
            .detach();
        Self { state, wake }
    }

    pub fn save(&self, value: &impl Serialize) {
        let bytes = match serde_json::to_vec_pretty(value) {
            Ok(bytes) => bytes,
            Err(error) => {
                log::warn!("couldn't serialize {}: {error}", self.state.path.display());
                return;
            }
        };
        *self
            .state
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(bytes);
        match self.wake.try_send(()) {
            Ok(()) | Err(smol::channel::TrySendError::Full(())) => {}
            Err(smol::channel::TrySendError::Closed(())) => {
                log::warn!("preference writer stopped; final state will flush on shutdown")
            }
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // These writers live with the app/settings state, not individual views.
        // At shutdown, wait for any in-flight write before flushing the latest.
        self.state.flush();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests use a synthetic preference file")]
mod tests {
    use super::*;

    #[test]
    fn a_burst_keeps_one_snapshot_and_drop_flushes_the_last_value() {
        let path =
            std::env::temp_dir().join(format!("encore-preferences-test-{}", std::process::id()));
        let (wake, _work) = smol::channel::bounded(1);
        let writer = Writer {
            state: Arc::new(State {
                path: path.clone(),
                pending: Mutex::new(None),
                writing: Mutex::new(()),
            }),
            wake,
        };
        for value in 0..100 {
            writer.save(&value);
        }
        assert_eq!(writer.wake.len(), 1);
        writer.flush();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "99");
        writer.save(&100);
        drop(writer);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "100");
        std::fs::remove_file(path).unwrap();
    }
}
