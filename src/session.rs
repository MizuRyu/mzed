//! Last-session state (`~/.config/mzed/state.json`).
//!
//! [`Session`] captures the transient window state worth restoring next launch:
//! the project root(s), the open tabs and which was active, and the sidebar
//! width. serde round-trips it; missing fields default so an older/partial file
//! still loads. FS access is split into thin [`load`]/[`save`] helpers.

use crate::project_tabs::ProjectTabs;
use crate::tabs::Tabs;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Default sidebar width in pixels (matches the inline style in `main.rs`).
pub const DEFAULT_SIDEBAR_WIDTH: u32 = 280;

/// Serialisable snapshot of a single project's open tabs + active file.
/// Stored in [`Session::project_tabs`] keyed by the project's primary root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PerProjectTabs {
    #[serde(default)]
    pub tabs: Vec<PathBuf>,
    #[serde(default)]
    pub active: Option<PathBuf>,
}

fn default_sidebar_width() -> u32 {
    DEFAULT_SIDEBAR_WIDTH
}

/// The restorable state of the last session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Project root(s) shown in the sidebar (multi-root aware).
    #[serde(default)]
    pub roots: Vec<PathBuf>,
    /// Open tab paths, in order.
    #[serde(default)]
    pub tabs: Vec<PathBuf>,
    /// The active tab path, if any.
    #[serde(default)]
    pub active: Option<PathBuf>,
    /// Sidebar width in pixels.
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: u32,
    /// Tab state for all known projects, keyed by primary root path.
    /// Allows restoring the last-opened file per project across restarts.
    #[serde(default)]
    pub project_tabs: HashMap<PathBuf, PerProjectTabs>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            tabs: Vec::new(),
            active: None,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            project_tabs: HashMap::new(),
        }
    }
}

impl Session {
    /// Build a session from the live state (without per-project tab map).
    /// Prefer [`capture_full`] when the full `ProjectTabs` map is available.
    pub fn capture(roots: Vec<PathBuf>, tabs: &Tabs, sidebar_width: u32) -> Self {
        Self {
            roots,
            tabs: tabs.paths().to_vec(),
            active: tabs.active().cloned(),
            sidebar_width,
            project_tabs: HashMap::new(),
        }
    }

    /// Rebuild a [`Tabs`] from the persisted tab list + active path. Tabs whose
    /// files no longer exist are dropped so we never restore a dead tab.
    pub fn restore_tabs(&self) -> Tabs {
        let mut t = Tabs::default();
        for p in &self.tabs {
            if p.exists() {
                t.open(p.clone());
            }
        }
        if let Some(a) = self.active.as_ref() {
            if a.exists() {
                t.activate(a);
            }
        }
        t
    }

    /// Like [`capture`] but also snapshots all parked project tabs so they
    /// survive a restart. The current project's live tabs (in `tabs`) are merged
    /// with the parked set from `pt`; non-existent paths are kept (they may be on
    /// a temporarily-offline volume) and trimmed on [`restore_project_tabs`].
    pub fn capture_full(
        roots: Vec<PathBuf>,
        tabs: &Tabs,
        sidebar_width: u32,
        pt: &ProjectTabs,
    ) -> Self {
        let mut sess = Self::capture(roots.clone(), tabs, sidebar_width);
        let mut ptmap = HashMap::new();
        // Live (current) project.
        if let Some(primary) = roots.first() {
            ptmap.insert(
                primary.clone(),
                PerProjectTabs {
                    tabs: tabs.paths().to_vec(),
                    active: tabs.active().cloned(),
                },
            );
        }
        // Parked projects.
        for root in pt.roots() {
            if roots.first().is_some_and(|p| p == root) {
                continue; // already captured above
            }
            if let Some(parked) = pt.get(root) {
                ptmap.insert(
                    root.clone(),
                    PerProjectTabs {
                        tabs: parked.paths().to_vec(),
                        active: parked.active().cloned(),
                    },
                );
            }
        }
        sess.project_tabs = ptmap;
        sess
    }

    /// Rebuild a [`ProjectTabs`] from the persisted per-project tab map.
    /// Tabs whose files no longer exist are dropped so we never restore dead tabs.
    pub fn restore_project_tabs(&self) -> ProjectTabs {
        let mut pt = ProjectTabs::default();
        for (root, entry) in &self.project_tabs {
            let mut t = Tabs::default();
            for p in &entry.tabs {
                if p.exists() {
                    t.open(p.clone());
                }
            }
            if let Some(a) = &entry.active {
                if a.exists() {
                    t.activate(a);
                }
            }
            if !t.paths().is_empty() {
                pt.park(root.clone(), t);
            }
        }
        pt
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    fn to_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec_pretty(self)
    }
}

fn state_path() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("state.json"))
}

/// Load the persisted session, or `Session::default()` if absent/unreadable.
pub fn load() -> Session {
    state_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| Session::from_json(&s).ok())
        .unwrap_or_default()
}

/// Atomically write the session to disk, creating its directory if needed.
#[allow(dead_code)]
pub fn save(sess: &Session) -> Result<()> {
    let path = state_path().context("failed to determine state.json target path")?;
    save_to_path(sess, &path)
}

pub(crate) async fn save_queued(sess: Session) -> Result<()> {
    let path = state_path().context("failed to determine state.json target path")?;
    save_to_path_queued(sess, path).await
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn save_to_path(sess: &Session, path: &Path) -> Result<()> {
    let json = sess
        .to_json_bytes()
        .context("failed to serialize session before saving")?;
    crate::services::persistence::atomic_write(path, &json)
}

pub(crate) async fn save_to_path_queued(sess: Session, path: PathBuf) -> Result<()> {
    let json = sess
        .to_json_bytes()
        .context("failed to serialize session before saving")?;
    crate::services::persistence::atomic_write_queued(path, json).await
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn デフォルト値が想定どおり() {
        let s = Session::default();
        assert!(s.roots.is_empty());
        assert!(s.tabs.is_empty());
        assert_eq!(s.active, None);
        assert_eq!(s.sidebar_width, DEFAULT_SIDEBAR_WIDTH);
    }

    #[test]
    fn serdeラウンドトリップで一致する() {
        let s = Session {
            roots: vec![p("/proj")],
            tabs: vec![p("/proj/a.md"), p("/proj/b.md")],
            active: Some(p("/proj/b.md")),
            sidebar_width: 320,
            ..Session::default()
        };
        let parsed = Session::from_json(&s.to_json()).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn 欠損フィールドはデフォルトで補完される() {
        let s = Session::from_json(r#"{ "tabs": ["/x.md"] }"#).unwrap();
        assert_eq!(s.tabs, vec![p("/x.md")]);
        assert!(s.roots.is_empty());
        assert_eq!(s.active, None);
        assert_eq!(s.sidebar_width, DEFAULT_SIDEBAR_WIDTH);
    }

    #[test]
    fn 空オブジェクトは全デフォルト() {
        let s = Session::from_json("{}").unwrap();
        assert_eq!(s, Session::default());
    }

    #[test]
    fn captureは現在のタブ状態を取り込む() {
        let mut t = Tabs::default();
        t.open(p("/a.md"));
        t.open(p("/b.md"));
        let s = Session::capture(vec![p("/root")], &t, 300);
        assert_eq!(s.roots, vec![p("/root")]);
        assert_eq!(s.tabs, vec![p("/a.md"), p("/b.md")]);
        assert_eq!(s.active, Some(p("/b.md")));
        assert_eq!(s.sidebar_width, 300);
    }

    #[test]
    fn restore_tabsは存在しないファイルを除外する() {
        // Neither path exists, so the restored Tabs is empty.
        let s = Session {
            tabs: vec![p("/definitely/missing/a.md")],
            active: Some(p("/definitely/missing/a.md")),
            ..Session::default()
        };
        let t = s.restore_tabs();
        assert!(t.paths().is_empty());
        assert_eq!(t.active(), None);
    }

    #[test]
    fn 不正なjsonはエラー() {
        assert!(Session::from_json("{ bad").is_err());
    }

    #[test]
    fn 任意pathへ保存できる() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/state.json");
        let session = Session {
            roots: vec![p("/project")],
            tabs: vec![p("/project/readme.md")],
            active: Some(p("/project/readme.md")),
            sidebar_width: 340,
            ..Session::default()
        };

        save_to_path(&session, &path).unwrap();

        let saved = std::fs::read_to_string(path).unwrap();
        assert_eq!(Session::from_json(&saved).unwrap(), session);
    }

    #[test]
    fn capture_fullは現プロジェクトとパーク済みを統合する() {
        use crate::project_tabs::ProjectTabs;
        let mut pt = ProjectTabs::default();
        // /b はパーク済み。
        let b_tabs = {
            let mut t = Tabs::default();
            t.open(p("/b/doc.md"));
            t
        };
        pt.park(p("/b"), b_tabs.clone());

        let mut live = Tabs::default();
        live.open(p("/a/readme.md"));

        let sess = Session::capture_full(vec![p("/a")], &live, 300, &pt);

        assert_eq!(sess.roots, vec![p("/a")]);
        // 現プロジェクト /a のタブが記録される。
        let a_entry = sess.project_tabs.get(&p("/a")).unwrap();
        assert_eq!(a_entry.tabs, vec![p("/a/readme.md")]);
        assert_eq!(a_entry.active, Some(p("/a/readme.md")));
        // パーク済み /b も記録される。
        let b_entry = sess.project_tabs.get(&p("/b")).unwrap();
        assert_eq!(b_entry.tabs, vec![p("/b/doc.md")]);
    }

    #[test]
    fn restore_project_tabsは存在しないファイルを除外する() {
        let sess = Session {
            project_tabs: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    p("/ghost"),
                    PerProjectTabs {
                        tabs: vec![p("/ghost/missing.md")],
                        active: Some(p("/ghost/missing.md")),
                    },
                );
                m
            },
            ..Session::default()
        };
        let pt = sess.restore_project_tabs();
        // /ghost には存在しないファイルしかないので parked にならない。
        assert!(pt.get(&p("/ghost")).is_none());
    }

    #[test]
    fn 旧フォーマットのjsonはproject_tabsなしでデフォルト補完される() {
        let json = r#"{"roots":["/proj"],"tabs":["/proj/a.md"],"active":"/proj/a.md","sidebar_width":280}"#;
        let sess = Session::from_json(json).unwrap();
        assert!(sess.project_tabs.is_empty());
    }

    #[test]
    fn capture_fullはproject_tabsをserdeラウンドトリップできる() {
        use crate::project_tabs::ProjectTabs;
        let pt = ProjectTabs::default();
        let mut live = Tabs::default();
        live.open(p("/proj/x.md"));
        let sess = Session::capture_full(vec![p("/proj")], &live, 280, &pt);
        let json = sess.to_json();
        let restored = Session::from_json(&json).unwrap();
        assert_eq!(restored.project_tabs, sess.project_tabs);
    }

    #[cfg(unix)]
    #[test]
    fn 保存時のserde失敗は既存ファイルを壊さずエラーにする() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, br#"{"tabs":[]}"#).unwrap();
        let invalid_path = PathBuf::from(OsString::from_vec(vec![0xff]));
        let session = Session {
            tabs: vec![invalid_path],
            ..Session::default()
        };

        let error = save_to_path(&session, &path).unwrap_err();

        assert!(error.to_string().contains("session"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"tabs":[]}"#);
    }
}
