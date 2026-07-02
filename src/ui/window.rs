use super::*;

/// Open a second main `mzed` window (independent state, same process).
pub(crate) fn open_main_window() {
    use dioxus::desktop::Config;
    let saved = config::load();
    let window = services::platform::main_window_builder(saved.window_width, saved.window_height);
    dioxus::desktop::window().new_window(
        VirtualDom::new(App),
        Config::new().with_menu(build_menu()).with_window(window),
    );
}

/// Open a mermaid diagram in its own desktop window, rendered at zoom 1.
pub(crate) fn open_mermaid_window(src: String, dark: bool) {
    use dioxus::desktop::Config;
    let dom = VirtualDom::new_with_props(MermaidWindow, MermaidWindowProps { src, dark });
    let window = services::platform::mermaid_window_builder();
    dioxus::desktop::window().new_window(dom, Config::new().with_window(window));
}

/// The popped-out mermaid viewer window. Renders the source as a centered
/// diagram with zoom/pan controls.
///
/// Layout:
///   ┌─ toolbar (fixed, top) ──────────────────────────────┐
///   │  [−] [+] [1:1] [fit]                                │
///   └─────────────────────────────────────────────────────┘
///   ┌─ viewport (#mdo-vp, fills remaining height) ────────┐
///   │   ┌─ stage (#mdo-stage, transform origin) ─────┐   │
///   │   │  <pre class="mermaid"> … </pre>            │   │
///   │   └─────────────────────────────────────────────┘   │
///   └─────────────────────────────────────────────────────┘
///
/// The JS in `mermaid.rs` attaches wheel (Cmd+scroll / pinch), mousedown drag,
/// and the four toolbar button handlers after mermaid renders.
#[component]
fn MermaidWindow(src: String, dark: bool) -> Element {
    let bg = if dark { "#0d1117" } else { "#ffffff" };
    let bar_bg = if dark {
        "rgba(22,27,34,0.92)"
    } else {
        "rgba(246,248,250,0.92)"
    };
    let bar_color = if dark { "#e6edf3" } else { "#1f2328" };
    let btn_style = format!(
        "display:inline-flex;align-items:center;justify-content:center;\
         padding:4px 10px;font-size:13px;line-height:1;border:none;border-radius:6px;\
         background:transparent;color:{bar_color};cursor:pointer;\
         transition:background 0.12s ease;"
    );

    use_effect(move || {
        let script = js::mermaid_window_js(dark);
        spawn(async move {
            let _ = document::eval(&script).recv::<()>().await;
        });
    });
    rsx! {
        document::Stylesheet { href: MDO_CSS }
        document::Script { src: MERMAID_JS }
        // Toolbar
        div {
            style: "position:fixed;top:0;left:0;right:0;z-index:100;\
                    display:flex;align-items:center;gap:4px;\
                    padding:6px 12px;backdrop-filter:blur(8px);\
                    background:{bar_bg};border-bottom:1px solid rgba(127,127,127,0.18);",
            button {
                id: "mdo-btn-out",
                style: "{btn_style}",
                title: "Zoom out (Cmd+Scroll)",
                "−"
            }
            button {
                id: "mdo-btn-in",
                style: "{btn_style}",
                title: "Zoom in (Cmd+Scroll)",
                "+"
            }
            button {
                id: "mdo-btn-reset",
                style: "{btn_style}",
                title: "Reset to 1:1",
                "1:1"
            }
            button {
                id: "mdo-btn-fit",
                style: "{btn_style}",
                title: "Fit to window",
                "Fit"
            }
        }
        // Viewport: overflow hidden, cursor grab — JS controls transforms
        div {
            id: "mdo-vp",
            style: "margin:0;padding-top:44px;width:100vw;height:100vh;\
                    box-sizing:border-box;overflow:hidden;\
                    background:{bg};cursor:grab;user-select:none;",
            // Stage: the transform origin for zoom/pan
            div {
                id: "mdo-stage",
                style: "display:inline-block;transform-origin:0 0;padding:24px;",
                pre { class: "mermaid", "{src}" }
            }
        }
    }
}
