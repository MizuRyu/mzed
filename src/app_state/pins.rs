//! Pins: markdown files an agent flagged for the reader with `mzed --pin`,
//! listed above Quick Access in every window whatever project is showing.
//! Newest first; each one leaves by hand, on open, or after `PinExpiry`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::PinExpiry;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub path: PathBuf,
    /// Seconds since the epoch, the clock [`prune`] measures expiry against.
    pub pinned_at: u64,
    /// Opened since it was pinned; an unopened pin carries the green dot.
    #[serde(default)]
    pub opened: bool,
}

/// Put `path` on top as unopened. Re-pinning a file already listed moves it
/// up and restarts its expiry instead of listing it twice.
pub fn add(pins: &mut Vec<Pin>, path: PathBuf, now: u64) {
    pins.retain(|p| p.path != path);
    pins.insert(
        0,
        Pin {
            path,
            pinned_at: now,
            opened: false,
        },
    );
}

/// The reader opened `path`: drop its pin under [`PinExpiry::OnOpen`], else
/// clear its dot. Returns whether the list changed.
pub fn mark_opened(pins: &mut Vec<Pin>, path: &Path, expiry: PinExpiry) -> bool {
    let Some(i) = pins.iter().position(|p| p.path == path) else {
        return false;
    };
    if expiry == PinExpiry::OnOpen {
        pins.remove(i);
        return true;
    }
    !std::mem::replace(&mut pins[i].opened, true)
}

pub fn remove(pins: &mut Vec<Pin>, path: &Path) -> bool {
    let before = pins.len();
    pins.retain(|p| p.path != path);
    pins.len() != before
}

/// Drop pins older than the expiry's lifetime. Returns whether any went.
pub fn prune(pins: &mut Vec<Pin>, expiry: PinExpiry, now: u64) -> bool {
    let Some(ttl) = expiry.ttl_secs() else {
        return false;
    };
    let before = pins.len();
    pins.retain(|p| now.saturating_sub(p.pinned_at) < ttl);
    pins.len() != before
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    fn paths(pins: &[Pin]) -> Vec<&str> {
        pins.iter().map(|p| p.path.to_str().unwrap()).collect()
    }

    #[test]
    fn 新しいピンは先頭に入り同じファイルは重複せず先頭に戻って期限もやり直す() {
        let mut pins = Vec::new();
        add(&mut pins, PathBuf::from("/a.md"), 10);
        add(&mut pins, PathBuf::from("/b.md"), 20);
        pins[1].opened = true;
        add(&mut pins, PathBuf::from("/a.md"), 30);
        assert_eq!(paths(&pins), vec!["/a.md", "/b.md"]);
        assert_eq!(pins[0].pinned_at, 30);
        assert!(!pins[0].opened);
    }

    #[test]
    fn 開いたら既定では印だけ消え開いたら消す設定ではピンごと消える() {
        let mut pins = Vec::new();
        add(&mut pins, PathBuf::from("/a.md"), 10);
        assert!(mark_opened(&mut pins, Path::new("/a.md"), PinExpiry::Day));
        assert!(pins[0].opened);
        // 2 回目は変化なし（保存し直さない）。
        assert!(!mark_opened(&mut pins, Path::new("/a.md"), PinExpiry::Day));
        assert!(!mark_opened(
            &mut pins,
            Path::new("/other.md"),
            PinExpiry::OnOpen
        ));
        assert!(mark_opened(
            &mut pins,
            Path::new("/a.md"),
            PinExpiry::OnOpen
        ));
        assert!(pins.is_empty());
    }

    #[test]
    fn 期限を過ぎたピンだけ消え期限なしの設定では残る() {
        let mut pins = Vec::new();
        add(&mut pins, PathBuf::from("/old.md"), 0);
        add(&mut pins, PathBuf::from("/new.md"), 3_000);
        assert!(!prune(&mut pins, PinExpiry::Never, 1_000_000));
        assert!(!prune(&mut pins, PinExpiry::OnOpen, 1_000_000));
        assert!(prune(&mut pins, PinExpiry::Hour, 3_600));
        assert_eq!(paths(&pins), vec!["/new.md"]);
        assert!(!prune(&mut pins, PinExpiry::Hour, 3_600));
    }

    #[test]
    fn 外したピンだけ消える() {
        let mut pins = Vec::new();
        add(&mut pins, PathBuf::from("/a.md"), 1);
        add(&mut pins, PathBuf::from("/b.md"), 2);
        assert!(remove(&mut pins, Path::new("/a.md")));
        assert!(!remove(&mut pins, Path::new("/a.md")));
        assert_eq!(paths(&pins), vec!["/b.md"]);
    }
}
