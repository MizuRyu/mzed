//! Obsidian-style `[[wikilink]]` pre-processor.
//!
//! Converts `[[...]]` in Markdown source to standard Markdown links **before**
//! the source reaches pulldown-cmark. Resolved links become normal relative
//! `[alias](path.md)` links that the existing `post_process` pipeline turns
//! into `class="mdo-link"` internal anchors. Unresolved ones get the
//! [`UNRESOLVED_SENTINEL`] href so `post_process` can demote them to a styled
//! `<span>`.
//!
//! Image embeds `![[image.ext|400]]` are rewritten to standard Markdown images
//! `![alt](path)` so they ride the existing `post_process` data-URL pipeline;
//! non-image embeds (e.g. `![[note.md]]`) are left unchanged.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// Sentinel href placed in unresolved wikilinks so `post_process` can
/// recognise and style them without a real href.
pub(super) const UNRESOLVED_SENTINEL: &str = "#__mdo-wiki-unresolved__";

/// Alt-text side-channel prefix carrying an image embed's display width from
/// this pre-processor to `post_process` (Markdown image syntax has no width).
/// `post_process` turns `alt="mdo-width-400"` into `width="400"`.
pub(super) const WIDTH_ALT_PREFIX: &str = "mdo-width-";

/// Image extensions eligible for `![[...]]` embed rewriting (case-insensitive).
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg"];

fn is_image_target(target: &str) -> bool {
    Path::new(target)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| IMAGE_EXTS.contains(&e.as_str()))
}

// ── Parsing ──────────────────────────────────────────────────────────────────

struct Wikilink<'a> {
    /// File target (may include sub-path). No heading, no alias.
    target: &'a str,
    /// Raw text after `#` (if present).
    heading: Option<&'a str>,
    /// Display text (alias if provided, otherwise target).
    alias: &'a str,
}

/// Parse the interior of `[[inner]]` (i.e. `inner` without the brackets).
fn parse_inner(inner: &str) -> Wikilink<'_> {
    // Split on first `|` to separate target+heading from alias.
    let (before_pipe, alias_raw) = match inner.find('|') {
        Some(pos) => (&inner[..pos], inner[pos + 1..].trim()),
        None => (inner, ""),
    };
    // Split on first `#` to separate target from heading.
    let (target, heading) = match before_pipe.find('#') {
        Some(pos) => (
            before_pipe[..pos].trim(),
            Some(before_pipe[pos + 1..].trim()),
        ),
        None => (before_pipe.trim(), None),
    };
    let alias = if alias_raw.is_empty() {
        target
    } else {
        alias_raw
    };
    Wikilink {
        target,
        heading,
        alias,
    }
}

// ── Resolution ────────────────────────────────────────────────────────────────

fn ensure_md_extension(target: &str) -> String {
    if Path::new(target).extension().is_some() {
        target.to_string()
    } else {
        format!("{}.md", target)
    }
}

fn has_forbidden_component(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, Component::RootDir | Component::Prefix(_)))
}

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    ".hg",
    ".svn",
    "__pycache__",
    ".cache",
];

/// File name → the first file of that name under each root, in root order.
/// why: one walk serves every basename lookup of a document.
type BasenameIndex = HashMap<String, Vec<PathBuf>>;

#[cfg(test)]
thread_local! {
    static INDEX_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn build_basename_index(canon_roots: &[PathBuf]) -> BasenameIndex {
    #[cfg(test)]
    INDEX_BUILDS.with(|n| n.set(n.get() + 1));
    let mut index = BasenameIndex::new();
    for root in canon_roots {
        let mut first = HashMap::new();
        index_dir(root, 20, &mut first);
        for (name, path) in first {
            index.entry(name).or_default().push(path);
        }
    }
    index
}

/// Walk `dir` depth-first (depth-limited to `max_depth`), keeping the first
/// file seen per name: a directory's own files, then its subdirectories in
/// `read_dir` order. Skips noise directories.
fn index_dir(dir: &Path, max_depth: u8, first: &mut HashMap<String, PathBuf>) {
    if max_depth == 0 || !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut sub_dirs = Vec::new();
    for entry in entries.flatten() {
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if ft.is_dir() {
            if !SKIP_DIRS.contains(&name_str.as_ref()) {
                sub_dirs.push(entry.path());
            }
        } else if ft.is_file() {
            first
                .entry(name_str.into_owned())
                .or_insert_with(|| entry.path());
        }
    }
    for sub in sub_dirs {
        index_dir(&sub, max_depth - 1, first);
    }
}

/// The roots of one `preprocess_wikilinks` call, resolved once, and the
/// basename index built from exactly those roots.
struct Resolver {
    canon_roots: Vec<PathBuf>,
    index: OnceCell<BasenameIndex>,
}

impl Resolver {
    fn new(roots: &[PathBuf]) -> Self {
        Self {
            canon_roots: roots.iter().filter_map(|r| r.canonicalize().ok()).collect(),
            index: OnceCell::new(),
        }
    }
}

/// Resolve a wikilink `target` to a canonical absolute path inside the roots.
///
/// Resolution order (Obsidian-compatible):
///   (a) Relative from `base_dir`.
///   (b) Relative from each root.
///   (c) Basename match anywhere inside roots (skipped when no root resolved).
fn resolve_wikilink_path(target: &str, base_dir: &Path, resolver: &Resolver) -> Option<PathBuf> {
    if target.is_empty() {
        return None;
    }
    let with_ext = ensure_md_extension(target);
    let rel = Path::new(&with_ext);
    if has_forbidden_component(rel) {
        return None;
    }

    let canon_roots = &resolver.canon_roots;

    let inside_roots = |p: &Path| -> bool {
        canon_roots.is_empty() || canon_roots.iter().any(|r| p.starts_with(r))
    };

    // (a) Relative from base_dir.
    if let Ok(base) = base_dir.canonicalize() {
        if let Ok(canon) = base.join(&with_ext).canonicalize() {
            if inside_roots(&canon) && canon.is_file() {
                return Some(canon);
            }
        }
    }

    // (b) Relative from each root.
    for root in canon_roots {
        if let Ok(canon) = root.join(&with_ext).canonicalize() {
            if inside_roots(&canon) && canon.is_file() {
                return Some(canon);
            }
        }
    }

    // (c) Basename search.
    if canon_roots.is_empty() {
        return None;
    }
    let basename = Path::new(&with_ext).file_name()?.to_str()?;
    let index = resolver
        .index
        .get_or_init(|| build_basename_index(canon_roots));
    for found in index.get(basename).into_iter().flatten() {
        if let Ok(canon) = found.canonicalize() {
            if inside_roots(&canon) {
                return Some(canon);
            }
        }
    }

    None
}

// ── Path utilities ────────────────────────────────────────────────────────────

/// Compute a POSIX-style relative path from `from_dir` to `to_file`.
/// Both must be canonical absolute paths.
fn relative_path_from(from_dir: &Path, to_file: &Path) -> String {
    let from: Vec<_> = from_dir.components().collect();
    let to: Vec<_> = to_file.components().collect();
    let common = from
        .iter()
        .zip(to.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let up = from.len() - common;
    let mut buf = PathBuf::new();
    for _ in 0..up {
        buf.push("..");
    }
    for comp in &to[common..] {
        buf.push(comp.as_os_str());
    }
    // Convert to forward-slash string (macOS is always POSIX, but be explicit).
    buf.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

// ── Text escaping for Markdown output ────────────────────────────────────────

/// Escape characters that would break a Markdown link label `[…]`.
fn escape_link_text(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

/// Percent-encode characters that are not safe inside a Markdown URL `(…)`.
fn escape_url(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            ' ' => out.push_str("%20"),
            _ => out.push(ch),
        }
    }
    out
}

// ── Main entry point ─────────────────────────────────────────────────────────

/// Pre-process `source`, replacing `[[...]]` wikilinks with standard Markdown.
///
/// - Resolved: `[alias](relative.md)` or `[alias](relative.md#heading-slug)`.
/// - Unresolved: `[alias](`[`UNRESOLVED_SENTINEL`]`)` so `post_process` can
///   convert the rendered `<a>` into a styled `<span>`.
/// - Embed wikilinks `![[...]]`: left unchanged (out of scope for v1).
pub fn preprocess_wikilinks(source: &str, base_dir: &Path, roots: &[PathBuf]) -> String {
    if !source.contains("[[") {
        return source.to_string();
    }

    let mut out = String::with_capacity(source.len() + 64);
    let mut rest = source;
    let mut closes = CloseScanner::new(source);
    let resolver = Resolver::new(roots);

    while !rest.is_empty() {
        // Advance to the next `[` or `!`.
        let next = rest
            .char_indices()
            .find(|(_, c)| *c == '[' || *c == '!')
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        out.push_str(&rest[..next]);
        rest = &rest[next..];

        if rest.is_empty() {
            break;
        }

        // ── Embed `![[…]]`. Image embeds become standard Markdown images;
        //    everything else (e.g. `![[note.md]]`) passes through unchanged. ──
        let pos = source.len() - rest.len();
        if rest.starts_with("![[") {
            if let Some((close_at, has_bracket)) = closes.find(pos + 3) {
                let close = close_at - pos - 3;
                let end = 3 + close + 2;
                let inner = &rest[3..3 + close];
                // Reject nested-bracket content; only rewrite image embeds.
                let is_image = !has_bracket
                    && is_image_target(inner.split('|').next().unwrap_or(inner).trim());
                if is_image {
                    out.push_str(&build_embed_replacement(inner, base_dir, &resolver));
                } else {
                    out.push_str(&rest[..end]);
                }
                rest = &rest[end..];
                continue;
            }
        }

        // ── Wikilink `[[…]]`. ─────────────────────────────────────────────────
        if rest.starts_with("[[") {
            if let Some((close_at, has_bracket)) = closes.find(pos + 2) {
                let close = close_at - pos - 2;
                let inner = &rest[2..2 + close];
                // Reject empty or nested-bracket content.
                if !inner.is_empty() && !has_bracket {
                    let wl = parse_inner(inner);
                    let replacement = build_replacement(&wl, base_dir, &resolver);
                    out.push_str(&replacement);
                    rest = &rest[2 + close + 2..];
                    continue;
                }
            }
        }

        // Not a wikilink: emit one char and continue.
        let ch = rest.chars().next().expect("non-empty");
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }

    out
}

#[cfg(test)]
thread_local! {
    /// Bytes searched by `CloseScanner`.
    static CLOSE_SCANNED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Answers "first `]]` at or after `start`" for non-decreasing `start`,
/// reusing the previous answer while it still applies, so the total search
/// work stays linear in the source length.
struct CloseScanner<'a> {
    src: &'a str,
    /// The last search start; `close` and `last_bracket` describe `src[from..]`.
    from: usize,
    close: Option<usize>,
    /// Last `[` or `]` in `src[from..close]`.
    last_bracket: Option<usize>,
}

impl<'a> CloseScanner<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            from: usize::MAX,
            close: None,
            last_bracket: None,
        }
    }

    /// Absolute offset of the first `]]` at or after `start`, and whether
    /// `src[start..close]` contains `[` or `]`.
    fn find(&mut self, start: usize) -> Option<(usize, bool)> {
        let reusable = start >= self.from && self.close.is_none_or(|c| start <= c);
        if !reusable {
            self.from = start;
            self.close = self.src[start..].find("]]").map(|i| start + i);
            let end = self.close.unwrap_or(start);
            #[cfg(test)]
            CLOSE_SCANNED.with(|n| {
                let found = self.close.map_or(self.src.len(), |c| c + 2);
                n.set(n.get() + (found - start) + (end - start))
            });
            self.last_bracket = self.src[start..end].rfind(['[', ']']).map(|i| start + i);
        }
        let close = self.close?;
        Some((close, self.last_bracket.is_some_and(|b| b >= start)))
    }
}

fn build_replacement(wl: &Wikilink<'_>, base_dir: &Path, resolver: &Resolver) -> String {
    if wl.target.is_empty() {
        // Degenerate: leave as plain text.
        return format!(
            "[[{}]]",
            if let Some(h) = wl.heading {
                format!("{}#{}", wl.target, h)
            } else {
                wl.target.to_string()
            }
        );
    }

    let alias_escaped = escape_link_text(wl.alias);

    match resolve_wikilink_path(wl.target, base_dir, resolver) {
        Some(resolved) => {
            let base_canon = base_dir.canonicalize().ok();
            let file_path = base_canon
                .as_deref()
                .map(|b| relative_path_from(b, &resolved))
                .unwrap_or_else(|| resolved.to_string_lossy().into_owned());
            let mut url = escape_url(&file_path);
            if let Some(h) = wl.heading {
                let slug = super::toc::slugify(h);
                if !slug.is_empty() {
                    url.push('#');
                    url.push_str(&slug);
                }
            }
            format!("[{}]({})", alias_escaped, url)
        }
        None => {
            // Unresolved: sentinel href; post_process will convert to span.
            format!("[{}]({})", alias_escaped, UNRESOLVED_SENTINEL)
        }
    }
}

/// Build the replacement for an image embed `![[target|spec]]`.
///
/// - `spec` all-digits → display width, carried to `post_process` via the
///   `alt="mdo-width-<N>"` side-channel.
/// - `spec` non-empty non-numeric → alt text.
/// - Resolved → `![alt](relative-path)` (rides the data-URL pipeline).
/// - Unresolved → demoted to a styled `.mdo-wikilink-unresolved` span (same as
///   an unresolved link) showing the alt or filename.
fn build_embed_replacement(inner: &str, base_dir: &Path, resolver: &Resolver) -> String {
    let (target, spec) = match inner.find('|') {
        Some(pos) => (inner[..pos].trim(), inner[pos + 1..].trim()),
        None => (inner.trim(), ""),
    };
    let is_width = !spec.is_empty() && spec.bytes().all(|b| b.is_ascii_digit());

    match resolve_wikilink_path(target, base_dir, resolver) {
        Some(resolved) => {
            let base_canon = base_dir.canonicalize().ok();
            let file_path = base_canon
                .as_deref()
                .map(|b| relative_path_from(b, &resolved))
                .unwrap_or_else(|| resolved.to_string_lossy().into_owned());
            let url = escape_url(&file_path);
            let alt = if is_width {
                format!("{}{}", WIDTH_ALT_PREFIX, spec)
            } else {
                escape_link_text(spec)
            };
            format!("![{}]({})", alt, url)
        }
        None => {
            // Unresolved: show alt-or-filename as a styled non-link span.
            let display = if !spec.is_empty() && !is_width {
                spec
            } else {
                target
            };
            format!("[{}]({})", escape_link_text(display), UNRESOLVED_SENTINEL)
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn pp(source: &str, dir: &Path) -> String {
        preprocess_wikilinks(source, dir, &[dir.to_path_buf()])
    }

    #[test]
    fn 単純なwikilink_解決済み() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("target.md"), "# Target").unwrap();
        let out = pp("[[target]]", dir.path());
        assert!(out.contains("[target]("), "got: {out}");
        assert!(out.contains("target.md"), "got: {out}");
        assert!(!out.contains("[["), "got: {out}");
    }

    #[test]
    fn alias付きwikilink_解決済み() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("target.md"), "").unwrap();
        let out = pp("[[target|My Alias]]", dir.path());
        assert!(out.contains("[My Alias]("), "got: {out}");
        assert!(out.contains("target.md"), "got: {out}");
    }

    #[test]
    fn 未解決wikilinkはsentinel_href() {
        let dir = tempdir().unwrap();
        let out = pp("[[missing-file]]", dir.path());
        assert!(out.contains(UNRESOLVED_SENTINEL), "got: {out}");
        assert!(out.contains("[missing-file]("), "got: {out}");
    }

    #[test]
    fn heading付きwikilink_解決済み() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("target.md"), "# Section Header").unwrap();
        let out = pp("[[target#Section Header]]", dir.path());
        assert!(out.contains("target.md#section-header"), "got: {out}");
        // Default alias is just the target (no heading in display).
        assert!(out.contains("[target]("), "got: {out}");
    }

    #[test]
    fn heading_alias付きwikilink() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("target.md"), "").unwrap();
        let out = pp("[[target#heading|My Alias]]", dir.path());
        assert!(out.contains("[My Alias]("), "got: {out}");
        assert!(out.contains("#heading"), "got: {out}");
    }

    #[test]
    fn 非画像embed_wikilinkは変更しない() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("image.md"), "").unwrap();
        let out = pp("![[image.md]]", dir.path());
        assert_eq!(out, "![[image.md]]");
    }

    #[test]
    fn 画像embedを標準markdown画像に変換する() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pic.png"), "x").unwrap();
        let out = pp("![[pic.png]]", dir.path());
        assert!(out.starts_with("![]("), "got: {out}");
        assert!(out.contains("pic.png"), "got: {out}");
        assert!(!out.contains("![["), "got: {out}");
    }

    #[test]
    fn svg画像embedを変換する() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("icon.svg"), "x").unwrap();
        let out = pp("![[icon.svg]]", dir.path());
        assert!(out.contains("icon.svg"), "got: {out}");
        assert!(!out.contains("![["), "got: {out}");
    }

    #[test]
    fn 幅指定画像embedはaltマーカーを埋め込む() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pic.png"), "x").unwrap();
        let out = pp("![[pic.png|400]]", dir.path());
        assert!(out.contains("![mdo-width-400]("), "got: {out}");
        assert!(out.contains("pic.png"), "got: {out}");
    }

    #[test]
    fn 説明付き画像embedはaltテキストになる() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pic.png"), "x").unwrap();
        let out = pp("![[pic.png|図の説明]]", dir.path());
        assert!(out.contains("![図の説明]("), "got: {out}");
        assert!(!out.contains("mdo-width"), "got: {out}");
    }

    #[test]
    fn 未解決の画像embedはunresolvedにフォールバックする() {
        let dir = tempdir().unwrap();
        let out = pp("![[missing.png]]", dir.path());
        assert!(out.contains(UNRESOLVED_SENTINEL), "got: {out}");
        // Non-image embed syntax should not leak through.
        assert!(!out.contains("![["), "got: {out}");
    }

    #[test]
    fn 空白入りファイル名の画像embedはエスケープされる() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("my pic.png"), "x").unwrap();
        let out = pp("![[my pic.png]]", dir.path());
        // Space percent-encoded so pulldown-cmark keeps a single URL token.
        assert!(out.contains("my%20pic.png"), "got: {out}");
        assert!(!out.contains("![["), "got: {out}");
    }

    #[test]
    fn roots外の画像embedは解決しない() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret.png"), "x").unwrap();
        let out = preprocess_wikilinks(
            "![[../outside/secret.png]]",
            dir.path(),
            &[dir.path().to_path_buf()],
        );
        assert!(out.contains(UNRESOLVED_SENTINEL), "got: {out}");
        // Demoted to an unresolved span, not an image embed.
        assert!(!out.contains("!["), "got: {out}");
    }

    #[test]
    fn wikilinkなし文字列は変更しない() {
        let dir = tempdir().unwrap();
        let source = "# Hello\n\nNo wikilinks here.\n";
        let out = pp(source, dir.path());
        assert_eq!(out, source);
    }

    #[test]
    fn basename検索でサブディレクトリのファイルを解決する() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("deep.md"), "").unwrap();
        let out = pp("[[deep]]", dir.path());
        assert!(out.contains("deep.md"), "got: {out}");
        assert!(!out.contains(UNRESOLVED_SENTINEL), "got: {out}");
    }

    #[test]
    fn roots外のファイルは解決しない() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret.md"), "").unwrap();
        // root is `dir`; target traverses outside → unresolved.
        let out = preprocess_wikilinks(
            "[[../outside/secret]]",
            dir.path(),
            &[dir.path().to_path_buf()],
        );
        assert!(out.contains(UNRESOLVED_SENTINEL), "got: {out}");
    }

    #[test]
    fn 複数wikiliinksを同一ソースで処理する() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "").unwrap();
        fs::write(dir.path().join("b.md"), "").unwrap();
        let out = pp("[[a]] and [[b]] and [[missing]]", dir.path());
        assert!(out.contains("a.md"), "got: {out}");
        assert!(out.contains("b.md"), "got: {out}");
        assert!(out.contains(UNRESOLVED_SENTINEL), "got: {out}");
    }

    #[test]
    fn alias内の角括弧はエスケープされる() {
        let dir = tempdir().unwrap();
        // alias containing `]` — rejected because inner contains `]`
        // `[[target|foo]bar]]` → inner = "target|foo" (close at first `]]`... wait no)
        // Actually `[[target|foo]bar]]`: rest[2..] = "target|foo]bar]]", find("]]") = finds
        // "]]" at position... "target|foo]bar]]" → pos 14. inner = "target|foo]bar"
        // inner.contains(']') = true → skip. Emitted char by char.
        // So the `[[` is emitted as literal text. Good.
        let out = pp("[[target|foo]bar]]", dir.path());
        // Treated as literal text since inner contains `]`.
        assert!(!out.contains("<"), "got: {out}");
    }

    #[test]
    fn 空のwikilink_は変更しない() {
        let dir = tempdir().unwrap();
        // `[[]]` inner = "" → empty, skip.
        let out = pp("[[]]", dir.path());
        // Emitted char by char as `[[]]`.
        assert_eq!(out, "[[]]");
    }

    fn index_builds() -> usize {
        INDEX_BUILDS.with(|n| n.get())
    }

    #[test]
    fn 未解決が多数でもbasename索引の構築は1回() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub/deeper")).unwrap();
        fs::write(dir.path().join("sub/deeper/found.md"), "").unwrap();
        let source: String = (0..100)
            .map(|i| format!("[[missing-{i}]] "))
            .chain(["[[found]] ![[nope.png]]".to_string()])
            .collect();

        let before = index_builds();
        let out = pp(&source, dir.path());
        assert_eq!(index_builds() - before, 1);
        assert_eq!(out.matches(UNRESOLVED_SENTINEL).count(), 101);
        assert!(out.contains("sub/deeper/found.md"), "got: {out}");
    }

    #[test]
    fn 相対とroot直下で解決できればbasename索引を作らない() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("a.md"), "").unwrap();
        fs::write(dir.path().join("sub/b.md"), "").unwrap();

        let before = index_builds();
        let out = pp("[[a]] [[sub/b]]", dir.path());
        assert_eq!(index_builds() - before, 0);
        assert!(!out.contains(UNRESOLVED_SENTINEL), "got: {out}");
    }

    #[test]
    fn basename同名は浅い階層のファイルを優先する() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("other")).unwrap();
        fs::create_dir_all(dir.path().join("x/y")).unwrap();
        fs::write(dir.path().join("x/y/dup.md"), "").unwrap();
        fs::write(dir.path().join("x/dup.md"), "").unwrap();
        // base_dir にも root 直下にも無いので basename 検索になる。
        let out = preprocess_wikilinks(
            "[[dup]]",
            &dir.path().join("other"),
            &[dir.path().to_path_buf()],
        );
        assert!(out.contains("(../x/dup.md)"), "got: {out}");
    }

    #[test]
    fn 入れ子の角括弧は内側のwikilinkだけ変換する() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "").unwrap();
        let out = pp("[[[[a]] x [[b]c]] ![[[[a]]", dir.path());
        assert_eq!(out, "[[[a](a.md) x [[b]c]] ![[[[a]]");
    }

    #[test]
    fn 閉じない角括弧の連続でも探索量は入力長に比例する() {
        let dir = tempdir().unwrap();
        for n in [50_000, 100_000, 200_000] {
            let source = "[".repeat(n) + "[[a]]";
            let before = CLOSE_SCANNED.with(|c| c.get());
            let out = pp(&source, dir.path());
            let scanned = CLOSE_SCANNED.with(|c| c.get()) - before;
            assert!(
                out.ends_with(&format!("[a]({UNRESOLVED_SENTINEL})")),
                "n={n}"
            );
            assert!(scanned <= 2 * source.len(), "n={n}: scanned {scanned}");
        }
    }

    #[test]
    fn rootsが空ならbasename索引を使わない() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/deep.md"), "").unwrap();
        fs::write(dir.path().join("near.md"), "").unwrap();

        let before = index_builds();
        let out = preprocess_wikilinks("[[deep]] [[near]]", dir.path(), &[]);
        assert_eq!(index_builds() - before, 0);
        // base_dir からの相対解決は従来どおり効く。
        assert!(out.contains("(near.md)"), "got: {out}");
        assert!(
            out.contains(&format!("[deep]({UNRESOLVED_SENTINEL})")),
            "got: {out}"
        );
    }

    #[test]
    fn basename同名は先に渡したrootを優先する() {
        let dir = tempdir().unwrap();
        let (r1, r2) = (dir.path().join("r1"), dir.path().join("r2"));
        fs::create_dir_all(r1.join("x/y")).unwrap();
        fs::create_dir_all(r2.join("z")).unwrap();
        fs::write(r1.join("x/y/dup.md"), "").unwrap(); // r1 側は深い
        fs::write(r2.join("z/dup.md"), "").unwrap();

        let out = preprocess_wikilinks("[[dup]]", dir.path(), &[r1.clone(), r2.clone()]);
        assert!(out.contains("(r1/x/y/dup.md)"), "got: {out}");
        let out = preprocess_wikilinks("[[dup]]", dir.path(), &[r2, r1]);
        assert!(out.contains("(r2/z/dup.md)"), "got: {out}");
    }

    #[test]
    fn basename同名の別枝は深さでなく走査順で決まる() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("a/x/y")).unwrap();
        fs::create_dir_all(root.join("b")).unwrap();
        fs::write(root.join("a/x/y/dup.md"), "").unwrap();
        fs::write(root.join("b/dup.md"), "").unwrap();
        fs::create_dir_all(root.join("other")).unwrap();

        // 深さ優先: read_dir で先に出た枝の中を、浅い別枝より先に探す。
        let first_branch = fs::read_dir(root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|n| n == "a" || n == "b")
            .unwrap();
        let expected = if first_branch == "a" {
            "(../a/x/y/dup.md)"
        } else {
            "(../b/dup.md)"
        };
        let out = preprocess_wikilinks("[[dup]]", &root.join("other"), &[root.to_path_buf()]);
        assert!(out.contains(expected), "got: {out}");
    }

    #[test]
    fn basename候補のcanonicalizeに失敗したら次の候補へ進む() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/dup.md"), "").unwrap();
        let mut index = BasenameIndex::new();
        index.insert(
            "dup.md".into(),
            vec![root.join("gone/dup.md"), root.join("sub/dup.md")],
        );
        let resolver = Resolver {
            canon_roots: vec![root.clone()],
            index: OnceCell::from(index),
        };

        let found = resolve_wikilink_path("dup", &root.join("sub/.."), &resolver);
        assert_eq!(found, Some(root.join("sub/dup.md")));
    }

    #[test]
    fn 日本語ファイル名を解決する() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("メモ.md"), "").unwrap();
        let out = pp("[[メモ]]", dir.path());
        assert!(out.contains("メモ.md"), "got: {out}");
        assert!(!out.contains(UNRESOLVED_SENTINEL), "got: {out}");
    }
}
