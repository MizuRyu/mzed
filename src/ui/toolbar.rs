use super::*;
/// Vertical toolbar floating at the content pane's top-right (mo-style). Three
/// icon buttons: toggle the table of contents, copy the active file's raw
/// markdown, and toggle raw (source) view. Shifts down when the find bar is open
/// so the two don't overlap.
#[component]
pub(crate) fn ContentToolbar(
    mut theme: Signal<theme::Theme>,
    mut toc_open: Signal<bool>,
    mut raw_view: Signal<bool>,
    has_toc: bool,
    find_open: bool,
    dark: bool,
    on_copy: EventHandler<()>,
) -> Element {
    let icon = if dark { "#c9d1d9" } else { "#57606a" };
    let on_bg = if dark { "#1f6feb" } else { "#0969da" };
    let on_fg = "#ffffff";
    // Float over the content area, not the tab bar. The toolbar is absolutely
    // positioned within the (non-scrolling) content column whose top edge is
    // the tab bar, so clear the tab bar's height (~38px) first, then drop
    // further when the find bar is showing below it.
    const TABBAR_H: i32 = 38;
    const FINDBAR_H: i32 = 44;
    let top = TABBAR_H + 8 + if find_open { FINDBAR_H } else { 0 };

    let toc_on = toc_open() && has_toc;
    let raw_on = raw_view();

    let btn_style = |active: bool| {
        if active {
            format!(
                "display: flex; align-items: center; justify-content: center; width: 30px; height: 30px; border: none; border-radius: 7px; cursor: pointer; background: {on_bg}; color: {on_fg};"
            )
        } else {
            format!(
                "display: flex; align-items: center; justify-content: center; width: 30px; height: 30px; border: none; border-radius: 7px; cursor: pointer; background: transparent; color: {icon};"
            )
        }
    };

    rsx! {
        div {
            class: "mdo-content-toolbar",
            style: "position: absolute; top: {top}px; right: 8px; z-index: 800; display: flex; flex-direction: column; gap: 4px;",
            button {
                style: btn_style(false),
                class: "mdo-tool-btn",
                title: if dark { "Switch to light theme" } else { "Switch to dark theme" },
                onclick: move |_| {
                    // Explicit toggle off the resolved appearance (escapes System).
                    theme.set(if dark { theme::Theme::Light } else { theme::Theme::Dark });
                },
                if dark { {toolbar_sun_icon()} } else { {toolbar_moon_icon()} }
            }
            div {
                style: "height: 1px; margin: 2px 4px; background: rgba(127,127,127,0.25);",
            }
            button {
                style: btn_style(toc_on),
                class: "mdo-tool-btn",
                title: "Toggle table of contents",
                onclick: move |_| {
                    let v = toc_open();
                    toc_open.set(!v);
                },
                {toolbar_list_icon()}
            }
            button {
                id: "mdo-copy-md-btn",
                style: btn_style(false),
                class: "mdo-tool-btn",
                title: "Copy markdown source",
                onclick: move |_| on_copy.call(()),
                {toolbar_clipboard_icon()}
            }
            button {
                style: btn_style(raw_on),
                class: "mdo-tool-btn",
                title: "Toggle raw markdown view",
                onclick: move |_| {
                    let v = raw_view();
                    raw_view.set(!v);
                },
                {toolbar_code_icon()}
            }
        }
    }
}

/// lucide-style list / table-of-contents icon (currentColor stroke).
fn toolbar_list_icon() -> Element {
    rsx! {
        svg {
            width: "16", height: "16", view_box: "0 0 24 24", fill: "none",
            stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
            line { x1: "8", y1: "6", x2: "21", y2: "6" }
            line { x1: "8", y1: "12", x2: "21", y2: "12" }
            line { x1: "8", y1: "18", x2: "21", y2: "18" }
            line { x1: "3", y1: "6", x2: "3.01", y2: "6" }
            line { x1: "3", y1: "12", x2: "3.01", y2: "12" }
            line { x1: "3", y1: "18", x2: "3.01", y2: "18" }
        }
    }
}

/// lucide-style clipboard icon (currentColor stroke).
fn toolbar_clipboard_icon() -> Element {
    rsx! {
        svg {
            width: "16", height: "16", view_box: "0 0 24 24", fill: "none",
            stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
            rect { x: "9", y: "2", width: "6", height: "4", rx: "1" }
            path { d: "M9 4H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2h-2" }
        }
    }
}

/// lucide-style code (`</>`) icon (currentColor stroke).
fn toolbar_code_icon() -> Element {
    rsx! {
        svg {
            width: "16", height: "16", view_box: "0 0 24 24", fill: "none",
            stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
            path { d: "M16 18l6-6-6-6" }
            path { d: "M8 6l-6 6 6 6" }
        }
    }
}

/// lucide-style sun icon (shown in dark mode: click to switch to light).
fn toolbar_sun_icon() -> Element {
    rsx! {
        svg {
            width: "16", height: "16", view_box: "0 0 24 24", fill: "none",
            stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
            circle { cx: "12", cy: "12", r: "4" }
            line { x1: "12", y1: "2", x2: "12", y2: "4" }
            line { x1: "12", y1: "20", x2: "12", y2: "22" }
            line { x1: "4.93", y1: "4.93", x2: "6.34", y2: "6.34" }
            line { x1: "17.66", y1: "17.66", x2: "19.07", y2: "19.07" }
            line { x1: "2", y1: "12", x2: "4", y2: "12" }
            line { x1: "20", y1: "12", x2: "22", y2: "12" }
            line { x1: "4.93", y1: "19.07", x2: "6.34", y2: "17.66" }
            line { x1: "17.66", y1: "6.34", x2: "19.07", y2: "4.93" }
        }
    }
}

/// lucide-style moon icon (shown in light mode: click to switch to dark).
fn toolbar_moon_icon() -> Element {
    rsx! {
        svg {
            width: "16", height: "16", view_box: "0 0 24 24", fill: "none",
            stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
            path { d: "M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" }
        }
    }
}
