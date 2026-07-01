//! Per-project tab retention (pure logic, UI-agnostic).
//!
//! mzed keeps one live [`Tabs`] set for the *current* project but stashes every
//! other project's tabs in a [`ProjectTabs`] map keyed by that project's primary
//! root. Switching projects parks the current tabs and restores (or creates) the
//! target project's tabs, so each project remembers what was open.

use crate::tabs::Tabs;
use std::collections::HashMap;
use std::path::PathBuf;

/// Map of primary-root -> that project's parked [`Tabs`].
#[derive(Debug, Default, Clone)]
pub struct ProjectTabs {
    map: HashMap<PathBuf, Tabs>,
}

impl ProjectTabs {
    /// Stash `current`'s tabs under `old_root` (if any), then take the tabs for
    /// `new_root`: the previously parked set if known, else a fresh empty set.
    ///
    /// When `old_root == new_root` this is a no-op re-selection of the same
    /// project: `current` is returned unchanged and nothing is parked.
    pub fn switch(
        &mut self,
        old_root: Option<&PathBuf>,
        current: &Tabs,
        new_root: &PathBuf,
    ) -> Tabs {
        if old_root == Some(new_root) {
            return current.clone();
        }
        if let Some(old) = old_root {
            self.map.insert(old.clone(), current.clone());
        }
        self.map.get(new_root).cloned().unwrap_or_default()
    }

    /// The parked tabs for `root`, if any (does not remove them).
    #[allow(dead_code)] // Test/introspection helper.
    pub fn get(&self, root: &PathBuf) -> Option<&Tabs> {
        self.map.get(root)
    }

    /// Stash `tabs` under `root` (overwriting any existing entry).
    #[allow(dead_code)] // Test helper.
    pub fn park(&mut self, root: PathBuf, tabs: Tabs) {
        self.map.insert(root, tabs);
    }

    /// Roots that have parked tabs (projects opened at least once in mzed).
    pub fn roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.map.keys()
    }
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn tabs(paths: &[&str]) -> Tabs {
        let mut t = Tabs::default();
        for p in paths {
            t.open(PathBuf::from(p));
        }
        t
    }

    #[test]
    fn 別プロジェクトに切り替えると旧タブが退避され新タブに入れ替わる() {
        let mut pt = ProjectTabs::default();
        let cur = tabs(&["/a/x.md", "/a/y.md"]);
        // /a から /b へ切替。/b は未知なので空タブ。
        let new = pt.switch(Some(&p("/a")), &cur, &p("/b"));
        assert!(new.paths().is_empty());
        // 旧タブ /a は退避されている。
        assert_eq!(pt.get(&p("/a")), Some(&cur));
    }

    #[test]
    fn 退避済みプロジェクトに戻るとタブが復元される() {
        let mut pt = ProjectTabs::default();
        let a_tabs = tabs(&["/a/x.md"]);
        pt.park(p("/a"), a_tabs.clone());
        // /b から /a へ戻る。
        let restored = pt.switch(Some(&p("/b")), &tabs(&["/b/z.md"]), &p("/a"));
        assert_eq!(restored, a_tabs);
        // /b も退避された。
        assert_eq!(pt.get(&p("/b")), Some(&tabs(&["/b/z.md"])));
    }

    #[test]
    fn 同じプロジェクトならタブは維持される() {
        let mut pt = ProjectTabs::default();
        let cur = tabs(&["/a/x.md", "/a/y.md"]);
        let same = pt.switch(Some(&p("/a")), &cur, &p("/a"));
        assert_eq!(same, cur);
        // 退避は発生しない。
        assert!(pt.roots().next().is_none());
    }

    #[test]
    fn 未知のプロジェクトは空タブから始まる() {
        let mut pt = ProjectTabs::default();
        let new = pt.switch(None, &Tabs::default(), &p("/fresh"));
        assert!(new.paths().is_empty());
    }

    #[test]
    fn old_rootがNoneでも新タブを返し退避しない() {
        let mut pt = ProjectTabs::default();
        pt.park(p("/a"), tabs(&["/a/x.md"]));
        // 初回起動など旧 root が無い場合。
        let new = pt.switch(None, &tabs(&["/ignored.md"]), &p("/a"));
        assert_eq!(new, tabs(&["/a/x.md"]));
    }
}
