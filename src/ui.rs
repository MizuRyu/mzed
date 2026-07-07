use std::collections::HashSet;
use std::path::PathBuf;

use dioxus::prelude::*;

use crate::app::{build_menu, export_dir, App, CtxMenu, MDO_CSS, MERMAID_JS};
use crate::tabs::Tabs;
use crate::{config, files, fuzzy, js, markdown, palette, search, services, theme};

mod command_palette;
mod find_bar;
mod navigation;
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
pub(crate) use project_menu::ProjectMenu;
pub(crate) use search_panel::SearchPanel;
pub(crate) use settings::{Settings, SettingsTab};
pub(crate) use sidebar::{file_icon, folder_closed_icon, TreeView};
pub(crate) use task_view::TaskView;
pub(crate) use toolbar::ContentToolbar;
pub(crate) use window::{open_main_window, open_mermaid_window};
