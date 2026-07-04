use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use dioxus::desktop::tao::dpi::LogicalSize;
use dioxus::desktop::WindowBuilder;

/// Axis-aligned rectangle in logical pixels. Used for off-screen safety checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    fn intersection_area(self, other: Rect) -> i64 {
        let ix = self.x.max(other.x);
        let iy = self.y.max(other.y);
        let iw = (self.x + self.w).min(other.x + other.w) - ix;
        let ih = (self.y + self.h).min(other.y + other.h) - iy;
        if iw <= 0 || ih <= 0 {
            0
        } else {
            iw as i64 * ih as i64
        }
    }
}

/// Decide whether the saved window position is safe to restore.
///
/// Returns `Some((x, y))` when the title-bar area of `saved` (top 30 px strip,
/// minimum 100 px wide) intersects at least one monitor with area ≥ 3 000 px².
/// Returns `None` when the window would be unreachable (all off-screen), so the
/// caller should fall back to OS-default positioning.
///
/// This is a pure function; no OS calls are made.
pub fn clamp_to_monitors(saved: Rect, monitors: &[Rect]) -> Option<(i32, i32)> {
    // Title-bar grab zone: top 30 logical pixels of the window, clamped to w.
    let title_bar = Rect::new(saved.x, saved.y, saved.w, 30_i32);
    // Require at least 100 × 30 = 3 000 px² of title-bar overlap.
    const MIN_AREA: i64 = 3_000;

    let visible = monitors
        .iter()
        .any(|m| title_bar.intersection_area(*m) >= MIN_AREA);

    if visible {
        Some((saved.x, saved.y))
    } else {
        None
    }
}

pub(crate) fn main_window_builder(width: i32, height: i32) -> WindowBuilder {
    WindowBuilder::new()
        .with_title("mzed")
        .with_always_on_top(false)
        .with_min_inner_size(LogicalSize::new(360.0, 240.0))
        .with_inner_size(LogicalSize::new(
            width.max(360) as f64,
            height.max(240) as f64,
        ))
}

pub(crate) fn mermaid_window_builder() -> WindowBuilder {
    WindowBuilder::new()
        .with_title("mzed — Mermaid")
        .with_inner_size(LogicalSize::new(900.0, 700.0))
        .with_min_inner_size(LogicalSize::new(320.0, 240.0))
}

pub(crate) fn canonical_clipboard_text(path: PathBuf) -> String {
    std::fs::canonicalize(&path)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

/// Write `text` to the macOS clipboard via `pbcopy`, bypassing the WebView
/// clipboard API (which can throw `NotAllowedError` in WKWebView sandboxes
/// even when the write actually succeeds, producing a spurious error toast).
pub(crate) fn native_clipboard_write(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut child = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    child.wait().and_then(command_status_result)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn clipboard_write_result_js(text: &str) -> String {
    let text_json = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"(async () => {{
  const mzedErrorMessage = (e) => e && e.message ? String(e.message) : String(e);
  try {{
    await navigator.clipboard.writeText({text_json});
    dioxus.send({{ ok: true, error: null }});
  }} catch (e) {{
    dioxus.send({{ ok: false, error: mzedErrorMessage(e) }});
  }}
}})();"#
    )
}

pub(crate) fn clipboard_write_with_feedback_js(text: &str, button_id: &str) -> String {
    let text_json = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string());
    let button_id_json = serde_json::to_string(button_id).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"(async () => {{
  const mzedErrorMessage = (e) => e && e.message ? String(e.message) : String(e);
  try {{
    await navigator.clipboard.writeText({text_json});
    const btn = document.getElementById({button_id_json});
    if (btn) {{
      if (!btn.dataset.mdoIcon) btn.dataset.mdoIcon = btn.innerHTML;
      btn.innerHTML = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6L9 17l-5-5"/></svg>';
      setTimeout(() => {{ btn.innerHTML = btn.dataset.mdoIcon; }}, 1200);
    }}
    dioxus.send({{ ok: true, error: null }});
  }} catch (e) {{
    dioxus.send({{ ok: false, error: mzedErrorMessage(e) }});
  }}
}})();"#
    )
}

pub(crate) fn print_result_js() -> &'static str {
    r#"(async () => {
  const mzedErrorMessage = (e) => e && e.message ? String(e.message) : String(e);
  try {
    window.print();
    dioxus.send({ ok: true, error: null });
  } catch (e) {
    dioxus.send({ ok: false, error: mzedErrorMessage(e) });
  }
})();"#
}

pub(crate) fn reveal_in_finder(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
        .map(|_| ())
}

pub(crate) fn open_target(path: impl AsRef<std::ffi::OsStr>) -> std::io::Result<()> {
    open::that_in_background(path).join().unwrap_or_else(|_| {
        Err(std::io::Error::other(
            "platform open thread panicked before returning a result",
        ))
    })
}

pub(crate) fn finder_delete_script(path: &Path) -> String {
    let p = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!("tell application \"Finder\" to delete (POSIX file \"{p}\")")
}

pub(crate) fn move_to_trash(path: &Path) -> std::io::Result<()> {
    let status = std::process::Command::new("osascript")
        .arg("-e")
        .arg(finder_delete_script(path))
        .status()?;
    command_status_result(status)
}

fn command_status_result(status: ExitStatus) -> std::io::Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "platform command exited with status {status}"
        )))
    }
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    #[test]
    fn clipboard_write_result_js_reports_success_and_failure() {
        let js = clipboard_write_result_js("\"quoted\"\n</script>");

        assert!(js.contains("await navigator.clipboard.writeText(\"\\\"quoted\\\"\\n</script>\");"));
        assert!(js.contains("dioxus.send({ ok: true, error: null });"));
        assert!(js.contains("dioxus.send({ ok: false, error: mzedErrorMessage(e) });"));
    }

    #[test]
    fn print_result_js_reports_success_and_failure() {
        let js = print_result_js();

        assert!(js.contains("window.print();"));
        assert!(js.contains("dioxus.send({ ok: true, error: null });"));
        assert!(js.contains("dioxus.send({ ok: false, error: mzedErrorMessage(e) });"));
    }

    #[test]
    fn finder_delete_script_escapes_quotes_and_backslashes() {
        let script = finder_delete_script(Path::new(r#"/tmp/a"b\c.md"#));

        assert!(script.contains(r#"POSIX file "/tmp/a\"b\\c.md""#));
    }

    #[test]
    fn clipboard_write_with_feedback_js_encodes_text_and_button_id() {
        let js = clipboard_write_with_feedback_js("\"md\"\n</script>", "copy\"btn");
        let expected = r#"(async () => {
  const mzedErrorMessage = (e) => e && e.message ? String(e.message) : String(e);
  try {
    await navigator.clipboard.writeText("\"md\"\n</script>");
    const btn = document.getElementById("copy\"btn");
    if (btn) {
      if (!btn.dataset.mdoIcon) btn.dataset.mdoIcon = btn.innerHTML;
      btn.innerHTML = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6L9 17l-5-5"/></svg>';
      setTimeout(() => { btn.innerHTML = btn.dataset.mdoIcon; }, 1200);
    }
    dioxus.send({ ok: true, error: null });
  } catch (e) {
    dioxus.send({ ok: false, error: mzedErrorMessage(e) });
  }
})();"#;

        assert_eq!(js, expected);
    }

    // ── clamp_to_monitors tests ──────────────────────────────────────────────

    fn built_in_monitor() -> Rect {
        Rect::new(0, 0, 1440, 900)
    }

    #[test]
    fn ウィンドウがモニタ内に完全に収まる場合は座標を返す() {
        let saved = Rect::new(100, 80, 1100, 760);
        let result = clamp_to_monitors(saved, &[built_in_monitor()]);
        assert_eq!(result, Some((100, 80)));
    }

    #[test]
    fn ウィンドウが完全にオフスクリーンの場合はNoneを返す() {
        // Window is entirely to the right of the only monitor (1440 wide).
        let saved = Rect::new(2000, 100, 1100, 760);
        let result = clamp_to_monitors(saved, &[built_in_monitor()]);
        assert!(result.is_none());
    }

    #[test]
    fn タイトルバーが切れているが十分重なる場合は座標を返す() {
        // Window starts at x=-300 but 1140px of title bar overlaps the monitor.
        let saved = Rect::new(-300, 0, 1100, 760);
        let result = clamp_to_monitors(saved, &[built_in_monitor()]);
        assert_eq!(result, Some((-300, 0)));
    }

    #[test]
    fn タイトルバーがわずかしか見えない場合はNoneを返す() {
        // Only 50px of title bar visible (< 100px threshold for 3 000 px² at 30px height).
        let saved = Rect::new(1390, 0, 1100, 760);
        let result = clamp_to_monitors(saved, &[built_in_monitor()]);
        assert!(result.is_none());
    }

    #[test]
    fn 外部モニタで保存し内蔵のみ残った場合はNoneを返す() {
        // Saved on a 4K external display to the right.
        let saved = Rect::new(1440, 200, 1100, 760);
        let only_internal = built_in_monitor(); // 0..1440 × 0..900
        let result = clamp_to_monitors(saved, &[only_internal]);
        assert!(result.is_none());
    }

    #[test]
    fn 複数モニタのいずれかに収まればSomeを返す() {
        let external = Rect::new(1440, 0, 2560, 1440);
        let saved = Rect::new(1500, 100, 1100, 760);
        let result = clamp_to_monitors(saved, &[built_in_monitor(), external]);
        assert_eq!(result, Some((1500, 100)));
    }

    #[test]
    fn モニタリストが空の場合はNoneを返す() {
        let saved = Rect::new(0, 0, 1100, 760);
        assert!(clamp_to_monitors(saved, &[]).is_none());
    }

    // ── end clamp_to_monitors tests ─────────────────────────────────────────

    #[test]
    fn command_status_result_reports_nonzero_exit() {
        let status = std::process::Command::new("false").status().unwrap();

        assert!(command_status_result(status).is_err());
    }

    /// B2: native_clipboard_write must not return an error on the happy path.
    /// This only verifies the OS call succeeds; we cannot read the clipboard
    /// back in a headless test environment without additional setup.
    #[test]
    fn native_clipboard_writeはエラーを返さない() {
        // pbcopy is macOS-only; skip on other platforms.
        if std::process::Command::new("which")
            .arg("pbcopy")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            assert!(
                native_clipboard_write("test path /tmp/a.md").is_ok(),
                "native clipboard write should succeed via pbcopy"
            );
        }
    }
}
