//! Reader notes: read the selection the WebView holds, and turn a right-click
//! on selected text into a note request.

/// Installed once: remembers the last selection made inside a rendered pane,
/// and reports a right-click on selected text as `{kind:'note_menu', x, y, …}`.
///
/// why: remembering is what makes the palette and the menu work at all —
/// opening the palette moves focus into its input and clicking a menu button
/// collapses the document selection, both before Rust can read it. The memory
/// hangs off the text node the selection started in, so re-rendering a pane
/// (live reload, tab switch, theme switch, collapsing the split) drops it: the
/// quote would no longer describe what is on screen.
const NOTE_BRIDGE_JS: &str = r#"
if (!window.__mdoNoteBound) {
  window.__mdoNoteBound = true;
  window.__mdoNoteSel = null;
  // why: a note needs a pane index to know which file it belongs to, so only
  // selections inside a pane body (not the Task View preview) can be quoted.
  const paneBody = (node) => {
    const el = node && (node.nodeType === 1 ? node : node.parentElement);
    return el ? el.closest('.markdown-body[data-mdo-pane]') : null;
  };
  const paneIndex = (body) => Number(body.dataset.mdoPane) || 0;
  window.__mdoNoteCapture = () => {
    const sel = document.getSelection();
    if (!sel || sel.isCollapsed || !sel.rangeCount) return null;
    const quote = sel.toString().trim();
    if (!quote) return null;
    const range = sel.getRangeAt(0);
    const body = paneBody(range.startContainer);
    // why: a selection dragged across the split belongs to no single file.
    if (!body || body !== paneBody(range.endContainer)) return null;
    let heading = null;
    for (const h of body.querySelectorAll('h1,h2,h3,h4,h5,h6')) {
      // why: headings come in document order, so the first one the selection
      // does not follow ends the search (being inside one still counts).
      if (!(h.compareDocumentPosition(range.startContainer) & Node.DOCUMENT_POSITION_FOLLOWING)) break;
      const text = h.textContent.replace(/\s+/g, ' ').trim();
      if (text) heading = '#'.repeat(Number(h.tagName.slice(1))) + ' ' + text;
    }
    return { quote, heading, pane: paneIndex(body), node: range.startContainer, body };
  };
  window.__mdoNoteRemembered = () => {
    const cap = window.__mdoNoteSel;
    if (!cap || !document.contains(cap.node)) return null;
    const body = paneBody(cap.node);
    if (!body) return null;
    // why: re-read the pane index — the same document can move panes.
    return { quote: cap.quote, heading: cap.heading, pane: paneIndex(body) };
  };
  document.addEventListener('selectionchange', () => {
    const cap = window.__mdoNoteCapture();
    if (cap) { window.__mdoNoteSel = cap; return; }
    // why: forget only when the user collapsed the selection in the document
    // itself. A click that moves focus into an overlay input, or onto app
    // chrome, must leave what they had selected intact.
    const focused = document.activeElement;
    if (focused && (focused.tagName === 'INPUT' || focused.tagName === 'TEXTAREA')) return;
    if (paneBody(document.getSelection()?.anchorNode)) window.__mdoNoteSel = null;
  });
  document.addEventListener('contextmenu', (e) => {
    const cap = window.__mdoNoteCapture();
    // why: take over the menu only for a right-click inside the very pane body
    // the selection would be quoted from; elsewhere the WebView's own menu
    // (copy, look up) is the right one.
    if (!cap || paneBody(e.target) !== cap.body) return;
    e.preventDefault();
    dioxus.send({ kind: 'note_menu', x: e.clientX, y: e.clientY, quote: cap.quote, heading: cap.heading, pane: cap.pane });
  });
}
"#;

/// One-shot probe: report the live selection, or the last remembered one.
const NOTE_SELECTION_JS: &str = r#"
(() => {
  const live = window.__mdoNoteCapture && window.__mdoNoteCapture();
  const cap = live || (window.__mdoNoteRemembered && window.__mdoNoteRemembered());
  dioxus.send({
    kind: 'note_selection',
    quote: cap ? cap.quote : '',
    heading: cap ? cap.heading : null,
    pane: cap ? cap.pane : 0,
  });
})();
"#;

/// The persistent selection tracker + right-click bridge.
pub(crate) fn note_bridge_js() -> &'static str {
    NOTE_BRIDGE_JS
}

/// A one-shot request for the current selection.
pub(crate) fn note_selection_js() -> &'static str {
    NOTE_SELECTION_JS
}
