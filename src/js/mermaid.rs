use serde_json::json;

/// The single source of mermaid's `initialize` config. Every render path — the
/// inline view (`js::render`), the popped-out window, HTML export and
/// `mzed serve` — is fed from here so the four cannot drift apart.
pub(crate) fn init_config_json(dark: bool) -> String {
    json!({
        "startOnLoad": false,
        "securityLevel": "strict",
        // why: native SVG <text> labels, not foreignObject/HTML. HTML-label widths
        // are measured with getBoundingClientRect = page-zoomed CSS pixels; our
        // webview applies page zoom, so that desyncs from the SVG's user units and
        // clips node text. SVG text measures in user units (zoom-independent).
        // This is what fixed flowchart/sequence/class/state/er. (cline #7398.)
        "htmlLabels": false,
        // why: useMaxWidth lets mermaid stretch the SVG to its container width, so
        // wide diagrams (ER, gantt, …) are not capped at a fixed-pixel width.
        "flowchart": { "htmlLabels": false, "useMaxWidth": true },
        "er": { "useMaxWidth": true },
        "sequence": { "useMaxWidth": true },
        "gantt": { "useMaxWidth": true },
        "theme": if dark { "dark" } else { "default" },
        "themeVariables": if dark { dark_theme_variables() } else { json!({}) },
        "themeCSS": MINDMAP_LABEL_CSS,
    })
    .to_string()
}

fn mindmap_theme_variables(dark: bool) -> serde_json::Value {
    let (sections, root) = if dark {
        (MINDMAP_SECTIONS_DARK, MINDMAP_ROOT_DARK)
    } else {
        (MINDMAP_SECTIONS_LIGHT, MINDMAP_ROOT_LIGHT)
    };
    let mut vars = serde_json::Map::new();
    for (i, (fill, label)) in sections.iter().enumerate() {
        vars.insert(format!("cScale{i}"), json!(fill));
        vars.insert(format!("cScaleLabel{i}"), json!(label));
    }
    vars.insert("git0".into(), json!(root.0));
    vars.insert("gitBranchLabel0".into(), json!(root.1));
    serde_json::Value::Object(vars)
}

/// The shared `MDO_MERMAID` helper injected into every render path.
///
/// It owns the two things that must not be decided per call site: which config a
/// diagram gets, and the fact that mindmaps render in their own `initialize` +
/// `run` pass. The split is what keeps the mindmap palette off pie / gitGraph,
/// which share the same `cScale*` theme variables.
/// The normalisation mermaid's own `detectType` applies before matching a
/// diagram keyword: YAML frontmatter, `%%{...}%%` directives, `%%` comments.
/// Copied verbatim from the bundled mermaid — `判定用の正規表現がmermaid同梱の
/// ものと一致する` fails if an upgrade changes them — so a mindmap hidden behind
/// any of the three is classified the way mermaid classifies it.
const FRONT_MATTER_RE: &str = r"/^-{3}\s*[\n\r](.*?)[\n\r]-{3}\s*[\n\r]+/s";
const DIRECTIVE_RE: &str =
    r"/%{2}{\s*(?:(\w+)\s*:|(\w+))\s*(?:(\w+)|((?:(?!}%{2}).|\r?\n)*))?\s*(?:}%{2})?/gi";
const COMMENT_RE: &str = r"/\s*%%.*\n/gm";

pub(crate) fn helper_js() -> String {
    HELPER_TEMPLATE
        .replace("__MDO_FRONT_MATTER_RE__", FRONT_MATTER_RE)
        .replace("__MDO_DIRECTIVE_RE__", DIRECTIVE_RE)
        .replace("__MDO_COMMENT_RE__", COMMENT_RE)
        .replace("__MDO_BASE_LIGHT__", &init_config_json(false))
        .replace("__MDO_BASE_DARK__", &init_config_json(true))
        .replace(
            "__MDO_MINDMAP_VARS_LIGHT__",
            &mindmap_theme_variables(false).to_string(),
        )
        .replace(
            "__MDO_MINDMAP_VARS_DARK__",
            &mindmap_theme_variables(true).to_string(),
        )
}

const HELPER_TEMPLATE: &str = r#"
const MDO_MERMAID = {
  base: { light: __MDO_BASE_LIGHT__, dark: __MDO_BASE_DARK__ },
  mindmapVars: { light: __MDO_MINDMAP_VARS_LIGHT__, dark: __MDO_MINDMAP_VARS_DARK__ },

  config(dark, mindmap) {
    const base = dark ? this.base.dark : this.base.light;
    if (!mindmap) return base;
    const vars = dark ? this.mindmapVars.dark : this.mindmapVars.light;
    return { ...base, themeVariables: { ...base.themeVariables, ...vars } };
  },

  frontMatterRe: __MDO_FRONT_MATTER_RE__,
  directiveRe: __MDO_DIRECTIVE_RE__,
  commentRe: __MDO_COMMENT_RE__,

  isMindmap(src) {
    const text = String(src)
      .replace(this.frontMatterRe, '')
      .replace(this.directiveRe, '')
      .replace(this.commentRe, '\n');
    return /^\s*mindmap/.test(text);
  },

  // mzed owns the mindmap palette, so the two places a source can set colours
  // are removed before rendering: `%%{init}%%` directives (mermaid applies them
  // after mermaid.initialize, so they would win) and the frontmatter's
  // `config.themeVariables`. The whole frontmatter block goes rather than that
  // one key: editing YAML in place needs a parser in the WebView, and the
  // mindmap renderer draws nothing from frontmatter. Other diagram types keep
  // both.
  stripSourceConfig(src) {
    return String(src).replace(this.frontMatterRe, '').replace(this.directiveRe, '');
  },

  // One initialize+run pair must finish before the next starts: the config is
  // global to mermaid, so an interleaved call would render a flowchart with the
  // mindmap palette. The chain lives on `window` because this helper is
  // re-evaluated on every render.
  run(pres, dark) {
    const next = (window.__mdoMermaidQueue ?? Promise.resolve())
      .catch(() => {})
      .then(() => this.render(pres, dark));
    window.__mdoMermaidQueue = next;
    return next;
  },

  async render(pres, dark) {
    const mindmaps = [];
    const others = [];
    for (const pre of pres) {
      // mermaid.run skips these itself; the stage below would not.
      if (pre.getAttribute('data-processed')) continue;
      const src = pre.dataset.mdoSrc ?? pre.textContent;
      if (this.isMindmap(src)) {
        pre.textContent = this.stripSourceConfig(src);
        mindmaps.push(pre);
      } else {
        others.push(pre);
      }
    }
    if (!others.length && !mindmaps.length) return;
    const frame = await this.frame().catch((e) => {
      console.error('mzed mermaid frame', e);
      return null;
    });
    if (frame) {
      try {
        await this.draw(frame, others, mindmaps, dark);
        return;
      } catch (e) {
        console.error('mzed mermaid frame', e);
        this.dropFrame();
      }
    }
    await this.draw(window, others, mindmaps, dark);
  },

  // The first few diagrams go into the page as soon as they are drawn, so the
  // top of a long document does not wait for the whole batch.
  firstBatch: 3,

  // Draws in `win` (the frame, or this page as the fallback) and moves each
  // finished SVG into its <pre>. A diagram mermaid rejects keeps its source and
  // stays unprocessed. Throws only when `win`'s mermaid itself is unusable;
  // diagrams already moved stay, and the rest are left for the caller.
  async draw(win, others, mindmaps, dark) {
    const pending = (nodes) => nodes.filter((pre) => !pre.getAttribute('data-processed'));
    const stages = this.stage(win.document, pending([...others, ...mindmaps]));
    const drawn = [];
    const settle = () => {
      for (const [pre, stage] of drawn.splice(0)) {
        if (win !== window) this.localMarkers(stage);
        pre.setAttribute('data-processed', 'true');
        pre.replaceChildren(...stage.childNodes);
      }
    };
    try {
      let count = 0;
      for (const [nodes, mindmap] of [[others, false], [mindmaps, true]]) {
        const todo = pending(nodes);
        if (!todo.length) continue;
        win.mermaid.initialize(win.JSON.parse(JSON.stringify(this.config(dark, mindmap))));
        for (const pre of todo) {
          const stage = stages.get(pre);
          try {
            await win.mermaid.run({ nodes: [stage] });
            drawn.push([pre, stage]);
          } catch (e) { console.error('mzed mermaid', e); }
          if (!mindmap && ++count === this.firstBatch) settle();
        }
      }
    } finally {
      settle();
      for (const stage of stages.values()) stage.remove();
    }
  },

  // why: with arrowMarkerAbsolute, mermaid writes marker references as
  // `url(<its document's URL>#id)`. Drawn in the frame that URL is about:blank,
  // so the arrowheads would vanish once moved here. `#id` is what the absolute
  // URL of this page resolves to anyway.
  localMarkers(root) {
    for (const attr of ['marker-start', 'marker-mid', 'marker-end']) {
      for (const el of root.querySelectorAll(`[${attr}]`)) {
        el.setAttribute(attr, el.getAttribute(attr).replace(/^url\([^#)]*#/, 'url(#'));
      }
    }
  },

  // why: mermaid removes its scratch <div> (and the <style> in it) after every
  // diagram. In this document that re-lays out the whole page: 2.2s per diagram
  // in a 5MB file. A hidden same-origin frame with its own mermaid takes that
  // churn, and only the finished SVG nodes are moved over. A frame that failed
  // is dropped, so the next render builds a new one instead of reusing it.
  frame() {
    const cached = window.__mdoMermaidFrame;
    if (cached && cached.el.isConnected) return cached.ready;
    const el = document.createElement('iframe');
    el.setAttribute('aria-hidden', 'true');
    el.tabIndex = -1;
    el.style.cssText = 'position:absolute; left:-99999px; top:0; width:1px; height:1px; border:0; visibility:hidden;';
    const ready = new Promise((resolve, reject) => {
      const src = document.querySelector('script[src*="mermaid"]')?.src;
      if (!src) { reject(new Error('mermaid script not found')); return; }
      document.body.appendChild(el);
      const script = el.contentDocument.createElement('script');
      script.src = src;
      // A script that loads but throws while running still fires onload.
      script.onload = () => el.contentWindow.mermaid
        ? resolve(el.contentWindow)
        : reject(new Error('mermaid did not start in frame'));
      script.onerror = () => reject(new Error('mermaid failed to load in frame'));
      el.contentDocument.head.appendChild(script);
    }).catch((e) => {
      this.dropFrame();
      throw e;
    });
    window.__mdoMermaidFrame = { el, ready };
    return ready;
  },

  dropFrame() {
    window.__mdoMermaidFrame?.el.remove();
    window.__mdoMermaidFrame = null;
  },

  // One stage per diagram with the <pre>'s content width, layout and text
  // properties, so mermaid measures what it would have measured in place (gantt
  // sizes itself from its parent, and the card's <pre> is a centring flexbox).
  stageProps: ['display', 'flexDirection', 'justifyContent', 'alignItems', 'fontFamily', 'fontSize',
    'fontWeight', 'fontStyle', 'fontStretch', 'fontKerning', 'fontVariantLigatures', 'fontFeatureSettings',
    'lineHeight', 'letterSpacing', 'wordSpacing', 'whiteSpace', 'textRendering', 'textTransform'],

  stage(doc, pres) {
    const looks = pres.map((pre) => {
      const cs = getComputedStyle(pre);
      const look = { width: Math.max(0, pre.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight)) + 'px' };
      for (const prop of this.stageProps) look[prop] = cs[prop];
      return look;
    });
    return new Map(pres.map((pre, i) => {
      const stage = doc.createElement('pre');
      stage.style.cssText = 'position:absolute; left:-99999px; top:0; margin:0; padding:0; border:0;';
      Object.assign(stage.style, looks[i]);
      stage.textContent = pre.textContent;
      doc.body.appendChild(stage);
      return [pre, stage];
    }));
  },
};
"#;

/// Horizontally centre the mindmap node labels mermaid leaves un-centred.
///
/// With `htmlLabels: false`, mermaid 11 centres a label only when the shape asks
/// for it. The generic shapes mindmap reuses (circle, rect, rounded, hexagon) do
/// not, so their label group keeps `translate(0, -h/2)` and the text spills to
/// the right of the node — most visibly on `root((…))`. Selecting on that exact
/// transform hits those labels and nothing else: the shapes that do centre
/// themselves (bang, cloud, plain nodes) write `-w/2` in the same slot.
const MINDMAP_LABEL_CSS: &str =
    r#"g.mindmap-node > g.label[transform^="translate(0, "] text { text-anchor: middle; }"#;

/// Mindmap section colours as `(fill, label)`, one per `cScale`/`cScaleLabel`
/// slot. Primer hues, so the diagram sits on GitHub's markdown surfaces; every
/// pair clears WCAG AA (4.5:1), checked by `z-ai/2026-09-09-p1-mermaid/contrast.py`.
const MINDMAP_SECTIONS_LIGHT: [(&str, &str); 12] = [
    ("#b6e3ff", "#0a3069"),
    ("#aceebb", "#033a16"),
    ("#ecd8ff", "#3c1e70"),
    ("#ffd8b5", "#6b2900"),
    ("#ffcecb", "#6e011a"),
    ("#ffd3eb", "#4d0336"),
    ("#fae17d", "#4d2d00"),
    ("#b7e4e0", "#04403c"),
    ("#d0d7de", "#24292f"),
    ("#c8d1ff", "#1b2b8f"),
    ("#d4f0a3", "#2f4310"),
    ("#ffd0c4", "#6b1c05"),
];
const MINDMAP_SECTIONS_DARK: [(&str, &str); 12] = [
    ("#0d419d", "#cae8ff"),
    ("#033a16", "#aceebb"),
    ("#3c1e70", "#ecd8ff"),
    ("#6b2900", "#ffd8b5"),
    ("#67060c", "#ffdcd7"),
    ("#4d0336", "#ffd3eb"),
    ("#4d2d00", "#fae17d"),
    ("#04403c", "#b7e4e0"),
    ("#30363d", "#c9d1d9"),
    ("#1b2b8f", "#c8d1ff"),
    ("#2f4310", "#d4f0a3"),
    ("#6b1c05", "#ffd0c4"),
];

/// The mindmap root node is styled from `git0` / `gitBranchLabel0`, not `cScale`.
const MINDMAP_ROOT_LIGHT: (&str, &str) = ("#0550ae", "#ffffff");
const MINDMAP_ROOT_DARK: (&str, &str) = ("#0d419d", "#cae8ff");

/// GitHub-dark surfaces, so diagrams sit on the same palette as the document.
fn dark_theme_variables() -> serde_json::Value {
    json!({
        "background": "#161b22",
        "primaryColor": "#1c2128",
        "primaryBorderColor": "#444c56",
        "primaryTextColor": "#e6edf3",
        "lineColor": "#8b949e",
        "secondaryColor": "#22272e",
        "tertiaryColor": "#1c2128",
    })
}

/// The `mdoZoomPan` factory, shared by the inline diagram cards and the
/// popped-out window so both pan and zoom the same way.
pub(crate) fn zoom_pan_js() -> &'static str {
    ZOOM_PAN_JS
}

const ZOOM_PAN_JS: &str = r#"
// Cursor-anchored wheel/pinch zoom + drag pan over a (viewport, stage) pair.
// Options:
//   requireModifier – zoom only on ⌘/ctrl wheel, so a plain wheel still scrolls
//                     the page (inline cards; macOS pinch arrives as a wheel
//                     with ctrlKey, and is meant to zoom)
//   readout         – element that shows the live percentage
//   grabCursor      – idle cursor to restore after a drag
//   dblclick        – 'fit' to fit the viewport, otherwise back to 1:1
function mdoZoomPan(viewport, stage, opts) {
  if (!viewport || !stage) return null;
  const o = opts || {};
  const MIN = 0.1, MAX = 10, STEP = 0.15, DRAG_SLOP = 4;
  let scale = 1, tx = 0, ty = 0, dragged = false;

  const clamp = (s) => Math.min(MAX, Math.max(MIN, s));

  function apply() {
    stage.style.transformOrigin = '0 0';
    stage.style.transform = `translate(${tx}px,${ty}px) scale(${scale})`;
    if (o.readout) o.readout.textContent = Math.round(scale * 100) + '%';
  }

  function reset() { scale = 1; tx = 0; ty = 0; apply(); }

  function fit() {
    const svgEl = stage.querySelector('svg');
    if (!svgEl) { reset(); return; }
    const vw = viewport.clientWidth  || window.innerWidth;
    const vh = viewport.clientHeight || window.innerHeight;
    const sw = svgEl.getBoundingClientRect().width  / scale;
    const sh = svgEl.getBoundingClientRect().height / scale;
    if (sw <= 0 || sh <= 0) { reset(); return; }
    scale = clamp(Math.min((vw - 48) / sw, (vh - 96) / sh));
    tx = (vw - sw * scale) / 2;
    ty = (vh - sh * scale) / 2;
    apply();
  }

  // tx/ty live in the stage's own coordinate space, so a cursor position has to
  // be measured from the stage's *untransformed* top-left — not the viewport's,
  // which sits a padding + border away and would skew the anchor. With
  // transform-origin 0 0 the stage's rect left is that origin plus tx.
  function stagePoint(e) {
    const r = stage.getBoundingClientRect();
    return [e.clientX - (r.left - tx), e.clientY - (r.top - ty)];
  }

  function zoomAt(factor, cx, cy) {
    const prev = scale;
    scale = clamp(scale * factor);
    tx = cx - (cx - tx) * (scale / prev);
    ty = cy - (cy - ty) * (scale / prev);
    apply();
  }

  viewport.addEventListener('wheel', (e) => {
    if (o.requireModifier && !e.metaKey && !e.ctrlKey) return;
    e.preventDefault();
    const delta = e.deltaY !== 0 ? -e.deltaY : e.deltaX;
    zoomAt(1 + Math.max(-0.9, Math.min(2, delta * 0.005)), ...stagePoint(e));
  }, { passive: false });

  let startX = 0, startY = 0, baseTx = 0, baseTy = 0;
  const onMove = (e) => {
    const dx = e.clientX - startX, dy = e.clientY - startY;
    if (!dragged && Math.hypot(dx, dy) < DRAG_SLOP) return;
    dragged = true;
    tx = baseTx + dx;
    ty = baseTy + dy;
    apply();
  };
  const onUp = () => {
    window.removeEventListener('mousemove', onMove);
    window.removeEventListener('mouseup', onUp);
    if (o.grabCursor) viewport.style.cursor = o.grabCursor;
  };
  viewport.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    dragged = false;
    startX = e.clientX; startY = e.clientY;
    baseTx = tx; baseTy = ty;
    if (o.grabCursor) viewport.style.cursor = 'grabbing';
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    e.preventDefault();
  });

  viewport.addEventListener('dblclick', (e) => {
    e.preventDefault();
    if (o.dblclick === 'fit') fit(); else reset();
  });

  return {
    zoomIn:  () => { scale = clamp(scale + STEP); apply(); },
    zoomOut: () => { scale = clamp(scale - STEP); apply(); },
    reset,
    fit,
    // True once per drag, so a click handler can tell a pan from a click.
    tookDrag() { const d = dragged; dragged = false; return d; },
  };
}
"#;

/// JS for the popped-out mermaid window. It renders at the window's native
/// scale (this window has no page zoom), so mermaid's HTML-label measurement
/// stays correct and every diagram type — gantt, journey, mindmap included —
/// renders the way mo/arto show them. `__MDO_MERMAID_HELPER__` / `__MDO_DARK__`
/// are substituted by [`mermaid_window_js`].
///
/// After rendering, a zoom/pan layer is set up:
///   • Scroll wheel / trackpad pinch → zoom toward the cursor (no modifier)
///   • Mouse/touch drag → pan; double-click → fit
///   • Toolbar buttons: zoom in/out/reset/fit, with a live % readout
const MERMAID_WINDOW_JS: &str = r#"
__MDO_MERMAID_HELPER__
__MDO_ZOOM_PAN__
await new Promise(r => requestAnimationFrame(r));
if (window.mermaid) {
  await MDO_MERMAID.run([...document.querySelectorAll('pre.mermaid')], __MDO_DARK__);
}

// ── Zoom / Pan ──────────────────────────────────────────────────────────────
// This window is a canvas, not a scrolling page, so a plain wheel zooms.
const mdoZoom = mdoZoomPan(
  document.getElementById('mdo-vp'),
  document.getElementById('mdo-stage'),
  { readout: document.getElementById('mdo-zoom-pct'), grabCursor: 'grab', dblclick: 'fit' },
);
if (mdoZoom) {
  document.getElementById('mdo-btn-in')   ?.addEventListener('click', mdoZoom.zoomIn);
  document.getElementById('mdo-btn-out')  ?.addEventListener('click', mdoZoom.zoomOut);
  document.getElementById('mdo-btn-reset')?.addEventListener('click', mdoZoom.reset);
  document.getElementById('mdo-btn-fit')  ?.addEventListener('click', mdoZoom.fit);
  requestAnimationFrame(mdoZoom.fit);
}
"#;

/// Build the popped-out Mermaid window JS for the current appearance.
pub(crate) fn mermaid_window_js(dark: bool) -> String {
    MERMAID_WINDOW_JS
        .replace("__MDO_MERMAID_HELPER__", &helper_js())
        .replace("__MDO_ZOOM_PAN__", ZOOM_PAN_JS)
        .replace("__MDO_DARK__", if dark { "true" } else { "false" })
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn ウィンドウJSのdarkフラグが置換される() {
        let js = mermaid_window_js(true);
        assert!(js.contains(r#""theme":"dark""#));
        assert!(js.contains(r#""securityLevel":"strict""#));
        assert!(!js.contains("__MDO_MERMAID_HELPER__"));
        assert!(!js.contains("loose"));
    }

    #[test]
    fn ウィンドウJSのlightフラグが置換される() {
        let js = mermaid_window_js(false);
        assert!(js.contains(r#""theme":"default""#));
        assert!(!js.contains("__MDO_MERMAID_HELPER__"));
    }

    #[test]
    fn ウィンドウJSにdark時のthemeVariablesが含まれる() {
        let js = mermaid_window_js(true);
        assert!(js.contains(r##""primaryColor":"#1c2128""##));
        assert!(js.contains(r##""primaryTextColor":"#e6edf3""##));
        assert!(js.contains(r##""lineColor":"#8b949e""##));
    }

    #[test]
    fn ウィンドウJSにuseMaxWidthが含まれる() {
        let js = mermaid_window_js(true);
        assert!(js.contains(r#""useMaxWidth":true"#));
    }

    /// mindmap のラベル中央寄せ CSS が light / dark 双方に載る。
    #[test]
    fn 設定JSONにmindmapラベル補正CSSが含まれる() {
        for dark in [true, false] {
            let json = init_config_json(dark);
            assert!(json.contains("mindmap-node"), "dark={dark}");
            assert!(json.contains("text-anchor: middle"), "dark={dark}");
        }
    }

    #[test]
    fn 設定JSONはlight時にthemeVariablesを空にする() {
        let json = init_config_json(false);
        assert!(json.contains(r#""themeVariables":{}"#));
        assert!(!json.contains("#1c2128"));
    }

    /// mindmap 配色は 12 セクション + root 分が light / dark 両方に揃う。
    #[test]
    fn mindmap配色が12セクションとrootを定義する() {
        for dark in [true, false] {
            let vars = mindmap_theme_variables(dark);
            let obj = vars.as_object().expect("themeVariables はオブジェクト");
            for i in 0..12 {
                assert!(obj.contains_key(&format!("cScale{i}")), "dark={dark} i={i}");
                assert!(
                    obj.contains_key(&format!("cScaleLabel{i}")),
                    "dark={dark} i={i}"
                );
            }
            assert!(obj.contains_key("git0"), "dark={dark}");
            assert!(obj.contains_key("gitBranchLabel0"), "dark={dark}");
        }
    }

    /// 配色の文字色/背景色コントラストは WCAG AA (4.5:1) を満たす。
    #[test]
    fn mindmap配色のコントラストが4_5以上() {
        let all = MINDMAP_SECTIONS_LIGHT
            .iter()
            .chain(MINDMAP_SECTIONS_DARK.iter())
            .chain([&MINDMAP_ROOT_LIGHT, &MINDMAP_ROOT_DARK]);
        for (fill, label) in all {
            let ratio = contrast_ratio(fill, label);
            assert!(ratio >= 4.5, "{label} on {fill} = {ratio:.2}:1");
        }
    }

    fn contrast_ratio(a: &str, b: &str) -> f64 {
        let lum = |hex: &str| {
            let channel = |i: usize| {
                let v = u8::from_str_radix(&hex[i..i + 2], 16).unwrap() as f64 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5)
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// 図種判定の正規表現が同梱 mermaid のものと一致する。
    /// mermaid を上げて `detectType` の前処理が変わったらここで落ちる。
    #[test]
    fn 判定用の正規表現がmermaid同梱のものと一致する() {
        let bundle = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/mermaid.min.js"
        ))
        .expect("assets/mermaid.min.js が読めない");
        for re in [FRONT_MATTER_RE, DIRECTIVE_RE, COMMENT_RE] {
            assert!(bundle.contains(re), "mermaid 同梱の正規表現に無い: {re}");
        }
    }

    /// 3 つの正規表現がすべてヘルパ JS に埋め込まれる。
    #[test]
    fn ヘルパーJSに判定用の正規表現が埋め込まれる() {
        let js = helper_js();
        for re in [FRONT_MATTER_RE, DIRECTIVE_RE, COMMENT_RE] {
            assert!(js.contains(re), "{re}");
        }
        assert!(!js.contains("__MDO_FRONT_MATTER_RE__"));
        assert!(!js.contains("__MDO_DIRECTIVE_RE__"));
        assert!(!js.contains("__MDO_COMMENT_RE__"));
    }

    /// 描画は共有キューで直列化する（mermaid の設定はグローバルなため）。
    #[test]
    fn ヘルパーJSが描画を直列化する() {
        let js = helper_js();
        assert!(js.contains("window.__mdoMermaidQueue"));
        assert!(js.contains("window.__mdoMermaidQueue = next"));
    }

    /// 図は隠した同一オリジンの iframe 内の mermaid で描き、完成した SVG ノードだけを
    /// 元の `pre` に移す。本文側に innerHTML の入口を足さない。
    #[test]
    fn ヘルパーJSは別文書で描いて結果のノードだけ移す() {
        let js = helper_js();
        assert!(js.contains("document.createElement('iframe')"));
        assert!(js.contains("script[src*=\"mermaid\"]"));
        assert!(js.contains("await win.mermaid.run({ nodes: [stage] })"));
        assert!(js.contains("pre.replaceChildren(...stage.childNodes)"));
        assert!(!js.contains("innerHTML"));
        // iframe が使えないときは本文の文書で描く。
        assert!(js.contains("await this.draw(window, others, mindmaps, dark)"));
    }

    /// 読み込み失敗・実行失敗の iframe は捨て、次の描画で作り直す。
    /// 描画途中で iframe 側の mermaid が壊れたら本文で描き直す。
    #[test]
    fn ヘルパーJSは失敗したiframeを捨てて本文で描く() {
        let js = helper_js();
        assert!(js.contains("el.contentWindow.mermaid"));
        assert!(js.contains("mermaid did not start in frame"));
        let frame = js.split_once("  frame() {").unwrap().1;
        assert!(frame.contains("this.dropFrame();"));
        let render = js.split_once("async render(pres, dark)").unwrap().1;
        let fallback = render.find("this.dropFrame();").unwrap();
        let in_page = render.find("await this.draw(window,").unwrap();
        assert!(fallback < in_page);
        // stage は成否にかかわらず片付ける。
        let draw = js
            .split_once("async draw(win, others, mindmaps, dark)")
            .unwrap()
            .1;
        let fin = draw.find("} finally {").unwrap();
        assert!(fin < draw.find("stage.remove()").unwrap());
    }

    /// mermaid が描けなかった図は SVG を移さず、ソースのまま未処理で残す。
    #[test]
    fn ヘルパーJSは成功した図だけを本文へ移す() {
        let js = helper_js();
        let draw = js
            .split_once("async draw(win, others, mindmaps, dark)")
            .unwrap()
            .1;
        let run = draw
            .find("await win.mermaid.run({ nodes: [stage] });")
            .unwrap();
        let keep = draw.find("drawn.push([pre, stage]);").unwrap();
        let catch = draw
            .find("catch (e) { console.error('mzed mermaid', e); }")
            .unwrap();
        assert!(run < keep && keep < catch);
        assert!(js.contains("pre.setAttribute('data-processed', 'true');"));
    }

    /// 通常図の最初の 3 枚は、残りを待たずに本文へ反映する。
    #[test]
    fn ヘルパーJSは最初の3枚を先に反映する() {
        let js = helper_js();
        assert!(js.contains("firstBatch: 3,"));
        assert!(js.contains("if (!mindmap && ++count === this.firstBatch) settle();"));
    }

    /// iframe で描いた図のマーカー参照（arrowMarkerAbsolute で about:blank 基準の絶対 URL）を
    /// 本文内の `#id` 参照に直す。
    #[test]
    fn ヘルパーJSはiframeで描いた矢印マーカーをローカル参照に直す() {
        let js = helper_js();
        assert!(js.contains("if (win !== window) this.localMarkers(stage);"));
        assert!(js.contains(r"replace(/^url\([^#)]*#/, 'url(#')"));
        for attr in ["marker-start", "marker-mid", "marker-end"] {
            assert!(js.contains(&format!("'{attr}'")), "{attr}");
        }
    }

    /// 描画済みの図は mermaid.run と同じく飛ばす（stage 経由でも二重描画しない）。
    #[test]
    fn ヘルパーJSは描画済みの図を飛ばす() {
        let js = helper_js();
        let render = js.split_once("async render(pres, dark)").unwrap().1;
        let skip = render
            .find("if (pre.getAttribute('data-processed')) continue;")
            .unwrap();
        let classify = render.find("this.isMindmap(src)").unwrap();
        assert!(skip < classify);
    }

    /// mindmap は専用パスで描画され、ソースの色指定は取り除かれる。
    #[test]
    fn ヘルパーJSがmindmapを別パスで描画する() {
        let js = helper_js();
        assert!(js.contains("isMindmap"));
        assert!(js.contains("stripSourceConfig"));
        assert!(js.contains(r##""cScale1":"#aceebb""##));
        assert!(js.contains(r##""cScale1":"#033a16""##));
        assert!(!js.contains("__MDO_BASE_LIGHT__"));
        assert!(!js.contains("__MDO_MINDMAP_VARS_DARK__"));
    }

    #[test]
    fn ウィンドウJSにズームパン機能が含まれる() {
        let js = mermaid_window_js(true);
        assert!(js.contains("mdoZoomPan"));
        assert!(js.contains("mdo-btn-in"));
        assert!(js.contains("mdo-btn-out"));
        assert!(js.contains("mdo-btn-reset"));
        assert!(js.contains("mdo-btn-fit"));
        assert!(js.contains("wheel"));
        assert!(js.contains("mousedown"));
        assert!(js.contains("requestAnimationFrame(mdoZoom.fit)"));
    }

    /// ポップアウトは修飾キーなしでズームし、率を表示し、dblclick で fit する。
    #[test]
    fn ウィンドウJSは修飾キーなしホイールでズームし率を表示する() {
        let js = mermaid_window_js(true);
        assert!(!js.contains("requireModifier: true"));
        assert!(js.contains("mdo-zoom-pct"));
        assert!(js.contains("dblclick: 'fit'"));
    }

    /// カーソル基準ズームは変形対象（stage）の原点から座標を測る。
    /// viewport 原点だとカードの padding + border 分ずれる。
    #[test]
    fn ズーム座標をstage原点から測る() {
        let js = zoom_pan_js();
        assert!(js.contains("function stagePoint(e)"));
        assert!(js.contains("stage.getBoundingClientRect()"));
        assert!(js.contains("...stagePoint(e)"));
        assert!(!js.contains("e.clientX - rect.left"));
    }

    #[test]
    fn ウィンドウJSにsecurityLevel_strictが維持される() {
        let js_dark = mermaid_window_js(true);
        let js_light = mermaid_window_js(false);
        assert!(js_dark.contains(r#""securityLevel":"strict""#));
        assert!(js_light.contains(r#""securityLevel":"strict""#));
        assert!(!js_dark.contains("loose"));
        assert!(!js_light.contains("loose"));
    }
}
