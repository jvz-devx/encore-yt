//! Recovery for locks around replaceable snapshots and cache containers.
//! These critical sections use safe container operations, not user callbacks.
//! A poisoned lock means a prior task panicked, not that its container is
//! unsafe to access. Report that fault without panicking again in cleanup.

use std::sync::LockResult;

pub(crate) trait Recover<T> {
    fn recover(self) -> T;
}

impl<T> Recover<T> for LockResult<T> {
    fn recover(self) -> T {
        self.unwrap_or_else(|poisoned| {
            log::warn!("recovering state after a worker panicked while holding its lock");
            poisoned.into_inner()
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions report worker failures")]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_poisoned_container_remains_accessible_for_cleanup() {
        let state = Arc::new(Mutex::new(vec![1]));
        let worker = state.clone();
        assert!(
            std::thread::spawn(move || {
                let mut values = worker.lock().unwrap();
                values.push(2);
                panic!("synthetic worker panic");
            })
            .join()
            .is_err()
        );
        state.lock().recover().clear();
        assert!(state.lock().recover().is_empty());
    }
}
