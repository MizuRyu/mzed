use super::*;
/// Tab strip above the content. Each tab shows the file name with a close (×)
/// button; clicking a tab activates it. The active tab is visually lifted.
#[component]
pub(crate) fn TabBar(mut tabs: Signal<Tabs>, root: Option<PathBuf>, dark: bool) -> Element {
    let snapshot = tabs.read();
    let active = snapshot.active().cloned();
    let paths: Vec<PathBuf> = snapshot.paths().to_vec();
    drop(snapshot);

    if paths.is_empty() {
        return rsx! {};
    }

    let bar_bg = if dark { "#161b22" } else { "#f6f8fa" };
    let border = if dark { "#30363d" } else { "#d0d7de" };
    let active_bg = if dark { "#0d1117" } else { "#fff" };
    let active_fg = if dark { "#e6edf3" } else { "#1f2328" };

    rsx! {
        div {
            class: "mdo-tabbar",
            style: "flex: 0 0 auto; display: flex; overflow-x: auto; background: {bar_bg}; border-bottom: 1px solid {border}; font: 13px -apple-system, sans-serif;",
            for path in paths {
                {
                    let name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    let is_active = active.as_deref() == Some(path.as_path());
                    let tab_style = if is_active {
                        format!("background: {active_bg}; color: {active_fg}; border-bottom: 2px solid #0969da;")
                    } else {
                        "background: transparent; color: #8b949e; border-bottom: 2px solid transparent;".to_string()
                    };
                    let act_path = path.clone();
                    let close_path = path.clone();
                    rsx! {
                        div {
                            style: "display: flex; align-items: center; gap: 6px; padding: 6px 10px; cursor: pointer; white-space: nowrap; border-right: 1px solid {border}; {tab_style}",
                            onclick: move |_| tabs.write().activate(&act_path),
                            span { "{name}" }
                            span {
                                style: "color: #8c959f; padding: 0 2px; border-radius: 3px;",
                                onclick: move |e| {
                                    e.stop_propagation();
                                    tabs.write().close(&close_path);
                                },
                                "×"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Right-hand table-of-contents panel. Each entry links to its heading anchor
/// (native `#slug` scroll in the WebView), indented by heading level.
#[component]
pub(crate) fn TocPanel(entries: Vec<markdown::TocEntry>, dark: bool) -> Element {
    if entries.is_empty() {
        return rsx! {};
    }
    let bg = if dark { "#161b22" } else { "#f6f8fa" };
    let border = if dark { "#30363d" } else { "#d0d7de" };
    let link = if dark { "#c9d1d9" } else { "#1f2328" };
    rsx! {
        div {
            style: "width: 240px; flex: 0 0 auto; overflow: auto; border-left: 1px solid {border}; background: {bg}; padding: 10px 8px; font: 12px -apple-system, sans-serif;",
            div {
                style: "font-weight: 600; color: #8b949e; padding: 0 8px 6px; text-transform: uppercase; letter-spacing: 0.4px; font-size: 11px;",
                "On this page"
            }
            for entry in entries {
                {
                    let indent = 8 + (entry.level.saturating_sub(1) as usize) * 12;
                    let href = format!("#{}", entry.anchor);
                    let anchor = entry.anchor.clone();
                    rsx! {
                        a {
                            href: "{href}",
                            style: "display: block; padding: 3px 8px 3px {indent}px; color: {link}; text-decoration: none; border-radius: 4px;",
                            onclick: move |e| {
                                // Native `#anchor` nav scrolls the document, but we
                                // lock body overflow, so it never reaches the inner
                                // scroll container. Scroll the heading into view via
                                // JS instead (works for the nested scroller).
                                e.prevent_default();
                                let id = serde_json::to_string(&anchor)
                                    .unwrap_or_else(|_| "\"\"".to_string());
                                spawn(async move {
                                    let js = format!(
                                        "document.getElementById({id})?.scrollIntoView({{behavior:'smooth',block:'start'}});"
                                    );
                                    let _ = document::eval(&js).recv::<()>().await;
                                });
                            },
                            "{entry.text}"
                        }
                    }
                }
            }
        }
    }
}
