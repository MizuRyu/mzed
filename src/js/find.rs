/// In-document find. Uses the CSS Custom Highlight API to mark every occurrence
/// of a query inside `.markdown-body` without mutating its DOM (so re-rendering
/// and code highlighting are untouched). The query is JSON-encoded here before
/// injection. An empty query clears the highlight. Defines `window.__mdoFind`
/// once and (re)invokes it; also exposes next/prev scrolling via `__mdoFindStep`.
pub(crate) fn find_highlight_js(query: &str) -> String {
    let query_json = serde_json::to_string(query).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"
(() => {{
  const q = {query_json};
  const body = document.querySelector('.markdown-body');
  if (!body) return;
  if (!window.__mdoFindState) window.__mdoFindState = {{ ranges: [], idx: 0 }};
  const supported = ('highlights' in CSS) && (typeof Highlight !== 'undefined');
  if (!supported) return;
  CSS.highlights.delete('mdo-find');
  CSS.highlights.delete('mdo-find-current');
  window.__mdoFindState.ranges = [];
  window.__mdoFindState.idx = 0;
  if (!q) return;
  const needle = q.toLowerCase();
  const walker = document.createTreeWalker(body, NodeFilter.SHOW_TEXT);
  const ranges = [];
  let node;
  while ((node = walker.nextNode())) {{
    const text = node.nodeValue.toLowerCase();
    let from = 0, i;
    while ((i = text.indexOf(needle, from)) !== -1) {{
      const r = document.createRange();
      r.setStart(node, i);
      r.setEnd(node, i + needle.length);
      ranges.push(r);
      from = i + needle.length;
    }}
  }}
  window.__mdoFindState.ranges = ranges;
  if (ranges.length) {{
    CSS.highlights.set('mdo-find', new Highlight(...ranges));
    const cur = ranges[0];
    CSS.highlights.set('mdo-find-current', new Highlight(cur));
    cur.startContainer.parentElement?.scrollIntoView({{ block: 'center' }});
  }}
}})();
"#
    )
}

/// Move the current find match forward (`+1`) or backward (`-1`) and re-centre.
const FIND_STEP_JS: &str = r#"
(() => {
  const st = window.__mdoFindState;
  if (!st || !st.ranges.length) return;
  const dir = __MDO_DIR__;
  st.idx = (st.idx + dir + st.ranges.length) % st.ranges.length;
  const cur = st.ranges[st.idx];
  if (!('highlights' in CSS)) return;
  CSS.highlights.set('mdo-find-current', new Highlight(cur));
  cur.startContainer.parentElement?.scrollIntoView({ block: 'center' });
})();
"#;

/// Build JS that moves the current find match in `dir`.
pub(crate) fn find_step_js(dir: i32) -> String {
    FIND_STEP_JS.replace("__MDO_DIR__", &dir.to_string())
}
