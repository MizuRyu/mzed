//! Command palette: command catalogue and pure selection logic.
//!
//! The palette is two modes: a fixed list of named commands (theme/zoom/sync
//! /file-search entry points), and a file-search mode that fuzzy-filters the
//! project's markdown files. Both reuse [`crate::fuzzy`] for ranking. All logic
//! here is UI-agnostic and unit-tested; `main.rs` wires actions to state.

use crate::fuzzy;

/// A command the palette can run. `Action` is dispatched by `main.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    SetThemeLight,
    SetThemeDark,
    SetThemeSystem,
    SetSyncAuto,
    SetSyncSelf,
    SetSyncOff,
    ToggleZedSync,
    /// Toggle sync mode between Auto ⇄ SelfPinned (Off → Auto).
    ToggleSyncPin,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    /// Enter file-search mode (filter project markdown files).
    FileSearch,
    /// Open the full-text search panel (search markdown contents).
    FullTextSearch,
    /// Export the active document as a self-contained HTML file.
    ExportHtml,
    /// Export the active document as PDF via the OS print dialog.
    ExportPdf,
    /// Copy the active file's absolute path to the clipboard.
    CopyFilePath,
}

/// A selectable command entry: a human label plus its action.
#[derive(Debug, Clone, Copy)]
pub struct Command {
    pub label: &'static str,
    pub action: Action,
}

/// The static command catalogue shown when the palette opens.
pub fn commands() -> Vec<Command> {
    vec![
        Command {
            label: "Theme: Light",
            action: Action::SetThemeLight,
        },
        Command {
            label: "Theme: Dark",
            action: Action::SetThemeDark,
        },
        Command {
            label: "Theme: System",
            action: Action::SetThemeSystem,
        },
        Command {
            label: "Sync Mode: Auto",
            action: Action::SetSyncAuto,
        },
        Command {
            label: "Sync Mode: Self",
            action: Action::SetSyncSelf,
        },
        Command {
            label: "Sync Mode: Off",
            action: Action::SetSyncOff,
        },
        Command {
            label: "Toggle Zed Sync",
            action: Action::ToggleZedSync,
        },
        Command {
            label: "Toggle Sync Pin (Auto ⇄ Self)",
            action: Action::ToggleSyncPin,
        },
        Command {
            label: "Zoom In",
            action: Action::ZoomIn,
        },
        Command {
            label: "Zoom Out",
            action: Action::ZoomOut,
        },
        Command {
            label: "Zoom Reset",
            action: Action::ZoomReset,
        },
        Command {
            label: "Search Files…",
            action: Action::FileSearch,
        },
        Command {
            label: "Search in Project…",
            action: Action::FullTextSearch,
        },
        Command {
            label: "Copy File Path",
            action: Action::CopyFilePath,
        },
        Command {
            label: "Export: HTML",
            action: Action::ExportHtml,
        },
        Command {
            label: "Export: PDF",
            action: Action::ExportPdf,
        },
    ]
}

/// Filter the command catalogue by `query`, ranked best-first.
pub fn filter_commands(query: &str) -> Vec<Command> {
    let all = commands();
    fuzzy::rank(query, &all, |c| c.label)
        .into_iter()
        .map(|(c, _)| *c)
        .collect()
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    #[test]
    fn 空クエリは全コマンドを返す() {
        let all = filter_commands("");
        assert_eq!(all.len(), commands().len());
    }

    #[test]
    fn クエリでコマンドを絞り込む() {
        let res = filter_commands("zoom");
        assert!(!res.is_empty());
        assert!(res.iter().all(|c| c.label.to_lowercase().contains("zoom")));
    }

    #[test]
    fn themeクエリはテーマ系を含む() {
        let res = filter_commands("theme");
        let labels: Vec<&str> = res.iter().map(|c| c.label).collect();
        assert!(labels.contains(&"Theme: Light"));
        assert!(labels.contains(&"Theme: Dark"));
    }

    #[test]
    fn マッチしないクエリは空() {
        assert!(filter_commands("zzzzz").is_empty());
    }

    #[test]
    fn toggle_sync_pinコマンドがカタログに含まれる() {
        let all = commands();
        assert!(all.iter().any(|c| c.action == Action::ToggleSyncPin));
    }

    #[test]
    fn syncピンクエリでtoggle_sync_pinが返る() {
        let res = filter_commands("sync pin");
        assert!(
            res.iter().any(|c| c.action == Action::ToggleSyncPin),
            "expected ToggleSyncPin in results for 'sync pin', got: {:?}",
            res.iter().map(|c| c.label).collect::<Vec<_>>()
        );
    }
}
