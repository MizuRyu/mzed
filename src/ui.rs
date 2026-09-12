use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use dioxus::prelude::*;

use crate::app::{build_menu, export_dir, App, CtxMenu, MDO_CSS, MERMAID_JS};
use crate::tabs::Tabs;
use crate::{config, files, fuzzy, js, markdown, palette, search, services, sync, theme};

mod command_palette;
mod find_bar;
mod navigation;
mod note_popover;
mod project_menu;
mod search_panel;
mod settings;
mod sidebar;
mod task_view;
mod toolbar;
mod window;

pub(crate) use command_palette::Palette;
pub(crate) use find_bar::FindBar;
pub(crate) use navigation::{TabBar, TocPanel};
pub(crate) use note_popover::NotePopover;
pub(crate) use project_menu::ProjectMenu;
pub(crate) use search_panel::SearchPanel;
pub(crate) use settings::{Settings, SettingsTab};
pub(crate) use sidebar::{file_icon, folder_closed_icon, TreeView};
pub(crate) use task_view::TaskView;
pub(crate) use toolbar::ContentToolbar;
pub(crate) use window::{open_main_window, open_mermaid_window};

/// Unread green, dark enough to read on white and light enough on #0d1117.
pub(crate) fn unread_color(dark: bool) -> &'static str {
    if dark {
        "#3fb950"
    } else {
        "#1a7f37"
    }
}

/// The 6px dot in front of an unread file (sidebar row and palette row).
pub(crate) fn unread_dot(dark: bool) -> Element {
    let color = unread_color(dark);
    rsx! {
        span {
            style: "flex: 0 0 auto; width: 6px; height: 6px; border-radius: 50%; background: {color};",
        }
    }
}

/// Arrow keys scroll an overlay list under a stationary cursor, and the row that
/// slides under it fires `mouseenter`; ignoring hover for a moment after a key
/// press keeps that from yanking the selection back. A "wait for a real
/// mousemove" gate would need an `onmousemove` handler firing on every frame, so
/// this time-based guard is used instead.
const HOVER_KEY_GUARD: Duration = Duration::from_millis(250);

/// What last moved an overlay list's selection. Only keyboard moves scroll the
/// list into view; scrolling on hover would slide another row under the cursor.
#[derive(Clone, Copy, PartialEq)]
enum SelChange {
    Hover,
    Keyboard,
}

/// Whether a `mouseenter` on row `row` should take the selection.
/// `since_last_key` is `None` until the first key press.
fn hover_takes_selection(row: usize, current: usize, since_last_key: Option<Duration>) -> bool {
    row != current && since_last_key.is_none_or(|d| d > HOVER_KEY_GUARD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn キー操作前のホバーは即座に選択を取る() {
        assert!(hover_takes_selection(1, 0, None));
    }

    #[test]
    fn キー操作直後のホバーは無視される() {
        assert!(!hover_takes_selection(1, 0, Some(Duration::ZERO)));
        assert!(!hover_takes_selection(1, 0, Some(HOVER_KEY_GUARD)));
    }

    #[test]
    fn ガード経過後のホバーは選択を取る() {
        assert!(hover_takes_selection(
            1,
            0,
            Some(HOVER_KEY_GUARD + Duration::from_millis(1))
        ));
    }

    #[test]
    fn 選択中の行へのホバーは何もしない() {
        assert!(!hover_takes_selection(3, 3, None));
        assert!(!hover_takes_selection(3, 3, Some(Duration::from_secs(10))));
    }
}
