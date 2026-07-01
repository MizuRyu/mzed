/// Build JS that makes the sidebar active row authoritative in the DOM.
///
/// This deliberately clears every previously-active row first. Dioxus can skip
/// re-rendering an old tree row, which leaves stale inline/class state behind;
/// a DOM-level sync guarantees there is at most one highlighted file row.
pub(crate) fn sidebar_active_js(active_path: Option<&str>) -> String {
    let active = active_path
        .map(|p| serde_json::to_string(p).unwrap_or_else(|_| "null".to_string()))
        .unwrap_or_else(|| "null".to_string());
    format!(
        r#"
await new Promise(r => requestAnimationFrame(r));
const activePath = {active};
document.querySelectorAll('.mdo-tree-row-active').forEach((row) => {{
  row.classList.remove('mdo-tree-row-active');
}});
if (activePath !== null) {{
  document.querySelectorAll('.mdo-tree-row-file[data-mdo-tree-path]').forEach((row) => {{
    if (row.getAttribute('data-mdo-tree-path') === activePath) {{
      row.classList.add('mdo-tree-row-active');
    }}
  }});
}}
"#
    )
}

/// Build JS that keeps a highlighted overlay row visible without scrolling the
/// document. `scrollIntoView()` can move WKWebView's root scroll position even
/// for fixed overlays, leaving a visible strip below the app after Esc closes
/// the palette. This adjusts only the overlay list's `scrollTop`.
#[derive(Clone, Copy)]
pub(crate) enum OverlayRowKind {
    Command,
    Project,
    Settings,
}

impl OverlayRowKind {
    fn row_attr(self) -> &'static str {
        match self {
            Self::Command => "data-mdo-row",
            Self::Project => "data-mdo-prow",
            Self::Settings => "data-mdo-srow",
        }
    }
}

pub(crate) fn overlay_row_scroll_js(kind: OverlayRowKind, index: usize) -> String {
    let row_attr = kind.row_attr();
    format!(
        r#"
await new Promise(r => requestAnimationFrame(r));
const row = document.querySelector('[{row_attr}="{index}"]');
const scroller = row?.closest('[data-mdo-scroll]');
if (row && scroller) {{
  const top = row.offsetTop;
  const bottom = top + row.offsetHeight;
  if (top < scroller.scrollTop) {{
    scroller.scrollTop = top;
  }} else if (bottom > scroller.scrollTop + scroller.clientHeight) {{
    scroller.scrollTop = bottom - scroller.clientHeight;
  }}
}}
window.scrollTo(0, 0);
document.documentElement.scrollTop = 0;
document.body.scrollTop = 0;
"#
    )
}

/// Reset the WebView root scroll after overlays close. WKWebView can retain a
/// tiny document scroll offset after focused fixed-position inputs disappear,
/// exposing the default page background below the 100vh app frame.
pub(crate) fn reset_root_scroll_js() -> &'static str {
    r#"
await new Promise(r => requestAnimationFrame(r));
window.scrollTo(0, 0);
const root = document.scrollingElement || document.documentElement;
if (root) root.scrollTop = 0;
document.documentElement.scrollTop = 0;
document.body.scrollTop = 0;
"#
}
