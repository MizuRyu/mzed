use std::sync::OnceLock;
use std::time::{Duration, Instant};

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

pub(crate) fn mark_process_start() {
    let _ = PROCESS_START.set(Instant::now());
}

pub(crate) fn measure<T>(label: &str, fields: &[(&str, String)], f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let start = Instant::now();
    let value = f();
    let borrowed = fields
        .iter()
        .map(|(k, v)| (*k, v.as_str()))
        .collect::<Vec<_>>();
    eprintln!("{}", perf_line(label, start.elapsed(), &borrowed));
    value
}

pub(crate) fn log_elapsed_ms(label: &str, elapsed_ms: f64, fields: &[(&str, String)]) {
    if !enabled() {
        return;
    }
    let borrowed = fields
        .iter()
        .map(|(k, v)| (*k, v.as_str()))
        .collect::<Vec<_>>();
    eprintln!("{}", perf_line_ms(label, elapsed_ms, &borrowed));
}

pub(crate) fn log_since_process_start(label: &str, fields: &[(&str, String)]) {
    if !enabled() {
        return;
    }
    if let Some(start) = PROCESS_START.get() {
        let borrowed = fields
            .iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect::<Vec<_>>();
        eprintln!("{}", perf_line(label, start.elapsed(), &borrowed));
    }
}

fn enabled() -> bool {
    env_enabled(std::env::var("MZED_PERF").ok().as_deref())
}

fn env_enabled(value: Option<&str>) -> bool {
    matches!(
        value.map(str::to_ascii_lowercase).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

fn perf_line(label: &str, elapsed: Duration, fields: &[(&str, &str)]) -> String {
    perf_line_ms(label, elapsed.as_secs_f64() * 1000.0, fields)
}

fn perf_line_ms(label: &str, elapsed_ms: f64, fields: &[(&str, &str)]) -> String {
    let mut line = format!("mzed_perf label={label} elapsed_ms={elapsed_ms:.3}");
    for (key, value) in fields {
        line.push(' ');
        line.push_str(key);
        line.push('=');
        line.push_str(value);
    }
    line
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn perf_line_includes_label_elapsed_and_fields() {
        let line = perf_line(
            "markdown.render",
            Duration::from_micros(1_234),
            &[("input_bytes", "42"), ("output_bytes", "84")],
        );

        assert_eq!(
            line,
            "mzed_perf label=markdown.render elapsed_ms=1.234 input_bytes=42 output_bytes=84"
        );
    }

    #[test]
    fn env_enabled_accepts_truthy_values() {
        assert!(env_enabled(Some("1")));
        assert!(env_enabled(Some("true")));
        assert!(env_enabled(Some("yes")));
        assert!(!env_enabled(None));
        assert!(!env_enabled(Some("0")));
        assert!(!env_enabled(Some("false")));
    }

    #[test]
    fn perf_line_ms_formats_webview_elapsed_values() {
        let line = perf_line_ms(
            "webview.post_render",
            12.3456,
            &[("panes", "2"), ("katex", "true")],
        );

        assert_eq!(
            line,
            "mzed_perf label=webview.post_render elapsed_ms=12.346 panes=2 katex=true"
        );
    }

    #[test]
    fn process_start_can_be_marked_once() {
        mark_process_start();
        mark_process_start();

        assert!(PROCESS_START.get().is_some());
    }
}
