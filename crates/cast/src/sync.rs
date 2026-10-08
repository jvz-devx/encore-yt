//! Locks protect independent container operations, never user callbacks.
//! A panic cannot leave a partially updated protocol transaction under a lock.

use std::sync::{Mutex, MutexGuard};

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| {
        log::warn!("cast: recovering a poisoned container lock");
        mutex.clear_poison();
        poisoned.into_inner()
    })
}
