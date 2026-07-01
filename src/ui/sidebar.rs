use super::*;
#[component]
pub(crate) fn TreeView(
    node: files::TreeNode,
    depth: usize,
    mut expanded: Signal<HashSet<PathBuf>>,
    // `tabs` is read only for the active-file highlight; opening a file goes
    // through `on_open` so it lands in the focused pane.
    tabs: Signal<Tabs>,
    on_open: EventHandler<PathBuf>,
    on_context: EventHandler<CtxMenu>,
    // Inline rename: when `rename_target` equals a row's path, that row renders
    // an input bound to `rename_buf`; Enter calls `on_rename_commit`.
    mut rename_target: Signal<Option<PathBuf>>,
    mut rename_buf: Signal<String>,
    on_rename_commit: EventHandler<()>,
    // Quick Access: `favorites` is read for the star state; toggling goes through
    // `on_toggle_fav`.
    favorites: Signal<Vec<PathBuf>>,
    on_toggle_fav: EventHandler<PathBuf>,
    dark: bool,
) -> Element {
    let path = node.path.clone();
    let indent = 8 + depth * 14;
    let fg = if dark { "#c9d1d9" } else { "#1f2328" };

    let muted = if dark { "#8b949e" } else { "#8c959f" };

    if node.is_dir {
        let open = expanded.read().contains(&path);
        let toggle_path = path.clone();
        let ctx_path = path.clone();
        rsx! {
            div {
                style: "padding: 5px 8px 5px {indent}px; cursor: pointer; display: flex; justify-content: space-between; align-items: center; gap: 6px; user-select: none; font: 13px -apple-system, sans-serif; line-height: 1.4; color: {fg}; border-radius: 4px;",
                class: "mdo-tree-row",
                onclick: move |_| {
                    let mut e = expanded.write();
                    if !e.remove(&toggle_path) {
                        e.insert(toggle_path.clone());
                    }
                },
                oncontextmenu: move |e| {
                    e.prevent_default();
                    let c = e.client_coordinates();
                    on_context.call(CtxMenu { x: c.x as i32, y: c.y as i32, path: ctx_path.clone(), is_dir: true });
                },
                span {
                    style: "display: flex; align-items: center; gap: 4px; min-width: 0;",
                    {chevron_icon(open, muted)}
                    if open {
                        {folder_open_icon(muted)}
                    } else {
                        {folder_closed_icon(muted)}
                    }
                    span { style: "overflow: hidden; text-overflow: ellipsis; white-space: nowrap;", "{node.name}" }
                }
                span { style: "color: {muted}; font-size: 11px; opacity: 0.7; flex: 0 0 auto;", "{node.md_count}" }
            }
            if open {
                for child in node.children.clone() {
                    TreeView { key: "{child.path.display()}", node: child, depth: depth + 1, expanded, tabs, on_open, on_context, rename_target, rename_buf, on_rename_commit, favorites, on_toggle_fav, dark }
                }
            }
        }
    } else {
        // Highlight when this file is the active tab. Read the tabs signal
        // directly (rather than via a prop) so this row re-renders whenever the
        // active tab changes, leaving exactly one row highlighted.
        let is_sel = tabs.read().active().map(|p| p == &path).unwrap_or(false);
        let click_path = path.clone();
        let ctx_path = path.clone();
        let star_path = path.clone();
        let path_key = path.to_string_lossy().to_string();
        let editing = rename_target.read().as_deref() == Some(path.as_path());
        let is_fav = favorites.read().iter().any(|p| p == &path);
        let input_bg = if dark { "#0d1117" } else { "#ffffff" };
        let star_color = if is_fav { "#e3b341" } else { muted };
        rsx! {
            div {
                style: "padding: 5px 8px 5px {indent}px; cursor: pointer; user-select: none; display: flex; align-items: center; gap: 5px; font: 13px -apple-system, sans-serif; line-height: 1.4; color: {fg}; border-radius: 4px;",
                class: "mdo-tree-row mdo-tree-row-file",
                class: if is_sel { "mdo-tree-row-active" },
                "data-mdo-tree-path": "{path_key}",
                onclick: move |_| on_open.call(click_path.clone()),
                oncontextmenu: move |e| {
                    e.prevent_default();
                    let c = e.client_coordinates();
                    on_context.call(CtxMenu { x: c.x as i32, y: c.y as i32, path: ctx_path.clone(), is_dir: false });
                },
                {file_icon(muted)}
                if editing {
                    input {
                        value: "{rename_buf}",
                        autofocus: true,
                        onmounted: move |e| { spawn(async move { let _ = e.set_focus(true).await; }); },
                        onclick: move |e| e.stop_propagation(),
                        onmousedown: move |e| e.stop_propagation(),
                        oninput: move |e| rename_buf.set(e.value()),
                        onkeydown: move |e| match e.key() {
                            Key::Enter => on_rename_commit.call(()),
                            Key::Escape => rename_target.set(None),
                            _ => {}
                        },
                        style: "flex: 1 1 auto; min-width: 0; padding: 1px 4px; border: 1px solid #1f6feb; border-radius: 4px; background: {input_bg}; color: {fg}; font: 13px -apple-system, sans-serif; outline: none;",
                    }
                } else {
                    span { style: "overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1 1 auto;", "{node.name}" }
                    button {
                        class: if is_fav { "mdo-fav-star on" } else { "mdo-fav-star" },
                        style: "margin-left: auto; background: transparent; border: none; cursor: pointer; flex: 0 0 auto; padding: 0 2px; display: flex; align-items: center; color: {star_color};",
                        title: if is_fav { "お気に入りから外す" } else { "お気に入りに追加" },
                        onclick: move |e| { e.stop_propagation(); on_toggle_fav.call(star_path.clone()); },
                        svg {
                            width: "13", height: "13", view_box: "0 0 24 24",
                            fill: if is_fav { "currentColor" } else { "none" },
                            stroke: "currentColor", stroke_width: "2", stroke_linejoin: "round",
                            path { d: "M12 2l3 7h7l-5.5 4.5L18 21l-6-4-6 4 1.5-7.5L2 9h7z" }
                        }
                    }
                }
            }
        }
    }
}

/// Inline lucide-style chevron (rotates open/closed via the right/down glyph).
fn chevron_icon(open: bool, color: &str) -> Element {
    let d = if open {
        "M6 9l6 6 6-6" // chevron-down
    } else {
        "M9 6l6 6-6 6" // chevron-right
    };
    rsx! {
        svg {
            width: "12", height: "12", view_box: "0 0 24 24", fill: "none",
            stroke: "{color}", stroke_width: "2.2", stroke_linecap: "round", stroke_linejoin: "round",
            style: "flex: 0 0 auto;",
            path { d: "{d}" }
        }
    }
}

/// Inline lucide-style closed-folder icon (shown when a directory is collapsed).
pub(crate) fn folder_closed_icon(color: &str) -> Element {
    rsx! {
        svg {
            width: "15", height: "15", view_box: "0 0 24 24", fill: "none",
            stroke: "{color}", stroke_width: "1.8", stroke_linecap: "round", stroke_linejoin: "round",
            style: "flex: 0 0 auto;",
            path { d: "M4 4h5l2 3h9v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z" }
        }
    }
}

/// Inline lucide-style open-folder icon (shown when a directory is expanded).
fn folder_open_icon(color: &str) -> Element {
    rsx! {
        svg {
            width: "15", height: "15", view_box: "0 0 24 24", fill: "none",
            stroke: "{color}", stroke_width: "1.8", stroke_linecap: "round", stroke_linejoin: "round",
            style: "flex: 0 0 auto;",
            path { d: "M6 14l1.5-4.5A1 1 0 0 1 8.5 9H21l-2.5 7.5A1 1 0 0 1 17.5 17H4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h5l2 3h6a1 1 0 0 1 1 1v1" }
        }
    }
}

/// Inline lucide-style file/document icon.
pub(crate) fn file_icon(color: &str) -> Element {
    rsx! {
        svg {
            width: "14", height: "14", view_box: "0 0 24 24", fill: "none",
            stroke: "{color}", stroke_width: "1.8", stroke_linecap: "round", stroke_linejoin: "round",
            style: "flex: 0 0 auto; margin-left: 12px;",
            path { d: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" }
            path { d: "M14 2v6h6" }
        }
    }
}
