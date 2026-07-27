//! `mzed serve` — browser viewer for a folder (screen-sharing use case).
//!
//! A small synchronous HTTP server on 127.0.0.1 that reuses the exact desktop
//! rendering pipeline (`file_service::load_document`: sanitised HTML, wikilink
//! resolution, data-URL images, roots containment). The browser shell adds a
//! sidebar tree with a name filter, a ToC panel, dark/light, and live reload
//! by polling the served document's mtime.
//!
//! Security model:
//! - Binds 127.0.0.1 only; there is deliberately no way to expose it to the
//!   LAN. The use case is sharing one's own screen, not serving others.
//! - Every document request is canonicalised and must stay inside the served
//!   root, and must be markdown — the same containment rule as the app.
//! - Assets are served from an embedded copy of `assets/` (compile-time),
//!   so asset paths can never touch the filesystem.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use include_dir::{include_dir, Dir};
use percent_encoding::percent_decode_str;
use tiny_http::{Header, Response, Server};

use crate::files;
use crate::services::file_service;

mod shell;

/// Embedded copy of the app's asset directory (CSS, highlight.js, mermaid,
/// KaTeX incl. fonts). Served under `/assets/`.
static ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/assets");

/// Worker threads answering requests. More than one matters: a heavy document
/// render (many base64 images, wikilink walks) must not freeze assets, the
/// tree, and the live-reload polls behind it.
const WORKERS: usize = 4;

/// How long a computed tree JSON stays valid. The browser polls every 3s;
/// re-walking a large root on each poll is wasted work.
const TREE_CACHE: std::time::Duration = std::time::Duration::from_secs(2);

/// Rotate `serve.log` past this size (one `.old` generation is kept).
const LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Persistent request log. The GUI app discards stdout/stderr, so stderr-only
/// logging leaves nothing to diagnose in-app share failures with.
pub(crate) fn log_file_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join("Library/Logs/mzed/serve.log"))
}

/// Open the request log for appending, rotating an oversized file to
/// `serve.log.old` first. `None` (no home dir, IO error) degrades to
/// stderr-only logging.
fn open_log() -> Option<Mutex<std::fs::File>> {
    let path = log_file_path()?;
    std::fs::create_dir_all(path.parent()?).ok()?;
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_MAX_BYTES) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
        .map(Mutex::new)
}

/// One request log line to stderr and (when available) the log file.
fn log_request(file: &Option<Mutex<std::fs::File>>, ms: u128, status: u16, url: &str) {
    let line = format!("{} mzed serve: {ms:>5}ms {status} {url}", utc_timestamp());
    eprintln!("{line}");
    if let Some(f) = file {
        if let Ok(mut f) = f.lock() {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Seconds-precision UTC timestamp (`2026-07-25T09:30:12Z`) without a date
/// crate: civil-from-days per Howard Hinnant's algorithm.
fn utc_timestamp() -> String {
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

/// A running server: the shared listener (for [`ServeHandle::stop`]) and the
/// request worker threads.
pub(crate) struct ServeHandle {
    server: Arc<Server>,
    threads: Vec<std::thread::JoinHandle<()>>,
    pub(crate) url: String,
    root: PathBuf,
}

impl ServeHandle {
    /// Unblock the request loops and wait for every worker to finish.
    /// `unblock()` queues exactly ONE wake-up (tiny_http notifies a single
    /// waiter), so it must be called once per worker or the join hangs.
    fn stop(self) {
        for _ in 0..self.threads.len() {
            self.server.unblock();
        }
        for t in self.threads {
            let _ = t.join();
        }
    }
}

/// Bind 127.0.0.1:`port` and start serving `dir` on background threads.
fn start(dir: &Path, port: u16) -> anyhow::Result<ServeHandle> {
    let root = dir
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    if !root.is_dir() {
        anyhow::bail!("{} is not a directory", root.display());
    }

    // Bind explicitly first for a clear "port in use" error. Read the actual
    // address back so port 0 (OS-assigned, used by tests) yields a real URL.
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr)
        .map_err(|e| anyhow::anyhow!("cannot bind {addr}: {e} (try --port)"))?;
    let addr = listener.local_addr().map(|a| a.to_string()).unwrap_or(addr);
    let server = Arc::new(
        Server::from_listener(listener, None)
            .map_err(|e| anyhow::anyhow!("server start failed: {e}"))?,
    );

    let tree_cache: Arc<Mutex<Option<(std::time::Instant, String)>>> = Arc::new(Mutex::new(None));
    let log = Arc::new(open_log());
    // Rendering honours the user's config (frontmatter disclosure state),
    // read once at server start.
    let fm_open = crate::config::load().frontmatter_default_open;
    let threads = (0..WORKERS)
        .map(|_| {
            let server = Arc::clone(&server);
            let root = root.clone();
            let tree_cache = Arc::clone(&tree_cache);
            let log = Arc::clone(&log);
            std::thread::spawn(move || {
                // `incoming_requests` ends when `unblock()` is called.
                for request in server.incoming_requests() {
                    let started = std::time::Instant::now();
                    let url = request.url().to_string();
                    let response = route_cached(&root, &url, &tree_cache, fm_open);
                    let status = response.status_code().0;
                    let _ = request.respond(response);
                    log_request(&log, started.elapsed().as_millis(), status, &url);
                }
            })
        })
        .collect();
    Ok(ServeHandle {
        server,
        threads,
        url: format!("http://{addr}/"),
        root,
    })
}

/// `route`, with the tree endpoint served from a short-lived cache.
fn route_cached(
    root: &Path,
    url: &str,
    tree_cache: &Mutex<Option<(std::time::Instant, String)>>,
    fm_open: bool,
) -> Response<std::io::Cursor<Vec<u8>>> {
    if url.split('?').next() == Some("/api/tree") {
        let mut cache = tree_cache.lock().expect("tree cache lock");
        let fresh = matches!(&*cache, Some((at, _)) if at.elapsed() < TREE_CACHE);
        if !fresh {
            *cache = Some((std::time::Instant::now(), tree_json(root)));
        }
        let (_, json) = cache.as_ref().expect("cache was just filled");
        return json_response(json);
    }
    route(root, url, fm_open)
}

/// `mzed serve`: run in the foreground until the process is interrupted.
/// `open_browser` launches the default browser once the socket is bound.
pub(crate) fn run(dir: Option<PathBuf>, port: u16, open_browser: bool) -> anyhow::Result<()> {
    let dir = dir.unwrap_or_else(|| PathBuf::from("."));
    let handle = start(&dir, port)?;
    println!("Serving {} at {}", handle.root.display(), handle.url);
    if let Some(p) = log_file_path() {
        println!("request log: {}", p.display());
    }
    println!("(Ctrl+C で停止)");
    if open_browser {
        let _ = open::that(&handle.url);
    }
    for t in handle.threads {
        let _ = t.join();
    }
    Ok(())
}

/// The desktop app's single share server (palette「Toggle Web Share」). One at
/// a time is plenty: the use case is sharing the current project's docs.
static APP_SHARE: OnceLock<Mutex<Option<ServeHandle>>> = OnceLock::new();

fn app_share_slot() -> &'static Mutex<Option<ServeHandle>> {
    APP_SHARE.get_or_init(|| Mutex::new(None))
}

/// URL of the in-app share server, if one is running. Drives the palette
/// label (Start vs Stop) so a second toggle isn't mistaken for "re-open".
pub(crate) fn app_share_url() -> Option<String> {
    let slot = app_share_slot().lock().expect("share lock");
    slot.as_ref().map(|h| h.url.clone())
}

/// Toggle the in-app share server. Starting returns `Some(url)`; stopping
/// (when one is already running, whatever root it serves) returns `None`.
pub(crate) fn toggle_app_share(root: &Path, port: u16) -> anyhow::Result<Option<String>> {
    let mut slot = app_share_slot().lock().expect("share lock");
    if let Some(handle) = slot.take() {
        handle.stop();
        return Ok(None);
    }
    let handle = start(root, port)?;
    let url = handle.url.clone();
    *slot = Some(handle);
    Ok(Some(url))
}

/// Dispatch one request URL to a response. Pure with respect to the request
/// (all state is the served root + the filesystem).
fn route(root: &Path, url: &str, fm_open: bool) -> Response<std::io::Cursor<Vec<u8>>> {
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url, ""),
    };
    match path {
        "/" => html_response(&shell::page(root)),
        "/api/tree" => json_response(&tree_json(root)),
        "/api/doc" => match doc_param(root, query) {
            Ok(file) => json_response(&doc_json(root, &file, fm_open)),
            Err(msg) => error_response(400, &msg),
        },
        "/api/stat" => match doc_param(root, query) {
            Ok(file) => json_response(&format!(r#"{{"mtime":{}}}"#, mtime_ms(&file))),
            Err(msg) => error_response(400, &msg),
        },
        _ => match path.strip_prefix("/assets/") {
            Some(rel) => match ASSETS.get_file(rel) {
                Some(f) => asset_response(rel, f.contents()),
                None => error_response(404, "not found"),
            },
            None => error_response(404, "not found"),
        },
    }
}

/// Extract and validate the `path` query parameter: percent-decoded, absolute
/// or root-relative, canonicalised, inside the root, and markdown.
fn doc_param(root: &Path, query: &str) -> Result<PathBuf, String> {
    let raw = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("path="))
        .ok_or("missing path parameter")?;
    let decoded = percent_decode_str(&raw.replace('+', "%20"))
        .decode_utf8()
        .map_err(|_| "invalid path encoding")?
        .to_string();
    let candidate = {
        let p = PathBuf::from(&decoded);
        if p.is_absolute() {
            p
        } else {
            root.join(p)
        }
    };
    let file = candidate
        .canonicalize()
        .map_err(|_| "no such file".to_string())?;
    if !file.starts_with(root) {
        return Err("outside the served root".into());
    }
    if !file.is_file() || !files::is_markdown(&file) {
        return Err("not a markdown file".into());
    }
    Ok(file)
}

/// Modification time in milliseconds since the epoch (0 when unavailable).
fn mtime_ms(file: &Path) -> u128 {
    std::fs::metadata(file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// The sidebar tree as JSON (absolute paths: the server re-validates every
/// path on each request, and this never leaves localhost).
fn tree_json(root: &Path) -> String {
    fn node_json(n: &files::TreeNode, out: &mut String) {
        out.push('{');
        out.push_str(&format!(
            r#""name":{},"path":{},"is_dir":{},"md_count":{},"children":["#,
            json_str(&n.name),
            json_str(&n.path.to_string_lossy()),
            n.is_dir,
            n.md_count
        ));
        for (i, c) in n.children.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            node_json(c, out);
        }
        out.push_str("]}");
    }
    let mut out = String::from("[");
    for (i, n) in files::build_tree(root).iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        node_json(n, &mut out);
    }
    out.push(']');
    out
}

/// A rendered document as JSON: sanitised HTML, flat ToC, and mtime for the
/// client's live-reload polling.
fn doc_json(root: &Path, file: &Path, fm_open: bool) -> String {
    let snapshot =
        file_service::load_document(Some(file.to_path_buf()), &[root.to_path_buf()], fm_open);
    let mut toc = String::from("[");
    for (i, e) in snapshot.toc().iter().enumerate() {
        if i > 0 {
            toc.push(',');
        }
        toc.push_str(&format!(
            r#"{{"level":{},"text":{},"anchor":{}}}"#,
            e.level,
            json_str(&e.text),
            json_str(&e.anchor)
        ));
    }
    toc.push(']');

    let name = file
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    format!(
        r#"{{"name":{},"path":{},"html":{},"toc":{},"mtime":{}}}"#,
        json_str(&name),
        json_str(&file.to_string_lossy()),
        json_str(snapshot.rendered_html()),
        toc,
        mtime_ms(file)
    )
}

/// Minimal JSON string encoder (enough for paths, names, and HTML payloads).
fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

fn html_response(body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    with_type(Response::from_string(body), "text/html; charset=utf-8")
}

fn json_response(body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    with_type(
        Response::from_string(body),
        "application/json; charset=utf-8",
    )
}

fn error_response(status: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    with_type(
        Response::from_string(msg).with_status_code(status),
        "text/plain; charset=utf-8",
    )
}

fn asset_response(rel: &str, bytes: &[u8]) -> Response<std::io::Cursor<Vec<u8>>> {
    let mime = match rel.rsplit('.').next().unwrap_or("") {
        "css" => "text/css",
        "js" => "text/javascript",
        "png" => "image/png",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    };
    with_type(Response::from_data(bytes.to_vec()), mime)
}

fn with_type(
    r: Response<std::io::Cursor<Vec<u8>>>,
    mime: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let header =
        Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).expect("static header is valid");
    r.with_header(header)
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn doc_paramはroot外と非mdを拒否する() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::write(root.join("a.md"), "# a").unwrap();
        fs::write(root.join("note.txt"), "x").unwrap();
        let outside = tmp.path().parent().unwrap().join("outside.md");

        // Relative and absolute forms of a valid file both resolve.
        let q = "path=a.md";
        assert_eq!(doc_param(&root, q).unwrap(), root.join("a.md"));
        let abs = format!("path={}", root.join("a.md").display());
        assert_eq!(doc_param(&root, &abs).unwrap(), root.join("a.md"));

        // Traversal out of the root is rejected even when the file exists.
        fs::write(&outside, "# out").ok();
        let esc = "path=../outside.md";
        assert!(doc_param(&root, esc).is_err());

        // Non-markdown is rejected.
        assert!(doc_param(&root, "path=note.txt").is_err());
        // Missing file is rejected.
        assert!(doc_param(&root, "path=missing.md").is_err());
    }

    #[test]
    fn doc_paramはパーセントエンコードされたパスを解決する() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir(root.join("メモ")).unwrap();
        fs::write(root.join("メモ/日誌.md"), "# d").unwrap();

        let q = format!(
            "path={}",
            percent_encoding::utf8_percent_encode(
                "メモ/日誌.md",
                percent_encoding::NON_ALPHANUMERIC
            )
        );
        assert_eq!(doc_param(&root, &q).unwrap(), root.join("メモ/日誌.md"));
    }

    #[test]
    fn tree_jsonが有効なJSONを返す() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir(root.join("docs")).unwrap();
        fs::write(root.join("docs/guide.md"), "# g").unwrap();
        fs::write(root.join("README.md"), "# r").unwrap();

        let json = tree_json(&root);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2); // docs dir + README.md
        assert_eq!(arr[0]["name"], "docs");
        assert_eq!(arr[0]["children"][0]["name"], "guide.md");
    }

    #[test]
    fn doc_jsonがレンダリング済みHTMLとtocを返す() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::write(root.join("a.md"), "# Title\n\n## Sub\n\ntext").unwrap();

        let json = doc_json(&root, &root.join("a.md"), false);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(v["html"].as_str().unwrap().contains("<h1"));
        assert_eq!(v["toc"][0]["text"], "Title");
        assert!(v["mtime"].as_u64().unwrap() > 0);
    }

    #[test]
    fn 埋め込みアセットに主要ファイルが揃っている() {
        for name in [
            "mdo.css",
            "github-markdown.css",
            "github-markdown-dark.css",
            "highlight.min.js",
            "highlight-github.css",
            "highlight-github-dark.css",
            "mermaid.min.js",
            "katex/katex.min.js",
            "katex/katex.min.css",
            "katex/auto-render.min.js",
        ] {
            assert!(ASSETS.get_file(name).is_some(), "missing asset: {name}");
        }
        // KaTeX fonts came along too.
        assert!(ASSETS
            .get_file("katex/fonts/KaTeX_Main-Regular.woff2")
            .is_some());
    }

    #[test]
    fn toggle_app_shareは起動と停止を往復する() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.md"), "# a").unwrap();

        // Port 0 lets the OS pick a free port, keeping the test parallel-safe.
        let started = toggle_app_share(tmp.path(), 0).expect("start");
        let url = started.expect("first toggle starts the server");
        assert!(url.starts_with("http://127.0.0.1:"));

        // Second toggle stops it (returns None) and frees the slot.
        assert!(toggle_app_share(tmp.path(), 0).expect("stop").is_none());
        // And a third starts again.
        assert!(toggle_app_share(tmp.path(), 0).expect("restart").is_some());
        assert!(toggle_app_share(tmp.path(), 0)
            .expect("stop again")
            .is_none());
    }

    #[test]
    fn 実リクエストがログファイルに記録される() {
        use std::io::{Read as _, Write as _};
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.md"), "# a").unwrap();

        let handle = start(tmp.path(), 0).expect("start");
        let addr = handle
            .url
            .trim_start_matches("http://")
            .trim_end_matches('/');
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        write!(s, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).unwrap();
        assert!(buf.starts_with("HTTP/1.1 200"), "got: {}", &buf[..40]);
        // stop() joins the workers, so the log line is on disk afterwards.
        handle.stop();

        let log = fs::read_to_string(log_file_path().unwrap()).unwrap();
        let last = log.lines().last().unwrap();
        assert!(last.contains("mzed serve:"), "last log line: {last}");
        assert!(last.contains(" 200 /"), "last log line: {last}");
    }

    #[test]
    fn format_utcは既知のepochを正しく整形する() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        // 2026-07-25 00:00:00 UTC
        assert_eq!(format_utc(1_784_937_600), "2026-07-25T00:00:00Z");
        // うるう年 2 月末日
        assert_eq!(format_utc(1_709_251_199), "2024-02-29T23:59:59Z");
    }

    #[test]
    fn routeはassetsの外を404にする() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let r = route(&root, "/assets/../Cargo.toml", false);
        assert_eq!(r.status_code().0, 404);
        let r = route(&root, "/etc/passwd", false);
        assert_eq!(r.status_code().0, 404);
    }
}
