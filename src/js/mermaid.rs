/// JS for the popped-out mermaid window. It renders at the window's native
/// scale (this window has no page zoom), so mermaid's HTML-label measurement
/// stays correct and every diagram type — gantt, journey, mindmap included —
/// renders the way mo/arto show them. `__MDO_DARK__` is substituted by the
/// `open_mermaid_window` component.
const MERMAID_WINDOW_JS: &str = r#"
await new Promise(r => requestAnimationFrame(r));
if (window.mermaid) {
  mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: __MDO_DARK__ ? 'dark' : 'default' });
  try { await mermaid.run(); } catch (e) { console.error('mzed mermaid window', e); }
}
"#;

/// Build the popped-out Mermaid window JS for the current appearance.
pub(crate) fn mermaid_window_js(dark: bool) -> String {
    MERMAID_WINDOW_JS.replace("__MDO_DARK__", if dark { "true" } else { "false" })
}
