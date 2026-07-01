use crate::config;

/// A single persistent keydown listener on the whole webview. It captures the
/// global shortcuts (palette toggle and zoom) and posts a `{kind}` message to
/// Rust. It is installed once on mount via `document::eval`; the Rust side keeps
/// the channel open and dispatches. Palette-internal keys (Esc/arrows/Enter) are
/// handled by the overlay input's Dioxus `onkeydown`, not here.
const KEYDOWN_BRIDGE_TEMPLATE: &str = r#"
// Live keymap: re-evaluating this script (after a rebind) just swaps the array;
// the single listener below reads it on every keypress.
window.__mdoKeymap = __MDO_KEYMAP__;
if (!window.__mdoKeyBound) {
  window.__mdoKeyBound = true;
  window.addEventListener('keydown', (e) => {
    const meta = e.metaKey || e.ctrlKey;
    // Config-driven rebindable shortcuts (matched by physical `e.code`).
    for (const b of (window.__mdoKeymap || [])) {
      if (e.code === b.code && meta === !!b.meta && e.shiftKey === !!b.shift && e.altKey === !!b.alt) {
        e.preventDefault();
        dioxus.send({ kind: b.action });
        return;
      }
    }
    // --- Fixed shortcuts below (not rebindable in v1) ---
    // Cmd+1 / Cmd+2 -> focus the left / right pane (VSCode/Zed style).
    if (meta && !e.shiftKey && e.code && e.code.startsWith('Digit')) {
      const n = parseInt(e.code.slice(5), 10);
      if (n === 1 || n === 2) {
        e.preventDefault();
        dioxus.send({ kind: 'focus_pane', index: n - 1 });
        return;
      }
      if (n >= 3 && n <= 9) { e.preventDefault(); return; }
    }
    // Ctrl+Shift+Tab -> previous tab (check before next so shift wins).
    if (e.ctrlKey && e.shiftKey && e.key === 'Tab') {
      e.preventDefault();
      dioxus.send({ kind: 'prev_tab' });
      return;
    }
    // Ctrl+Tab -> next tab.
    if (e.ctrlKey && !e.shiftKey && e.key === 'Tab') {
      e.preventDefault();
      dioxus.send({ kind: 'next_tab' });
      return;
    }
    // Enter (no modifiers, not in a text field) -> rename the selected file.
    if (e.key === 'Enter' && !meta && !e.ctrlKey && !e.altKey && !e.shiftKey) {
      const t = e.target;
      const tag = t && t.tagName;
      if (tag !== 'INPUT' && tag !== 'TEXTAREA' && !(t && t.isContentEditable)) {
        e.preventDefault();
        dioxus.send({ kind: 'rename_active' });
        return;
      }
    }
    // Esc -> notify Rust to close the topmost open overlay. Notification only
    // (no preventDefault) so overlay inputs' own onkeydown still works.
    if (e.key === 'Escape') {
      dioxus.send({ kind: 'escape' });
    }
    // Zoom (fixed): match both `e.key` and `e.code` for JIS/non-US keyboards.
    if (meta && !e.shiftKey) {
      if (e.code === 'Equal' || e.key === '=' || e.key === '+') {
        e.preventDefault();
        dioxus.send({ kind: 'zoom_in' });
        return;
      }
      if (e.code === 'Minus' || e.key === '-' || e.key === '_') {
        e.preventDefault();
        dioxus.send({ kind: 'zoom_out' });
        return;
      }
      if (e.code === 'Digit0' || e.key === '0') {
        e.preventDefault();
        dioxus.send({ kind: 'zoom_reset' });
        return;
      }
    }
  });
}
"#;

/// Build the keydown bridge JS with the current keymap injected as JSON.
pub(crate) fn keydown_bridge_js(keymap: &[config::KeyBinding]) -> String {
    let json = serde_json::to_string(keymap).unwrap_or_else(|_| "[]".to_string());
    KEYDOWN_BRIDGE_TEMPLATE.replace("__MDO_KEYMAP__", &json)
}

/// Installed once when the user starts dragging the sidebar divider. Attaches
/// document-level mousemove/mouseup listeners that report the cursor X to Rust
/// as `{kind:'sidebar_width', x}` until the button is released. Re-running while
/// a drag is active is a no-op (guarded by `window.__mdoResizing`).
const SIDEBAR_RESIZE_JS: &str = r#"
if (!window.__mdoResizing) {
  window.__mdoResizing = true;
  const onMove = (e) => { dioxus.send({ kind: 'sidebar_width', x: e.clientX }); };
  const onUp = () => {
    window.__mdoResizing = false;
    document.removeEventListener('mousemove', onMove);
    document.removeEventListener('mouseup', onUp);
  };
  document.addEventListener('mousemove', onMove);
  document.addEventListener('mouseup', onUp);
}
"#;

/// Return the sidebar resize bridge.
pub(crate) fn sidebar_resize_js() -> &'static str {
    SIDEBAR_RESIZE_JS
}
