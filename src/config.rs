//! Persisted user settings (`~/.config/mzed/config.json`).
//!
//! [`Config`] holds the durable preferences (theme, sync mode, zoom). serde
//! does the round-trip; every field has a default via `#[serde(default)]` so a
//! partial or older file still loads. FS read/write is split into thin helpers
//! ([`load`]/[`save`]) that the unit tests skip.

use crate::theme::{SyncMode, Theme};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What to show on launch when no file/dir is passed on the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StartupBehavior {
    /// Restore the previous session (open tabs + roots).
    #[default]
    Restore,
    /// Follow Zed and show the active project's docs (no session restore).
    Docs,
    /// Start empty.
    Blank,
}

/// A single rebindable shortcut. `code` is a DOM `KeyboardEvent.code` value
/// (e.g. "KeyO", "Comma", "Backslash") so matching is keyboard-layout
/// independent. `meta` matches Cmd or Ctrl.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBinding {
    pub action: String,
    pub code: String,
    #[serde(default)]
    pub meta: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
}

impl KeyBinding {
    fn new(action: &str, code: &str, meta: bool, shift: bool, alt: bool) -> Self {
        Self {
            action: action.into(),
            code: code.into(),
            meta,
            shift,
            alt,
        }
    }
}

/// The built-in (default) rebindable shortcuts. Order is the display order in
/// the settings Hotkeys list. Zoom / Esc / tab-nav / pane-focus / Enter-rename
/// are fixed and not listed here.
pub fn default_keybindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("open_project_menu", "KeyO", true, false, false),
        KeyBinding::new("new_window", "KeyN", true, false, false),
        KeyBinding::new("quick_open", "KeyP", true, false, false),
        KeyBinding::new("palette_toggle", "KeyP", true, true, false),
        KeyBinding::new("full_text_search", "KeyF", true, true, false),
        KeyBinding::new("find_toggle", "KeyF", true, false, false),
        KeyBinding::new("toggle_sidebar", "KeyB", true, false, false),
        KeyBinding::new("toggle_split", "Backslash", true, false, false),
        KeyBinding::new("toggle_fav", "KeyD", true, false, false),
        KeyBinding::new("open_task_view", "KeyD", true, true, false),
        KeyBinding::new("copy_path", "KeyC", true, true, false),
        KeyBinding::new("close_tab", "KeyW", true, false, false),
        KeyBinding::new("settings", "Comma", true, false, false),
        KeyBinding::new("toggle_sync_pin", "KeyL", true, true, false),
    ]
}

/// Merge saved overrides over the defaults: for each default action use the
/// user's binding when present, else the default. Drops unknown/stale actions
/// and keeps the canonical display order.
pub fn merged_keybindings(saved: &[KeyBinding]) -> Vec<KeyBinding> {
    default_keybindings()
        .into_iter()
        .map(|def| {
            saved
                .iter()
                .find(|b| b.action == def.action)
                .cloned()
                .unwrap_or(def)
        })
        .collect()
}

/// Durable user preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub sync_mode: SyncMode,
    #[serde(default = "default_zoom")]
    pub zoom: f32,
    /// Quick Access bookmarks (files and project dirs), in display order.
    #[serde(default)]
    pub favorites: Vec<PathBuf>,
    #[serde(default = "default_window_width")]
    pub window_width: i32,
    #[serde(default = "default_window_height")]
    pub window_height: i32,
    /// Last known window X position (logical pixels). None = let OS decide.
    #[serde(default)]
    pub window_x: Option<i32>,
    /// Last known window Y position (logical pixels). None = let OS decide.
    #[serde(default)]
    pub window_y: Option<i32>,
    #[serde(default)]
    pub startup: StartupBehavior,
    #[serde(default = "default_true")]
    pub sidebar_visible_default: bool,
    #[serde(default = "default_true")]
    pub external_links_in_browser: bool,
    /// Code-block font family (empty = the built-in monospace stack).
    #[serde(default)]
    pub code_font: String,
    #[serde(default = "default_code_font_size")]
    pub code_font_size: i32,
    /// Rebindable shortcuts (overrides over [`default_keybindings`]).
    #[serde(default = "default_keybindings")]
    pub keybindings: Vec<KeyBinding>,
    /// Export output directory (None = the OS Downloads folder).
    #[serde(default)]
    pub export_dir: Option<PathBuf>,
    /// Feature flags for "extension-like" features (toggle in settings).
    #[serde(default = "default_true")]
    pub feature_katex: bool,
    #[serde(default = "default_true")]
    pub feature_html_export: bool,
    #[serde(default = "default_true")]
    pub feature_pdf_export: bool,
    /// When true, automatically open the most-recently-modified markdown file
    /// when switching to a project that has no previously parked tabs.
    /// Falls back to the existing representative-file pick when false (default).
    #[serde(default)]
    pub open_latest_on_project_open: bool,
    /// Body line-height (1.2–2.4). Injected as `!important` CSS so it overrides
    /// the static `line-height: 1.7` in mdo.css.
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    /// Enable the Task View mode (Cmd+Shift+D). Toggle in the Features tab.
    #[serde(default = "default_true")]
    pub feature_task_view: bool,
    /// Relative subpath within a project root where task folders live.
    #[serde(default = "default_task_view_tasks_subpath")]
    pub task_view_tasks_subpath: String,
    /// Root directories whose direct children are candidate projects for
    /// the "All Projects" scan. Empty = show current project only.
    #[serde(default)]
    pub task_view_scan_roots: Vec<PathBuf>,
    /// Number of days used in the "All Projects" date filter.
    #[serde(default = "default_task_view_days")]
    pub task_view_days: u32,
    /// Directory names to skip during the All Projects scan (in addition to
    /// the built-in prune list). Case-sensitive.
    #[serde(default)]
    pub task_view_scan_exclude: Vec<String>,
}

fn default_zoom() -> f32 {
    crate::theme::ZOOM_DEFAULT
}
fn default_window_width() -> i32 {
    1100
}
fn default_window_height() -> i32 {
    760
}
fn default_true() -> bool {
    true
}
fn default_code_font_size() -> i32 {
    14
}
fn default_line_height() -> f32 {
    1.7
}
fn default_task_view_tasks_subpath() -> String {
    "docs/memo/tasks".to_string()
}
fn default_task_view_days() -> u32 {
    7
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            sync_mode: SyncMode::default(),
            zoom: crate::theme::ZOOM_DEFAULT,
            favorites: Vec::new(),
            window_width: default_window_width(),
            window_height: default_window_height(),
            window_x: None,
            window_y: None,
            startup: StartupBehavior::default(),
            sidebar_visible_default: true,
            external_links_in_browser: true,
            code_font: String::new(),
            code_font_size: default_code_font_size(),
            keybindings: default_keybindings(),
            export_dir: None,
            feature_katex: true,
            feature_html_export: true,
            feature_pdf_export: true,
            open_latest_on_project_open: false,
            line_height: default_line_height(),
            feature_task_view: true,
            task_view_tasks_subpath: default_task_view_tasks_subpath(),
            task_view_scan_roots: Vec::new(),
            task_view_days: default_task_view_days(),
            task_view_scan_exclude: Vec::new(),
        }
    }
}

impl Config {
    /// Parse from JSON text, falling back to defaults for missing fields.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// Serialize to pretty JSON.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    fn to_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec_pretty(self)
    }
}

/// `~/.config/mzed/` (created on demand by [`save`]). Uses `dirs::config_dir()`,
/// which on macOS is `~/Library/Application Support`; we prefer the XDG-style
/// `~/.config` to match arto, falling back to `dirs` when HOME is unset.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(home) = dirs::home_dir() {
        return Some(home.join(".config/mzed"));
    }
    dirs::config_dir().map(|d| d.join("mzed"))
}

fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.json"))
}

/// Load the persisted config, or `Config::default()` if absent/unreadable.
pub fn load() -> Config {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| Config::from_json(&s).ok())
        .unwrap_or_default()
}

/// Atomically write the config to disk, creating its directory if needed.
#[allow(dead_code)]
pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path().context("failed to determine config.json target path")?;
    save_to_path(cfg, &path)
}

pub(crate) async fn save_queued(cfg: Config) -> Result<()> {
    let path = config_path().context("failed to determine config.json target path")?;
    save_to_path_queued(cfg, path).await
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn save_to_path(cfg: &Config, path: &Path) -> Result<()> {
    let json = cfg
        .to_json_bytes()
        .context("failed to serialize config before saving")?;
    crate::services::persistence::atomic_write(path, &json)
}

pub(crate) async fn save_to_path_queued(cfg: Config, path: PathBuf) -> Result<()> {
    let json = cfg
        .to_json_bytes()
        .context("failed to serialize config before saving")?;
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

    #[test]
    fn デフォルト値が想定どおり() {
        let c = Config::default();
        assert_eq!(c.theme, Theme::System);
        assert_eq!(c.sync_mode, SyncMode::Auto);
        assert!((c.zoom - crate::theme::ZOOM_DEFAULT).abs() < f32::EPSILON);
    }

    #[test]
    fn serdeラウンドトリップで一致する() {
        let c = Config {
            theme: Theme::Dark,
            sync_mode: SyncMode::SelfPinned,
            zoom: 1.3,
            favorites: vec![PathBuf::from("/proj/a.md"), PathBuf::from("/proj")],
            ..Config::default()
        };
        let parsed = Config::from_json(&c.to_json()).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn 欠損フィールドはデフォルトで補完される() {
        // Only theme present; sync_mode and zoom should default.
        let c = Config::from_json(r#"{ "theme": "dark" }"#).unwrap();
        assert_eq!(c.theme, Theme::Dark);
        assert_eq!(c.sync_mode, SyncMode::Auto);
        assert!((c.zoom - crate::theme::ZOOM_DEFAULT).abs() < f32::EPSILON);
    }

    #[test]
    fn 空オブジェクトは全デフォルト() {
        let c = Config::from_json("{}").unwrap();
        assert_eq!(c, Config::default());
    }

    #[test]
    fn syncはself表記でシリアライズされる() {
        let c = Config {
            sync_mode: SyncMode::SelfPinned,
            ..Config::default()
        };
        assert!(c.to_json().contains("\"self\""));
    }

    #[test]
    fn 不正なjsonはエラー() {
        assert!(Config::from_json("{ not json").is_err());
    }

    #[test]
    fn 任意pathへ保存できる() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/config.json");
        let config = Config {
            theme: Theme::Dark,
            zoom: 1.25,
            ..Config::default()
        };

        save_to_path(&config, &path).unwrap();

        let saved = std::fs::read_to_string(path).unwrap();
        assert_eq!(Config::from_json(&saved).unwrap(), config);
    }

    #[cfg(unix)]
    #[test]
    fn 保存時のserde失敗は既存ファイルを壊さずエラーにする() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, br#"{"theme":"dark"}"#).unwrap();
        let invalid_path = PathBuf::from(OsString::from_vec(vec![0xff]));
        let config = Config {
            favorites: vec![invalid_path],
            ..Config::default()
        };

        let error = save_to_path(&config, &path).unwrap_err();

        assert!(error.to_string().contains("config"));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"theme":"dark"}"#
        );
    }
}
