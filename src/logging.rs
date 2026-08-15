//! On-disk logs under `~/Library/Logs/mzed/`.
//!
//! The GUI app discards stdout and stderr: a `.app` launched from Finder or
//! the Dock has nowhere to print. Anything worth diagnosing later — a panic in
//! an event handler, a clipboard write that failed, the serve request log —
//! therefore has to reach a file, or the failure is simply invisible to the
//! user *and* to whoever reads the bug report.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

/// Rotate a log past this size (one `.old` generation is kept).
const LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Path of a named log file, e.g. `log_path("serve.log")`.
pub(crate) fn log_path(name: &str) -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join("Library/Logs/mzed").join(name))
}

/// An append-only log file. Every write is best-effort: logging must never
/// take down the thing it is observing.
pub(crate) struct LogFile(Option<Mutex<std::fs::File>>);

impl LogFile {
    /// Open `name` for appending, rotating an oversized file to `<name>.old`
    /// first. A missing home dir or an IO error yields a sink that silently
    /// drops writes.
    pub(crate) fn open(name: &str) -> Self {
        Self(log_path(name).and_then(|path| open_appending(&path).map(Mutex::new)))
    }

    /// Append one line, stamped with the current UTC time.
    pub(crate) fn line(&self, message: &str) {
        let Some(file) = &self.0 else { return };
        let Ok(mut file) = file.lock() else { return };
        let _ = writeln!(file, "{} {message}", utc_timestamp());
    }
}

fn open_appending(path: &Path) -> Option<std::fs::File> {
    std::fs::create_dir_all(path.parent()?).ok()?;
    if std::fs::metadata(path).is_ok_and(|m| m.len() > LOG_MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()
}

/// The shared application log (`mzed.log`), opened on first use.
fn app_log_file() -> &'static LogFile {
    static APP_LOG: OnceLock<LogFile> = OnceLock::new();
    APP_LOG.get_or_init(|| LogFile::open("mzed.log"))
}

/// Record an application event: to `mzed.log` always, and to stderr as well so
/// a terminal-launched run still shows it inline.
pub(crate) fn app(message: impl AsRef<str>) {
    let message = message.as_ref();
    eprintln!("mzed: {message}");
    app_log_file().line(message);
}

/// Route panics to `mzed.log` before the default handler runs. Without this a
/// panic inside a UI event handler leaves no trace at all in a bundled app —
/// the action just silently does nothing.
pub(crate) fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".to_string());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".to_string());
        app_log_file().line(&format!("PANIC at {location}: {payload}"));
        previous(info);
    }));
}

/// Seconds-precision UTC timestamp (`2026-07-25T09:30:12Z`) without a date
/// crate: civil-from-days per Howard Hinnant's algorithm.
pub(crate) fn utc_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_utc(secs)
}

fn format_utc(secs: u64) -> String {
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    let z = days as i64 + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn format_utcは既知のepochを正しく整形する() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        // 2026-07-25 00:00:00 UTC
        assert_eq!(format_utc(1_784_937_600), "2026-07-25T00:00:00Z");
        // うるう年 2 月末日
        assert_eq!(format_utc(1_709_251_199), "2024-02-29T23:59:59Z");
    }

    #[test]
    fn log_pathはLibrary_Logs配下を指す() {
        let path = log_path("serve.log").expect("home dir");
        assert!(path.ends_with("Library/Logs/mzed/serve.log"));
    }

    #[test]
    fn 開けないログはサイレントに書き捨てる() {
        // A sink LogFile must not panic when written to.
        let sink = LogFile(None);
        sink.line("dropped");
    }
}
