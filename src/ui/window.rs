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

/// The popped-out mermaid viewer window. Renders the source as a centered,
/// scrollable diagram at the window's native scale.
#[component]
fn MermaidWindow(src: String, dark: bool) -> Element {
    let bg = if dark { "#0d1117" } else { "#ffffff" };
    use_effect(move || {
        let script = js::mermaid_window_js(dark);
        spawn(async move {
            let _ = document::eval(&script).recv::<()>().await;
        });
    });
    rsx! {
        document::Stylesheet { href: MDO_CSS }
        document::Script { src: MERMAID_JS }
        div {
            // `safe center` keeps small diagrams centered while letting large
            // ones scroll from the top-left instead of clipping their edges.
            style: "margin: 0; width: 100vw; height: 100vh; overflow: auto; background: {bg}; display: flex; justify-content: safe center; align-items: safe center;",
            div {
                style: "padding: 24px;",
                pre { class: "mermaid", "{src}" }
            }
        }
    }
}
