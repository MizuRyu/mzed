use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use dioxus::desktop::tao::dpi::LogicalSize;
use dioxus::desktop::WindowBuilder;

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

    #[test]
    fn command_status_result_reports_nonzero_exit() {
        let status = std::process::Command::new("false").status().unwrap();

        assert!(command_status_result(status).is_err());
    }
}
