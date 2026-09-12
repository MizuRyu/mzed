use super::*;
/// Note bar (Cmd+Shift+M): the in-document find bar's counterpart for leaving a
/// note on the selected text. Same place, same look, two rows — the quoted
/// selection above, the note input below. Enter saves, Esc closes, an empty
/// note does nothing.
#[component]
pub(crate) fn NoteBar(
    mut text: Signal<String>,
    mut open: Signal<bool>,
    quote: String,
    dark: bool,
    on_save: EventHandler<String>,
) -> Element {
    let bg = if dark { "#161b22" } else { "#ffffff" };
    let border = if dark { "#30363d" } else { "#d0d7de" };
    let fg = if dark { "#e6edf3" } else { "#1f2328" };
    let value = text();
    let preview = quote_preview(&quote);
    rsx! {
        div {
            style: "position: absolute; top: 8px; right: 16px; z-index: 900; display: flex; flex-direction: column; gap: 6px; width: 340px; padding: 8px; background: {bg}; border: 1px solid {border}; border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.25); font: 13px -apple-system, sans-serif; color: {fg};",
            div {
                style: "display: flex; align-items: center; gap: 6px;",
                span {
                    style: "flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; opacity: 0.7; border-left: 2px solid {border}; padding-left: 6px;",
                    "{preview}"
                }
                button {
                    style: "flex: 0 0 auto; border: none; background: transparent; color: {fg}; cursor: pointer; padding: 2px 6px; border-radius: 4px;",
                    onclick: move |_| open.set(false),
                    "×"
                }
            }
            input {
                autofocus: true,
                // autofocus is unreliable in the webview; force focus on mount so
                // Esc/Enter reach this input the moment the bar opens.
                onmounted: move |e| {
                    spawn(async move {
                        let _ = e.set_focus(true).await;
                    });
                },
                value: "{value}",
                placeholder: "メモ…",
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

/// One line of the quoted selection, for the bar's header.
fn quote_preview(quote: &str) -> String {
    const MAX: usize = 80;
    let one_line = quote.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= MAX {
        return one_line;
    }
    one_line.chars().take(MAX).collect::<String>() + "…"
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    #[test]
    fn 引用は1行に潰される() {
        assert_eq!(quote_preview("  一行目\n 二行目  "), "一行目 二行目");
    }

    #[test]
    fn 長い引用は80文字で省略される() {
        let preview = quote_preview(&"あ".repeat(100));

        assert_eq!(preview.chars().count(), 81);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn 上限内の引用は省略しない() {
        let quote = "あ".repeat(80);

        assert_eq!(quote_preview(&quote), quote);
    }
}
