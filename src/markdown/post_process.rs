//! Post-process rendered HTML against the source file's directory.
//!
//! Two rewrites happen here:
//! - project-contained relative `<img src>` are read from disk and inlined as
//!   `data:` URLs so the WebView can display them without local file access.
//! - project-contained relative `.md` `<a href>` become internal links
//!   (`class="mdo-link"` + `data-path="<abs>"`) handled by the host.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use lol_html::{element, rewrite_str, RewriteStrSettings};

fn has_scheme(url: &str) -> Option<&str> {
    let trimmed = url.trim_start_matches(|c: char| c.is_ascii_whitespace() || c.is_control());
    let scheme_end = trimmed.find(':')?;
    let first_separator = trimmed.find(['/', '?', '#']).unwrap_or(trimmed.len());
    (scheme_end < first_separator).then_some(&trimmed[..scheme_end])
}

fn is_external_link(url: &str) -> bool {
    matches!(
        has_scheme(url).map(str::to_ascii_lowercase).as_deref(),
        Some("http" | "https" | "mailto")
    ) || url.trim_start().starts_with('#')
}

fn is_unsafe_absolute_url(url: &str) -> bool {
    url.trim_start().starts_with("//")
        || matches!(
            has_scheme(url).map(str::to_ascii_lowercase).as_deref(),
            Some(scheme) if !matches!(scheme, "http" | "https" | "mailto")
        )
}

/// Guess an image MIME type from a path extension; defaults to png.
fn image_mime(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        // SVG can embed scripts and external references, but loading it as an
        // `<img src="data:image/svg+xml;base64,…">` runs neither (browser spec:
        // images loaded via <img>/data: are a non-scripted, non-fetching
        // context). mzed also fully escapes raw HTML before re-parsing, so there
        // is no inline `<svg>` path — this data-URL `<img>` route is the only
        // way an SVG reaches the WebView, and it is safe.
        Some("svg") => Some("image/svg+xml"),
        _ => None,
    }
}

/// Decode `%XX` escapes so a percent-encoded relative URL (as emitted by the
/// Markdown renderer for paths containing spaces or parentheses) maps back to
/// the real on-disk filename. Non-`%XX` bytes pass through unchanged. The
/// decoded path is still gated by the canonical-root containment check below,
/// so decoding cannot broaden filesystem access.
fn percent_decode(s: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn local_path_part(url: &str) -> &str {
    url.split('#')
        .next()
        .unwrap_or(url)
        .split('?')
        .next()
        .unwrap_or(url)
}

fn has_forbidden_absolute_component(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, Component::RootDir | Component::Prefix(_)))
}

const MAX_INLINE_IMAGE_BYTES: usize = 8 * 1024 * 1024;

fn canonical_roots(allowed_roots: &[PathBuf], base_dir: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = allowed_roots
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .collect();
    if roots.is_empty() {
        if let Ok(base) = base_dir.canonicalize() {
            roots.push(base);
        }
    }
    roots
}

fn resolve_inside_roots(base_dir: &Path, target: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let local = percent_decode(local_path_part(target).trim());
    if local.is_empty() || is_unsafe_absolute_url(&local) {
        return None;
    }
    let local = Path::new(&local);
    if has_forbidden_absolute_component(local) {
        return None;
    }

    let base = base_dir.canonicalize().ok()?;
    if !roots.iter().any(|root| base.starts_with(root)) {
        return None;
    }

    let candidate = base.join(local).canonicalize().ok()?;
    roots
        .iter()
        .any(|root| candidate.starts_with(root))
        .then_some(candidate)
}

/// Read a project-contained image and turn it into a `data:` URL.
fn to_data_url(base_dir: &Path, src: &str, roots: &[PathBuf]) -> Option<String> {
    let path = resolve_inside_roots(base_dir, src, roots)?;
    path.is_file().then_some(())?;
    let mime = image_mime(&path)?;
    let file = std::fs::File::open(&path).ok()?;
    if file.metadata().ok()?.len() > MAX_INLINE_IMAGE_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take((MAX_INLINE_IMAGE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_INLINE_IMAGE_BYTES {
        return None;
    }
    let encoded = STANDARD.encode(&bytes);
    Some(format!("data:{mime};base64,{encoded}"))
}

/// Rewrite relative image/link URLs in `html` relative to `base_dir`.
pub fn post_process(html: &str, base_dir: &Path, allowed_roots: &[PathBuf]) -> String {
    // Re-introduce the allow-listed raw-HTML subset (`<img>`, `<p align>`, …)
    // by rebuilding validated tags from the escaped text. This must run before
    // the lol_html rewrites so the reconstructed `<img src>` rides the same
    // data-URL / roots-containment path as Markdown images.
    let html = super::raw_html::reconstruct_allowed(html);
    let html = html.as_str();
    let base_dir = base_dir.to_path_buf();
    let roots = canonical_roots(allowed_roots, &base_dir);
    let img_base = base_dir.clone();
    let img_roots = roots.clone();
    let img = element!("img[src]", move |el| {
        if let Some(src) = el.get_attribute("src") {
            if let Some(data) = to_data_url(&img_base, &src, &img_roots) {
                el.set_attribute("src", &data).ok();
            } else {
                el.remove_attribute("src");
            }
        }
        // Obsidian embeds `![[img|400]]` carry the display width as an
        // `alt="mdo-width-<N>"` side-channel from the wikilink pre-processor
        // (Markdown image syntax has no width). Promote it to a real `width`
        // attribute and clear the marker so it never shows as alt text.
        if let Some(alt) = el.get_attribute("alt") {
            if let Some(w) = alt.strip_prefix(super::wikilink::WIDTH_ALT_PREFIX) {
                if !w.is_empty() && w.bytes().all(|b| b.is_ascii_digit()) {
                    el.set_attribute("width", w).ok();
                    el.set_attribute("alt", "").ok();
                }
            }
        }
        Ok(())
    });

    let anchor_base = base_dir;
    let anchor_roots = roots;
    let anchor = element!("a[href]", move |el| {
        if let Some(href) = el.get_attribute("href") {
            // Unresolved wikilinks: demote to a styled span-like anchor with no href.
            if href == super::wikilink::UNRESOLVED_SENTINEL {
                el.remove_attribute("href");
                el.set_attribute("class", "mdo-wikilink-unresolved").ok();
                return Ok(());
            }
            if is_external_link(&href) {
                return Ok(());
            }
            if is_unsafe_absolute_url(&href) {
                el.remove_attribute("href");
                return Ok(());
            }

            let path_part = local_path_part(&href);
            let is_markdown = path_part.to_ascii_lowercase().ends_with(".md");
            if is_markdown {
                if let Some(abs) = resolve_inside_roots(&anchor_base, path_part, &anchor_roots)
                    .filter(|p| p.is_file())
                {
                    el.remove_attribute("href");
                    el.set_attribute("class", "mdo-link").ok();
                    el.set_attribute("data-path", &abs.to_string_lossy()).ok();
                } else {
                    el.remove_attribute("href");
                }
            } else if has_forbidden_absolute_component(Path::new(path_part)) {
                el.remove_attribute("href");
            } else {
                // Relative non-md link (e.g. ./page.html): disable navigation
                // to prevent WebView from leaving the app. Stash the original
                // href in a data attribute so it remains inspectable.
                el.set_attribute("data-original-href", &href).ok();
                el.remove_attribute("href");
            }
        }
        Ok(())
    });

    match rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![img, anchor],
            ..RewriteStrSettings::default()
        },
    ) {
        Ok(rewritten) => rewritten,
        Err(_) => super::escape_html(html),
    }
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // 1x1 transparent PNG.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89,
    ];

    fn post_process_in_root(html: &str, base_dir: &Path) -> String {
        post_process(html, base_dir, &[base_dir.to_path_buf()])
    }

    // End-to-end: Markdown source containing an allow-listed raw <img> must
    // reach the WebView through the exact same data-URL path as a Markdown
    // image, and `<p align="center">` must survive as a real centering tag.
    #[test]
    fn 生htmlのimgはrender経由でdataURL化されalignが効く() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("icon.png"), PNG).unwrap();

        let md = r#"<p align="center"><img src="icon.png" width="128"></p>"#;
        let rendered = super::super::render::render(md);
        // After render the raw HTML is escaped text, not a real tag.
        assert!(rendered.contains("&lt;img"), "render leaked a raw tag");

        let out = post_process_in_root(&rendered, dir.path());
        assert!(out.contains(r#"align="center""#), "got: {out}");
        assert!(out.contains("src=\"data:image/png;base64,"), "got: {out}");
        assert!(out.contains(r#"width="128""#), "got: {out}");
        assert!(!out.contains("&lt;img"), "img was not reconstructed: {out}");
        assert!(!out.contains("onerror"));
    }

    // End-to-end adversarial: script / unsafe src / unknown tags stay escaped.
    #[test]
    fn 生htmlの敵対ケースはrender経由でも無害化される() {
        let dir = tempdir().unwrap();
        let md = concat!(
            "<script>alert(1)</script>\n\n",
            "<img src=x onerror=alert(1)>\n\n",
            r#"<img src="javascript:alert(1)">"#,
            "\n\n<iframe src=x></iframe>",
        );
        let rendered = super::super::render::render(md);
        let out = post_process_in_root(&rendered, dir.path());
        assert!(!out.contains("onerror"), "got: {out}");
        assert!(!out.contains("<script"), "got: {out}");
        assert!(!out.contains("<iframe"), "got: {out}");
        // The javascript: img is never emitted as a real tag; it stays as inert
        // escaped text (so the literal string is present, but harmless).
        assert!(!out.contains(r#"<img src="javascript:"#), "got: {out}");
        assert!(
            out.contains(r#"&lt;img src="javascript:alert(1)"&gt;"#),
            "got: {out}"
        );
        // script / iframe remain escaped text.
        assert!(out.contains("&lt;script&gt;"), "got: {out}");
        assert!(out.contains("&lt;iframe"), "got: {out}");
    }

    #[test]
    fn 相対画像パスをdataURLに変換する() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("img")).unwrap();
        fs::write(dir.path().join("img/a.png"), PNG).unwrap();

        let html = r#"<p><img src="./img/a.png" alt="a"></p>"#;
        let out = post_process_in_root(html, dir.path());
        assert!(out.contains("src=\"data:image/png;base64,"));
        assert!(!out.contains("./img/a.png"));
    }

    #[test]
    fn project内に収まる親ディレクトリ参照画像はdataURLに変換する() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("repo");
        let docs = root.join("docs");
        let assets = root.join("assets");
        fs::create_dir_all(&docs).unwrap();
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("a.png"), PNG).unwrap();

        let html = r#"<img src="../assets/a.png">"#;
        let out = post_process(html, &docs, &[root]);

        assert!(out.contains("src=\"data:image/png;base64,"));
        assert!(!out.contains("../assets/a.png"));
    }

    #[test]
    fn 外部画像とdataURL画像はsrcを削除する() {
        let dir = tempdir().unwrap();
        let html = r#"<img src="https://example.com/a.png"><img src="data:image/gif;base64,AAAA">"#;
        let out = post_process_in_root(html, dir.path());
        assert!(!out.contains("https://example.com/a.png"));
        assert!(!out.contains("data:image/gif;base64,AAAA"));
        assert!(!out.contains("src="));
    }

    #[test]
    fn 上限を超えるローカル画像は読み込まない() {
        let dir = tempdir().unwrap();
        let image = fs::File::create(dir.path().join("large.png")).unwrap();
        image.set_len((MAX_INLINE_IMAGE_BYTES + 1) as u64).unwrap();

        let out = post_process_in_root(r#"<img src="large.png">"#, dir.path());

        assert!(!out.contains("src="));
        assert!(!out.contains("data:image/png"));
    }

    #[test]
    fn 読めない相対画像はsrcを削除する() {
        let dir = tempdir().unwrap();
        let html = r#"<img src="missing.png">"#;
        let out = post_process_in_root(html, dir.path());
        assert!(!out.contains("src="));
        assert!(!out.contains("missing.png"));
        assert!(!out.contains("data:"));
    }

    #[test]
    fn 拡張子からMIMEを決める() {
        assert_eq!(image_mime(Path::new("a.jpg")), Some("image/jpeg"));
        assert_eq!(image_mime(Path::new("a.jpeg")), Some("image/jpeg"));
        assert_eq!(image_mime(Path::new("a.svg")), Some("image/svg+xml"));
        assert_eq!(image_mime(Path::new("a.txt")), None);
        assert_eq!(image_mime(Path::new("a.webp")), Some("image/webp"));
        assert_eq!(image_mime(Path::new("a.gif")), Some("image/gif"));
    }

    // Minimal valid SVG document.
    const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#;

    #[test]
    fn ローカルsvgをdataURLに変換する() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("icon.svg"), SVG).unwrap();

        let html = r#"<img src="icon.svg" alt="i">"#;
        let out = post_process_in_root(html, dir.path());
        assert!(
            out.contains("src=\"data:image/svg+xml;base64,"),
            "got: {out}"
        );
        assert!(!out.contains("\"icon.svg\""));
    }

    #[test]
    fn project外のsvgはsrcを削除する() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("repo");
        let docs = root.join("docs");
        fs::create_dir_all(&docs).unwrap();
        fs::write(dir.path().join("secret.svg"), SVG).unwrap();

        let html = r#"<img src="../../secret.svg">"#;
        let out = post_process(html, &docs, &[root]);
        assert!(!out.contains("secret.svg"));
        assert!(!out.contains("src="));
    }

    #[test]
    fn 上限を超えるsvgは読み込まない() {
        let dir = tempdir().unwrap();
        let f = fs::File::create(dir.path().join("big.svg")).unwrap();
        f.set_len((MAX_INLINE_IMAGE_BYTES + 1) as u64).unwrap();
        let out = post_process_in_root(r#"<img src="big.svg">"#, dir.path());
        assert!(!out.contains("src="));
        assert!(!out.contains("data:image/svg"));
    }

    #[test]
    fn width用altマーカーをwidth属性に変換する() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.png"), PNG).unwrap();
        let html = r#"<img src="a.png" alt="mdo-width-400">"#;
        let out = post_process_in_root(html, dir.path());
        assert!(out.contains(r#"width="400""#), "got: {out}");
        assert!(out.contains(r#"alt="""#), "got: {out}");
        assert!(!out.contains("mdo-width-400"), "got: {out}");
        assert!(out.contains("src=\"data:image/png;base64,"));
    }

    #[test]
    fn 非数値のwidthマーカー風altはそのまま() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.png"), PNG).unwrap();
        // Not a valid width marker (non-digits) → alt left untouched.
        let html = r#"<img src="a.png" alt="mdo-width-abc">"#;
        let out = post_process_in_root(html, dir.path());
        assert!(!out.contains("width="), "got: {out}");
        assert!(out.contains(r#"alt="mdo-width-abc""#), "got: {out}");
    }

    #[test]
    fn パーセントエンコードされた空白入り画像を解決する() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("my file.png"), PNG).unwrap();
        // Renderer emits spaces as %20 in src; post_process must decode.
        let html = r#"<img src="my%20file.png">"#;
        let out = post_process_in_root(html, dir.path());
        assert!(out.contains("src=\"data:image/png;base64,"), "got: {out}");
    }

    #[test]
    fn パーセントエンコードされた括弧入り画像を解決する() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a(b).png"), PNG).unwrap();
        let html = r#"<img src="a%28b%29.png">"#;
        let out = post_process_in_root(html, dir.path());
        assert!(out.contains("src=\"data:image/png;base64,"), "got: {out}");
    }

    #[test]
    fn 相対mdリンクは内部リンクに書き換える() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("other.md"), "# Other").unwrap();
        let html = r#"<a href="./other.md">x</a>"#;
        let out = post_process_in_root(html, dir.path());
        assert!(out.contains(r#"class="mdo-link""#));
        assert!(out.contains("data-path="));
        assert!(out.contains("other.md")); // 絶対パス内に残る
        assert!(!out.contains("href="));
    }

    #[test]
    fn httpリンクはそのまま() {
        let dir = tempdir().unwrap();
        let html = r#"<a href="https://example.com">x</a><a href="http://e.org/b.md">y</a>"#;
        let out = post_process_in_root(html, dir.path());
        assert!(out.contains(r#"href="https://example.com""#));
        assert!(out.contains(r#"href="http://e.org/b.md""#));
        assert!(!out.contains("mdo-link"));
    }

    #[test]
    fn md以外の相対リンクはhrefを除去してdata属性に退避する() {
        let dir = tempdir().unwrap();
        let html = r#"<a href="./page.html">x</a>"#;
        let out = post_process_in_root(html, dir.path());
        // href attribute is stripped to prevent WebView navigation.
        assert!(!out.contains(r#" href="#));
        assert!(!out.contains("mdo-link"));
        // Original URL is preserved in a data attribute.
        assert!(out.contains(r#"data-original-href="./page.html""#));
    }

    #[test]
    fn アンカーリンクはそのまま() {
        let dir = tempdir().unwrap();
        let html = r##"<a href="#section">jump</a>"##;
        let out = post_process_in_root(html, dir.path());
        assert!(out.contains(r##"href="#section""##));
        assert!(!out.contains("data-original-href"));
    }

    #[test]
    fn md以外の相対リンク複数パターンを無効化する() {
        let dir = tempdir().unwrap();
        let html = r#"<a href="./file.html">a</a><a href="image.png">b</a><a href="doc.pdf">c</a>"#;
        let out = post_process_in_root(html, dir.path());
        assert!(!out.contains(r#" href=""#));
        assert!(out.contains(r#"data-original-href="./file.html""#));
        assert!(out.contains(r#"data-original-href="image.png""#));
        assert!(out.contains(r#"data-original-href="doc.pdf""#));
    }

    #[test]
    fn project外へ出る画像パスはsrcを削除する() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("repo");
        let docs = root.join("docs");
        fs::create_dir_all(&docs).unwrap();
        fs::write(dir.path().join("secret.png"), PNG).unwrap();

        let html = r#"<img src="../../secret.png">"#;
        let out = post_process(html, &docs, &[root]);

        assert!(!out.contains("secret.png"));
        assert!(!out.contains("src="));
    }

    #[test]
    #[cfg(unix)]
    fn project外へのsymlink画像はsrcを削除する() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret.png"), PNG).unwrap();
        symlink(
            outside.path().join("secret.png"),
            dir.path().join("secret-link.png"),
        )
        .unwrap();

        let html = r#"<img src="secret-link.png">"#;
        let out = post_process(html, dir.path(), &[dir.path().to_path_buf()]);

        assert!(!out.contains("secret-link.png"));
        assert!(!out.contains("data:image/png"));
        assert!(!out.contains("src="));
    }

    #[test]
    fn 未解決wikilinksentinelはhref除去とクラス付与をする() {
        let dir = tempdir().unwrap();
        let sentinel = super::super::wikilink::UNRESOLVED_SENTINEL;
        let html = format!(r#"<a href="{sentinel}">broken link</a>"#);
        let out = post_process_in_root(&html, dir.path());
        assert!(!out.contains("href="), "got: {out}");
        assert!(
            out.contains(r#"class="mdo-wikilink-unresolved""#),
            "got: {out}"
        );
        assert!(out.contains("broken link"), "got: {out}");
    }

    #[test]
    fn project外へのmdリンクはhrefを削除する() {
        let dir = tempdir().unwrap();
        let html = r#"<a href="../outside.md">x</a>"#;
        let out = post_process(html, dir.path(), &[dir.path().to_path_buf()]);

        assert!(!out.contains("href="));
        assert!(!out.contains("data-path="));
        assert!(out.contains(">x</a>"));
    }
}
