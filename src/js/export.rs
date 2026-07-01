/// JS that returns the rendered `.markdown-body` HTML (Mermaid as inline SVG,
/// KaTeX laid out, code highlighted) with the interactive chrome (copy buttons,
/// mermaid toolbars, per-theme inline styles) stripped, for a clean export.
const EXPORT_CAPTURE_JS_TEMPLATE: &str = r#"
(async () => {
  const src = document.querySelector('.markdown-body[data-mdo-pane="__MDO_PANE__"]') || document.querySelector('.markdown-body');
  if (!src) { dioxus.send(''); return; }
  const clone = src.cloneNode(true);
  clone.querySelectorAll('.mdo-copy-btn, .mdo-mermaid-bar').forEach((el) => el.remove());
  clone.querySelectorAll('.mdo-mermaid').forEach((el) => { el.removeAttribute('style'); el.removeAttribute('title'); el.style.cursor = 'default'; });

  // Re-render Mermaid in the LIGHT theme for the white export page (the live
  // view may be dark, which is unreadable on white). We render off-DOM-ish:
  // the clone must be attached for mermaid to measure, so use a hidden host.
  const pres = [...clone.querySelectorAll('pre.mermaid')];
  if (window.mermaid && pres.length) {
    const host = document.createElement('div');
    host.style.cssText = 'position:fixed; left:-99999px; top:0; width:1200px;';
    host.appendChild(clone);
    document.body.appendChild(host);
    pres.forEach((pre) => {
      if (pre.dataset.mdoSrc) pre.textContent = pre.dataset.mdoSrc;
      pre.removeAttribute('data-processed');
    });
    try {
      // htmlLabels:false keeps SVG text labels (zoom-independent) so they don't
      // clip under the app's page zoom; theme 'default' gives the light palette.
      mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: 'default', htmlLabels: false, flowchart: { htmlLabels: false } });
      await mermaid.run({ nodes: pres });
    } catch (e) { console.error('mzed export mermaid', e); }
    document.body.removeChild(host); // detach but keep `clone` reference
  }

  // Restore the live view's Mermaid theme (we changed the global config above).
  if (window.mermaid) {
    mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', htmlLabels: false, flowchart: { htmlLabels: false } });
  }

  dioxus.send(clone.innerHTML);
})();
"#;

pub(crate) fn export_capture_js(pane: u8) -> String {
    let pane = if pane == 1 { "1" } else { "0" };
    EXPORT_CAPTURE_JS_TEMPLATE.replace("__MDO_PANE__", pane)
}

pub(crate) fn webview_action_error(value: &serde_json::Value, context: &str) -> Option<String> {
    if value.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    let detail = value
        .get("error")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("unknown WebView error");
    Some(format!("{context}: {detail}"))
}
