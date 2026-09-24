/// Paints the matches around the current one. why: `Highlight` needs a Range per
/// match, and a one-letter query on a large document has tens of thousands of
/// them; ranges far from the current match would be rebuilt on every keystroke
/// for text nobody is looking at. Stepping past the window re-centres it.
const FIND_PAINT_JS: &str = r#"
const MDO_FIND_WINDOW = 500;
function mdoFindPaint(st) {
  const lo = Math.max(0, st.idx - MDO_FIND_WINDOW);
  const hi = Math.min(st.nodes.length, st.idx + MDO_FIND_WINDOW + 1);
  const ranges = [];
  for (let k = lo; k < hi; k++) {
    const r = document.createRange();
    r.setStart(st.nodes[k], st.offsets[k]);
    r.setEnd(st.nodes[k], st.offsets[k] + st.len);
    ranges.push(r);
  }
  CSS.highlights.set('mdo-find', new Highlight(...ranges));
  const cur = ranges[st.idx - lo];
  CSS.highlights.set('mdo-find-current', new Highlight(cur));
  cur.startContainer.parentElement?.scrollIntoView({ block: 'center' });
}
"#;

/// In-document find. Uses the CSS Custom Highlight API to mark occurrences of a
/// query inside `.markdown-body` without mutating its DOM (so re-rendering and
/// code highlighting are untouched). Every match position is recorded; only the
/// window around the current one is painted. The query is JSON-encoded here
/// before injection. An empty query clears the highlight.
const FIND_HIGHLIGHT_JS: &str = r#"
(() => {
  __MDO_FIND_PAINT__
  const q = __MDO_QUERY__;
  const body = document.querySelector('.markdown-body');
  if (!body) return;
  const supported = ('highlights' in CSS) && (typeof Highlight !== 'undefined');
  if (!supported) return;
  CSS.highlights.delete('mdo-find');
  CSS.highlights.delete('mdo-find-current');
  const needle = q.toLowerCase();
  const st = { nodes: [], offsets: [], len: needle.length, idx: 0 };
  window.__mdoFindState = st;
  if (!q) return;
  const walker = document.createTreeWalker(body, NodeFilter.SHOW_TEXT);
  let node;
  while ((node = walker.nextNode())) {
    const text = node.nodeValue.toLowerCase();
    let from = 0, i;
    while ((i = text.indexOf(needle, from)) !== -1) {
      st.nodes.push(node);
      st.offsets.push(i);
      from = i + needle.length;
    }
  }
  if (st.nodes.length) mdoFindPaint(st);
})();
"#;

/// Build the find script for `query`.
pub(crate) fn find_highlight_js(query: &str) -> String {
    let query_json = serde_json::to_string(query).unwrap_or_else(|_| "\"\"".to_string());
    FIND_HIGHLIGHT_JS
        .replace("__MDO_FIND_PAINT__", FIND_PAINT_JS)
        .replace("__MDO_QUERY__", &query_json)
}

/// Move the current find match forward (`+1`) or backward (`-1`) and re-centre.
const FIND_STEP_JS: &str = r#"
(() => {
  __MDO_FIND_PAINT__
  const st = window.__mdoFindState;
  if (!st || !st.nodes.length) return;
  const dir = __MDO_DIR__;
  st.idx = (st.idx + dir + st.nodes.length) % st.nodes.length;
  if (!('highlights' in CSS)) return;
  mdoFindPaint(st);
})();
"#;

/// Build JS that moves the current find match in `dir`.
pub(crate) fn find_step_js(dir: i32) -> String {
    FIND_STEP_JS
        .replace("__MDO_FIND_PAINT__", FIND_PAINT_JS)
        .replace("__MDO_DIR__", &dir.to_string())
}
