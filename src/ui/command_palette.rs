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
    files: Vec<PathBuf>,
    html_export_on: bool,
    pdf_export_on: bool,
    dark: bool,
    on_action: EventHandler<palette::Action>,
) -> Element {
    let q = query();
    // Candidate rows: either commands or matched file paths. Feature-flagged
    // export commands are hidden when disabled.
    let cmd_rows: Vec<palette::Command> = palette::filter_commands(&q)
        .into_iter()
        .filter(|c| match c.action {
            palette::Action::ExportHtml => html_export_on,
            palette::Action::ExportPdf => pdf_export_on,
            _ => true,
        })
        .collect();
    let file_rows: Vec<PathBuf> = if file_mode() {
        fuzzy::rank(&q, &files, |p| {
            p.file_name().and_then(|s| s.to_str()).unwrap_or("")
        })
        .into_iter()
        .map(|(p, _)| p.clone())
        .collect()
    } else {
        Vec::new()
    };

    let len = if file_mode() {
        file_rows.len()
    } else {
        cmd_rows.len()
    };
    let cur = if len == 0 { 0 } else { sel().min(len - 1) };

    // Keep the selected row in view: on every selection change, scroll the row
    // with the matching data-mdo-row into the candidate list's visible range.
    use_effect(move || {
        let i = sel();
        spawn(async move {
            let script = js::overlay_row_scroll_js(js::OverlayRowKind::Command, i);
            let _ = document::eval(&script).recv::<()>().await;
        });
    });

    let overlay_bg = if dark { "#161b22" } else { "#ffffff" };
    let overlay_border = if dark { "#30363d" } else { "#d0d7de" };
    let text_color = if dark { "#e6edf3" } else { "#1f2328" };
    let sel_bg = if dark { "#1f6feb" } else { "#0969da" };

    // Commit the current selection.
    let file_rows_for_enter = file_rows.clone();
    let cmd_rows_for_enter = cmd_rows.clone();
    let mut commit = move || {
        if file_mode() {
            if let Some(p) = file_rows_for_enter.get(cur) {
                on_open.call(p.clone());
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
                        query.set(e.value());
                        sel.set(0);
                    },
                    onkeydown: move |e| {
                        match e.key() {
                            Key::Escape => {
                                e.prevent_default();
                                // Esc backs out of file mode first, else closes.
                                if file_mode() {
                                    file_mode.set(false);
                                    query.set(String::new());
                                    sel.set(0);
                                } else {
                                    open.set(false);
                                }
                            }
                            Key::ArrowDown => {
                                e.prevent_default();
                                if len > 0 {
                                    sel.set((cur + 1) % len);
                                }
                            }
                            Key::ArrowUp => {
                                e.prevent_default();
                                if len > 0 {
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
                        for (i, p) in file_rows.iter().enumerate() {
                            {
                                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
                                let sub = p.display().to_string();
                                let bg = if i == cur { sel_bg } else { "transparent" };
                                let fg = if i == cur { "#ffffff" } else { text_color };
                                let pick = p.clone();
                                rsx! {
                                    div {
                                        "data-mdo-row": "{i}",
                                        style: "padding: 8px 12px; border-radius: 6px; cursor: pointer; background: {bg}; color: {fg}; font: 14px -apple-system, sans-serif;",
                                        onclick: move |_| {
                                            on_open.call(pick.clone());
                                            open.set(false);
                                        },
                                        div { "{name}" }
                                        div { style: "font-size: 11px; opacity: 0.6;", "{sub}" }
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
