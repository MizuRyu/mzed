use super::*;
use crate::services::task_scan::{self, TaskItem};
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

/// Full-screen Task View mode (Cmd+Shift+D).
///
/// Replaces the normal sidebar + content area with a 2-pane layout:
/// left = task tree (project-grouped, status-coloured), right = selected
/// `task.md` rendered via the existing markdown pipeline.
#[component]
pub(crate) fn TaskView(
    roots: Signal<Vec<PathBuf>>,
    /// Bumped by the app's file watcher on any change under the current
    /// roots. Subscribed only in This Project scope for live rescans.
    fs_tick: Signal<u32>,
    /// Bumped by the app (Cmd+R) or the ↻ header button to force a re-scan.
    mut refresh_token: Signal<u32>,
    scan_roots: Signal<Vec<PathBuf>>,
    scan_exclude: Signal<Vec<String>>,
    subpath: Signal<String>,
    default_days: Signal<u32>,
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
    let fg = if dark { "#c9d1d9" } else { "#1f2328" };
    let muted = if dark { "#8b949e" } else { "#57606a" };
    let btn_border = if dark { "#30363d" } else { "#d0d7de" };

    let mut scope = use_signal(|| Scope::ThisProject);
    let initial_days = default_days();
    let mut selected_days = use_signal(move || initial_days);
    let mut groups: Signal<Vec<(String, PathBuf, Vec<TaskItem>)>> = use_signal(Vec::new);
    let mut loading = use_signal(|| true);
    // (selected_file, project_path)
    let mut selected: Signal<Option<(PathBuf, PathBuf)>> = use_signal(|| None);
    let mut doc_html = use_signal(String::new);
    let mut doc_gen = use_signal(|| 0u32);
    let mut expanded: Signal<HashSet<PathBuf>> = use_signal(HashSet::new);
    let mut scan_gen = use_signal(|| 0u32);
    // Left pane width (px), adjustable via the drag divider. Session-local.
    let mut pane_width = use_signal(|| 300u32);
    // Task View local context menu (independent of the sidebar CtxMenu).
    let mut ctx_menu: Signal<Option<TaskCtxMenu>> = use_signal(|| None);

    // ── Scan effect ────────────────────────────────────────────────────────
    // Runs every time scope/days/roots/scan_roots/subpath/refresh_token change.
    // No cache — always re-scans. Walk is cheap (repo-boundary-pruned) and runs
    // in spawn_blocking so the UI never blocks.
    use_effect(move || {
        let current_scope = scope();
        let current_roots = roots();
        let current_scan_roots = scan_roots();
        let current_exclude = scan_exclude();
        let current_subpath = subpath();
        let n_days = selected_days();
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
            let result = tokio::task::spawn_blocking(move || match current_scope {
                Scope::ThisProject => task_scan::group_and_sort(
                    task_scan::scan_this_project_blocking(&current_roots, &current_subpath),
                ),
                Scope::AllProjects => {
                    task_scan::group_and_sort(task_scan::scan_all_projects_blocking(
                        &current_scan_roots,
                        &current_roots,
                        &current_subpath,
                        Some(n_days),
                        &current_exclude,
                    ))
                }
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
                        for (proj_name, proj_path, tasks) in groups() {
                            // Project root node (variant-2: bold name + muted full path)
                            {
                                let proj_display = proj_path.display().to_string();
                                let ctx_path = proj_path.clone();
                                let copy_path = proj_path.clone();
                                rsx! {
                                    div {
                                        style: "padding: 8px 12px 4px; user-select: none;",
                                        class: "mdo-tree-row",
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
                                        // First line: project name + hover copy button
                                        div {
                                            style: "display: flex; align-items: center; gap: 4px;",
                                            span {
                                                style: "font: 600 13px -apple-system, sans-serif; \
                                                        color: {fg}; overflow: hidden; text-overflow: ellipsis; \
                                                        white-space: nowrap; flex: 1 1 auto;",
                                                "{proj_name}"
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
                                                    white-space: nowrap; margin-top: 1px;",
                                            title: "{proj_display}",
                                            "{proj_display}"
                                        }
                                    }
                                    // Task folder rows
                                    for task in tasks {
                                        {
                                            let folder_path = task.folder_path.clone();
                                            let task_md = task.task_md.clone();
                                            let project_path = task.project_path.clone();
                                            let folder_name = task.folder_name.clone();
                                            let extra_files = task.extra_files.clone();
                                            let status_color = task.meta.status.color();
                                            let status_label = task.meta.status.label();
                                            let is_open = expanded.read().contains(&folder_path);
                                            let is_folder_active = selected.read().as_ref().map(|(p, _)| {
                                                p == &task.task_md || task.extra_files.contains(p)
                                            }).unwrap_or(false);

                                            let folder_bg = if is_folder_active {
                                                if dark { "rgba(9,105,218,0.08)" } else { "rgba(9,105,218,0.05)" }
                                            } else { "transparent" };
                                            let folder_border = if is_folder_active { "#0969da" } else { "transparent" };

                                            let toggle_path = folder_path.clone();
                                            let open_md = task_md.clone();
                                            let open_proj = project_path.clone();
                                            let chevron = if is_open { "▾" } else { "▸" };

                                            let ctx_folder_path = folder_path.clone();
                                            let copy_folder_path = folder_path.clone();
                                            let fav_folder_path = folder_path.clone();
                                            let is_fav_folder = favorites.read().iter().any(|p| p == &folder_path);
                                            let star_folder_color = if is_fav_folder { "#e3b341" } else { muted };

                                            rsx! {
                                                // Task folder row
                                                div {
                                                    key: "{folder_path.display()}",
                                                    style: "padding: 5px 8px 5px 22px; cursor: pointer; \
                                                            display: flex; align-items: center; gap: 6px; \
                                                            user-select: none; \
                                                            font: 13px -apple-system, sans-serif; line-height: 1.4; \
                                                            color: {fg}; background: {folder_bg}; \
                                                            border-left: 2px solid {folder_border}; \
                                                            border-radius: 0 4px 4px 0;",
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
                                                    // Chevron
                                                    span {
                                                        style: "color: {muted}; font-size: 10px; flex: 0 0 auto; width: 10px;",
                                                        "{chevron}"
                                                    }
                                                    // Status dot
                                                    span {
                                                        style: "width: 8px; height: 8px; border-radius: 50%; \
                                                                background: {status_color}; flex: 0 0 auto;",
                                                        title: "{status_label}",
                                                    }
                                                    // Folder name
                                                    span {
                                                        style: "overflow: hidden; text-overflow: ellipsis; \
                                                                white-space: nowrap; flex: 1 1 auto;",
                                                        "{folder_name}"
                                                    }
                                                    // Hover copy button
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
                                                    // Hover star button (task folders only)
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
                                                // Children (task.md + output files)
                                                if is_open {
                                                    // task.md
                                                    {
                                                        let is_sel = selected.read().as_ref()
                                                            .map(|(p, _)| p == &task_md).unwrap_or(false);
                                                        let row_bg = if is_sel {
                                                            if dark { "rgba(9,105,218,0.12)" } else { "rgba(9,105,218,0.08)" }
                                                        } else { "transparent" };
                                                        let row_border = if is_sel { "#0969da" } else { "transparent" };
                                                        let c_path = task_md.clone();
                                                        let c_proj = project_path.clone();
                                                        let ctx_md_path = task_md.clone();
                                                        let copy_md_path = task_md.clone();
                                                        let fav_md_path = task_md.clone();
                                                        let is_fav_md = favorites.read().iter().any(|p| p == &task_md);
                                                        let star_md_color = if is_fav_md { "#e3b341" } else { muted };
                                                        rsx! {
                                                            div {
                                                                style: "padding: 4px 8px 4px 48px; cursor: pointer; \
                                                                        display: flex; align-items: center; gap: 5px; \
                                                                        font: 12px -apple-system, sans-serif; line-height: 1.4; \
                                                                        color: {fg}; background: {row_bg}; \
                                                                        border-left: 2px solid {row_border}; \
                                                                        border-radius: 0 4px 4px 0;",
                                                                class: "mdo-tree-row",
                                                                onclick: move |_| {
                                                                    selected.set(Some((c_path.clone(), c_proj.clone())));
                                                                },
                                                                oncontextmenu: move |e| {
                                                                    e.prevent_default();
                                                                    let c = e.client_coordinates();
                                                                    ctx_menu.set(Some(TaskCtxMenu {
                                                                        x: c.x as i32,
                                                                        y: c.y as i32,
                                                                        path: ctx_md_path.clone(),
                                                                        is_dir: false,
                                                                        show_fav: true,
                                                                    }));
                                                                },
                                                                {task_view_file_icon(muted)}
                                                                span {
                                                                    style: "flex: 1 1 auto; overflow: hidden; \
                                                                            text-overflow: ellipsis; white-space: nowrap;",
                                                                    "task.md"
                                                                }
                                                                // Hover copy button
                                                                button {
                                                                    class: "mdo-copy-path",
                                                                    style: "background: transparent; border: none; cursor: pointer; \
                                                                            flex: 0 0 auto; padding: 0 2px; display: flex; \
                                                                            align-items: center; color: {muted};",
                                                                    title: "パスをコピー",
                                                                    onclick: move |e| {
                                                                        e.stop_propagation();
                                                                        on_copy_path.call(copy_md_path.clone());
                                                                    },
                                                                    {copy_icon()}
                                                                }
                                                                // Hover star button
                                                                button {
                                                                    class: if is_fav_md { "mdo-fav-star on" } else { "mdo-fav-star" },
                                                                    style: "background: transparent; border: none; cursor: pointer; \
                                                                            flex: 0 0 auto; padding: 0 2px; display: flex; \
                                                                            align-items: center; color: {star_md_color};",
                                                                    title: if is_fav_md { "お気に入りから外す" } else { "お気に入りに追加" },
                                                                    onclick: move |e| {
                                                                        e.stop_propagation();
                                                                        on_toggle_fav.call(fav_md_path.clone());
                                                                    },
                                                                    svg {
                                                                        width: "13", height: "13", view_box: "0 0 24 24",
                                                                        fill: if is_fav_md { "currentColor" } else { "none" },
                                                                        stroke: "currentColor", stroke_width: "2", stroke_linejoin: "round",
                                                                        path { d: "M12 2l3 7h7l-5.5 4.5L18 21l-6-4-6 4 1.5-7.5L2 9h7z" }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                    // Extra output files
                                                    for extra in extra_files.clone() {
                                                        {
                                                            let name = extra.file_name()
                                                                .map(|s| s.to_string_lossy().to_string())
                                                                .unwrap_or_default();
                                                            let is_sel = selected.read().as_ref()
                                                                .map(|(p, _)| p == &extra).unwrap_or(false);
                                                            let row_bg = if is_sel {
                                                                if dark { "rgba(9,105,218,0.12)" } else { "rgba(9,105,218,0.08)" }
                                                            } else { "transparent" };
                                                            let row_border = if is_sel { "#0969da" } else { "transparent" };
                                                            let c_path = extra.clone();
                                                            let c_proj = project_path.clone();
                                                            let ctx_extra_path = extra.clone();
                                                            let copy_extra_path = extra.clone();
                                                            let fav_extra_path = extra.clone();
                                                            let is_fav_extra = favorites.read().iter().any(|p| p == &extra);
                                                            let star_extra_color = if is_fav_extra { "#e3b341" } else { muted };
                                                            rsx! {
                                                                div {
                                                                    key: "{extra.display()}",
                                                                    style: "padding: 4px 8px 4px 48px; cursor: pointer; \
                                                                            display: flex; align-items: center; gap: 5px; \
                                                                            font: 12px -apple-system, sans-serif; line-height: 1.4; \
                                                                            color: {muted}; background: {row_bg}; \
                                                                            border-left: 2px solid {row_border}; \
                                                                            border-radius: 0 4px 4px 0;",
                                                                    class: "mdo-tree-row",
                                                                    onclick: move |_| {
                                                                        selected.set(Some((c_path.clone(), c_proj.clone())));
                                                                    },
                                                                    oncontextmenu: move |e| {
                                                                        e.prevent_default();
                                                                        let c = e.client_coordinates();
                                                                        ctx_menu.set(Some(TaskCtxMenu {
                                                                            x: c.x as i32,
                                                                            y: c.y as i32,
                                                                            path: ctx_extra_path.clone(),
                                                                            is_dir: false,
                                                                            show_fav: true,
                                                                        }));
                                                                    },
                                                                    {task_view_file_icon(muted)}
                                                                    span {
                                                                        style: "flex: 1 1 auto; overflow: hidden; \
                                                                                text-overflow: ellipsis; white-space: nowrap;",
                                                                        "{name}"
                                                                    }
                                                                    // Hover copy button
                                                                    button {
                                                                        class: "mdo-copy-path",
                                                                        style: "background: transparent; border: none; cursor: pointer; \
                                                                                flex: 0 0 auto; padding: 0 2px; display: flex; \
                                                                                align-items: center; color: {muted};",
                                                                        title: "パスをコピー",
                                                                        onclick: move |e| {
                                                                            e.stop_propagation();
                                                                            on_copy_path.call(copy_extra_path.clone());
                                                                        },
                                                                        {copy_icon()}
                                                                    }
                                                                    // Hover star button
                                                                    button {
                                                                        class: if is_fav_extra { "mdo-fav-star on" } else { "mdo-fav-star" },
                                                                        style: "background: transparent; border: none; cursor: pointer; \
                                                                                flex: 0 0 auto; padding: 0 2px; display: flex; \
                                                                                align-items: center; color: {star_extra_color};",
                                                                        title: if is_fav_extra { "お気に入りから外す" } else { "お気に入りに追加" },
                                                                        onclick: move |e| {
                                                                            e.stop_propagation();
                                                                            on_toggle_fav.call(fav_extra_path.clone());
                                                                        },
                                                                        svg {
                                                                            width: "13", height: "13", view_box: "0 0 24 24",
                                                                            fill: if is_fav_extra { "currentColor" } else { "none" },
                                                                            stroke: "currentColor", stroke_width: "2", stroke_linejoin: "round",
                                                                            path { d: "M12 2l3 7h7l-5.5 4.5L18 21l-6-4-6 4 1.5-7.5L2 9h7z" }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
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
                        while let Ok(msg) = eval.recv::<serde_json::Value>().await {
                            if let Some(x) = msg.get("x").and_then(|v| v.as_f64()) {
                                let w = (x.round() as i64).clamp(200, 600) as u32;
                                pane_width.set(w);
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
