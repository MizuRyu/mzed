/// JS for the popped-out mermaid window. It renders at the window's native
/// scale (this window has no page zoom), so mermaid's HTML-label measurement
/// stays correct and every diagram type — gantt, journey, mindmap included —
/// renders the way mo/arto show them. `__MDO_DARK__` is substituted by the
/// `open_mermaid_window` component.
///
/// After rendering, a zoom/pan layer is set up:
///   • Scroll wheel / trackpad pinch → zoom toward the cursor (no modifier)
///   • Mouse/touch drag → pan; double-click → fit
///   • Toolbar buttons: zoom in/out/reset/fit, with a live % readout
const MERMAID_WINDOW_JS: &str = r#"
await new Promise(r => requestAnimationFrame(r));
if (window.mermaid) {
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: 'strict',
    htmlLabels: false,
    flowchart: { htmlLabels: false, useMaxWidth: true },
    er: { useMaxWidth: true },
    sequence: { useMaxWidth: true },
    gantt: { useMaxWidth: true },
    theme: __MDO_DARK__ ? 'dark' : 'default',
    themeVariables: __MDO_DARK__
      ? {
          background: '#161b22',
          primaryColor: '#1c2128',
          primaryBorderColor: '#444c56',
          primaryTextColor: '#e6edf3',
          lineColor: '#8b949e',
          secondaryColor: '#22272e',
          tertiaryColor: '#1c2128',
        }
      : {},
  });
  try { await mermaid.run(); } catch (e) { console.error('mzed mermaid window', e); }
}

// ── Zoom / Pan ──────────────────────────────────────────────────────────────
(function mdoZoomPan() {
  const viewport = document.getElementById('mdo-vp');
  const stage    = document.getElementById('mdo-stage');
  if (!viewport || !stage) return;

  let scale  = 1;
  let tx     = 0;
  let ty     = 0;
  const MIN  = 0.1;
  const MAX  = 10;
  const STEP = 0.15;

  const pct = document.getElementById('mdo-zoom-pct');

  function apply() {
    stage.style.transform = `translate(${tx}px,${ty}px) scale(${scale})`;
    if (pct) pct.textContent = Math.round(scale * 100) + '%';
  }

  function clampScale(s) { return Math.min(MAX, Math.max(MIN, s)); }

  function fit() {
    const svgEl = stage.querySelector('svg');
    if (!svgEl) { scale = 1; tx = 0; ty = 0; apply(); return; }
    const vw = viewport.clientWidth  || window.innerWidth;
    const vh = viewport.clientHeight || window.innerHeight;
    const sw = svgEl.getBoundingClientRect().width  / scale;
    const sh = svgEl.getBoundingClientRect().height / scale;
    if (sw <= 0 || sh <= 0) { scale = 1; tx = 0; ty = 0; apply(); return; }
    scale = clampScale(Math.min((vw - 48) / sw, (vh - 96) / sh));
    tx = (vw - sw * scale) / 2;
    ty = (vh - sh * scale) / 2;
    apply();
  }

  // Toolbar buttons
  document.getElementById('mdo-btn-in')   ?.addEventListener('click', () => { scale = clampScale(scale + STEP); apply(); });
  document.getElementById('mdo-btn-out')  ?.addEventListener('click', () => { scale = clampScale(scale - STEP); apply(); });
  document.getElementById('mdo-btn-reset')?.addEventListener('click', () => { scale = 1; tx = 0; ty = 0; apply(); });
  document.getElementById('mdo-btn-fit')  ?.addEventListener('click', fit);

  // Any wheel / pinch zooms (no modifier needed): this window is a canvas,
  // not a scrolling page, so plain scroll has nothing better to do. Pinch
  // arrives as a synthetic wheel with ctrlKey via WKWebView.
  viewport.addEventListener('wheel', (e) => {
    e.preventDefault();
    const delta = e.deltaY !== 0 ? -e.deltaY : e.deltaX;
    const factor = 1 + Math.max(-0.9, Math.min(2, delta * 0.005));
    // Zoom toward cursor
    const rect = viewport.getBoundingClientRect();
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;
    const prevScale = scale;
    scale = clampScale(scale * factor);
    tx = cx - (cx - tx) * (scale / prevScale);
    ty = cy - (cy - ty) * (scale / prevScale);
    apply();
  }, { passive: false });

  // Drag to pan
  let dragging = false;
  let dragStartX = 0;
  let dragStartY = 0;
  let dragTx = 0;
  let dragTy = 0;

  viewport.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    dragging = true;
    dragStartX = e.clientX;
    dragStartY = e.clientY;
    dragTx = tx;
    dragTy = ty;
    viewport.style.cursor = 'grabbing';
    e.preventDefault();
  });
  window.addEventListener('mousemove', (e) => {
    if (!dragging) return;
    tx = dragTx + (e.clientX - dragStartX);
    ty = dragTy + (e.clientY - dragStartY);
    apply();
  });
  window.addEventListener('mouseup', () => {
    if (!dragging) return;
    dragging = false;
    viewport.style.cursor = 'grab';
  });

  // Double-click → fit (mirrors the toolbar button without aiming for it)
  viewport.addEventListener('dblclick', (e) => { e.preventDefault(); fit(); });

  // Initial fit once DOM is painted
  requestAnimationFrame(fit);
})();
"#;

/// Build the popped-out Mermaid window JS for the current appearance.
pub(crate) fn mermaid_window_js(dark: bool) -> String {
    MERMAID_WINDOW_JS.replace("__MDO_DARK__", if dark { "true" } else { "false" })
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn ウィンドウJSのdarkフラグが置換される() {
        let js = mermaid_window_js(true);
        assert!(js.contains("theme: true ? 'dark' : 'default'"));
        assert!(js.contains("securityLevel: 'strict'"));
        assert!(!js.contains("__MDO_DARK__"));
        assert!(!js.contains("securityLevel: 'loose'"));
    }

    #[test]
    fn ウィンドウJSのlightフラグが置換される() {
        let js = mermaid_window_js(false);
        assert!(js.contains("theme: false ? 'dark' : 'default'"));
        assert!(!js.contains("__MDO_DARK__"));
    }

    #[test]
    fn ウィンドウJSにdark時のthemeVariablesが含まれる() {
        let js = mermaid_window_js(true);
        // themeVariables block must be present for dark mode
        assert!(js.contains("primaryColor: '#1c2128'"));
        assert!(js.contains("primaryTextColor: '#e6edf3'"));
        assert!(js.contains("lineColor: '#8b949e'"));
    }

    #[test]
    fn ウィンドウJSにuseMaxWidthが含まれる() {
        let js = mermaid_window_js(true);
        assert!(js.contains("useMaxWidth: true"));
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
        assert!(js.contains("requestAnimationFrame(fit)"));
    }

    #[test]
    fn ウィンドウJSは修飾キーなしホイールでズームし率を表示する() {
        let js = mermaid_window_js(true);
        // No modifier gate on the wheel handler.
        assert!(!js.contains("if (!e.metaKey && !e.ctrlKey) return;"));
        // Live % readout and dblclick-to-fit are wired.
        assert!(js.contains("mdo-zoom-pct"));
        assert!(js.contains("dblclick"));
    }

    #[test]
    fn ウィンドウJSにsecurityLevel_strictが維持される() {
        let js_dark = mermaid_window_js(true);
        let js_light = mermaid_window_js(false);
        assert!(js_dark.contains("securityLevel: 'strict'"));
        assert!(js_light.contains("securityLevel: 'strict'"));
        assert!(!js_dark.contains("securityLevel: 'loose'"));
        assert!(!js_light.contains("securityLevel: 'loose'"));
    }
}
