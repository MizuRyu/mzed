use super::*;
use crate::config::{DateOrder, GroupOrder};
use crate::services::task_scan::{self, GroupKey, TaskGroup, TaskItem};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Scope {
    ThisProject,
    AllProjects,
}

/// Context-menu state for a Task View row.
#[derive(Clone, PartialEq)]
struct TaskCtxMenu {
    x: i32,
    y: i32,
    /// The absolute path of the row (file or directory).
    path: PathBuf,
    /// `true` for project-root and task-folder rows (directories), `false` for files.
    is_dir: bool,
    /// `false` for project-root rows — favorites apply only to task folders and files.
    show_fav: bool,
}

/// Signals and callbacks every tree row needs. Bundled so the row components
/// don't take a dozen props each.
#[derive(Clone, Copy, PartialEq)]
struct RowCtx {
    dark: bool,
    /// Collapsed group headings, keyed by `TaskGroup::id` (absent = expanded).
    collapsed: Signal<HashSet<String>>,
    /// Expanded task folders, keyed by folder path (absent = collapsed).
    expanded: Signal<HashSet<PathBuf>>,
    /// (selected_file, project_path)
    selected: Signal<Option<(PathBuf, PathBuf)>>,
    favorites: Signal<Vec<PathBuf>>,
    ctx_menu: Signal<Option<TaskCtxMenu>>,
    on_copy_path: EventHandler<PathBuf>,
    on_toggle_fav: EventHandler<PathBuf>,
    on_toast: EventHandler<String>,
}

impl RowCtx {
    fn fg(&self) -> &'static str {
        if self.dark {
            "#c9d1d9"
        } else {
            "#1f2328"
        }
    }
    fn muted(&self) -> &'static str {
        if self.dark {
            "#8b949e"
        } else {
            "#57606a"
        }
    }
    /// Row background for the active/selected state.
    fn sel_bg(&self, strong: bool) -> &'static str {
        match (self.dark, strong) {
            (true, true) => "rgba(9,105,218,0.12)",
            (true, false) => "rgba(9,105,218,0.08)",
            (false, true) => "rgba(9,105,218,0.08)",
            (false, false) => "rgba(9,105,218,0.05)",
        }
    }
}

/// Left indent for a row at `depth` (0 = outermost group heading).
fn indent(depth: usize) -> usize {
    8 + depth * 14
}

/// Full-screen Task View mode (Cmd+Shift+D).
///
/// Replaces the normal sidebar + content area with a 2-pane layout:
/// left = task tree (grouped by project and/or status, status-coloured),
/// right = selected `task.md` rendered via the existing markdown pipeline.
#[component]
pub(crate) fn TaskView(
    roots: Signal<Vec<PathBuf>>,
    /// Bumped by the app's file watcher on any change under the current
    /// roots. Subscribed only in This Project scope for live rescans.
    fs_tick: Signal<u32>,
    /// Bumped by the app (Cmd+R) or the ↻ header button to force a re-scan.
    mut refresh_token: Signal<u32>,
    /// Bumped by the app (Ctrl+Tab) to toggle This Project ⇄ All Projects.
    scope_token: Signal<u32>,
    /// Bumped here after the right pane's HTML is set, so the app re-runs the
    /// post-render pass (highlight / mermaid / KaTeX) over the new content.
    mut doc_tick: Signal<u32>,
    scan_roots: Signal<Vec<PathBuf>>,
    scan_exclude: Signal<Vec<String>>,
    subpath: Signal<String>,
    default_days: Signal<u32>,
    /// Grouping settings (Settings → Task View).
    group_by_status: Signal<bool>,
    group_order: Signal<GroupOrder>,
    status_order: Signal<Vec<String>>,
    date_order: Signal<DateOrder>,
    /// Current project name (directory basename) shown in the header.
    proj_name: String,
    dark: bool,
    favorites: Signal<Vec<PathBuf>>,
    on_toggle_fav: EventHandler<PathBuf>,
    on_copy_path: EventHandler<PathBuf>,
    on_toast: EventHandler<String>,
    on_open_project_menu: EventHandler<()>,
) -> Element {
    let panel_bg = if dark { "#161b22" } else { "#f6f8fa" };
    let panel_border = if dark { "#30363d" } else { "#d0d7de" };
    let body_bg = if dark { "#0d1117" } else { "#ffffff" };
    let muted = if dark { "#8b949e" } else { "#57606a" };
    let btn_border = if dark { "#30363d" } else { "#d0d7de" };

    let mut scope = use_signal(|| Scope::ThisProject);
    let initial_days = default_days();
    let mut selected_days = use_signal(move || initial_days);
    let mut groups: Signal<Vec<TaskGroup>> = use_signal(Vec::new);
    let mut loading = use_signal(|| true);
    let mut selected: Signal<Option<(PathBuf, PathBuf)>> = use_signal(|| None);
    let mut doc_html = use_signal(String::new);
    let mut doc_gen = use_signal(|| 0u32);
    let expanded: Signal<HashSet<PathBuf>> = use_signal(HashSet::new);
    let collapsed: Signal<HashSet<String>> = use_signal(HashSet::new);
    let mut scan_gen = use_signal(|| 0u32);
    // Left pane width (px), adjustable via the drag divider. Session-local.
    let mut pane_width = use_signal(|| 300u32);
    // Task View local context menu (independent of the sidebar CtxMenu).
    let mut ctx_menu: Signal<Option<TaskCtxMenu>> = use_signal(|| None);

    let row_ctx = RowCtx {
        dark,
        collapsed,
        expanded,
        selected,
        favorites,
        ctx_menu,
        on_copy_path,
        on_toggle_fav,
        on_toast,
    };

    // Scope toggle via keybind (Ctrl+Tab). The token comparison skips the
    // effect's initial run so mounting never flips the scope.
    let mut seen_scope_token = use_signal(|| *scope_token.peek());
    use_effect(move || {
        let t = scope_token();
        if t != *seen_scope_token.peek() {
            seen_scope_token.set(t);
            let next = match *scope.peek() {
                Scope::ThisProject => Scope::AllProjects,
                Scope::AllProjects => Scope::ThisProject,
            };
            scope.set(next);
            selected.set(None);
        }
    });

    // ── Scan effect ────────────────────────────────────────────────────────
    // Runs every time scope/days/roots/scan_roots/subpath/grouping/refresh_token
    // change. No cache — always re-scans. The walk is cheap (repo-boundary
    // pruned) and runs in spawn_blocking so the UI never blocks.
    use_effect(move || {
        let current_scope = scope();
        let current_roots = roots();
        let current_scan_roots = scan_roots();
        let current_exclude = scan_exclude();
        let current_subpath = subpath();
        let n_days = selected_days();
        let by_status = group_by_status();
        let project_first = group_order() == GroupOrder::ProjectFirst;
        let statuses = status_order();
        let date_desc = date_order() == DateOrder::Desc;
        let _ = refresh_token();
        // Live rescan on file changes, This Project only: the watcher covers
        // the current roots, and dioxus tracks reads dynamically, so this
        // subscription simply doesn't exist while in All Projects scope
        // (avoiding a full multi-repo re-walk on every local file save).
        if current_scope == Scope::ThisProject {
            let _ = fs_tick();
        }

        let gen_id = {
            let mut g = scan_gen.write();
            *g += 1;
            *g
        };
        loading.set(true);

        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let items = match current_scope {
                    Scope::ThisProject => {
                        task_scan::scan_this_project_blocking(&current_roots, &current_subpath)
                    }
                    Scope::AllProjects => task_scan::scan_all_projects_blocking(
                        &current_scan_roots,
                        &current_roots,
                        &current_subpath,
                        Some(n_days),
                        &current_exclude,
                    ),
                };
                task_scan::build_groups(items, by_status, project_first, &statuses, date_desc)
            })
            .await;

            if scan_gen() != gen_id {
                return;
            }
            loading.set(false);
            if let Ok(g) = result {
                groups.set(g);
            }
        });
    });

    // ── Document-load effect ────────────────────────────────────────────────
    use_effect(move || {
        let sel = selected();
        let gen_id = {
            let mut g = doc_gen.write();
            *g += 1;
            *g
        };
        let Some((file_path, project_path)) = sel else {
            doc_html.set(String::new());
            return;
        };
        doc_html.set("<p style=\"color:#8b949e\">読み込み中…</p>".to_string());
        spawn(async move {
            let roots_vec = vec![project_path];
            let result = tokio::task::spawn_blocking(move || {
                services::file_service::load_document(Some(file_path), &roots_vec)
            })
            .await;
            if doc_gen() != gen_id {
                return;
            }
            if let Ok(snap) = result {
                doc_html.set(snap.rendered_html().to_string());
                doc_tick += 1;
            }
        });
    });

    let scan_roots_empty = scan_roots().is_empty();

    // Scope toggle style: flat text with accent underline when active, muted when not.
    let scope_tab_style = move |active: bool| -> String {
        let (color, weight, border_b) = if active {
            if dark {
                ("#58a6ff", "600", "2px solid #1f6feb")
            } else {
                ("#0969da", "600", "2px solid #0969da")
            }
        } else if dark {
            ("#8b949e", "400", "2px solid transparent")
        } else {
            ("#57606a", "400", "2px solid transparent")
        };
        format!(
            "padding: 2px 0; background: none; border: none; border-bottom: {border_b}; \
             color: {color}; font: {weight} 12px -apple-system, sans-serif; \
             cursor: pointer; letter-spacing: 0.1px;"
        )
    };

    // Shared context-menu item styles.
    let menu_bg = if dark { "#1c2128" } else { "#ffffff" };
    let menu_border = if dark { "#30363d" } else { "#d0d7de" };
    let item_fg = if dark { "#e6edf3" } else { "#1f2328" };
    let item_style = "display: block; width: 100%; text-align: left; padding: 6px 12px; \
                      border: none; background: transparent; color: inherit; cursor: pointer; \
                      border-radius: 5px; font: 13px -apple-system, sans-serif;";
    let sep_style = format!("height: 1px; margin: 4px 6px; background: {menu_border};");

    rsx! {
        // Full-screen 2-pane layout (sits below the 37px top bar).
        div {
            style: "position: fixed; inset: 0; top: 37px; display: flex; background: {body_bg}; z-index: 100;",

            // ── Left pane ─────────────────────────────────────────────────
            div {
                style: "width: {pane_width}px; flex: 0 0 auto; display: flex; flex-direction: column; \
                        border-right: 1px solid {panel_border}; background: {panel_bg};",

                // Header
                div {
                    style: "flex: 0 0 auto; padding: 10px 12px 8px; \
                            border-bottom: 1px solid {panel_border};",
                    div {
                        style: "display: flex; align-items: center; justify-content: space-between; \
                                margin-bottom: 8px;",
                        span {
                            style: "font: 600 11px -apple-system, sans-serif; color: {muted}; \
                                    text-transform: uppercase; letter-spacing: 0.4px;",
                            "Task View"
                        }
                        button {
                            title: "再スキャン",
                            style: "background: none; border: none; cursor: pointer; \
                                    color: {muted}; font-size: 13px; padding: 0 2px; line-height: 1;",
                            onclick: move |_| {
                                *refresh_token.write() += 1;
                            },
                            "↻"
                        }
                    }
                    // Scope toggle – flat text tabs
                    div {
                        style: "display: flex; gap: 14px;",
                        button {
                            style: "{scope_tab_style(scope() == Scope::ThisProject)}",
                            onclick: move |_| { scope.set(Scope::ThisProject); selected.set(None); },
                            "This Project"
                        }
                        button {
                            style: "{scope_tab_style(scope() == Scope::AllProjects)}",
                            onclick: move |_| { scope.set(Scope::AllProjects); selected.set(None); },
                            "All Projects"
                        }
                    }
                    // Days selector (All Projects only) – compact inline select
                    if scope() == Scope::AllProjects {
                        div {
                            style: "display: flex; align-items: center; gap: 5px; margin-top: 6px; \
                                    font: 11px -apple-system, sans-serif; color: {muted};",
                            "直近"
                            select {
                                class: "mdo-select",
                                style: "appearance: none; -webkit-appearance: none; \
                                        padding: 2px 22px 2px 5px; border: 1px solid {btn_border}; \
                                        border-radius: 4px; background-color: {panel_bg}; \
                                        color: {muted}; font: 11px -apple-system, sans-serif; cursor: pointer;",
                                onchange: move |e| {
                                    if let Ok(v) = e.value().parse::<u32>() {
                                        selected_days.set(v);
                                    }
                                },
                                for d in [3u32, 7, 14, 30, 90] {
                                    option { value: "{d}", selected: selected_days() == d, "{d} 日" }
                                }
                            }
                        }
                    }
                }

                // Task tree (scrollable)
                div {
                    style: "flex: 1 1 auto; overflow: auto; padding: 6px 0;",

                    if loading() {
                        div {
                            style: "padding: 16px 14px; font: 13px -apple-system, sans-serif; color: {muted};",
                            "スキャン中…"
                        }
                    } else if groups().is_empty() {
                        div {
                            style: "padding: 16px 14px; font: 13px -apple-system, sans-serif; \
                                    color: {muted}; line-height: 1.6;",
                            "タスクが見つかりませんでした。"
                            if scope() == Scope::AllProjects && scan_roots_empty {
                                div {
                                    style: "margin-top: 10px; font-size: 12px;",
                                    "設定 → Task View Scan Roots にプロジェクトの親ディレクトリを追加すると、複数プロジェクトを横断して表示できます。"
                                }
                            }
                        }
                    } else {
                        for group in groups() {
                            GroupNode { key: "{group.id}", group, depth: 0, ctx: row_ctx }
                        }
                    }
                }
            }

            // Drag divider: same document-level drag bridge as the sidebar's
            // (streams cursor X to Rust, clamped here).
            div {
                style: "flex: 0 0 auto; width: 5px; cursor: col-resize; background: transparent; align-self: stretch;",
                class: "mdo-sidebar-divider",
                onmousedown: move |e| {
                    e.prevent_default();
                    spawn(async move {
                        let mut eval = document::eval(js::sidebar_resize_js());
                        while let Ok(value) = eval.recv::<serde_json::Value>().await {
                            if let Some(x) = value.get("x").and_then(|v| v.as_f64()) {
                                pane_width.set((x as u32).clamp(200, 600));
                            }
                        }
                    });
                },
            }

            // ── Right pane ────────────────────────────────────────────────
            div {
                style: "flex: 1 1 auto; min-width: 0; overflow: auto; background: {body_bg};",
                if selected().is_none() {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; \
                                height: 100%; font: 14px -apple-system, sans-serif; color: {muted};",
                        "← タスクを選択してください"
                    }
                } else {
                    div {
                        style: "max-width: 900px; margin: 0 auto; padding: 24px 32px;",
                        div {
                            class: "markdown-body",
                            dangerous_inner_html: "{doc_html}"
                        }
                    }
                }
            }

            // ── Task View context menu overlay ─────────────────────────────
            if let Some(c) = ctx_menu() {
                {
                    let is_fav = favorites.read().iter().any(|p| p == &c.path);
                    let (p_copy, p_reveal, p_app, p_fav) = (
                        c.path.clone(), c.path.clone(), c.path.clone(), c.path.clone(),
                    );
                    rsx! {
                        // Backdrop: click closes the menu.
                        div {
                            style: "position: fixed; inset: 0; z-index: 200;",
                            onclick: move |_| ctx_menu.set(None),
                            oncontextmenu: move |e| { e.prevent_default(); ctx_menu.set(None); },
                        }
                        // Menu panel.
                        div {
                            style: "position: fixed; left: {c.x}px; top: {c.y}px; z-index: 201; \
                                    min-width: 200px; background: {menu_bg}; border: 1px solid {menu_border}; \
                                    border-radius: 8px; padding: 4px; \
                                    box-shadow: 0 8px 24px rgba(0,0,0,0.3); color: {item_fg};",
                            // 絶対パスをコピー
                            button {
                                class: "mdo-ctx-item", style: "{item_style}",
                                onclick: move |_| {
                                    on_copy_path.call(p_copy.clone());
                                    ctx_menu.set(None);
                                },
                                "絶対パスをコピー"
                            }
                            div { style: "{sep_style}" }
                            // Finder で表示
                            button {
                                class: "mdo-ctx-item", style: "{item_style}",
                                onclick: move |_| {
                                    if let Err(err) = services::platform::reveal_in_finder(&p_reveal) {
                                        on_toast.call(format!("Finder failed: {err}"));
                                    }
                                    ctx_menu.set(None);
                                },
                                "Finder で表示"
                            }
                            // デフォルトアプリで開く (files only; dirs → Finder と同義なので省略)
                            if !c.is_dir {
                                button {
                                    class: "mdo-ctx-item", style: "{item_style}",
                                    onclick: move |_| {
                                        if let Err(err) = services::platform::open_target(&p_app) {
                                            on_toast.call(format!("Open failed: {err}"));
                                        }
                                        ctx_menu.set(None);
                                    },
                                    "デフォルトアプリで開く"
                                }
                            }
                            // お気に入り (task folders + files; not project-root rows)
                            if c.show_fav {
                                div { style: "{sep_style}" }
                                button {
                                    class: "mdo-ctx-item", style: "{item_style}",
                                    onclick: move |_| {
                                        on_toggle_fav.call(p_fav.clone());
                                        ctx_menu.set(None);
                                    },
                                    if is_fav { "お気に入りから外す" } else { "お気に入りに追加" }
                                }
                            }
                        }
                    }
                }
            }

            // Hidden: the project menu is opened from the app's global keybind.
            // Kept as a prop so the Task View header can trigger it later.
            {
                let _ = &on_open_project_menu;
                let _ = &proj_name;
                rsx! {}
            }
        }
    }
}

/// One group heading (project or status) plus whatever it contains: nested
/// groups when `group.children` is non-empty, otherwise the task folders.
#[component]
fn GroupNode(group: TaskGroup, depth: usize, ctx: RowCtx) -> Element {
    let mut collapsed = ctx.collapsed;
    let is_open = !collapsed.read().contains(&group.id);
    let chevron = if is_open { "▾" } else { "▸" };
    let toggle_id = group.id.clone();
    let (fg, muted) = (ctx.fg(), ctx.muted());
    let pad = indent(depth);
    let count = group.task_count();

    rsx! {
        match &group.key {
            GroupKey::Project { name, path } => {
                let display = path.display().to_string();
                let ctx_path = path.clone();
                let copy_path = path.clone();
                let mut ctx_menu = ctx.ctx_menu;
                let on_copy_path = ctx.on_copy_path;
                rsx! {
                    div {
                        style: "padding: 8px 12px 4px {pad}px; user-select: none; cursor: pointer;",
                        class: "mdo-tree-row",
                        onclick: move |_| {
                            let mut c = collapsed.write();
                            if !c.remove(&toggle_id) {
                                c.insert(toggle_id.clone());
                            }
                        },
                        oncontextmenu: move |e| {
                            e.prevent_default();
                            let c = e.client_coordinates();
                            ctx_menu.set(Some(TaskCtxMenu {
                                x: c.x as i32,
                                y: c.y as i32,
                                path: ctx_path.clone(),
                                is_dir: true,
                                show_fav: false,
                            }));
                        },
                        // First line: chevron + project name + hover copy button
                        div {
                            style: "display: flex; align-items: center; gap: 4px;",
                            span {
                                style: "color: {muted}; font-size: 10px; flex: 0 0 auto; width: 10px;",
                                "{chevron}"
                            }
                            span {
                                style: "font: 600 13px -apple-system, sans-serif; \
                                        color: {fg}; overflow: hidden; text-overflow: ellipsis; \
                                        white-space: nowrap; flex: 1 1 auto;",
                                "{name}"
                            }
                            button {
                                class: "mdo-copy-path",
                                style: "background: transparent; border: none; cursor: pointer; \
                                        flex: 0 0 auto; padding: 0 2px; display: flex; \
                                        align-items: center; color: {muted};",
                                title: "パスをコピー",
                                onclick: move |e| {
                                    e.stop_propagation();
                                    on_copy_path.call(copy_path.clone());
                                },
                                {copy_icon()}
                            }
                        }
                        // Second line: muted absolute path
                        div {
                            style: "font: 11px ui-monospace, monospace; color: {muted}; \
                                    overflow: hidden; text-overflow: ellipsis; \
                                    white-space: nowrap; margin-top: 1px; padding-left: 14px;",
                            title: "{display}",
                            "{display}"
                        }
                    }
                }
            }
            GroupKey::Status(status) => {
                let color = status.color();
                let heading = status.heading();
                rsx! {
                    div {
                        style: "padding: 5px 8px 5px {pad}px; cursor: pointer; user-select: none; \
                                display: flex; align-items: center; gap: 6px; \
                                font: 600 12px -apple-system, sans-serif; color: {muted};",
                        class: "mdo-tree-row",
                        onclick: move |_| {
                            let mut c = collapsed.write();
                            if !c.remove(&toggle_id) {
                                c.insert(toggle_id.clone());
                            }
                        },
                        span {
                            style: "color: {muted}; font-size: 10px; flex: 0 0 auto; width: 10px;",
                            "{chevron}"
                        }
                        span {
                            style: "width: 8px; height: 8px; border-radius: 50%; \
                                    background: {color}; flex: 0 0 auto;",
                        }
                        span { style: "flex: 1 1 auto;", "{heading}" }
                        span { style: "flex: 0 0 auto; font-weight: 400;", "{count}" }
                    }
                }
            }
        }

        if is_open {
            for child in group.children.iter().cloned() {
                GroupNode { key: "{child.id}", group: child, depth: depth + 1, ctx }
            }
            for task in group.tasks.iter().cloned() {
                TaskFolderNode {
                    key: "{task.folder_path.display()}",
                    task,
                    depth: depth + 1,
                    ctx,
                }
            }
        }
    }
}

/// A task folder row and, when expanded, its `task.md` plus every other file
/// in the folder.
#[component]
fn TaskFolderNode(task: TaskItem, depth: usize, ctx: RowCtx) -> Element {
    let mut expanded = ctx.expanded;
    let mut selected = ctx.selected;
    let mut ctx_menu = ctx.ctx_menu;
    let (on_copy_path, on_toggle_fav, on_toast) =
        (ctx.on_copy_path, ctx.on_toggle_fav, ctx.on_toast);
    let (fg, muted) = (ctx.fg(), ctx.muted());

    let folder_path = task.folder_path.clone();
    let task_md = task.task_md.clone();
    let project_path = task.project_path.clone();
    let is_open = expanded.read().contains(&folder_path);
    let chevron = if is_open { "▾" } else { "▸" };
    let is_folder_active = selected
        .read()
        .as_ref()
        .map(|(p, _)| p.starts_with(&task.folder_path))
        .unwrap_or(false);
    let folder_bg = if is_folder_active {
        ctx.sel_bg(false)
    } else {
        "transparent"
    };
    let folder_border = if is_folder_active {
        "#0969da"
    } else {
        "transparent"
    };
    let is_fav_folder = ctx.favorites.read().iter().any(|p| p == &folder_path);
    let star_folder_color = if is_fav_folder { "#e3b341" } else { muted };
    let status_color = task.meta.status.color();
    let status_label = task.meta.status.label();
    let folder_name = task.folder_name.clone();
    let pad = indent(depth);
    let child_pad = indent(depth + 1) + 12;

    let toggle_path = folder_path.clone();
    let open_md = task_md.clone();
    let open_proj = project_path.clone();
    let ctx_folder_path = folder_path.clone();
    let copy_folder_path = folder_path.clone();
    let fav_folder_path = folder_path.clone();

    rsx! {
        div {
            style: "padding: 5px 8px 5px {pad}px; cursor: pointer; \
                    display: flex; align-items: center; gap: 6px; user-select: none; \
                    font: 13px -apple-system, sans-serif; line-height: 1.4; \
                    color: {fg}; background: {folder_bg}; \
                    border-left: 2px solid {folder_border}; border-radius: 0 4px 4px 0;",
            class: "mdo-tree-row",
            onclick: move |_| {
                let mut e = expanded.write();
                if !e.remove(&toggle_path) {
                    e.insert(toggle_path.clone());
                }
                selected.set(Some((open_md.clone(), open_proj.clone())));
            },
            oncontextmenu: move |e| {
                e.prevent_default();
                let c = e.client_coordinates();
                ctx_menu.set(Some(TaskCtxMenu {
                    x: c.x as i32,
                    y: c.y as i32,
                    path: ctx_folder_path.clone(),
                    is_dir: true,
                    show_fav: true,
                }));
            },
            span {
                style: "color: {muted}; font-size: 10px; flex: 0 0 auto; width: 10px;",
                "{chevron}"
            }
            span {
                style: "width: 8px; height: 8px; border-radius: 50%; \
                        background: {status_color}; flex: 0 0 auto;",
                title: "{status_label}",
            }
            span {
                style: "overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1 1 auto;",
                "{folder_name}"
            }
            button {
                class: "mdo-copy-path",
                style: "background: transparent; border: none; cursor: pointer; \
                        flex: 0 0 auto; padding: 0 2px; display: flex; \
                        align-items: center; color: {muted};",
                title: "パスをコピー",
                onclick: move |e| {
                    e.stop_propagation();
                    on_copy_path.call(copy_folder_path.clone());
                },
                {copy_icon()}
            }
            button {
                class: if is_fav_folder { "mdo-fav-star on" } else { "mdo-fav-star" },
                style: "background: transparent; border: none; cursor: pointer; \
                        flex: 0 0 auto; padding: 0 2px; display: flex; \
                        align-items: center; color: {star_folder_color};",
                title: if is_fav_folder { "お気に入りから外す" } else { "お気に入りに追加" },
                onclick: move |e| {
                    e.stop_propagation();
                    on_toggle_fav.call(fav_folder_path.clone());
                },
                svg {
                    width: "13", height: "13", view_box: "0 0 24 24",
                    fill: if is_fav_folder { "currentColor" } else { "none" },
                    stroke: "currentColor", stroke_width: "2", stroke_linejoin: "round",
                    path { d: "M12 2l3 7h7l-5.5 4.5L18 21l-6-4-6 4 1.5-7.5L2 9h7z" }
                }
            }
        }

        if is_open {
            // task.md first, then the rest of the folder (dirs first, then
            // files; subdirectories expand recursively).
            FileRow {
                key: "{task_md.display()}",
                path: task_md.clone(),
                project_path: project_path.clone(),
                label: "task.md".to_string(),
                pad: child_pad,
                ctx,
            }
            for entry in task.extra_files.iter().cloned() {
                TaskEntryNode {
                    key: "{entry.path.display()}",
                    node: entry,
                    project_path: project_path.clone(),
                    pad: child_pad,
                    ctx,
                }
            }
        }
        // `on_toast` is used by the file rows; silence the unused binding here.
        { let _ = &on_toast; rsx! {} }
    }
}

/// One entry inside an expanded task folder: a file row, or a subdirectory
/// row that expands to its children (indented one level per depth).
#[component]
fn TaskEntryNode(
    node: task_scan::TaskFileNode,
    project_path: PathBuf,
    pad: usize,
    ctx: RowCtx,
) -> Element {
    if !node.is_dir {
        return rsx! {
            FileRow {
                label: node.name.clone(),
                path: node.path.clone(),
                project_path,
                pad,
                ctx,
            }
        };
    }

    let mut expanded = ctx.expanded;
    let mut ctx_menu = ctx.ctx_menu;
    let muted = ctx.muted();
    let fg = ctx.fg();
    let is_open = expanded.read().contains(&node.path);
    let chevron = if is_open { "▾" } else { "▸" };
    let toggle_path = node.path.clone();
    let ctx_path = node.path.clone();
    let child_pad = pad + 14;

    rsx! {
        div {
            style: "padding: 4px 8px 4px {pad}px; cursor: pointer; user-select: none; \
                    display: flex; align-items: center; gap: 5px; \
                    font: 12px -apple-system, sans-serif; line-height: 1.4; color: {fg};",
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
                ctx_menu.set(Some(TaskCtxMenu {
                    x: c.x as i32,
                    y: c.y as i32,
                    path: ctx_path.clone(),
                    is_dir: true,
                    show_fav: false,
                }));
            },
            span {
                style: "color: {muted}; font-size: 10px; flex: 0 0 auto; width: 10px;",
                "{chevron}"
            }
            span {
                style: "flex: 1 1 auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                "{node.name}"
            }
        }
        if is_open {
            for child in node.children.iter().cloned() {
                TaskEntryNode {
                    key: "{child.path.display()}",
                    node: child,
                    project_path: project_path.clone(),
                    pad: child_pad,
                    ctx,
                }
            }
        }
    }
}

/// A single file row under a task folder. Markdown opens in the right pane;
/// anything else opens in the OS default app.
#[component]
fn FileRow(
    path: PathBuf,
    project_path: PathBuf,
    label: String,
    pad: usize,
    ctx: RowCtx,
) -> Element {
    let mut selected = ctx.selected;
    let mut ctx_menu = ctx.ctx_menu;
    let (on_copy_path, on_toggle_fav, on_toast) =
        (ctx.on_copy_path, ctx.on_toggle_fav, ctx.on_toast);
    let muted = ctx.muted();
    let is_md = crate::files::is_markdown(&path);
    let fg = if is_md { ctx.fg() } else { muted };

    let is_sel = selected
        .read()
        .as_ref()
        .map(|(p, _)| p == &path)
        .unwrap_or(false);
    let row_bg = if is_sel {
        ctx.sel_bg(true)
    } else {
        "transparent"
    };
    let row_border = if is_sel { "#0969da" } else { "transparent" };
    let is_fav = ctx.favorites.read().iter().any(|p| p == &path);
    let star_color = if is_fav { "#e3b341" } else { muted };

    let (open_path, open_proj) = (path.clone(), project_path.clone());
    let (ctx_path, copy_path, fav_path) = (path.clone(), path.clone(), path.clone());

    rsx! {
        div {
            style: "padding: 4px 8px 4px {pad}px; cursor: pointer; \
                    display: flex; align-items: center; gap: 5px; \
                    font: 12px -apple-system, sans-serif; line-height: 1.4; \
                    color: {fg}; background: {row_bg}; \
                    border-left: 2px solid {row_border}; border-radius: 0 4px 4px 0;",
            class: "mdo-tree-row",
            onclick: move |_| {
                if is_md {
                    selected.set(Some((open_path.clone(), open_proj.clone())));
                } else if let Err(err) = services::platform::open_target(&open_path) {
                    on_toast.call(format!("Open failed: {err}"));
                }
            },
            oncontextmenu: move |e| {
                e.prevent_default();
                let c = e.client_coordinates();
                ctx_menu.set(Some(TaskCtxMenu {
                    x: c.x as i32,
                    y: c.y as i32,
                    path: ctx_path.clone(),
                    is_dir: false,
                    show_fav: true,
                }));
            },
            {task_view_file_icon(muted)}
            span {
                style: "flex: 1 1 auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                "{label}"
            }
            button {
                class: "mdo-copy-path",
                style: "background: transparent; border: none; cursor: pointer; \
                        flex: 0 0 auto; padding: 0 2px; display: flex; \
                        align-items: center; color: {muted};",
                title: "パスをコピー",
                onclick: move |e| {
                    e.stop_propagation();
                    on_copy_path.call(copy_path.clone());
                },
                {copy_icon()}
            }
            button {
                class: if is_fav { "mdo-fav-star on" } else { "mdo-fav-star" },
                style: "background: transparent; border: none; cursor: pointer; \
                        flex: 0 0 auto; padding: 0 2px; display: flex; \
                        align-items: center; color: {star_color};",
                title: if is_fav { "お気に入りから外す" } else { "お気に入りに追加" },
                onclick: move |e| {
                    e.stop_propagation();
                    on_toggle_fav.call(fav_path.clone());
                },
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

/// Compact copy icon (clipboard) for hover copy buttons in task view rows.
fn copy_icon() -> Element {
    rsx! {
        svg {
            width: "13", height: "13", view_box: "0 0 24 24", fill: "none",
            stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
            rect { x: "9", y: "9", width: "13", height: "13", rx: "2", ry: "2" }
            path { d: "M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" }
        }
    }
}

/// Compact file icon for task view rows (no sidebar indent margin).
fn task_view_file_icon(color: &str) -> Element {
    rsx! {
        svg {
            width: "13", height: "13", view_box: "0 0 24 24", fill: "none",
            stroke: "{color}", stroke_width: "1.8", stroke_linecap: "round", stroke_linejoin: "round",
            style: "flex: 0 0 auto;",
            path { d: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" }
            path { d: "M14 2v6h6" }
        }
    }
}
