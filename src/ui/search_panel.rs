use super::*;
/// Full-text search panel: a centred modal listing content hits across all
/// project markdown. Each row shows the file (relative to its root), line number
/// and snippet; clicking or Enter opens it in a tab. Esc closes; ↑/↓ navigate.
#[component]
pub(crate) fn SearchPanel(
    mut query: Signal<String>,
    mut sel: Signal<usize>,
    mut open: Signal<bool>,
    on_open: EventHandler<PathBuf>,
    hits: Vec<search::Hit>,
    roots: Vec<PathBuf>,
    dark: bool,
) -> Element {
    let overlay_bg = if dark { "#161b22" } else { "#ffffff" };
    let overlay_border = if dark { "#30363d" } else { "#d0d7de" };
    let text_color = if dark { "#e6edf3" } else { "#1f2328" };
    let sel_bg = if dark { "#1f6feb" } else { "#0969da" };

    let len = hits.len();
    let cur = if len == 0 { 0 } else { sel().min(len - 1) };
    let q = query();

    // Keep the selected hit in view as ↑/↓ moves the selection.
    use_effect(move || {
        let i = sel();
        spawn(async move {
            let script = js::overlay_row_scroll_js(js::OverlayRowKind::Settings, i);
            let _ = document::eval(&script).recv::<()>().await;
        });
    });

    let hits_for_enter = hits.clone();
    let mut commit = move || {
        if let Some(h) = hits_for_enter.get(cur) {
            on_open.call(h.path.clone());
            open.set(false);
        }
    };

    // Primary root for relative-path display.
    let root0 = roots.first().cloned();

    rsx! {
        div {
            style: "position: fixed; inset: 0; background: rgba(0,0,0,0.25); display: flex; justify-content: center; align-items: flex-start; z-index: 1000;",
            onclick: move |_| open.set(false),
            div {
                style: "margin-top: 12vh; width: 640px; max-width: 92vw; background: {overlay_bg}; border: 1px solid {overlay_border}; border-radius: 10px; box-shadow: 0 12px 40px rgba(0,0,0,0.35); overflow: hidden; color: {text_color};",
                onclick: move |e| e.stop_propagation(),
                input {
                    autofocus: true,
                    onmounted: move |e| {
                        spawn(async move {
                            let _ = e.set_focus(true).await;
                        });
                    },
                    value: "{q}",
                    placeholder: "Search in project…",
                    style: "width: 100%; box-sizing: border-box; padding: 14px 16px; font: 15px -apple-system, sans-serif; border: none; border-bottom: 1px solid {overlay_border}; background: transparent; color: {text_color}; outline: none;",
                    oninput: move |e| {
                        query.set(e.value());
                        sel.set(0);
                    },
                    onkeydown: move |e| match e.key() {
                        Key::Escape => {
                            e.prevent_default();
                            open.set(false);
                        }
                        Key::ArrowDown => {
                            e.prevent_default();
                            if len > 0 { sel.set((cur + 1) % len); }
                        }
                        Key::ArrowUp => {
                            e.prevent_default();
                            if len > 0 { sel.set((cur + len - 1) % len); }
                        }
                        Key::Enter => {
                            e.prevent_default();
                            commit();
                        }
                        _ => {}
                    },
                }
                div {
                    "data-mdo-scroll": "search-panel",
                    style: "max-height: 56vh; overflow: auto; padding: 6px;",
                    if hits.is_empty() {
                        div {
                            style: "padding: 14px 16px; opacity: 0.6; font: 13px -apple-system, sans-serif;",
                            if q.trim().is_empty() { "Type to search markdown contents." } else { "No matches." }
                        }
                    }
                    for (i, h) in hits.iter().enumerate() {
                        {
                            let bg = if i == cur { sel_bg } else { "transparent" };
                            let fg = if i == cur { "#ffffff" } else { text_color };
                            let rel = search::relative_display(&h.path, root0.as_deref());
                            let loc = if h.line == 0 {
                                rel.clone()
                            } else {
                                format!("{rel}:{}", h.line)
                            };
                            let snip = h.snippet.clone();
                            let pick = h.path.clone();
                            rsx! {
                                div {
                                    "data-mdo-srow": "{i}",
                                    style: "padding: 8px 12px; border-radius: 6px; cursor: pointer; background: {bg}; color: {fg}; font: 14px -apple-system, sans-serif;",
                                    onclick: move |_| {
                                        on_open.call(pick.clone());
                                        open.set(false);
                                    },
                                    div { style: "font-size: 12px; opacity: 0.7;", "{loc}" }
                                    div { style: "white-space: nowrap; overflow: hidden; text-overflow: ellipsis;", "{snip}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
