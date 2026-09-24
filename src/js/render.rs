/// JS run after each render: syntax highlight, mermaid, katex, copy buttons on
/// code blocks, an arto-style toolbar (copy SVG + copy as image) on mermaid
/// diagrams, and link bridging (internal `.mdo-link` and external `http(s)`
/// anchors post messages to Rust).
///
/// `__MDO_DARK__` is substituted with `true`/`false` by [`post_render_js`] so
/// mermaid renders with a theme that matches the current appearance, and
/// re-renders when the user toggles light/dark; `__MDO_MERMAID_HELPER__` is
/// filled from [`super::mermaid::helper_js`].
const POST_RENDER_TEMPLATE: &str = r#"
__MDO_MERMAID_HELPER__
__MDO_ZOOM_PAN__
await new Promise(r => requestAnimationFrame(r));
const MDO_DARK = __MDO_DARK__;
const MDO_KATEX = __MDO_KATEX__;
const MDO_POST_RENDER_START = performance.now();

// Keep html/body background in sync with the active theme so WKWebView's
// rubber-band overscroll (macOS bounce) doesn't expose a white gap.
// The content pane already carries the right background via inline style;
// this covers the document root that shows when bouncing past the edge.
const MDO_PAGE_BG = MDO_DARK ? '#0d1117' : '#ffffff';
document.documentElement.style.background = MDO_PAGE_BG;
document.body.style.background = MDO_PAGE_BG;

// lucide-style inline icons for the code-block / mermaid copy buttons.
const MDO_COPY_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>';
const MDO_IMG_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="9" cy="9" r="2"/><path d="M21 15l-5-5L5 21"/></svg>';
const MDO_CHECK_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6L9 17l-5-5"/></svg>';
const MDO_CLOSE_ICON = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>';

function mdoFlash(btn, icon) {
  btn.innerHTML = MDO_CHECK_ICON;
  setTimeout(() => { btn.innerHTML = icon; }, 1200);
}

function mdoEnsureImageLightbox() {
  let overlay = document.querySelector('.mdo-lightbox');
  if (overlay) return overlay;

  overlay = document.createElement('div');
  overlay.className = 'mdo-lightbox';
  overlay.hidden = true;
  overlay.innerHTML = '<button class="mdo-lightbox-close" aria-label="Close">' + MDO_CLOSE_ICON + '</button><img class="mdo-lightbox-img" alt="">';
  const close = () => {
    overlay.hidden = true;
    const img = overlay.querySelector('.mdo-lightbox-img');
    img.removeAttribute('src');
    img.alt = '';
  };
  overlay.addEventListener('click', (event) => {
    if (event.target === overlay) close();
  });
  overlay.querySelector('.mdo-lightbox-close').addEventListener('click', close);
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && !overlay.hidden) close();
  });
  document.body.appendChild(overlay);
  return overlay;
}

function mdoOpenImageLightbox(src, alt) {
  const overlay = mdoEnsureImageLightbox();
  const img = overlay.querySelector('.mdo-lightbox-img');
  img.src = src;
  img.alt = alt || '';
  overlay.hidden = false;
}

// Render an SVG element to a PNG blob (2x for crispness) and copy it to the
// clipboard as an image. Mermaid inlines its <style> into the SVG, so colours
// survive serialization.
function mdoSvgToPngBlob(svg) {
  const rect = svg.getBoundingClientRect();
  const w = Math.max(1, Math.ceil(rect.width));
  const h = Math.max(1, Math.ceil(rect.height));
  const clone = svg.cloneNode(true);
  clone.setAttribute('width', w);
  clone.setAttribute('height', h);
  const xml = new XMLSerializer().serializeToString(clone);
  const src = 'data:image/svg+xml;base64,' + btoa(unescape(encodeURIComponent(xml)));
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => {
      const scale = 2;
      const canvas = document.createElement('canvas');
      canvas.width = w * scale;
      canvas.height = h * scale;
      const ctx = canvas.getContext('2d');
      ctx.fillStyle = MDO_DARK ? '#161b22' : '#ffffff';
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.scale(scale, scale);
      ctx.drawImage(img, 0, 0, w, h);
      canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('toBlob null'))), 'image/png');
    };
    img.onerror = reject;
    img.src = src;
  });
}

// Copy an SVG to the clipboard as a PNG image. WebKit/Safari requires the
// ClipboardItem to receive a *Promise* of the blob and clipboard.write() to be
// called synchronously within the click gesture — awaiting the blob first
// consumes the user-activation and the write is rejected. So pass the blob
// promise straight into ClipboardItem.
async function mdoCopySvgAsImage(svg) {
  await navigator.clipboard.write([
    new ClipboardItem({ 'image/png': mdoSvgToPngBlob(svg) }),
  ]);
}

// Live reload replaces the whole pane HTML (diffing the DOM is a separate
// design), so unchanged code blocks and diagrams would be redone from source.
// These keep their finished output by source, least recently used dropped first:
// highlight.js up to 500 blocks, mermaid up to 50 diagrams (keyed with the theme,
// so toggling it re-renders). Each cache also stays under 4M characters of keys
// + values, and an entry over 1M characters is not kept at all.
const MDO_HL_CACHE_MAX = 500;
const MDO_MERMAID_CACHE_MAX = 50;
const MDO_CACHE_MAX_CHARS = 4 * 1024 * 1024;
const MDO_CACHE_ENTRY_MAX_CHARS = 1024 * 1024;
window.__mdoHlCache ??= { entries: new Map(), chars: 0 };
window.__mdoMermaidCache ??= { entries: new Map(), chars: 0 };
function mdoCacheGet(cache, key) {
  const hit = cache.entries.get(key);
  if (hit !== undefined) { cache.entries.delete(key); cache.entries.set(key, hit); }
  return hit && hit.value;
}
function mdoCachePut(cache, max, key, value, chars) {
  const size = key.length + chars;
  const old = cache.entries.get(key);
  if (old) { cache.entries.delete(key); cache.chars -= old.size; }
  if (size > MDO_CACHE_ENTRY_MAX_CHARS) return;
  cache.entries.set(key, { value, size });
  cache.chars += size;
  while (cache.entries.size > max || cache.chars > MDO_CACHE_MAX_CHARS) {
    const [oldest, entry] = cache.entries.entries().next().value;
    cache.entries.delete(oldest);
    cache.chars -= entry.size;
  }
}
// Only a diagram mermaid finished is worth keeping. On success `mermaid.run`
// replaces the <pre> contents with exactly one <svg> carrying the diagram type;
// a failure leaves the source text next to mermaid's scratch <div> (with the
// error drawing), whether or not the error SVG got its ARIA attributes. A hidden
// window or a collapsed pane can lay a diagram out at zero size, so that one is
// not kept either.
function mdoMermaidDrawn(pre) {
  const svg = pre.firstChild;
  if (pre.childNodes.length !== 1 || !svg || svg.nodeName.toLowerCase() !== 'svg') return false;
  const kind = svg.getAttribute('aria-roledescription');
  if (!kind || kind === 'error' || document.hidden) return false;
  const rect = svg.getBoundingClientRect();
  return rect.width > 0 && rect.height > 0;
}

// Process one `.markdown-body` (there can be two when the view is split): syntax
// highlight, code-copy buttons, mermaid, katex and link bridging — all scoped to
// that body so each pane is independent.
async function mdoProcessBody(body) {
  if (!body) return;

  if (window.hljs) {
    body.querySelectorAll('pre code').forEach((el) => {
      // Already highlighted (a theme toggle re-runs this without new HTML).
      if (el.dataset.highlighted) return;
      const key = el.className + '\n' + el.textContent;
      const hit = mdoCacheGet(window.__mdoHlCache, key);
      if (hit) {
        el.innerHTML = hit.html;
        el.className = hit.cls;
        el.dataset.highlighted = 'yes';
        return;
      }
      try {
        hljs.highlightElement(el);
        const html = el.innerHTML;
        mdoCachePut(window.__mdoHlCache, MDO_HL_CACHE_MAX, key, { html, cls: el.className }, html.length);
      } catch (e) { console.error(e); }
    });
  }

  // Copy button on every code block (skip mermaid).
  body.querySelectorAll('pre > code').forEach((code) => {
    const pre = code.parentElement;
    if (pre.classList.contains('mermaid') || pre.dataset.mdoCopy) return;
    pre.dataset.mdoCopy = '1';
    pre.style.position = 'relative';
    const btn = document.createElement('button');
    btn.className = 'mdo-copy-btn';
    btn.title = 'Copy';
    btn.innerHTML = MDO_COPY_ICON;
    btn.addEventListener('click', async () => {
      try {
        await navigator.clipboard.writeText(code.innerText);
        mdoFlash(btn, MDO_COPY_ICON);
      } catch (e) { console.error(e); }
    });
    pre.appendChild(btn);
  });

  if (window.mermaid) {
    const pres = [...body.querySelectorAll('pre.mermaid')];
    const toRender = [];
    pres.forEach((pre) => {
      // Stash the original source once so we can re-render on theme toggle.
      if (!pre.dataset.mdoSrc) pre.dataset.mdoSrc = pre.textContent;
      // Wrap each diagram in a centered card with a hover toolbar (once).
      let wrap = pre.closest('.mdo-mermaid');
      if (!wrap) {
        wrap = document.createElement('div');
        wrap.className = 'mdo-mermaid';
        pre.parentNode.insertBefore(wrap, pre);
        wrap.appendChild(pre);
        const bar = document.createElement('div');
        bar.className = 'mdo-mermaid-bar';
        const copyBtn = document.createElement('button');
        copyBtn.className = 'mdo-mermaid-btn';
        copyBtn.title = 'Copy source';
        copyBtn.innerHTML = MDO_COPY_ICON;
        copyBtn.addEventListener('click', async (e) => {
          e.stopPropagation(); // don't also trigger the open-in-window click
          // Copy the raw mermaid source (always available; no svg dependency).
          try { await navigator.clipboard.writeText(pre.dataset.mdoSrc || ''); mdoFlash(copyBtn, MDO_COPY_ICON); }
          catch (e) { console.error('mzed: copy source failed', e); }
        });
        const imgBtn = document.createElement('button');
        imgBtn.className = 'mdo-mermaid-btn';
        imgBtn.title = 'Copy as image';
        imgBtn.innerHTML = MDO_IMG_ICON;
        imgBtn.addEventListener('click', async (e) => {
          e.stopPropagation();
          // Query the svg from the wrapper: mermaid does not always leave it as a
          // direct child of <pre>, so pre.querySelector could miss it.
          const svg = wrap.querySelector('svg');
          if (!svg) { console.error('mzed: no rendered svg to copy'); return; }
          try { await mdoCopySvgAsImage(svg); mdoFlash(imgBtn, MDO_IMG_ICON); }
          catch (e) { console.error('mzed: copy image failed', e); }
        });
        bar.append(copyBtn, imgBtn);
        wrap.appendChild(bar);
        // ⌘+wheel (and trackpad pinch) zooms toward the cursor, drag pans,
        // double-click goes back to 1:1. A plain wheel keeps scrolling the page.
        wrap.mdoZoom = mdoZoomPan(wrap, pre, { requireModifier: true, grabCursor: 'zoom-in' });
        // Click the diagram (anywhere but the toolbar) to pop it out into a
        // dedicated window that renders at zoom 1 — where gantt/journey/mindmap
        // also come out correctly (no page-zoom measurement skew). A drag that
        // ended here is a pan, not a click.
        wrap.style.cursor = 'zoom-in';
        wrap.title = 'Click to open in a new window (⌘+scroll to zoom)';
        let popoutTimer = null;
        wrap.addEventListener('click', () => {
          if (wrap.mdoZoom && wrap.mdoZoom.tookDrag()) return;
          // A double-click is a reset gesture, and it fires two clicks first.
          // Hold the open request long enough for the dblclick below to cancel it.
          clearTimeout(popoutTimer);
          popoutTimer = setTimeout(() => {
            dioxus.send({ kind: 'open_mermaid', src: pre.dataset.mdoSrc || '' });
          }, 250);
        });
        wrap.addEventListener('dblclick', () => clearTimeout(popoutTimer));
      }
      // Each render starts from 1:1: the diagram below is about to be replaced.
      if (wrap.mdoZoom) wrap.mdoZoom.reset();
      // Theme the card to match the appearance (arto-style navy in dark).
      wrap.style.background = MDO_DARK ? '#161b22' : '#ffffff';
      wrap.style.border = '1px solid ' + (MDO_DARK ? '#30363d' : '#d8dee4');
      const svg = mdoCacheGet(window.__mdoMermaidCache, MDO_DARK + '\n' + pre.dataset.mdoSrc);
      if (svg) {
        pre.innerHTML = svg;
        pre.setAttribute('data-processed', 'true');
        return;
      }
      // Reset to source so mermaid re-renders with the active theme.
      pre.textContent = pre.dataset.mdoSrc;
      pre.removeAttribute('data-processed');
      toRender.push(pre);
    });
    if (toRender.length) await MDO_MERMAID.run(toRender, MDO_DARK);
    for (const pre of toRender) {
      // A diagram mermaid could not draw is retried next time, not remembered.
      if (!mdoMermaidDrawn(pre)) continue;
      const svg = pre.innerHTML;
      mdoCachePut(window.__mdoMermaidCache, MDO_MERMAID_CACHE_MAX, MDO_DARK + '\n' + pre.dataset.mdoSrc, svg, svg.length);
    }
  }

  if (MDO_KATEX && window.renderMathInElement) {
    renderMathInElement(body, {
      delimiters: [
        { left: "$$",  right: "$$",  display: true  },
        { left: "\\(", right: "\\)", display: false },
        { left: "\\[", right: "\\]", display: true  }
      ]
    });
  }

  // Local markdown images are inlined as data URLs by Rust post-processing.
  // Make them inspectable without adding filesystem access from the WebView.
  body.querySelectorAll('img[src]').forEach((img) => {
    const src = img.getAttribute('src') || '';
    if (!src.startsWith('data:image/')) return;
    if (img.dataset.mdoImageBound) return;
    img.dataset.mdoImageBound = '1';
    img.classList.add('mdo-clickable-image');
    img.addEventListener('click', (event) => {
      event.preventDefault();
      mdoOpenImageLightbox(src, img.getAttribute('alt') || '');
    });
  });

  // Link bridging. Internal links open in-app; external links open in OS browser.
  body.querySelectorAll('a.mdo-link').forEach((a) => {
    if (a.dataset.mdoBound) return;
    a.dataset.mdoBound = '1';
    a.style.cursor = 'pointer';
    a.addEventListener('click', (e) => {
      e.preventDefault();
      dioxus.send({ kind: 'open', path: a.dataset.path });
    });
  });
  body.querySelectorAll('a[href]').forEach((a) => {
    const href = a.getAttribute('href') || '';
    const normalizedHref = href.toLowerCase();
    if (!(normalizedHref.startsWith('http://') || normalizedHref.startsWith('https://') || normalizedHref.startsWith('mailto:'))) return;
    if (a.dataset.mdoBound) return;
    a.dataset.mdoBound = '1';
    a.addEventListener('click', (e) => {
      e.preventDefault();
      dioxus.send({ kind: 'external', url: href });
    });
  });
}

for (const body of document.querySelectorAll('.markdown-body')) {
  await mdoProcessBody(body);
}
dioxus.send({
  kind: 'post_render_complete',
  elapsed_ms: performance.now() - MDO_POST_RENDER_START,
  panes: document.querySelectorAll('.markdown-body').length,
  dark: MDO_DARK,
  katex: MDO_KATEX,
});
"#;

/// Build the post-render JS for the current appearance by injecting the dark
/// flag into [`POST_RENDER_TEMPLATE`].
pub(crate) fn post_render_js(dark: bool, katex: bool) -> String {
    POST_RENDER_TEMPLATE
        .replace("__MDO_MERMAID_HELPER__", &super::mermaid::helper_js())
        .replace("__MDO_ZOOM_PAN__", super::mermaid::zoom_pan_js())
        .replace("__MDO_DARK__", if dark { "true" } else { "false" })
        .replace("__MDO_KATEX__", if katex { "true" } else { "false" })
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    /// ダークテーマ時は html/body の背景を #0d1117 に設定するコードが生成される。
    #[test]
    fn ダーク時にhtml_body背景がセットされる() {
        let js = post_render_js(true, false);
        assert!(
            js.contains("document.documentElement.style.background = MDO_PAGE_BG"),
            "html 背景設定コードが見つからない"
        );
        assert!(
            js.contains("document.body.style.background = MDO_PAGE_BG"),
            "body 背景設定コードが見つからない"
        );
        assert!(
            js.contains("#0d1117"),
            "ダーク背景色 #0d1117 が見つからない"
        );
    }

    /// 外部リンクブリッジが mailto: も external メッセージに流すコードを含む。
    #[test]
    fn 外部リンクブリッジがmailtoを含む() {
        let js = post_render_js(false, false);
        assert!(
            js.contains("startsWith('mailto:')"),
            "mailto: の external ブリッジが生成 JS に見つからない"
        );
    }

    /// ライトテーマ時は html/body の背景を #ffffff に設定するコードが生成される。
    #[test]
    fn ライト時にhtml_body背景がセットされる() {
        let js = post_render_js(false, false);
        assert!(
            js.contains("#ffffff"),
            "ライト背景色 #ffffff が見つからない"
        );
    }

    /// mdo.css の .markdown-body スタイルが読みやすいカラム幅を持つことを
    /// アセットファイルのテキストで確認する。
    #[test]
    fn mdo_cssに本文カラム幅とマージンが含まれる() {
        let css = include_str!("../../assets/mdo.css");
        assert!(
            css.contains("max-width: 900px"),
            "max-width: 900px が mdo.css に見つからない"
        );
        assert!(
            css.contains("margin: 0 auto"),
            "margin: 0 auto が mdo.css に見つからない"
        );
    }

    /// スクロールコンテナ（ペイン本文領域）に `width: 100%` が設定されていることを
    /// app.rs のソーステキストで確認する。
    /// サイドバー非表示時でも `.markdown-body` の `margin: 0 auto` が正しく
    /// 機能するために、親コンテナが確定幅を持つ必要がある。
    #[test]
    fn スクロールコンテナにwidth100が設定されている() {
        let src = include_str!("../../src/app.rs");
        // Count occurrences: expect at least 2 (pane 0 and pane 1 / split pane).
        let count = src
            .matches("flex: 1 1 auto; width: 100%; overflow: auto;")
            .count();
        assert!(
            count >= 2,
            "スクロールコンテナに `flex: 1 1 auto; width: 100%; overflow: auto;` が {count} 箇所しかない（最低2箇所必要）"
        );
    }

    /// mdo.css が overscroll を抑制することを確認する。
    #[test]
    fn mdo_cssにoverscroll抑制が含まれる() {
        let css = include_str!("../../assets/mdo.css");
        assert!(
            css.contains("overscroll-behavior: none"),
            "overscroll-behavior: none が mdo.css に見つからない"
        );
    }

    /// インライン mermaid の mermaid.initialize に useMaxWidth: true が含まれる。
    /// ER 図・ガント図など横長の図がコンテナ幅いっぱいに広がる。
    #[test]
    fn インラインmermaidにuseMaxWidthが設定される() {
        let js = post_render_js(true, false);
        assert!(
            js.contains(r#""useMaxWidth":true"#),
            "useMaxWidth が生成 JS に見つからない"
        );
    }

    /// インラインカードは ⌘+ホイールでのみズームし、ドラッグ後はポップアウトしない。
    #[test]
    fn インライン図にズームパンが結線される() {
        let js = post_render_js(false, false);
        assert!(js.contains("mdoZoomPan(wrap, pre, { requireModifier: true"));
        assert!(js.contains("if (o.requireModifier && !e.metaKey && !e.ctrlKey) return;"));
        assert!(js.contains("wrap.mdoZoom.tookDrag()"));
        assert!(js.contains("wrap.mdoZoom.reset()"));
        assert!(!js.contains("__MDO_ZOOM_PAN__"));
    }

    /// ダブルクリックはポップアウトを起こさない。単クリックは遅延確定し、
    /// 先に来る dblclick が取り消す。
    #[test]
    fn ダブルクリックでポップアウトしない() {
        let js = post_render_js(false, false);
        assert!(js.contains("clearTimeout(popoutTimer)"));
        assert!(js.contains("popoutTimer = setTimeout("));
        assert!(js.contains("wrap.addEventListener('dblclick', () => clearTimeout(popoutTimer))"));
    }

    /// mdo.css の .mdo-mermaid svg は幅を上限だけ縛る。
    /// `width: 100%` は小さい図（mindmap / pie）を横に引き伸ばすので使わない。
    #[test]
    fn mdo_cssのmermaid_svgは最大幅のみ縛る() {
        let css = include_str!("../../assets/mdo.css");
        let rule = css
            .split(".mdo-mermaid svg {")
            .nth(1)
            .and_then(|s| s.split('}').next())
            .expect(".mdo-mermaid svg ルールが mdo.css に見つからない");
        assert!(
            rule.contains("max-width: 100%"),
            "max-width: 100% が .mdo-mermaid svg ルールに見つからない"
        );
        assert!(
            !rule.contains("\n  width:"),
            ".mdo-mermaid svg に固定の width が残っている"
        );
    }

    /// インライン mermaid がダーク時に themeVariables を注入する。
    #[test]
    fn インラインmermaidのdarkテーマにthemeVariablesが含まれる() {
        let js = post_render_js(true, false);
        assert!(
            js.contains(r##""primaryColor":"#1c2128""##),
            "primaryColor が themeVariables に見つからない"
        );
        assert!(
            js.contains(r##""primaryTextColor":"#e6edf3""##),
            "primaryTextColor が themeVariables に見つからない"
        );
        assert!(
            js.contains(r##""lineColor":"#8b949e""##),
            "lineColor が themeVariables に見つからない"
        );
    }

    /// KaTeX delimiter に単一 `$` が含まれないことを確認する。
    /// 通貨表記 `$47K` などが数式化されないための要件。
    #[test]
    fn katexデリミタに単一ドルが含まれない() {
        let js = post_render_js(true, true);
        // Only the `$$` block delimiter should appear; lone `$` must not be a delimiter.
        // Find the delimiters array in the JS and check it doesn't have `"$"` as a standalone entry.
        // The pattern `"$",  right: "$"` would indicate the old single-$ delimiter.
        assert!(
            !js.contains(r#"left: "$",  right: "$""#) && !js.contains(r#"left: "$", right: "$""#),
            "単一 $ delimiter が renderMathInElement に残っている"
        );
    }

    /// KaTeX delimiter に `\\(` / `\\)` インライン記法が含まれる。
    #[test]
    fn katexデリミタにバックスラッシュ丸括弧が含まれる() {
        let js = post_render_js(true, true);
        assert!(
            js.contains(r#"left: "\\(""#),
            r#"\\( delimiter が renderMathInElement に見つからない"#
        );
        assert!(
            js.contains(r#"right: "\\)""#),
            r#"\\) delimiter が renderMathInElement に見つからない"#
        );
    }

    /// KaTeX delimiter に `\\[` / `\\]` ブロック記法が含まれる。
    #[test]
    fn katexデリミタにバックスラッシュ角括弧が含まれる() {
        let js = post_render_js(false, true);
        assert!(
            js.contains(r#"left: "\\[""#),
            r#"\\[ delimiter が renderMathInElement に見つからない"#
        );
        assert!(
            js.contains(r#"right: "\\]""#),
            r#"\\] delimiter が renderMathInElement に見つからない"#
        );
    }

    /// ライブリロードの全置換後、同じソースのコードブロックと図は描き直さない。
    /// 上限付きで、図のキーにはテーマを含める（テーマ切替は再描画される）。
    #[test]
    fn 同じソースのコードブロックと図は再処理しない() {
        let js = post_render_js(false, false);
        assert!(js.contains("const MDO_HL_CACHE_MAX = 500;"));
        assert!(js.contains("const MDO_MERMAID_CACHE_MAX = 50;"));

        let hl = js
            .split_once("mdoCacheGet(window.__mdoHlCache, key)")
            .expect("highlight cache lookup")
            .1;
        let hit = hl.find("el.innerHTML = hit.html;").expect("cache hit");
        let miss = hl.find("hljs.highlightElement(el)").expect("cache miss");
        assert!(hit < miss);

        assert!(js.contains(
            "mdoCacheGet(window.__mdoMermaidCache, MDO_DARK + '\\n' + pre.dataset.mdoSrc)"
        ));
        assert!(js.contains("await MDO_MERMAID.run(toRender, MDO_DARK)"));
    }

    /// 失敗・寸法ゼロ・非表示の図は覚えず、キャッシュは件数に加えて総文字数でも縛る。
    #[test]
    fn キャッシュは成功した図だけを総量上限つきで覚える() {
        let js = post_render_js(false, false);
        assert!(js.contains("if (!mdoMermaidDrawn(pre)) continue;"));
        assert!(js.contains("pre.childNodes.length !== 1"));
        assert!(js.contains("if (!kind || kind === 'error' || document.hidden) return false;"));
        assert!(js.contains("return rect.width > 0 && rect.height > 0;"));
        assert!(js.contains("const MDO_CACHE_MAX_CHARS = 4 * 1024 * 1024;"));
        assert!(js.contains("const MDO_CACHE_ENTRY_MAX_CHARS = 1024 * 1024;"));
        assert!(js.contains("cache.chars > MDO_CACHE_MAX_CHARS"));
    }

    /// テーマ切替時も mermaid.initialize の theme 値が dark フラグに連動する。
    #[test]
    fn mermaidテーマがdarkフラグに連動する() {
        let dark_js = post_render_js(true, false);
        let light_js = post_render_js(false, false);
        assert!(dark_js.contains(r#""theme":"dark""#));
        assert!(light_js.contains(r#""theme":"default""#));
        assert!(dark_js.contains("const MDO_DARK = true;"));
        assert!(light_js.contains("const MDO_DARK = false;"));
    }
}
