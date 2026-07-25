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
    /// Start/stop the `mzed serve` share server for the current project.
    ToggleWebShare,
}

/// A selectable command entry: a human label plus its action.
#[derive(Debug, Clone)]
pub struct Command {
    pub label: String,
    pub action: Action,
}

impl Command {
    fn new(label: impl Into<String>, action: Action) -> Self {
        Self {
            label: label.into(),
            action,
        }
    }
}

/// The command catalogue shown when the palette opens. `share_url` is the
/// running Web Share server's URL, if any: the share entry reads Start or
/// Stop accordingly, so a second toggle isn't mistaken for "re-open" (the
/// stop left an open browser tab pointing at a dead port).
pub fn commands(share_url: Option<&str>) -> Vec<Command> {
    let share_label = match share_url {
        Some(url) => format!("Web Share: Stop ({url})"),
        None => "Web Share: Start (Serve in Browser)".to_string(),
    };
    vec![
        Command::new("Theme: Light", Action::SetThemeLight),
        Command::new("Theme: Dark", Action::SetThemeDark),
        Command::new("Theme: System", Action::SetThemeSystem),
        Command::new("Sync Mode: Auto", Action::SetSyncAuto),
        Command::new("Sync Mode: Self", Action::SetSyncSelf),
        Command::new("Sync Mode: Off", Action::SetSyncOff),
        Command::new("Toggle Zed Sync", Action::ToggleZedSync),
        Command::new("Toggle Sync Pin (Auto ⇄ Self)", Action::ToggleSyncPin),
        Command::new("Zoom In", Action::ZoomIn),
        Command::new("Zoom Out", Action::ZoomOut),
        Command::new("Zoom Reset", Action::ZoomReset),
        Command::new("Search Files…", Action::FileSearch),
        Command::new("Search in Project…", Action::FullTextSearch),
        Command::new("Copy File Path", Action::CopyFilePath),
        Command::new(share_label, Action::ToggleWebShare),
        Command::new("Export: HTML", Action::ExportHtml),
        Command::new("Export: PDF", Action::ExportPdf),
    ]
}

/// Filter the command catalogue by `query`, ranked best-first.
pub fn filter_commands(query: &str, share_url: Option<&str>) -> Vec<Command> {
    let all = commands(share_url);
    fuzzy::rank(query, &all, |c| c.label.as_str())
        .into_iter()
        .map(|(c, _)| c.clone())
        .collect()
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    #[test]
    fn 空クエリは全コマンドを返す() {
        let all = filter_commands("", None);
        assert_eq!(all.len(), commands(None).len());
    }

    #[test]
    fn クエリでコマンドを絞り込む() {
        let res = filter_commands("zoom", None);
        assert!(!res.is_empty());
        assert!(res.iter().all(|c| c.label.to_lowercase().contains("zoom")));
    }

    #[test]
    fn themeクエリはテーマ系を含む() {
        let res = filter_commands("theme", None);
        let labels: Vec<&str> = res.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"Theme: Light"));
        assert!(labels.contains(&"Theme: Dark"));
    }

    #[test]
    fn マッチしないクエリは空() {
        assert!(filter_commands("zzzzz", None).is_empty());
    }

    #[test]
    fn toggle_sync_pinコマンドがカタログに含まれる() {
        let all = commands(None);
        assert!(all.iter().any(|c| c.action == Action::ToggleSyncPin));
    }

    #[test]
    fn web_shareコマンドがカタログに含まれshareクエリで返る() {
        assert!(commands(None)
            .iter()
            .any(|c| c.action == Action::ToggleWebShare));
        assert!(filter_commands("share", None)
            .iter()
            .any(|c| c.action == Action::ToggleWebShare));
    }

    #[test]
    fn web_shareラベルは稼働状態でstartとstopを切り替える() {
        let stopped = commands(None);
        let started = commands(Some("http://127.0.0.1:6280/"));
        let label = |cs: &[Command]| {
            cs.iter()
                .find(|c| c.action == Action::ToggleWebShare)
                .map(|c| c.label.clone())
                .unwrap()
        };
        assert_eq!(label(&stopped), "Web Share: Start (Serve in Browser)");
        assert_eq!(label(&started), "Web Share: Stop (http://127.0.0.1:6280/)");
    }

    #[test]
    fn syncピンクエリでtoggle_sync_pinが返る() {
        let res = filter_commands("sync pin", None);
        assert!(
            res.iter().any(|c| c.action == Action::ToggleSyncPin),
            "expected ToggleSyncPin in results for 'sync pin', got: {:?}",
            res.iter().map(|c| c.label.clone()).collect::<Vec<_>>()
        );
    }
}
