mod dom;
mod export;
mod find;
mod keyboard;
mod mermaid;
mod notes;
mod render;

pub(crate) use dom::{
    overlay_row_scroll_js, reset_root_scroll_js, sidebar_active_js, OverlayRowKind,
};
pub(crate) use export::{export_capture_js, webview_action_error};
pub(crate) use find::{find_highlight_js, find_step_js};
pub(crate) use keyboard::{keydown_bridge_js, sidebar_resize_js};
pub(crate) use mermaid::{helper_js as mermaid_helper_js, mermaid_window_js};
pub(crate) use notes::{note_bridge_js, note_overlay_js, note_selection_js, NoteOverlay};
pub(crate) use render::post_render_js;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::KeyBinding;

    #[test]
    fn post_render_replaces_dark_and_katex_flags() {
        let js = post_render_js(true, false);

        assert!(js.contains("const MDO_DARK = true;"));
        assert!(js.contains("const MDO_KATEX = false;"));
        assert!(js.contains(r#""securityLevel":"strict""#));
        assert!(js.contains("href.toLowerCase()"));
        assert!(js.contains("function mdoOpenImageLightbox"));
        assert!(js.contains("querySelectorAll('img[src]')"));
        assert!(js.contains("data:image/"));
        assert!(js.contains("post_render_complete"));
        assert!(js.contains("performance.now() - MDO_POST_RENDER_START"));
        assert!(!js.contains("__MDO_DARK__"));
        assert!(!js.contains("__MDO_KATEX__"));
        assert!(!js.contains("__MDO_MERMAID_CONFIG__"));
        assert!(!js.contains("loose"));
    }

    #[test]
    fn export_capture_js_targets_requested_pane() {
        let js = export_capture_js(1, false);

        assert!(js.contains(".markdown-body[data-mdo-pane=\"1\"]"));
        assert!(!js.contains(".markdown-body[data-mdo-pane=\"0\"]"));
        assert!(!js.contains("__MDO_PANE__"));
    }

    #[test]
    fn export_capture_js_uses_first_pane_for_unknown_index() {
        let js = export_capture_js(2, false);

        assert!(js.contains(".markdown-body[data-mdo-pane=\"0\"]"));
        assert!(!js.contains("__MDO_PANE__"));
    }

    /// エクスポートは常に light で描画し、終了後にライブ表示のテーマへ戻す。
    #[test]
    fn export_capture_js_renders_light_and_restores_live_theme() {
        let js = export_capture_js(0, true);

        assert!(js.contains("MDO_MERMAID.run(pres, false)"));
        assert!(js.contains("MDO_MERMAID.config(true, false)"));
        assert!(!js.contains("__MDO_MERMAID_HELPER__"));
        assert!(!js.contains("__MDO_LIVE_DARK__"));
    }

    /// エクスポートはインライン図のズーム / パン transform を落としてから複製する。
    #[test]
    fn export_capture_js_clears_inline_zoom_transform() {
        let js = export_capture_js(0, false);

        assert!(js.contains(
            "clone.querySelectorAll('.mdo-mermaid pre.mermaid').forEach((el) => el.removeAttribute('style'))"
        ));
    }

    #[test]
    fn webview_action_error_reports_payload_status() {
        let ok = serde_json::json!({ "ok": true, "error": null });
        let failure = serde_json::json!({ "ok": false, "error": "denied" });

        assert_eq!(webview_action_error(&ok, "Copy failed"), None);
        assert_eq!(
            webview_action_error(&failure, "Copy failed"),
            Some("Copy failed: denied".to_string())
        );
    }

    #[test]
    fn keydown_bridge_injects_keymap_as_json() {
        let keymap = [KeyBinding {
            action: "quote\"and\nnewline".into(),
            code: "KeyQ".into(),
            meta: true,
            shift: false,
            alt: true,
        }];

        let js = keydown_bridge_js(&keymap);
        let json = serde_json::to_string(&keymap).unwrap();

        assert!(js.contains(&format!("window.__mdoKeymap = {json};")));
        assert!(!js.contains("__MDO_KEYMAP__"));
    }

    #[test]
    fn find_highlight_json_encodes_unsafe_query_characters() {
        let query = "\"quoted\"\n</script><div>";

        let js = find_highlight_js(query);
        let json = serde_json::to_string(query).unwrap();

        assert!(js.contains(&format!("const q = {json};")));
        assert!(!js.contains("const q = \"quoted\""));
        assert!(!js.contains("__MDO_QUERY__"));
    }

    #[test]
    fn mermaid_window_replaces_dark_flag() {
        let js = mermaid_window_js(true);

        assert!(js.contains(r#""theme":"dark""#));
        assert!(js.contains(r#""securityLevel":"strict""#));
        assert!(!js.contains("__MDO_MERMAID_CONFIG__"));
        assert!(!js.contains("loose"));
    }

    #[test]
    fn sidebar_active_json_encodes_unsafe_path() {
        let path = "/tmp/\"quoted\"\n</script>.md";

        let js = sidebar_active_js(Some(path));
        let json = serde_json::to_string(path).unwrap();

        assert!(js.contains(&format!("const activePath = {json};")));
        assert!(!js.contains("const activePath = /tmp/"));
    }

    #[test]
    fn sidebar_active_js_removes_stale_active_rows_before_setting_current() {
        let js = sidebar_active_js(Some("/tmp/a.md"));

        assert!(js.contains("querySelectorAll('.mdo-tree-row-active')"));
        assert!(js.contains("classList.remove('mdo-tree-row-active')"));
        assert!(js.contains("classList.add('mdo-tree-row-active')"));
        assert!(js.contains("\"/tmp/a.md\""));
    }

    #[test]
    fn sidebar_active_js_uses_null_when_no_active_file() {
        let js = sidebar_active_js(None);

        assert!(js.contains("const activePath = null;"));
    }

    #[test]
    fn overlay_row_scroll_js_scrolls_only_the_overlay_list() {
        let js = overlay_row_scroll_js(OverlayRowKind::Command, 3);

        assert!(js.contains(r#"querySelector('[data-mdo-row="3"]')"#));
        assert!(js.contains(r#"closest('[data-mdo-scroll]')"#));
        assert!(js.contains("scroller.scrollTop"));
        assert!(js.contains("window.scrollTo(0, 0)"));
        assert!(!js.contains("scrollIntoView"));
    }

    #[test]
    fn overlay_row_scroll_js_supports_each_known_overlay() {
        let project_js = overlay_row_scroll_js(OverlayRowKind::Project, 4);
        let settings_js = overlay_row_scroll_js(OverlayRowKind::Settings, 5);

        assert!(project_js.contains(r#"querySelector('[data-mdo-prow="4"]')"#));
        assert!(settings_js.contains(r#"querySelector('[data-mdo-srow="5"]')"#));
    }

    #[test]
    fn find_step_js_replaces_direction_placeholder() {
        let js = find_step_js(-1);

        assert!(js.contains("const dir = -1;"));
        assert!(!js.contains("__MDO_DIR__"));
    }

    #[test]
    fn sidebar_resize_js_installs_drag_bridge_once() {
        let js = sidebar_resize_js();

        assert!(js.contains("window.__mdoResizing"));
        assert!(js.contains("kind: 'sidebar_width'"));
        assert!(js.contains("document.addEventListener('mousemove', onMove)"));
        assert!(js.contains("document.removeEventListener('mouseup', onUp)"));
    }

    /// A selection outside a rendered pane (the Task View preview, the sidebar)
    /// has no pane index, so it must not be quotable.
    #[test]
    fn note_bridge_only_captures_selections_inside_a_pane_body() {
        let js = note_bridge_js();

        assert!(js.contains(".markdown-body[data-mdo-pane]"));
        assert!(js.contains("window.__mdoNoteBound"));
        assert!(js.contains("DOCUMENT_POSITION_FOLLOWING"));
    }

    /// A selection dragged across the split belongs to no single file.
    #[test]
    fn note_bridge_rejects_a_selection_spanning_two_panes() {
        let js = note_bridge_js();

        assert!(js.contains("if (!body || body !== paneBody(range.endContainer)) return null;"));
    }

    /// A remembered selection must die with the nodes it quoted, so a re-render
    /// (live reload, tab switch, theme switch) cannot attach it to new content.
    #[test]
    fn note_bridge_drops_a_remembered_selection_once_its_text_is_gone() {
        let js = note_bridge_js();

        assert!(js.contains("if (!cap || !document.contains(cap.node)) return null;"));
    }

    /// The document's own menu (copy, look up) is never taken over: the note
    /// entry moved to the floating icon.
    #[test]
    fn note_bridge_never_touches_the_native_context_menu() {
        let js = note_bridge_js();

        assert!(!js.contains("contextmenu"));
        assert!(js.contains("kind: 'note_icon'"));
    }

    /// Selecting the whole document is a rewrite request, not a note: no icon,
    /// and the flag travels to Rust so the keybinding can say why.
    #[test]
    fn note_bridge_refuses_a_selection_covering_the_whole_pane() {
        let js = note_bridge_js();

        assert!(js.contains("dense(quote) >= whole * 0.9"));
        assert!(js.contains("if (!cap || cap.tooBroad) return;"));
    }

    /// The overlay belongs to the pane's scroll container, so it scrolls with
    /// the text and leaves the rendered document untouched.
    #[test]
    fn note_bridge_draws_the_overlay_outside_the_rendered_body() {
        let js = note_bridge_js();

        assert!(js.contains("const host = cap && cap.body && cap.body.parentElement;"));
        assert!(js.contains("plan.host.appendChild(layer)"));
        assert!(js.contains("className = 'mdo-note-mark'"));
        assert!(js.contains("--mdo-note-x"));
    }

    /// The icon is only an affordance: what gets quoted is whatever is selected
    /// when it is clicked, judged by the same Rust path as the keybinding.
    #[test]
    fn note_icon_click_re_reads_the_selection() {
        let js = note_bridge_js();
        let handler = js
            .split_once("addEventListener('click'")
            .expect("icon click handler")
            .1;

        assert!(handler.contains("const now = window.__mdoNoteCurrent();"));
        assert!(handler.contains("quote: now ? now.quote : ''"));
        assert!(handler.contains("too_broad: now ? !!now.tooBroad : false"));
        assert!(!handler.contains("cap.quote"));
    }

    /// While the popover is open the quoted range is frozen: the document's
    /// selection may move, the highlight and the note may not.
    #[test]
    fn note_bridge_freezes_the_quote_while_the_popover_is_open() {
        let js = note_bridge_js();
        let handler = js
            .split_once("addEventListener('selectionchange'")
            .expect("selectionchange handler")
            .1;

        assert!(handler.starts_with(", () => {\n    // why: the popover quotes a fixed range."));
        assert!(handler.contains("if (window.__mdoNoteMode === 'quote') return;"));
        assert!(js.contains("window.__mdoNoteFreeze = () =>"));
        assert!(note_overlay_js(NoteOverlay::Quote).contains("window.__mdoNoteFreeze()"));
        // Closing it catches the memory up with whatever the document did while
        // it was frozen — otherwise the icon returns for a dead selection.
        assert!(note_overlay_js(NoteOverlay::Icon).contains("window.__mdoNoteThaw()"));
    }

    /// A click outside the popover abandons the note; the one inside it (and on
    /// the icon) does not.
    #[test]
    fn note_bridge_dismisses_the_popover_on_an_outside_click() {
        let js = note_bridge_js();
        let handler = js
            .split_once("addEventListener('mousedown'")
            .expect("mousedown handler")
            .1;

        assert!(handler.contains("if (inside(e.target, '.mdo-note-layer') || inside(e.target, '.mdo-note-popover')) return;"));
        assert!(handler.contains("kind: 'note_dismiss'"));
    }

    /// The split's other pane re-rendering (its own live reload) says nothing
    /// about the selection this one holds.
    #[test]
    fn note_bridge_drops_a_selection_only_when_its_own_pane_re_renders() {
        let js = note_bridge_js();

        assert!(js.contains("if (!cap || cap.body === body) window.__mdoNoteForget();"));
    }

    /// A resize or a rewrap moves every rect the overlay was drawn from.
    #[test]
    fn note_bridge_repositions_the_overlay_on_scroll_and_resize() {
        let js = note_bridge_js();

        assert!(js.contains("document.addEventListener('scroll', schedule, true);"));
        assert!(js.contains("window.addEventListener('resize', schedule);"));
        assert!(js.contains("new ResizeObserver(() => schedule()).observe(body)"));
        assert!(
            js.contains("plan.items.forEach((box, index) => apply(layer.children[index], box));")
        );
    }

    /// The probe answers even with nothing selected: Rust waits on one message.
    #[test]
    fn note_selection_js_always_sends_a_payload() {
        let js = note_selection_js();

        assert!(js.contains("kind: 'note_selection'"));
        assert!(js.contains("quote: cap ? cap.quote : ''"));
        assert!(js.contains("too_broad: cap ? !!cap.tooBroad : false"));
        assert!(js.contains("window.__mdoNoteCurrent"));
    }

    /// Saving a note both clears the overlay and forgets what it quoted, so the
    /// icon cannot come back for a note that is already on disk.
    #[test]
    fn note_overlay_js_switches_modes_and_forgets_on_save() {
        assert!(note_overlay_js(NoteOverlay::Icon).contains("window.__mdoNoteMode = 'icon'"));
        assert!(note_overlay_js(NoteOverlay::Quote).contains("window.__mdoNoteMode = 'quote'"));

        let off = note_overlay_js(NoteOverlay::Off);

        assert!(off.contains("window.__mdoNoteForget()"));
        assert!(off.contains("if (!window.__mdoNoteRender) return;"));
    }

    #[test]
    fn reset_root_scroll_js_resets_all_root_scroll_targets() {
        let js = reset_root_scroll_js();

        assert!(js.contains("window.scrollTo(0, 0)"));
        assert!(js.contains("document.scrollingElement"));
        assert!(js.contains("document.documentElement.scrollTop = 0"));
        assert!(js.contains("document.body.scrollTop = 0"));
    }
}
