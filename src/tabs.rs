//! Open-tab set management (pure logic, UI-agnostic).
//!
//! A `Tabs` holds the ordered list of open file paths plus the active path.
//! Opening an already-open file just activates it (no duplicates); closing
//! removes it and picks a sensible neighbour as the new active tab.

use std::path::{Path, PathBuf};

/// The set of open markdown tabs and which one is active.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Tabs {
    paths: Vec<PathBuf>,
    active: Option<PathBuf>,
}

impl Tabs {
    /// Open `path`, activating it. If already open, no duplicate is added; the
    /// existing tab simply becomes active.
    pub fn open(&mut self, path: PathBuf) {
        if !self.paths.contains(&path) {
            self.paths.push(path.clone());
        }
        self.active = Some(path);
    }

    /// Close the tab for `path`. If it was active, the previous tab (or the new
    /// last tab) becomes active; closing the last tab clears the active path.
    pub fn close(&mut self, path: &Path) {
        let Some(idx) = self.paths.iter().position(|p| p == path) else {
            return;
        };
        let was_active = self.active.as_deref() == Some(path);
        self.paths.remove(idx);
        if was_active {
            self.active = if self.paths.is_empty() {
                None
            } else {
                // Prefer the tab now at the same index (the one to the right),
                // else the new last tab.
                let next = idx.min(self.paths.len() - 1);
                Some(self.paths[next].clone())
            };
        }
    }

    /// Make `path` the active tab if it is open.
    pub fn activate(&mut self, path: &Path) {
        if self.paths.iter().any(|p| p == path) {
            self.active = Some(path.to_path_buf());
        }
    }

    /// Replace an open tab path after an on-disk rename. Keeps tab order and
    /// preserves the active selection when the renamed path was active.
    pub fn replace_path(&mut self, old: &Path, new: PathBuf) {
        for path in &mut self.paths {
            if path == old {
                *path = new.clone();
            }
        }
        if self.active.as_deref() == Some(old) {
            self.active = Some(new);
        }
    }

    /// Close the currently active tab (no-op if none). A sensible neighbour
    /// becomes active, matching `close`.
    pub fn close_active(&mut self) {
        if let Some(active) = self.active.clone() {
            self.close(&active);
        }
    }

    /// Activate the next tab, wrapping around to the first. No-op if empty.
    pub fn activate_next(&mut self) {
        if self.paths.is_empty() {
            return;
        }
        let idx = self.active_index().unwrap_or(0);
        let next = (idx + 1) % self.paths.len();
        self.active = Some(self.paths[next].clone());
    }

    /// Activate the previous tab, wrapping around to the last. No-op if empty.
    pub fn activate_prev(&mut self) {
        if self.paths.is_empty() {
            return;
        }
        let idx = self.active_index().unwrap_or(0);
        let prev = (idx + self.paths.len() - 1) % self.paths.len();
        self.active = Some(self.paths[prev].clone());
    }

    /// Activate the tab at `index` (0-based). Out-of-range indices are ignored.
    // Retained for tab-index navigation; currently only exercised by tests since
    // Cmd+1/2 were repurposed for pane focus.
    #[allow(dead_code)]
    pub fn activate_index(&mut self, index: usize) {
        if let Some(p) = self.paths.get(index) {
            self.active = Some(p.clone());
        }
    }

    /// Index of the active path within `paths`, if any.
    fn active_index(&self) -> Option<usize> {
        let active = self.active.as_ref()?;
        self.paths.iter().position(|p| p == active)
    }

    /// Ordered open paths.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// The active path, if any.
    pub fn active(&self) -> Option<&PathBuf> {
        self.active.as_ref()
    }
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn openはタブを追加しアクティブにする() {
        let mut t = Tabs::default();
        t.open(p("/a.md"));
        t.open(p("/b.md"));
        assert_eq!(t.paths(), &[p("/a.md"), p("/b.md")]);
        assert_eq!(t.active(), Some(&p("/b.md")));
    }

    #[test]
    fn open済みファイルは重複せずアクティブ化される() {
        let mut t = Tabs::default();
        t.open(p("/a.md"));
        t.open(p("/b.md"));
        t.open(p("/a.md"));
        assert_eq!(t.paths(), &[p("/a.md"), p("/b.md")]);
        assert_eq!(t.active(), Some(&p("/a.md")));
    }

    #[test]
    fn アクティブタブを閉じると隣がアクティブになる() {
        let mut t = Tabs::default();
        t.open(p("/a.md"));
        t.open(p("/b.md"));
        t.open(p("/c.md"));
        t.activate(&p("/b.md"));
        t.close(&p("/b.md"));
        // b の位置(index 1)に来た c がアクティブ。
        assert_eq!(t.paths(), &[p("/a.md"), p("/c.md")]);
        assert_eq!(t.active(), Some(&p("/c.md")));
    }

    #[test]
    fn 末尾のアクティブタブを閉じると新末尾がアクティブ() {
        let mut t = Tabs::default();
        t.open(p("/a.md"));
        t.open(p("/b.md"));
        t.close(&p("/b.md"));
        assert_eq!(t.active(), Some(&p("/a.md")));
    }

    #[test]
    fn 非アクティブタブを閉じてもアクティブは変わらない() {
        let mut t = Tabs::default();
        t.open(p("/a.md"));
        t.open(p("/b.md"));
        t.activate(&p("/b.md"));
        t.close(&p("/a.md"));
        assert_eq!(t.paths(), &[p("/b.md")]);
        assert_eq!(t.active(), Some(&p("/b.md")));
    }

    #[test]
    fn 最後のタブを閉じるとアクティブは無くなる() {
        let mut t = Tabs::default();
        t.open(p("/a.md"));
        t.close(&p("/a.md"));
        assert!(t.paths().is_empty());
        assert_eq!(t.active(), None);
    }

    #[test]
    fn close_activeはアクティブを閉じ隣をアクティブにする() {
        let mut t = Tabs::default();
        t.open(p("/a.md"));
        t.open(p("/b.md"));
        t.open(p("/c.md"));
        t.activate(&p("/b.md"));
        t.close_active();
        assert_eq!(t.paths(), &[p("/a.md"), p("/c.md")]);
        assert_eq!(t.active(), Some(&p("/c.md")));
    }

    #[test]
    fn close_activeはタブが空なら何もしない() {
        let mut t = Tabs::default();
        t.close_active();
        assert!(t.paths().is_empty());
        assert_eq!(t.active(), None);
    }

    #[test]
    fn activate_nextは次のタブへ移り末尾で循環する() {
        let mut t = Tabs::default();
        t.open(p("/a.md"));
        t.open(p("/b.md"));
        t.open(p("/c.md"));
        t.activate(&p("/a.md"));
        t.activate_next();
        assert_eq!(t.active(), Some(&p("/b.md")));
        t.activate_next();
        assert_eq!(t.active(), Some(&p("/c.md")));
        // 末尾から先頭へ循環。
        t.activate_next();
        assert_eq!(t.active(), Some(&p("/a.md")));
    }

    #[test]
    fn activate_prevは前のタブへ移り先頭で循環する() {
        let mut t = Tabs::default();
        t.open(p("/a.md"));
        t.open(p("/b.md"));
        t.open(p("/c.md"));
        t.activate(&p("/a.md"));
        // 先頭から末尾へ循環。
        t.activate_prev();
        assert_eq!(t.active(), Some(&p("/c.md")));
        t.activate_prev();
        assert_eq!(t.active(), Some(&p("/b.md")));
    }

    #[test]
    fn activate_indexは指定indexをアクティブにする() {
        let mut t = Tabs::default();
        t.open(p("/a.md"));
        t.open(p("/b.md"));
        t.open(p("/c.md"));
        t.activate_index(0);
        assert_eq!(t.active(), Some(&p("/a.md")));
        t.activate_index(2);
        assert_eq!(t.active(), Some(&p("/c.md")));
    }

    #[test]
    fn 範囲外indexは無視される() {
        let mut t = Tabs::default();
        t.open(p("/a.md"));
        t.open(p("/b.md"));
        t.activate_index(0);
        t.activate_index(5);
        // 範囲外なのでアクティブは変わらない。
        assert_eq!(t.active(), Some(&p("/a.md")));
    }

    #[test]
    fn activate_nextとprevはタブが空なら何もしない() {
        let mut t = Tabs::default();
        t.activate_next();
        assert_eq!(t.active(), None);
        t.activate_prev();
        assert_eq!(t.active(), None);
    }

    #[test]
    fn replace_pathはアクティブタブと順序を維持してパスを更新する() {
        let mut t = Tabs::default();
        t.open(p("/a.md"));
        t.open(p("/old.md"));

        t.replace_path(&p("/old.md"), p("/new.md"));

        assert_eq!(t.paths(), &[p("/a.md"), p("/new.md")]);
        assert_eq!(t.active(), Some(&p("/new.md")));
    }

    #[test]
    fn replace_pathは非アクティブタブを更新してアクティブは変えない() {
        let mut t = Tabs::default();
        t.open(p("/old.md"));
        t.open(p("/active.md"));

        t.replace_path(&p("/old.md"), p("/new.md"));

        assert_eq!(t.paths(), &[p("/new.md"), p("/active.md")]);
        assert_eq!(t.active(), Some(&p("/active.md")));
    }
}
