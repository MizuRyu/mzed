use super::*;
use crate::services::task_scan::{self, TaskItem};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Scope {
    ThisProject,
    AllProjects,
}

/// Cache key for session-scoped scan results.
///
/// When the key matches an existing cache entry the walk is skipped entirely.
/// The user can force a re-scan via the ↻ button in the header.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ScanCacheKey {
    scope: Scope,
    scan_roots: Vec<PathBuf>,
    subpath: String,
    days: u32,
}

/// Session-scoped scan cache: maps a [`ScanCacheKey`] to grouped task results.
type ScanCache = HashMap<ScanCacheKey, Vec<(String, PathBuf, Vec<TaskItem>)>>;

/// Full-screen Task View mode (Cmd+Shift+D).
///
/// Replaces the normal sidebar + content area with a 2-pane layout:
/// left = task tree (project-grouped, status-coloured), right = selected
/// `task.md` rendered via the existing markdown pipeline.
#[component]
pub(crate) fn TaskView(
    roots: Signal<Vec<PathBuf>>,
    scan_roots: Signal<Vec<PathBuf>>,
    subpath: Signal<String>,
    default_days: Signal<u32>,
    dark: bool,
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
    // Session-scoped cache: keyed by (scope, scan_roots, subpath, days).
    let mut scan_cache: Signal<ScanCache> = use_signal(HashMap::new);
    // Incrementing this triggers a forced re-scan (cache bypassed).
    let mut refresh_token = use_signal(|| 0u32);

    // ── Scan effect ────────────────────────────────────────────────────────
    // Triggers on: scope, selected_days, roots, scan_roots, subpath, refresh_token changes.
    use_effect(move || {
        let current_scope = scope();
        let current_roots = roots();
        let current_scan_roots = scan_roots();
        let current_subpath = subpath();
        let n_days = selected_days();
        let _ = refresh_token(); // subscribe for manual refresh

        let cache_key = ScanCacheKey {
            scope: current_scope,
            scan_roots: current_scan_roots.clone(),
            subpath: current_subpath.clone(),
            days: n_days,
        };

        // Cache hit: serve immediately without re-scanning.
        // Use peek() to avoid subscribing — writing to the cache must not re-trigger this effect.
        if let Some(cached) = scan_cache.peek().get(&cache_key).cloned() {
            loading.set(false);
            groups.set(cached);
            return;
        }

        let gen_id = {
            let mut g = scan_gen.write();
            *g += 1;
            *g
        };
        loading.set(true);
        groups.set(Vec::new());

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
                    ))
                }
            })
            .await;

            if scan_gen() != gen_id {
                return;
            }
            loading.set(false);
            if let Ok(g) = result {
                scan_cache.write().insert(cache_key, g.clone());
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

    rsx! {
        // Full-screen 2-pane layout (sits below the 37px top bar).
        div {
            style: "position: fixed; inset: 0; top: 37px; display: flex; background: {body_bg}; z-index: 100;",

            // ── Left pane ─────────────────────────────────────────────────
            div {
                style: "width: 300px; flex: 0 0 auto; display: flex; flex-direction: column; \
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
                                // Evict the current key from cache, then trigger re-scan.
                                let key = ScanCacheKey {
                                    scope: scope(),
                                    scan_roots: scan_roots(),
                                    subpath: subpath(),
                                    days: selected_days(),
                                };
                                scan_cache.write().remove(&key);
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
                                rsx! {
                                    div {
                                        style: "padding: 8px 12px 4px; user-select: none;",
                                        div {
                                            style: "font: 600 13px -apple-system, sans-serif; \
                                                    color: {fg}; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                            "{proj_name}"
                                        }
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
                                                                {task_view_file_icon(muted)}
                                                                span { "task.md" }
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
                                                                    {task_view_file_icon(muted)}
                                                                    span { "{name}" }
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
