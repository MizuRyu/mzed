//! The single-page browser shell for `mzed serve`.
//!
//! Everything lives in one HTML string: layout CSS, and the JS that renders
//! the sidebar tree, loads documents from `/api/doc`, runs the same
//! post-render passes as the desktop (highlight.js / mermaid / KaTeX), keeps
//! a ToC panel, and polls `/api/stat` for live reload. Theme follows
//! `prefers-color-scheme` with a manual toggle; switching re-renders the
//! current document so mermaid picks up the theme.

use std::path::Path;

const SHELL: &str = r#"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="utf-8">
<title>__MDO_TITLE__ — mzed</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<link id="css-md" rel="stylesheet" href="/assets/github-markdown.css">
<link id="css-hl" rel="stylesheet" href="/assets/highlight-github.css">
<link rel="stylesheet" href="/assets/katex/katex.min.css">
<script src="/assets/highlight.min.js"></script>
<script src="/assets/mermaid.min.js"></script>
<script src="/assets/katex/katex.min.js"></script>
<script src="/assets/katex/auto-render.min.js"></script>
<style>
  :root {
    --bg: #ffffff; --panel: #f6f8fa; --border: #d0d7de;
    --fg: #1f2328; --muted: #57606a; --accent: #0969da;
    --hover: rgba(127,127,127,0.12); --active: rgba(9,105,218,0.1);
  }
  html.dark {
    --bg: #0d1117; --panel: #161b22; --border: #30363d;
    --fg: #c9d1d9; --muted: #8b949e; --accent: #58a6ff;
  }
  * { box-sizing: border-box; }
  html, body { margin: 0; height: 100%; }
  body {
    display: flex; flex-direction: column; background: var(--bg); color: var(--fg);
    font: 14px -apple-system, "Segoe UI", sans-serif;
  }
  header {
    flex: 0 0 auto; display: flex; align-items: center; gap: 10px;
    padding: 8px 14px; background: var(--panel); border-bottom: 1px solid var(--border);
  }
  header .title { font-weight: 600; font-size: 13px; }
  header .doc { color: var(--muted); font-size: 12px; overflow: hidden;
                text-overflow: ellipsis; white-space: nowrap; flex: 1 1 auto; }
  header button {
    border: 1px solid var(--border); background: transparent; color: var(--fg);
    border-radius: 6px; padding: 4px 10px; cursor: pointer; font-size: 12px;
  }
  main { flex: 1 1 auto; display: flex; min-height: 0; }
  #sidebar {
    flex: 0 0 260px; display: flex; flex-direction: column; min-height: 0;
    border-right: 1px solid var(--border); background: var(--panel);
  }
  #filter {
    margin: 8px; padding: 6px 10px; border: 1px solid var(--border);
    border-radius: 6px; background: var(--bg); color: var(--fg); outline: none;
    font-size: 12px;
  }
  #tree { flex: 1 1 auto; overflow: auto; padding: 0 4px 12px; font-size: 13px; }
  #tree .row {
    display: flex; align-items: center; gap: 5px; padding: 3px 8px;
    border-radius: 5px; cursor: pointer; white-space: nowrap; overflow: hidden;
    text-overflow: ellipsis; user-select: none;
  }
  #tree .row:hover { background: var(--hover); }
  #tree .row.active { background: var(--active); color: var(--accent); }
  #tree .chev { width: 10px; flex: 0 0 auto; color: var(--muted); font-size: 10px; }
  #tree .count { margin-left: auto; color: var(--muted); font-size: 11px; }
  #content { flex: 1 1 auto; overflow: auto; min-width: 0; }
  #content .markdown-body {
    max-width: 900px; margin: 0 auto; padding: 28px 36px 64px;
    background: transparent;
  }
  #toc {
    flex: 0 0 220px; overflow: auto; border-left: 1px solid var(--border);
    padding: 14px 10px; font-size: 12px;
  }
  #toc a {
    display: block; color: var(--muted); text-decoration: none;
    padding: 2px 6px; border-radius: 4px; overflow: hidden;
    text-overflow: ellipsis; white-space: nowrap;
  }
  #toc a:hover { color: var(--accent); background: var(--hover); }
  #toc .h1 { padding-left: 6px; }  #toc .h2 { padding-left: 18px; }
  #toc .h3 { padding-left: 30px; } #toc .h4, #toc .h5, #toc .h6 { padding-left: 42px; }
  .placeholder {
    display: flex; height: 100%; align-items: center; justify-content: center;
    color: var(--muted);
  }
  @media (max-width: 900px) { #toc { display: none; } }
</style>
</head>
<body>
<header>
  <span class="title">__MDO_TITLE__</span>
  <span class="doc" id="doc-name"></span>
  <button id="theme-toggle" title="ダーク/ライト切替">◐</button>
</header>
<main>
  <nav id="sidebar">
    <input id="filter" type="search" placeholder="ファイル名で絞り込み…">
    <div id="tree"></div>
  </nav>
  <section id="content"><div class="placeholder">← ファイルを選択してください</div></section>
  <aside id="toc"></aside>
</main>
<script>
(() => {
  'use strict';
  const treeEl = document.getElementById('tree');
  const contentEl = document.getElementById('content');
  const tocEl = document.getElementById('toc');
  const filterEl = document.getElementById('filter');
  const docNameEl = document.getElementById('doc-name');

  // ── Theme ────────────────────────────────────────────────────────────────
  let dark = window.matchMedia('(prefers-color-scheme: dark)').matches;
  function applyTheme() {
    document.documentElement.classList.toggle('dark', dark);
    document.getElementById('css-md').href =
      dark ? '/assets/github-markdown-dark.css' : '/assets/github-markdown.css';
    document.getElementById('css-hl').href =
      dark ? '/assets/highlight-github-dark.css' : '/assets/highlight-github.css';
  }
  document.getElementById('theme-toggle').addEventListener('click', () => {
    dark = !dark;
    applyTheme();
    if (state.path) loadDoc(state.path, { keepScroll: true }); // re-render mermaid
  });
  applyTheme();

  // ── State ────────────────────────────────────────────────────────────────
  const state = { path: null, mtime: 0, tree: null, treeJson: '', expanded: new Set() };

  // ── Tree ─────────────────────────────────────────────────────────────────
  function matches(node, needle) {
    if (!needle) return true;
    if (node.name.toLowerCase().includes(needle)) return true;
    return node.children.some((c) => matches(c, needle));
  }
  function renderTree() {
    const needle = filterEl.value.trim().toLowerCase();
    treeEl.textContent = '';
    const build = (nodes, depth, parent) => {
      for (const n of nodes) {
        if (!matches(n, needle)) continue;
        const row = document.createElement('div');
        row.className = 'row';
        row.style.paddingLeft = (8 + depth * 14) + 'px';
        const chev = document.createElement('span');
        chev.className = 'chev';
        const open = needle ? true : state.expanded.has(n.path);
        chev.textContent = n.is_dir ? (open ? '▾' : '▸') : '';
        row.appendChild(chev);
        const name = document.createElement('span');
        name.textContent = n.name;
        row.appendChild(name);
        if (n.is_dir) {
          const count = document.createElement('span');
          count.className = 'count';
          count.textContent = n.md_count;
          row.appendChild(count);
        }
        if (!n.is_dir && n.path === state.path) row.classList.add('active');
        row.addEventListener('click', () => {
          if (n.is_dir) {
            if (state.expanded.has(n.path)) state.expanded.delete(n.path);
            else state.expanded.add(n.path);
            renderTree();
          } else {
            loadDoc(n.path, {});
          }
        });
        parent.appendChild(row);
        if (n.is_dir && open) build(n.children, depth + 1, parent);
      }
    };
    build(state.tree || [], 0, treeEl);
  }
  filterEl.addEventListener('input', renderTree);

  async function refreshTree() {
    try {
      const res = await fetch('/api/tree');
      const text = await res.text();
      if (text !== state.treeJson) {
        state.treeJson = text;
        state.tree = JSON.parse(text);
        renderTree();
      }
    } catch (_) { /* server gone; keep the last tree */ }
  }

  // ── Document ─────────────────────────────────────────────────────────────
  async function loadDoc(path, { keepScroll = false, push = true } = {}) {
    let doc;
    try {
      const res = await fetch('/api/doc?path=' + encodeURIComponent(path));
      if (!res.ok) return;
      doc = await res.json();
    } catch (_) { return; }
    const scroll = keepScroll ? contentEl.scrollTop : 0;
    state.path = doc.path;
    state.mtime = doc.mtime;
    docNameEl.textContent = doc.path;
    document.title = doc.name + ' — mzed';
    contentEl.innerHTML = '<div class="markdown-body">' + doc.html + '</div>';
    renderToc(doc.toc);
    await postRender();
    contentEl.scrollTop = scroll;
    renderTree();
    if (push) history.replaceState(null, '', '#' + encodeURIComponent(doc.path));
  }

  function renderToc(entries) {
    tocEl.textContent = '';
    for (const e of entries) {
      const a = document.createElement('a');
      a.href = '#' + e.anchor;
      a.textContent = e.text;
      a.className = 'h' + Math.min(6, e.level);
      a.addEventListener('click', (ev) => {
        ev.preventDefault();
        document.getElementById(e.anchor)?.scrollIntoView({ block: 'start' });
      });
      tocEl.appendChild(a);
    }
  }

  // Same passes as the desktop post-render: highlight, mermaid, KaTeX, and
  // internal-link interception (data-path was containment-checked server-side).
  async function postRender() {
    const body = contentEl.querySelector('.markdown-body');
    if (!body) return;
    body.querySelectorAll('pre > code').forEach((code) => {
      if (window.hljs && !code.closest('pre.mermaid')) {
        try { hljs.highlightElement(code); } catch (_) {}
      }
    });
    if (window.mermaid) {
      mermaid.initialize({
        startOnLoad: false, securityLevel: 'strict', htmlLabels: false,
        flowchart: { htmlLabels: false, useMaxWidth: true },
        theme: dark ? 'dark' : 'default',
      });
      const pres = [...body.querySelectorAll('pre.mermaid')];
      if (pres.length) { try { await mermaid.run({ nodes: pres }); } catch (_) {} }
    }
    if (window.renderMathInElement) {
      try {
        renderMathInElement(body, { delimiters: [
          { left: '$$', right: '$$', display: true },
          { left: '\\(', right: '\\)', display: false },
          { left: '\\[', right: '\\]', display: true },
        ]});
      } catch (_) {}
    }
    body.querySelectorAll('a.mdo-link').forEach((a) => {
      a.addEventListener('click', (e) => {
        e.preventDefault();
        const p = a.dataset.path;
        if (p) loadDoc(p, {});
      });
    });
  }

  // ── Live reload ──────────────────────────────────────────────────────────
  setInterval(async () => {
    if (!state.path) return;
    try {
      const res = await fetch('/api/stat?path=' + encodeURIComponent(state.path));
      if (!res.ok) return;
      const { mtime } = await res.json();
      if (mtime !== state.mtime) loadDoc(state.path, { keepScroll: true, push: false });
    } catch (_) { /* transient */ }
  }, 700);
  setInterval(refreshTree, 3000);

  // ── Boot ─────────────────────────────────────────────────────────────────
  refreshTree().then(() => {
    const fromHash = decodeURIComponent(location.hash.slice(1));
    if (fromHash) {
      loadDoc(fromHash, { push: false });
    } else {
      // Open a sensible first document so the page never starts blank:
      // README at the root, else the shallowest file (files before subdirs).
      const firstMd = (nodes) => {
        const readme = nodes.find((n) => !n.is_dir && /^readme\.md$/i.test(n.name));
        if (readme) return readme.path;
        const file = nodes.find((n) => !n.is_dir);
        if (file) return file.path;
        for (const n of nodes) {
          const hit = firstMd(n.children);
          if (hit) return hit;
        }
        return null;
      };
      const p = firstMd(state.tree || []);
      if (p) loadDoc(p, { push: false });
    }
  });
})();
</script>
</body>
</html>
"#;

/// Build the shell page for the served root (title = the folder name).
pub(super) fn page(root: &Path) -> String {
    let title = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| root.display().to_string());
    SHELL.replace("__MDO_TITLE__", &html_escape(&title))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn pageはタイトルを埋めてプレースホルダを残さない() {
        let html = page(Path::new("/tmp/docs"));
        assert!(html.contains("<title>docs — mzed</title>"));
        assert!(!html.contains("__MDO_TITLE__"));
    }

    #[test]
    fn pageのタイトルはHTMLエスケープされる() {
        let html = page(Path::new("/tmp/<b>evil"));
        assert!(html.contains("&lt;b&gt;evil"));
        assert!(!html.contains("<b>evil"));
    }

    #[test]
    fn shellはデスクトップと同じKaTeXデリミタを使う() {
        // Single-$ inline math stays disabled, matching the desktop spec.
        assert!(SHELL.contains("left: '$$'"));
        assert!(SHELL.contains("left: '\\\\('"));
        assert!(!SHELL.contains("left: '$',"));
    }

    #[test]
    fn shellはmermaidをstrictで初期化する() {
        assert!(SHELL.contains("securityLevel: 'strict'"));
        assert!(!SHELL.contains("securityLevel: 'loose'"));
    }
}
