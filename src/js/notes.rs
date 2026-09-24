//! Reader notes: read the selection the WebView holds, float the "add a note"
//! icon at the end of it, and underline the quotes of notes not yet handled.

use crate::services::notes::Note;

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
    new MutationObserver(() => {
      measured.delete(body);
      indexed.delete(body);
      // why: highlighting, Mermaid and KaTeX replace text nodes after a
      // render, which leaves the underlines' ranges pointing at nothing.
      refreshMarks(true);
    }).observe(body, { childList: true, subtree: true, characterData: true });
    new MutationObserver(() => {
      const cap = window.__mdoNoteSel || window.__mdoNoteFrozen;
      // why: only the pane holding the quote invalidates it. The other pane
      // re-rendering (the split's other file, its live reload) must leave this
      // selection alone.
      if (!cap || cap.body === body) window.__mdoNoteForget();
    }).observe(body, { childList: true });
    // why: rewrapping the column moves every rect the overlay was drawn from.
    if (window.ResizeObserver) {
      new ResizeObserver(() => { schedule(); refreshMarks(false); }).observe(body);
    }
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
    if (window.__mdoNoteMode === 'quote') hideTip();
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
    hideTip();
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

  // Notes still in the inbox: a dashed underline under each quote, the note on
  // hover. Drawn beside the body like the overlay above, never inside it.
  const indexed = new WeakMap();
  const isHeading = (el) => /^H[1-6]$/.test(el.tagName);
  // The text from the walker's position up to `stop` (the end when null) with
  // every whitespace run collapsed to one space (the rule the needle follows),
  // where each text node starts in the raw text, and where each heading starts
  // in the collapsed one. `normAt` / `origAt` are the points where the two
  // offsets drift apart. Given a needle, it stops as soon as the needle is in,
  // and gives up (null) past SECTION_WALK characters.
  const SECTION_WALK = 65536;
  const collect = (walker, stop, needle) => {
    const nodes = [], starts = [], parts = [], heads = new Map();
    const normAt = [0], origAt = [0];
    let norm = 0, orig = 0, space = false, tail = '';
    for (let n = walker.nextNode(); n && n !== stop; n = walker.nextNode()) {
      if (n.nodeType !== 3) {
        if (isHeading(n)) heads.set(n, norm);
        continue;
      }
      const raw = n.nodeValue;
      nodes.push(n);
      starts.push(orig);
      let piece = '', last = 0;
      const runs = /\s+/g;
      for (let m = runs.exec(raw); m; m = runs.exec(raw)) {
        piece += raw.slice(last, m.index);
        last = m.index + m[0].length;
        if (space && m.index === 0) {
          // Continues the previous node's run: nothing to add.
          normAt.push(norm + piece.length);
          origAt.push(orig + last);
        } else {
          piece += ' ';
          if (m[0].length > 1) {
            normAt.push(norm + piece.length);
            origAt.push(orig + last);
          }
        }
      }
      piece += raw.slice(last);
      if (piece) space = piece.endsWith(' ');
      parts.push(piece);
      norm += piece.length;
      orig += raw.length;
      if (needle) {
        // Only the new text plus a needle's worth before it can hold a new hit.
        tail = tail.slice(-needle.length) + piece;
        if (tail.includes(needle)) break;
        if (norm > SECTION_WALK) return null;
      }
    }
    return { text: parts.join(''), nodes, starts, heads, normAt, origAt };
  };
  const walkFrom = (body, node) => {
    const walker = document.createTreeWalker(body, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
    if (node) walker.currentNode = node;
    return walker;
  };
  // A body's headings in the note format (`## title`), and its whole text once
  // a note needs it. Kept until the body's DOM changes (see `watch`).
  const indexOf = (body) => {
    let ix = indexed.get(body);
    if (!ix) {
      // why a walker rather than querySelectorAll: the same result, and several
      // times cheaper on a large document in the jsdom harness.
      const heads = [];
      const walker = document.createTreeWalker(body, NodeFilter.SHOW_ELEMENT);
      for (let el = walker.nextNode(); el; el = walker.nextNode()) {
        if (!isHeading(el)) continue;
        const level = Number(el.tagName.slice(1));
        heads.push({ el, level, title: '#'.repeat(level) + ' ' + el.textContent.replace(/\s+/g, ' ').trim() });
      }
      ix = { heads, whole: null };
      indexed.set(body, ix);
    }
    return ix;
  };
  const wholeOf = (body, ix) => ix.whole || (ix.whole = collect(walkFrom(body, null), null, null));
  // The last index of sorted `list` whose value is <= `value`.
  const floorIndex = (list, value) => {
    let lo = 0, hi = list.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (list[mid] <= value) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  };
  // The first `needle` inside [from, to) of a collected text, as a Range.
  const rangeIn = (part, needle, from, to) => {
    const at = part.text.indexOf(needle, from);
    if (at < 0 || at + needle.length > to) return null;
    const toOrig = (i) => {
      const k = floorIndex(part.normAt, i);
      return part.origAt[k] + (i - part.normAt[k]);
    };
    const start = toOrig(at), end = toOrig(at + needle.length - 1) + 1;
    const first = floorIndex(part.starts, start), last = floorIndex(part.starts, end - 1);
    const range = new Range();
    range.setStart(part.nodes[first], start - part.starts[first]);
    range.setEnd(part.nodes[last], end - part.starts[last]);
    return range;
  };
  // The quote's first line, first in the section under its heading, then
  // anywhere. why the first line only: toString() puts line breaks between
  // blocks that the text nodes do not have. why walk the section first: the
  // quote sits near its heading, so most notes cost a few paragraphs; a long
  // section (a headingless tail) falls back to the whole text, read once.
  const findQuote = (body, note) => {
    const line = String(note.quote || '').split('\n').find((l) => l.trim());
    if (!line) return null;
    const needle = line.trim().replace(/\s+/g, ' ');
    const ix = indexOf(body);
    for (let k = 0; k < ix.heads.length; k++) {
      const h = ix.heads[k];
      if (h.title !== note.heading) continue;
      let next = null;
      for (let j = k + 1; j < ix.heads.length && !next; j++) {
        if (ix.heads[j].level <= h.level) next = ix.heads[j].el;
      }
      const part = ix.whole ? null : collect(walkFrom(body, h.el), next, needle);
      const range = part
        ? rangeIn(part, needle, 0, part.text.length)
        : rangeIn(wholeOf(body, ix), needle, ix.whole.heads.get(h.el), next ? ix.whole.heads.get(next) : Infinity);
      if (range) return range;
    }
    const whole = wholeOf(body, ix);
    return rangeIn(whole, needle, 0, whole.text.length);
  };
  // Per pane with something to show: where, and which note each range is.
  let marks = [];
  // why clone rather than createElement: every document access rescans it for
  // named elements after a DOM change in the jsdom harness; WebKit is
  // indifferent.
  const underline = document.createElement('div');
  underline.className = 'mdo-note-underline';
  // Lay the underlines out from the ranges (after a reflow, without searching),
  // and keep where they are for the hover test.
  const placeMarks = () => {
    for (const mark of marks) {
      const { host, layer, items } = mark;
      const hostRect = host.getBoundingClientRect();
      mark.originX = hostRect.left + host.clientLeft;
      mark.originY = hostRect.top + host.clientTop;
      const dx = host.scrollLeft - mark.originX;
      const dy = host.scrollTop - mark.originY;
      const lines = [];
      mark.hits = [];
      for (const { range, index } of items) {
        for (const rect of range.getClientRects()) {
          if (!rect.width) continue;
          const left = rect.left + dx, top = rect.bottom + dy - 4;
          const line = underline.cloneNode(false);
          line.style.left = left + 'px';
          line.style.top = top + 'px';
          line.style.width = rect.width + 'px';
          lines.push(line);
          // The hit area is the underline plus a few pixels: the rest of the
          // line is the text's, for selecting and clicking links.
          mark.hits.push({ left, top, right: left + rect.width, bottom: top + 6, index });
        }
      }
      layer.replaceChildren(...lines);
    }
  };
  const drawMarks = () => {
    hideTip();
    const data = window.__mdoNoteMarkData;
    // Search every pane before changing the DOM (see `underline`).
    const next = [];
    (data ? data.panes : []).forEach((list, pane) => {
      if (!list || !list.length) return;
      const body = document.querySelector('.markdown-body[data-mdo-pane="' + pane + '"]');
      const host = body && body.parentElement;
      if (!host) return;
      watch(body);
      const items = [];
      list.forEach((note, index) => {
        const range = findQuote(body, note);
        if (range) items.push({ range, index });
      });
      if (items.length) next.push({ host, list, items });
    });
    marks.forEach(({ layer }) => layer.remove());
    marks = next.map(({ host, list, items }) => {
      const layer = underline.cloneNode(false);
      layer.className = 'mdo-note-marks' + (data.dark ? ' mdo-dark' : '');
      host.appendChild(layer);
      return { host, list, layer, items, hits: [] };
    });
    placeMarks();
  };
  window.__mdoNoteMarksDraw = drawMarks;
  let marksFrame = 0, marksSearch = false;
  const refreshMarks = (search) => {
    marksSearch = marksSearch || search;
    if (marksFrame) return;
    marksFrame = requestAnimationFrame(() => {
      marksFrame = 0;
      const again = marksSearch;
      marksSearch = false;
      if (again) drawMarks();
      else placeMarks();
    });
  };
  window.addEventListener('resize', () => refreshMarks(false));

  let tip = null;
  const hideTip = () => {
    if (tip) tip.remove();
    tip = null;
  };
  const two = (n) => String(n).padStart(2, '0');
  const stamp = (iso) => {
    const d = new Date(iso);
    if (isNaN(d)) return '';
    return two(d.getMonth() + 1) + '/' + two(d.getDate()) + ' ' + two(d.getHours()) + ':' + two(d.getMinutes());
  };
  // Every note underlined at the pointer, oldest first. why a hit test on the
  // kept rects instead of hover on the underlines: an element there would take
  // the clicks meant for the text under it (a link).
  const showTip = (e) => {
    if (!marks.length) return;
    // why: the note being written owns this spot while its popover is open.
    if (window.__mdoNoteMode === 'quote' || inside(e.target, '.mdo-note-layer')) return hideTip();
    const mark = marks.find((m) => m.host.contains(e.target));
    const notes = [];
    if (mark) {
      const x = e.clientX - mark.originX + mark.host.scrollLeft;
      const y = e.clientY - mark.originY + mark.host.scrollTop;
      for (const hit of mark.hits) {
        if (x < hit.left || x >= hit.right || y < hit.top || y >= hit.bottom) continue;
        const note = mark.list[hit.index];
        if (!notes.includes(note)) notes.push(note);
      }
    }
    if (!notes.length) return hideTip();
    notes.sort((a, b) => String(a.created_at).localeCompare(String(b.created_at)));
    if (!tip) {
      tip = document.createElement('div');
      document.body.appendChild(tip);
    }
    const data = window.__mdoNoteMarkData;
    tip.className = 'mdo-note-tip' + (data && data.dark ? ' mdo-dark' : '');
    // why textContent: notes and quotes are whatever the reader typed.
    tip.replaceChildren(...notes.map((note) => {
      const item = document.createElement('div');
      item.className = 'mdo-note-tip-item';
      const text = document.createElement('div');
      text.textContent = note.note;
      const time = document.createElement('div');
      time.className = 'mdo-note-tip-time';
      time.textContent = stamp(note.created_at);
      item.append(text, time);
      return item;
    }));
    const below = e.clientY + 16;
    const y = below + tip.offsetHeight > window.innerHeight - 8 ? e.clientY - tip.offsetHeight - 8 : below;
    tip.style.left = Math.max(8, Math.min(e.clientX + 12, window.innerWidth - tip.offsetWidth - 8)) + 'px';
    tip.style.top = Math.max(8, y) + 'px';
  };
  document.addEventListener('mousemove', showTip);
  document.addEventListener('scroll', hideTip, true);
  if (window.__mdoNoteMarkData) drawMarks();
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

/// One pane's notes as the JSON array [`note_marks_js`] takes. Blocking in
/// proportion to the notes: build it off the UI thread.
pub(crate) fn note_marks_pane_json(notes: &[Note]) -> String {
    let notes: Vec<serde_json::Value> = notes
        .iter()
        .map(|note| {
            serde_json::json!({
                "quote": note.quote,
                "heading": note.heading,
                "note": note.note,
                "created_at": note.created_at,
            })
        })
        .collect();
    serde_json::Value::Array(notes).to_string()
}

/// Hand the document the inbox notes of each pane (`[left, right]`, each from
/// [`note_marks_pane_json`]) to underline. The WebView keeps them and re-finds
/// the quotes after every re-render; this is only needed when the notes, the
/// files or the theme change.
pub(crate) fn note_marks_js(dark: bool, panes: [&str; 2]) -> String {
    let [left, right] = panes;
    format!(
        "(() => {{ window.__mdoNoteMarkData = {{\"dark\":{dark},\"panes\":[{left},{right}]}}; if (window.__mdoNoteMarksDraw) window.__mdoNoteMarksDraw(); }})();"
    )
}

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
