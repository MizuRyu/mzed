use super::*;
/// Top-left project switcher dropdown (Zed-style). Lists known + recent
/// projects (passed in as `candidates`), filterable via a search box, marks the
/// current project with a check, and offers an "Open Folder…" escape hatch.
/// Clicking a row calls `on_pick`; the backdrop / Esc closes it.
#[component]
pub(crate) fn ProjectMenu(
    mut open: Signal<bool>,
    mut query: Signal<String>,
    candidates: Vec<PathBuf>,
    current: Option<PathBuf>,
    dark: bool,
    on_pick: EventHandler<PathBuf>,
    on_open_folder: EventHandler<()>,
) -> Element {
    let menu_bg = if dark { "#161b22" } else { "#ffffff" };
    let border = if dark { "#30363d" } else { "#d0d7de" };
    let text_color = if dark { "#e6edf3" } else { "#1f2328" };
    let muted = if dark { "#8b949e" } else { "#57606a" };
    let hover_bg = if dark { "#1f6feb22" } else { "#0969da14" };

    let mut sel = use_signal(|| 0usize);

    let q = query();
    let needle = q.trim().to_lowercase();
    let rows: Vec<PathBuf> = candidates
        .into_iter()
        .filter(|p| {
            if needle.is_empty() {
                return true;
            }
            p.to_string_lossy().to_lowercase().contains(&needle)
        })
        .collect();

    // Selectable items = the filtered projects plus the trailing "Open Folder…".
    let open_folder_idx = rows.len();
    let total = rows.len() + 1;
    let cur = sel().min(total - 1);
    let sel_bg = if dark { "#1f6feb" } else { "#0969da" };

    // Keep the highlighted row scrolled into view as the selection moves.
    use_effect(move || {
        let i = sel();
        spawn(async move {
            let script = js::overlay_row_scroll_js(js::OverlayRowKind::Project, i);
            let _ = document::eval(&script).recv::<()>().await;
        });
    });

    // Commit the current selection: open the project, or the folder picker.
    let rows_for_enter = rows.clone();
    let commit = move || {
        if cur < rows_for_enter.len() {
            on_pick.call(rows_for_enter[cur].clone());
        } else {
            on_open_folder.call(());
        }
    };

    rsx! {
        // Backdrop closes on click.
        div {
            style: "position: fixed; inset: 0; z-index: 1000;",
            onclick: move |_| open.set(false),
        }
        div {
            // position: fixed (viewport-anchored): the menu is mounted at the
            // overlay layer outside the app frame div, so absolute positioning
            // would resolve against the wrong containing block.
            style: "position: fixed; top: 38px; left: 12px; z-index: 1001; width: 320px; max-height: 60vh; display: flex; flex-direction: column; background: {menu_bg}; border-radius: 10px; box-shadow: 0 8px 32px rgba(0,0,0,0.30); overflow: hidden; color: {text_color};",
            onclick: move |e| e.stop_propagation(),
            input {
                autofocus: true,
                onmounted: move |e| {
                    spawn(async move {
                        let _ = e.set_focus(true).await;
                    });
                },
                value: "{q}",
                placeholder: "Search projects…",
                style: "width: 100%; box-sizing: border-box; padding: 10px 12px; font: 13px -apple-system, sans-serif; border: none; border-bottom: 1px solid {border}; background: transparent; color: {text_color}; outline: none;",
                oninput: move |e| {
                    query.set(e.value());
                    sel.set(0);
                },
                // Escape is intentionally NOT handled here: the window-level
                // keyboard bridge receives the same native keydown regardless
                // (Dioxus stop_propagation cannot stop it), so handling it in
                // both places closed two overlay layers per keypress. The
                // global Escape chain in app.rs closes this menu first.
                onkeydown: move |e| match e.key() {
                    Key::ArrowDown => {
                        e.prevent_default();
                        sel.set((cur + 1) % total);
                    }
                    Key::ArrowUp => {
                        e.prevent_default();
                        sel.set((cur + total - 1) % total);
                    }
                    Key::Enter => {
                        e.prevent_default();
                        commit();
                    }
                    _ => {}
                },
            }
            div {
                "data-mdo-scroll": "project-menu",
                style: "flex: 1 1 auto; overflow: auto; padding: 6px;",
                if rows.is_empty() {
                    div {
                        style: "padding: 10px 12px; opacity: 0.6; font: 13px -apple-system, sans-serif;",
                        "No projects."
                    }
                }
                for (i, p) in rows.iter().enumerate() {
                    {
                        let name = p.file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| p.display().to_string());
                        let sub = p.display().to_string();
                        let is_current = current.as_ref() == Some(p);
                        let highlighted = i == cur;
                        let row_bg = if highlighted { sel_bg } else { "transparent" };
                        let row_fg = if highlighted { "#ffffff" } else { text_color };
                        let sub_fg = if highlighted { "#ffffffcc" } else { muted };
                        let pick = p.clone();
                        rsx! {
                            div {
                                "data-mdo-prow": "{i}",
                                style: "display: flex; align-items: center; gap: 8px; padding: 8px 10px; border-radius: 6px; cursor: pointer; font: 13px -apple-system, sans-serif; background: {row_bg}; color: {row_fg};",
                                class: "mdo-proj-row",
                                onclick: move |_| on_pick.call(pick.clone()),
                                span {
                                    style: "flex: 0 0 14px; width: 14px; text-align: center; color: #2da44e;",
                                    if is_current { "✓" } else { "" }
                                }
                                div {
                                    style: "min-width: 0;",
                                    div { style: "overflow: hidden; text-overflow: ellipsis; white-space: nowrap;", "{name}" }
                                    div { style: "font-size: 11px; color: {sub_fg}; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;", "{sub}" }
                                }
                            }
                        }
                    }
                }
            }
            // Footer: Open Folder… (keyboard-selectable as the last item).
            {
                let highlighted = cur == open_folder_idx;
                let row_bg = if highlighted { sel_bg } else { hover_bg };
                let row_fg = if highlighted { "#ffffff" } else { text_color };
                rsx! {
                    div {
                        style: "border-top: 1px solid {border}; padding: 6px;",
                        div {
                            "data-mdo-prow": "{open_folder_idx}",
                            style: "padding: 8px 10px; border-radius: 6px; cursor: pointer; font: 13px -apple-system, sans-serif; color: {row_fg}; background: {row_bg};",
                            onclick: move |_| on_open_folder.call(()),
                            "Open Folder…"
                        }
                    }
                }
            }
        }
    }
}
