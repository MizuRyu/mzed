use super::*;
/// Note popover: a single input floated at the end of the selection it quotes.
/// The document publishes the anchor as `--mdo-note-x` / `--mdo-note-y` (and
/// keeps it up to date while the page scrolls), so the popover follows the text
/// without Rust re-rendering it. No quote preview — the quoted range stays
/// highlighted behind it. Enter saves, Esc closes, an empty note does nothing.
#[component]
pub(crate) fn NotePopover(
    mut text: Signal<String>,
    mut open: Signal<bool>,
    dark: bool,
    on_save: EventHandler<String>,
) -> Element {
    let bg = if dark { "#161b22" } else { "#ffffff" };
    let border = if dark { "#30363d" } else { "#d0d7de" };
    let fg = if dark { "#e6edf3" } else { "#1f2328" };
    let value = text();
    rsx! {
        div {
            // The class is the document's handle on the popover: a mousedown
            // anywhere outside it dismisses the note.
            class: "mdo-note-popover",
            style: "position: fixed; left: var(--mdo-note-x, 50%); top: var(--mdo-note-y, 56px); z-index: 900; width: 320px; padding: 6px; background: {bg}; border: 1px solid {border}; border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.25); font: 13px -apple-system, sans-serif; color: {fg};",
            input {
                autofocus: true,
                // autofocus is unreliable in the webview; force focus on mount so
                // Esc/Enter reach this input the moment the popover opens.
                onmounted: move |e| {
                    spawn(async move {
                        let _ = e.set_focus(true).await;
                    });
                },
                value: "{value}",
                placeholder: "メモ",
                style: "width: 100%; box-sizing: border-box; padding: 5px 6px; border: 1px solid {border}; border-radius: 5px; background: transparent; color: {fg}; outline: none;",
                oninput: move |e| text.set(e.value()),
                onkeydown: move |e| match e.key() {
                    Key::Escape => {
                        e.prevent_default();
                        open.set(false);
                    }
                    Key::Enter => {
                        e.prevent_default();
                        let note = text();
                        if !note.trim().is_empty() {
                            on_save.call(note);
                        }
                    }
                    _ => {}
                },
            }
        }
    }
}
