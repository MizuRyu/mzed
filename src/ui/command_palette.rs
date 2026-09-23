use std::time::Instant;

use super::*;

/// Command palette overlay. Owns its candidate list derivation from the current
/// query + mode, and handles all navigation keys via the input's `onkeydown`
/// (Esc closes, ↑/↓ move selection, Enter runs). Commands dispatch through
/// `on_action`; file-search opens the chosen file in a new tab.
#[component]
pub(crate) fn Palette(
    mut query: Signal<String>,
    mut sel: Signal<usize>,
    mut file_mode: Signal<bool>,
    mut open: Signal<bool>,
    on_open: EventHandler<PathBuf>,
    files: Vec<files::PaletteFile>,
    html_export_on: bool,
    pdf_export_on: bool,
    dark: bool,
    on_action: EventHandler<palette::Action>,
) -> Element {
    let q = query();
    // Candidate rows: either commands or matched file paths. Feature-flagged
    // export commands are hidden when disabled.
    // Recomputed on every render; the palette re-opens per use, so the share
    // label always reflects whether the server is currently running.
    let share_url = crate::serve::app_share_url();
    let cmd_rows: Vec<palette::Command> = palette::filter_commands(&q, share_url.as_deref())
        .into_iter()
        .filter(|c| match c.action {
            palette::Action::ExportHtml => html_export_on,
            palette::Action::ExportPdf => pdf_export_on,
            _ => true,
        })
        .collect();
    let file_rows: Vec<files::PaletteFile> = if file_mode() {
        let mut ranked = fuzzy::rank_tiered(&q, &files, |f| f.keys(), |f| f.mtime);
        // Unread outranks every tier; the sort is stable, so the tiered order
        // survives inside the read group.
        ranked.sort_by_key(|f| !f.unread);
        sort_unread_by_recency(&mut ranked);
        ranked.into_iter().cloned().collect()
    } else {
        Vec::new()
    };

    let len = if file_mode() {
        file_rows.len()
    } else {
        cmd_rows.len()
    };
    let cur = if len == 0 { 0 } else { sel().min(len - 1) };

    // Hover and the keyboard drive the same `sel`; these two keep them apart.
    let mut sel_src = use_signal(|| SelChange::Keyboard);
    let mut last_key_at = use_signal(|| None::<Instant>);

    // Keep the selected row in view: on every selection change, scroll the row
    // with the matching data-mdo-row into the candidate list's visible range.
    use_effect(move || {
        let i = sel();
        if sel_src() == SelChange::Hover {
            return;
        }
        spawn(async move {
            let script = js::overlay_row_scroll_js(js::OverlayRowKind::Command, i);
            let _ = document::eval(&script).recv::<()>().await;
        });
    });

    let overlay_bg = if dark { "#161b22" } else { "#ffffff" };
    let overlay_border = if dark { "#30363d" } else { "#d0d7de" };
    let text_color = if dark { "#e6edf3" } else { "#1f2328" };
    let muted = if dark { "#8b949e" } else { "#57606a" };
    let sel_bg = if dark { "#1f6feb" } else { "#0969da" };

    // Commit the current selection.
    let file_rows_for_enter = file_rows.clone();
    let cmd_rows_for_enter = cmd_rows.clone();
    let mut commit = move || {
        if file_mode() {
            if let Some(f) = file_rows_for_enter.get(cur) {
                on_open.call(f.path.clone());
                open.set(false);
            }
        } else if let Some(c) = cmd_rows_for_enter.get(cur) {
            on_action.call(c.action);
        }
    };

    rsx! {
        // Click-away backdrop.
        div {
            style: "position: fixed; inset: 0; background: rgba(0,0,0,0.25); display: flex; justify-content: center; align-items: flex-start; z-index: 1000;",
            onclick: move |_| open.set(false),
            div {
                style: "margin-top: 12vh; width: 560px; max-width: 90vw; background: {overlay_bg}; border: 1px solid {overlay_border}; border-radius: 10px; box-shadow: 0 12px 40px rgba(0,0,0,0.35); overflow: hidden; color: {text_color};",
                // Stop backdrop click from closing when interacting with the box.
                onclick: move |e| e.stop_propagation(),
                input {
                    autofocus: true,
                    // autofocus is unreliable in the webview; force focus on mount
                    // so arrow/Enter/Esc reach the input immediately.
                    onmounted: move |e| {
                        spawn(async move {
                            let _ = e.set_focus(true).await;
                        });
                    },
                    value: "{q}",
                    placeholder: if file_mode() { "Search files…" } else { "Type a command…" },
                    style: "width: 100%; box-sizing: border-box; padding: 14px 16px; font: 15px -apple-system, sans-serif; border: none; border-bottom: 1px solid {overlay_border}; background: transparent; color: {text_color}; outline: none;",
                    oninput: move |e| {
                        last_key_at.set(Some(Instant::now()));
                        query.set(e.value());
                        sel_src.set(SelChange::Keyboard);
                        sel.set(0);
                    },
                    onkeydown: move |e| {
                        last_key_at.set(Some(Instant::now()));
                        match e.key() {
                            Key::Escape => {
                                e.prevent_default();
                                // Esc backs out of file mode first, else closes.
                                if file_mode() {
                                    file_mode.set(false);
                                    query.set(String::new());
                                    sel_src.set(SelChange::Keyboard);
                                    sel.set(0);
                                } else {
                                    open.set(false);
                                }
                            }
                            Key::ArrowDown => {
                                e.prevent_default();
                                if len > 0 {
                                    sel_src.set(SelChange::Keyboard);
                                    sel.set((cur + 1) % len);
                                }
                            }
                            Key::ArrowUp => {
                                e.prevent_default();
                                if len > 0 {
                                    sel_src.set(SelChange::Keyboard);
                                    sel.set((cur + len - 1) % len);
                                }
                            }
                            Key::Enter => {
                                e.prevent_default();
                                commit();
                            }
                            _ => {}
                        }
                    },
                }
                div {
                    "data-mdo-scroll": "palette",
                    style: "max-height: 50vh; overflow: auto; padding: 6px;",
                    if file_mode() {
                        for (i, f) in file_rows.iter().enumerate() {
                            {
                                let name = f.name.clone();
                                let dir = f.dir().to_string();
                                let bg = if i == cur { sel_bg } else { "transparent" };
                                let fg = if i == cur { "#ffffff" } else { text_color };
                                let dir_fg = if i == cur { "#ffffffcc" } else { muted };
                                let unread = f.unread;
                                let pick = f.path.clone();
                                rsx! {
                                    div {
                                        "data-mdo-row": "{i}",
                                        style: "display: flex; align-items: baseline; gap: 8px; padding: 8px 12px; border-radius: 6px; cursor: pointer; background: {bg}; color: {fg}; font: 14px -apple-system, sans-serif;",
                                        onmouseenter: move |_| {
                                            if hover_takes_selection(i, cur, (*last_key_at.peek()).map(|t| t.elapsed())) {
                                                sel_src.set(SelChange::Hover);
                                                sel.set(i);
                                            }
                                        },
                                        onclick: move |_| {
                                            on_open.call(pick.clone());
                                            open.set(false);
                                        },
                                        if unread {
                                            {unread_dot(dark)}
                                        }
                                        span { style: "flex: 0 0 auto;", "{name}" }
                                        span {
                                            style: "min-width: 0; flex: 1 1 auto; font-size: 12px; color: {dir_fg}; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                            "{dir}"
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        for (i, c) in cmd_rows.iter().enumerate() {
                            {
                                let bg = if i == cur { sel_bg } else { "transparent" };
                                let fg = if i == cur { "#ffffff" } else { text_color };
                                let action = c.action;
                                rsx! {
                                    div {
                                        "data-mdo-row": "{i}",
                                        style: "padding: 8px 12px; border-radius: 6px; cursor: pointer; background: {bg}; color: {fg}; font: 14px -apple-system, sans-serif;",
                                        onmouseenter: move |_| {
                                            if hover_takes_selection(i, cur, (*last_key_at.peek()).map(|t| t.elapsed())) {
                                                sel_src.set(SelChange::Hover);
                                                sel.set(i);
                                            }
                                        },
                                        onclick: move |_| on_action.call(action),
                                        "{c.label}"
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

/// Re-sort the unread prefix of an already `!f.unread`-partitioned list by
/// mtime descending, leaving the read suffix (still in match-tier order) and
/// tie order (stable) untouched.
fn sort_unread_by_recency(ranked: &mut [&files::PaletteFile]) {
    let unread_len = ranked.iter().take_while(|f| f.unread).count();
    ranked[..unread_len].sort_by_key(|f| std::cmp::Reverse(f.mtime));
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    fn file(name: &str, mtime: u64, unread: bool) -> files::PaletteFile {
        files::PaletteFile {
            path: PathBuf::from(name),
            rel: name.to_string(),
            name: name.to_string(),
            mtime,
            unread,
        }
    }

    #[test]
    fn 未読グループだけmtime降順に並び既読は元の順を保つ() {
        let a = file("a.md", 100, true);
        let b = file("b.md", 300, true);
        let c = file("c.md", 200, true);
        let d = file("d.md", 999, false);
        let e = file("e.md", 1, false);
        // 既読グループ(d, e)は一致順(呼び出し前の順)のまま、mtimeでは並ばない。
        let mut ranked = vec![&a, &b, &c, &d, &e];
        sort_unread_by_recency(&mut ranked);
        let names: Vec<&str> = ranked.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["b.md", "c.md", "a.md", "d.md", "e.md"]);
    }

    #[test]
    fn 未読が無ければ何もしない() {
        let d = file("d.md", 999, false);
        let e = file("e.md", 1, false);
        let mut ranked = vec![&d, &e];
        sort_unread_by_recency(&mut ranked);
        let names: Vec<&str> = ranked.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["d.md", "e.md"]);
    }
}
