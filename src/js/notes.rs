//! Reader notes: read the selection the WebView holds, and float the "add a
//! note" icon at the end of it.

/// Installed once: remembers the last selection made inside a rendered pane,
/// draws the note overlay (the icon, or the quoted range's highlight), and
/// reports a click on the icon as `{kind:'note_icon', …}`.
///
/// why: remembering is what makes the palette and the keybinding work at all —
/// opening the palette moves focus into its input and clicking the icon would
/// collapse the document selection, both before Rust can read it. The memory
/// hangs off the text node the selection started in, so re-rendering that pane
/// (live reload, tab switch, theme switch) drops it: the quote would no longer
/// describe what is on screen.
const NOTE_BRIDGE_JS: &str = r#"
if (!window.__mdoNoteBound) {
  window.__mdoNoteBound = true;
  // The selection the icon offers, and the one an open popover quotes. The
  // second is frozen on purpose: while the popover has the keyboard, nothing
  // the document does may move the highlight out from under the note being
  // written. Only Rust (closing the popover) thaws it.
  window.__mdoNoteSel = null;
  window.__mdoNoteFrozen = null;
  window.__mdoNoteMode = 'icon';
  const ICON = '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M3 2h10a1.5 1.5 0 0 1 1.5 1.5v5A1.5 1.5 0 0 1 13 10H7.5L4 13v-3H3a1.5 1.5 0 0 1-1.5-1.5v-5A1.5 1.5 0 0 1 3 2z"/></svg>';
  // why: a note needs a pane index to know which file it belongs to, so only
  // selections inside a pane body (not the Task View preview) can be quoted.
  const paneBody = (node) => {
    const el = node && (node.nodeType === 1 ? node : node.parentElement);
    return el ? el.closest('.markdown-body[data-mdo-pane]') : null;
  };
  const paneIndex = (body) => Number(body.dataset.mdoPane) || 0;
  const dense = (text) => text.replace(/\s+/g, '').length;
  // why: measuring a large document on every selectionchange drops frames, so
  // a pane's text length and heading list are kept until its DOM changes (see
  // `watch`).
  const measured = new WeakMap();
  const measure = (body) => {
    let m = measured.get(body);
    if (!m) {
      watch(body);
      m = {
        whole: dense(body.textContent),
        headings: Array.from(body.querySelectorAll('h1,h2,h3,h4,h5,h6')),
      };
      measured.set(body, m);
    }
    return m;
  };
  // The last non-empty heading the selection follows (being inside one still
  // counts). Headings are in document order, so the ones it follows form a
  // prefix: binary-search its end, then step back over empty ones.
  const headingBefore = (headings, node) => {
    let lo = 0, hi = headings.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (headings[mid].compareDocumentPosition(node) & Node.DOCUMENT_POSITION_FOLLOWING) lo = mid + 1;
      else hi = mid;
    }
    for (let k = lo - 1; k >= 0; k--) {
      const h = headings[k];
      const text = h.textContent.replace(/\s+/g, ' ').trim();
      if (text) return '#'.repeat(Number(h.tagName.slice(1))) + ' ' + text;
    }
    return null;
  };
  window.__mdoNoteCapture = () => {
    const sel = document.getSelection();
    if (!sel || sel.isCollapsed || !sel.rangeCount) return null;
    const quote = sel.toString().trim();
    if (!quote) return null;
    const range = sel.getRangeAt(0);
    const body = paneBody(range.startContainer);
    // why: a selection dragged across the split belongs to no single file.
    if (!body || body !== paneBody(range.endContainer)) return null;
    const m = measure(body);
    const heading = headingBefore(m.headings, range.startContainer);
    // why: selecting (nearly) the whole document is a request to rewrite the
    // file, not a note on a passage — there is nothing for the quote to point
    // at. Compare without whitespace: toString() and textContent collapse it
    // differently.
    const tooBroad = m.whole > 0 && dense(quote) >= m.whole * 0.9;
    return {
      quote, heading, tooBroad,
      pane: paneIndex(body),
      node: range.startContainer,
      body,
      range: range.cloneRange(),
    };
  };
  window.__mdoNoteRemembered = () => {
    const cap = window.__mdoNoteSel;
    if (!cap || !document.contains(cap.node)) return null;
    const body = paneBody(cap.node);
    if (!body) return null;
    // why: re-read the pane index — the same document can move panes.
    return Object.assign({}, cap, { body: body, pane: paneIndex(body) });
  };
  window.__mdoNoteCurrent = () => window.__mdoNoteCapture() || window.__mdoNoteRemembered();
  // What the overlay is drawing right now.
  const shown = () => (window.__mdoNoteMode === 'quote' ? window.__mdoNoteFrozen : window.__mdoNoteCurrent());

  // The overlay lives in the pane's scroll container, never in the rendered
  // body. why: an absolutely positioned child of the scroller follows the text
  // as it scrolls for free, and the document itself stays exactly as rendered —
  // nothing for copy, find highlighting or the next re-render to trip over.
  const drop = () => document.querySelectorAll('.mdo-note-layer').forEach((el) => el.remove());
  // Where the popover should sit, published to CSS rather than to Rust: a
  // re-render rewrites the popover's style attribute, but not these.
  const anchor = (rect) => {
    const style = document.documentElement.style;
    const clamp = (value, max) => Math.min(Math.max(value, 8), Math.max(8, max));
    style.setProperty('--mdo-note-x', clamp(rect.right - 12, window.innerWidth - 340) + 'px');
    style.setProperty('--mdo-note-y', clamp(rect.bottom + 8, window.innerHeight - 56) + 'px');
  };
  // Where each overlay element goes, in the scroller's (scrolling) coordinates.
  const boxes = (cap) => {
    const host = cap && cap.body && cap.body.parentElement;
    if (!host) return null;
    const rects = Array.from(cap.range.getClientRects());
    if (!rects.length) return null;
    const hostRect = host.getBoundingClientRect();
    const at = (x, y) => ({
      x: x - hostRect.left - host.clientLeft + host.scrollLeft,
      y: y - hostRect.top - host.clientTop + host.scrollTop,
    });
    const last = rects[rects.length - 1];
    if (window.__mdoNoteMode === 'quote') {
      return { host, last, items: rects.map((rect) => {
        const spot = at(rect.left, rect.top);
        return { left: spot.x, top: spot.y, width: rect.width, height: rect.height };
      }) };
    }
    const spot = at(last.right + 4, last.top - 26);
    return { host, last, items: [{
      // Keep the icon inside the scroller: past the right edge it would widen
      // the scrollable area instead.
      left: Math.max(0, Math.min(spot.x, host.scrollLeft + host.clientWidth - 28)),
      top: Math.max(spot.y, 2),
    }] };
  };
  const apply = (el, box) => {
    el.style.left = box.left + 'px';
    el.style.top = box.top + 'px';
    if (box.width !== undefined) {
      el.style.width = box.width + 'px';
      el.style.height = box.height + 'px';
    }
  };
  const watched = new WeakSet();
  const watch = (body) => {
    if (watched.has(body)) return;
    watched.add(body);
    // Any change below the body (a re-render, mermaid / KaTeX output) can change
    // its text and headings, so the measurement goes.
    new MutationObserver(() => measured.delete(body))
      .observe(body, { childList: true, subtree: true, characterData: true });
    new MutationObserver(() => {
      const cap = window.__mdoNoteSel || window.__mdoNoteFrozen;
      // why: only the pane holding the quote invalidates it. The other pane
      // re-rendering (the split's other file, its live reload) must leave this
      // selection alone.
      if (!cap || cap.body === body) window.__mdoNoteForget();
    }).observe(body, { childList: true });
    // why: rewrapping the column moves every rect the overlay was drawn from.
    if (window.ResizeObserver) new ResizeObserver(() => schedule()).observe(body);
  };
  window.__mdoNoteForget = () => {
    window.__mdoNoteSel = null;
    window.__mdoNoteFrozen = null;
    drop();
  };
  // What to do with the memory when the document has no usable selection.
  const settle = () => {
    // why: forget only when the user collapsed the selection in the document
    // itself. A click that moves focus into an overlay input, or onto app
    // chrome, must leave what they had selected intact.
    const focused = document.activeElement;
    if (focused && (focused.tagName === 'INPUT' || focused.tagName === 'TEXTAREA')) return;
    if (paneBody(document.getSelection()?.anchorNode)) window.__mdoNoteForget();
  };
  window.__mdoNoteFreeze = () => { window.__mdoNoteFrozen = window.__mdoNoteCurrent(); };
  window.__mdoNoteThaw = () => {
    // why: the popover held the memory frozen while the document moved on —
    // the click that dismissed it collapsed the selection. Catch up now, or the
    // icon comes back for a selection nobody has any more.
    const live = window.__mdoNoteCapture();
    if (live) window.__mdoNoteSel = live;
    else settle();
  };
  window.__mdoNoteRender = () => {
    drop();
    const cap = shown();
    if (!cap || cap.tooBroad) return;
    watch(cap.body);
    const plan = boxes(cap);
    if (!plan) return;
    const layer = document.createElement('div');
    layer.className = 'mdo-note-layer';
    if (window.__mdoNoteMode === 'quote') {
      for (const box of plan.items) {
        const mark = document.createElement('div');
        mark.className = 'mdo-note-mark';
        apply(mark, box);
        layer.appendChild(mark);
      }
    } else {
      const button = document.createElement('button');
      button.className = 'mdo-note-icon';
      button.title = 'メモ';
      button.innerHTML = ICON;
      apply(button, plan.items[0]);
      // why: a plain click collapses the selection before the popover can quote
      // it; preventing the mousedown default leaves it alone.
      button.addEventListener('mousedown', (e) => e.preventDefault());
      button.addEventListener('click', () => {
        // why: read the selection again instead of trusting what the icon was
        // drawn from, and let Rust judge it — the same payload, and the same
        // refusals, as the keybinding.
        const now = window.__mdoNoteCurrent();
        dioxus.send({
          kind: 'note_icon',
          quote: now ? now.quote : '',
          heading: now ? now.heading : null,
          pane: now ? now.pane : 0,
          too_broad: now ? !!now.tooBroad : false,
        });
      });
      layer.appendChild(button);
    }
    plan.host.appendChild(layer);
    anchor(plan.last);
  };
  // Move what is already drawn (scroll, resize, rewrap) without rebuilding it.
  const reposition = () => {
    const layer = document.querySelector('.mdo-note-layer');
    if (!layer) return;
    const cap = shown();
    const plan = cap && !cap.tooBroad ? boxes(cap) : null;
    // why: a reflow can split or join the quoted lines, and then the overlay
    // needs a different number of marks than it has.
    if (!plan || plan.items.length !== layer.children.length) {
      window.__mdoNoteRender();
      return;
    }
    plan.items.forEach((box, index) => apply(layer.children[index], box));
    anchor(plan.last);
  };
  let pending = false;
  const schedule = () => {
    if (pending) return;
    pending = true;
    requestAnimationFrame(() => { pending = false; reposition(); });
  };

  const inside = (target, selector) => !!(target && target.closest && target.closest(selector));
  const onSelection = () => {
    // why: the popover quotes a fixed range. Whatever the document does with
    // its selection now, the highlight and the note stay on that range.
    if (window.__mdoNoteMode === 'quote') return;
    const cap = window.__mdoNoteCapture();
    if (cap) {
      // why: the icon must never outlive the selection it was drawn for. It
      // comes back on mouseup / keyup, once the user stopped extending it.
      window.__mdoNoteSel = cap;
      drop();
      return;
    }
    settle();
  };
  // why: a drag fires selectionchange far more often than frames are drawn;
  // one capture per frame is all the overlay can show.
  let selFrame = 0;
  document.addEventListener('selectionchange', () => {
    if (!selFrame) selFrame = requestAnimationFrame(() => { selFrame = 0; onSelection(); });
  });
  // why: mouseup / keyup draw the icon; a capture still waiting for its frame
  // would run after that and drop the icon, so it runs first.
  const flushSelection = () => {
    if (!selFrame) return;
    cancelAnimationFrame(selFrame);
    selFrame = 0;
    onSelection();
  };
  document.addEventListener('mousedown', (e) => {
    if (inside(e.target, '.mdo-note-layer') || inside(e.target, '.mdo-note-popover')) return;
    // why: one rule for the open popover — a click anywhere else dismisses it
    // without saving, including the click that starts the next selection.
    if (window.__mdoNoteMode === 'quote') {
      dioxus.send({ kind: 'note_dismiss' });
      return;
    }
    drop();
  });
  // why: the icon appears once the user lets go, not while they are still
  // dragging the selection out.
  document.addEventListener('mouseup', (e) => {
    if (window.__mdoNoteMode === 'quote' || inside(e.target, '.mdo-note-layer')) return;
    flushSelection();
    window.__mdoNoteRender();
  });
  document.addEventListener('keyup', (e) => {
    if (window.__mdoNoteMode === 'quote') return;
    if (e.shiftKey || e.key === 'Shift') {
      flushSelection();
      window.__mdoNoteRender();
    }
  });
  document.addEventListener('scroll', schedule, true);
  window.addEventListener('resize', schedule);
}
"#;

/// One-shot probe: report the live selection, or the last remembered one.
const NOTE_SELECTION_JS: &str = r#"
(() => {
  const cap = window.__mdoNoteCurrent && window.__mdoNoteCurrent();
  dioxus.send({
    kind: 'note_selection',
    quote: cap ? cap.quote : '',
    heading: cap ? cap.heading : null,
    pane: cap ? cap.pane : 0,
    too_broad: cap ? !!cap.tooBroad : false,
  });
})();
"#;

/// What the document should be showing for the selection it remembers.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum NoteOverlay {
    /// The floating icon at the end of the selection.
    Icon,
    /// The quoted range highlighted and frozen, the icon out of the way
    /// (the popover is open and holds the keyboard).
    Quote,
    /// Nothing, and the remembered selection dropped (the note was saved).
    Off,
}

/// The persistent selection tracker + note overlay.
pub(crate) fn note_bridge_js() -> &'static str {
    NOTE_BRIDGE_JS
}

/// A one-shot request for the current selection.
pub(crate) fn note_selection_js() -> &'static str {
    NOTE_SELECTION_JS
}

/// Switch the document's note overlay.
pub(crate) fn note_overlay_js(overlay: NoteOverlay) -> String {
    let body = match overlay {
        NoteOverlay::Icon => {
            "window.__mdoNoteMode = 'icon'; window.__mdoNoteFrozen = null; window.__mdoNoteThaw();"
        }
        NoteOverlay::Quote => "window.__mdoNoteMode = 'quote'; window.__mdoNoteFreeze();",
        NoteOverlay::Off => "window.__mdoNoteMode = 'icon'; window.__mdoNoteForget();",
    };
    format!(
        "(() => {{ if (!window.__mdoNoteRender) return; {body} window.__mdoNoteRender(); }})();"
    )
}
