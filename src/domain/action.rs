//! Typed commands received from the WebView keyboard bridge.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppCommand {
    PaletteToggle,
    OpenProjectMenu,
    NewWindow,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Settings,
    ToggleSidebar,
    FindToggle,
    QuickOpen,
    FullTextSearch,
    CopyPath,
    ToggleFav,
    ToggleSplit,
    FocusPane {
        index: u8,
    },
    CloseTab,
    NextTab,
    PrevTab,
    RenameActive,
    /// Toggle sync mode between Auto and SelfPinned (Off -> Auto).
    ToggleSyncPin,
    /// Toggle the Task View mode (Cmd+Shift+D). No-op when feature_task_view is off.
    OpenTaskView,
    /// Re-scan the Task View task list (Cmd+R). No-op unless Task View is open.
    TaskViewRefresh,
    /// Toggle Task View scope This Project ⇄ All Projects (Ctrl+Tab).
    /// Falls back to NextTab behaviour when Task View is closed, since the
    /// default binding shadows the fixed Ctrl+Tab tab-switch shortcut.
    TaskViewToggleScope,
    Escape,
}

impl AppCommand {
    pub fn from_value(value: &serde_json::Value) -> serde_json::Result<Self> {
        serde_json::from_value(value.clone())
    }
}
