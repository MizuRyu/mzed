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

/// Run the server until the process is interrupted. `open_browser` launches
/// the default browser at the served URL once the socket is bound.
pub(crate) fn run(dir: Option<PathBuf>, port: u16, open_browser: bool) -> anyhow::Result<()> {
    let dir = dir.unwrap_or_else(|| PathBuf::from("."));
    let root = dir
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    if !root.is_dir() {
        anyhow::bail!("{} is not a directory", root.display());
    }

    // Bind explicitly first for a clear "port in use" error.
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr)
        .map_err(|e| anyhow::anyhow!("cannot bind {addr}: {e} (try --port)"))?;
    let server = Server::from_listener(listener, None)
        .map_err(|e| anyhow::anyhow!("server start failed: {e}"))?;

    let url = format!("http://{addr}/");
    println!("Serving {} at {url}", root.display());
    println!("(Ctrl+C で停止)");
    if open_browser {
        let _ = open::that(&url);
    }

    for request in server.incoming_requests() {
        let response = route(&root, request.url());
        let _ = request.respond(response);
    }
    Ok(())
}

/// Dispatch one request URL to a response. Pure with respect to the request
/// (all state is the served root + the filesystem).
fn route(root: &Path, url: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url, ""),
    };
    match path {
        "/" => html_response(&shell::page(root)),
        "/api/tree" => json_response(&tree_json(root)),
        "/api/doc" => match doc_param(root, query) {
            Ok(file) => json_response(&doc_json(root, &file)),
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
fn doc_json(root: &Path, file: &Path) -> String {
    let snapshot = file_service::load_document(Some(file.to_path_buf()), &[root.to_path_buf()]);
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

        let json = doc_json(&root, &root.join("a.md"));
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
    fn routeはassetsの外を404にする() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let r = route(&root, "/assets/../Cargo.toml");
        assert_eq!(r.status_code().0, 404);
        let r = route(&root, "/etc/passwd");
        assert_eq!(r.status_code().0, 404);
    }
}
