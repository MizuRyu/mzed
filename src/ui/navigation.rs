use super::*;
use std::collections::HashMap;
use std::path::Path;

/// The text a tab shows: the file name, or the whole path when it has none.
fn tab_label(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// Tab strip above the content. Each tab shows the file name with a close (×)
/// button; clicking a tab activates it. Every tab sits in the same scrollable
/// row — nothing is drawn on top of its neighbours — so a wide set is reached
/// by scrolling sideways (see `tab_wheel_js`) and the active tab is pulled back
/// into view on every switch.
#[component]
pub(crate) fn TabBar(
    mut tabs: Signal<Tabs>,
    root: Option<PathBuf>,
    pane: u8,
    dark: bool,
) -> Element {
    // Switching to a tab parked off the right edge would otherwise leave the
    // strip showing a different tab as the current one.
    use_effect(move || {
        let Some(active) = tabs.read().active().cloned() else {
            return;
        };
        let script = js::tab_scroll_js(pane, &active.to_string_lossy());
        spawn(async move {
            let _ = document::eval(&script).await;
        });
    });

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

    // Two tabs with the same file name are indistinguishable from the name
    // alone, so those tabs also show their parent directory.
    let mut label_counts: HashMap<String, usize> = HashMap::new();
    for path in &paths {
        *label_counts.entry(tab_label(path)).or_default() += 1;
    }
    let root_ref = root.as_deref();

    rsx! {
        div {
            class: "mdo-tabbar",
            "data-mdo-pane": "{pane}",
            style: "flex: 0 0 auto; display: flex; flex-wrap: nowrap; overflow-x: auto; overflow-y: hidden; background: {bar_bg}; border-bottom: 1px solid {border}; font: 13px -apple-system, sans-serif;",
            for path in paths {
                {
                    let name = tab_label(&path);
                    let parent = if label_counts.get(&name).copied().unwrap_or(0) > 1 {
                        path.parent()
                            .and_then(Path::file_name)
                            .map(|s| s.to_string_lossy().to_string())
                    } else {
                        None
                    };
                    // Names are truncated, so the tooltip carries the whole
                    // path (project-relative where it has one).
                    let tooltip = root_ref
                        .and_then(|r| path.strip_prefix(r).ok())
                        .unwrap_or(path.as_path())
                        .display()
                        .to_string();
                    let is_active = active.as_deref() == Some(path.as_path());
                    let tab_style = if is_active {
                        format!("background: {active_bg}; color: {active_fg}; border-bottom: 2px solid #0969da;")
                    } else {
                        "background: transparent; color: #8b949e; border-bottom: 2px solid transparent;".to_string()
                    };
                    let tab_path = path.to_string_lossy().to_string();
                    let act_path = path.clone();
                    let close_path = path.clone();
                    rsx! {
                        div {
                            style: "display: flex; align-items: center; gap: 6px; box-sizing: border-box; flex: 0 0 auto; min-width: 96px; max-width: 220px; padding: 6px 10px; cursor: pointer; border-right: 1px solid {border}; {tab_style}",
                            title: "{tooltip}",
                            "data-mdo-tab": "{tab_path}",
                            onclick: move |_| tabs.write().activate(&act_path),
                            span {
                                style: "flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                "{name}"
                            }
                            if let Some(parent) = parent {
                                span {
                                    style: "flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; color: #8b949e; opacity: 0.75;",
                                    "{parent}"
                                }
                            }
                            span {
                                style: "flex: 0 0 auto; color: #8c959f; padding: 0 2px; border-radius: 3px;",
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
