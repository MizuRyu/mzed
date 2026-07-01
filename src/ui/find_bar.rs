use super::*;
/// In-document find bar (Cmd+F). A small floating overlay at the content's
/// top-right: a query input, a match counter, prev/next steppers and a close
/// button. Enter / Shift+Enter step matches; Esc closes. Highlighting itself is
/// driven by the `find_highlight_js` effect in `App`.
#[component]
pub(crate) fn FindBar(
    mut query: Signal<String>,
    mut open: Signal<bool>,
    count: usize,
    dark: bool,
    on_step: EventHandler<i32>,
) -> Element {
    let bg = if dark { "#161b22" } else { "#ffffff" };
    let border = if dark { "#30363d" } else { "#d0d7de" };
    let fg = if dark { "#e6edf3" } else { "#1f2328" };
    let q = query();
    rsx! {
        div {
            style: "position: absolute; top: 8px; right: 16px; z-index: 900; display: flex; align-items: center; gap: 6px; padding: 6px 8px; background: {bg}; border: 1px solid {border}; border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.25); font: 13px -apple-system, sans-serif; color: {fg};",
            input {
                autofocus: true,
                // autofocus is unreliable in the webview; force focus on mount so
                // Esc/Enter reach this input the moment the bar opens.
                onmounted: move |e| {
                    spawn(async move {
                        let _ = e.set_focus(true).await;
                    });
                },
                value: "{q}",
                placeholder: "Find…",
                style: "width: 180px; padding: 4px 6px; border: 1px solid {border}; border-radius: 5px; background: transparent; color: {fg}; outline: none;",
                oninput: move |e| query.set(e.value()),
                onkeydown: move |e| match e.key() {
                    Key::Escape => {
                        e.prevent_default();
                        open.set(false);
                    }
                    Key::Enter => {
                        e.prevent_default();
                        on_step.call(if e.modifiers().shift() { -1 } else { 1 });
                    }
                    _ => {}
                },
            }
            span {
                style: "min-width: 36px; text-align: center; opacity: 0.7; font-variant-numeric: tabular-nums;",
                "{count}"
            }
            button {
                style: "border: none; background: transparent; color: {fg}; cursor: pointer; padding: 2px 6px; border-radius: 4px;",
                onclick: move |_| on_step.call(-1),
                "‹"
            }
            button {
                style: "border: none; background: transparent; color: {fg}; cursor: pointer; padding: 2px 6px; border-radius: 4px;",
                onclick: move |_| on_step.call(1),
                "›"
            }
            button {
                style: "border: none; background: transparent; color: {fg}; cursor: pointer; padding: 2px 6px; border-radius: 4px;",
                onclick: move |_| open.set(false),
                "×"
            }
        }
    }
}
