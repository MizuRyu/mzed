mod dom;
mod export;
mod find;
mod keyboard;
mod mermaid;
mod render;

pub(crate) use dom::{
    overlay_row_scroll_js, reset_root_scroll_js, sidebar_active_js, OverlayRowKind,
};
pub(crate) use export::{export_capture_js, webview_action_error};
pub(crate) use find::{find_highlight_js, find_step_js};
pub(crate) use keyboard::{keydown_bridge_js, sidebar_resize_js};
pub(crate) use mermaid::{helper_js as mermaid_helper_js, mermaid_window_js};
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

    #[test]
    fn reset_root_scroll_js_resets_all_root_scroll_targets() {
        let js = reset_root_scroll_js();

        assert!(js.contains("window.scrollTo(0, 0)"));
        assert!(js.contains("document.scrollingElement"));
        assert!(js.contains("document.documentElement.scrollTop = 0"));
        assert!(js.contains("document.body.scrollTop = 0"));
    }
}
