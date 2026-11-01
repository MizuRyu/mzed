//! The process-wide pin list (`~/.config/mzed/pins.json`). One list serves
//! every window and the IPC router, so a pin lands even before a window
//! exists. It lives outside `state.json`, which the base window rewrites whole.

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

use crate::app_state::pins::{self, Pin};
use crate::config::PinExpiry;

static PINS: LazyLock<Mutex<Vec<Pin>>> = LazyLock::new(|| Mutex::new(load()));

fn pins_path() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("pins.json"))
}

fn load() -> Vec<Pin> {
    pins_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub(crate) fn snapshot() -> Vec<Pin> {
    PINS.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Apply `edit`; when it reports a change, persist. The write is queued while
/// the lock is held, so concurrent edits reach disk in the order they applied.
fn update(edit: impl FnOnce(&mut Vec<Pin>) -> bool) -> bool {
    let mut list = PINS.lock().unwrap_or_else(PoisonError::into_inner);
    if !edit(&mut list) {
        return false;
    }
    match (pins_path(), serde_json::to_vec_pretty(&*list)) {
        (Some(path), Ok(bytes)) => super::persistence::atomic_write_detached(path, bytes),
        _ => crate::logging::app("pins: could not serialise or locate pins.json"),
    }
    true
}

/// Pin a markdown file. The path comes from the IPC socket or argv, so it is
/// canonicalised and checked here, not trusted. Returns whether it was pinned.
pub(crate) fn pin(path: &Path) -> bool {
    let path = super::file_service::canonical_path_or_original(path.to_path_buf());
    if !(path.is_file() && crate::files::is_markdown(&path)) {
        return false;
    }
    let now = crate::app_state::unread::now();
    update(|list| {
        pins::add(list, path, now);
        true
    })
}

pub(crate) fn mark_opened(path: &Path, expiry: PinExpiry) -> bool {
    update(|list| pins::mark_opened(list, path, expiry))
}

pub(crate) fn remove(path: &Path) -> bool {
    update(|list| pins::remove(list, path))
}

/// Drop expired pins and pins whose file is gone. Stats every pin, so call it
/// off the UI thread.
pub(crate) fn prune(expiry: PinExpiry) -> bool {
    let now = crate::app_state::unread::now();
    update(|list| {
        let expired = pins::prune(list, expiry, now);
        let before = list.len();
        list.retain(|p| p.path.is_file());
        expired || list.len() != before
    })
}
