//! Allow-listed reconstruction of a safe raw-HTML subset (`<img>` centric).
//!
//! mzed fully escapes user raw HTML before re-parsing (the XSS-defence
//! invariant). As a result common GitHub README constructs such as
//! `<p align="center"><img src="assets/icon.png" width="128"></p>` show up as
//! literal text. This module re-introduces a *minimal* safe subset the same way
//! GitHub does: it never un-escapes the original bytes. Instead it recognises
//! the escaped patterns (`&lt;img …&gt;`) in the already-rendered HTML, parses
//! their attributes, validates each against an allow-list, and rebuilds a brand
//! new tag string with freshly re-escaped attribute values. Anything not on the
//! allow-list (unknown tags, unknown attributes, unsafe `src`) stays escaped.
//!
//! Because the output is self-constructed from validated pieces, attribute
//! injection (`onerror=`, `<script>`, `javascript:` …) cannot structurally get
//! through: those tokens are simply never emitted.
//!
//! The reconstructed `<img>` tags are ordinary tags, so the existing lol_html
//! `post_process` image handler converts local `src` to `data:` URLs and drops
//! anything outside the roots — identical validation and lightbox behaviour to
//! Markdown images.

use super::security::safe_image_url;

/// A parsed HTML tag token (open, close, or void).
struct Tag {
    name: String,
    closing: bool,
    /// Raw attribute values still carry HTML entities; decoded on extraction.
    attrs: Vec<(String, Option<String>)>,
}

/// Reconstruct the allow-listed raw-HTML subset from `html`.
///
/// Scans for escaped tags (`&lt;…&gt;`) only; real tags (literal `<`) produced
/// by the Markdown renderer are left untouched.
pub(super) fn reconstruct_allowed(html: &str) -> String {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while i < bytes.len() {
        if html[i..].starts_with("&lt;") {
            // Only treat as a tag when `&lt;` is immediately followed by a tag
            // name or a closing slash; otherwise it is a literal `<` in text.
            let after = &html[i + 4..];
            let looks_like_tag = after
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '/');
            if looks_like_tag {
                if let Some(end_rel) = after.find("&gt;") {
                    let inner = &after[..end_rel];
                    // The inner text is still escaped by the render pipeline
                    // (`&amp;` for `&`). Undo exactly that one level to recover
                    // the original raw tag contents.
                    let raw = undo_render_escape(inner);
                    if let Some(rebuilt) = parse_tag(&raw).and_then(rebuild) {
                        out.push_str(&rebuilt);
                        i = i + 4 + end_rel + 4; // skip `&lt;` … `&gt;`
                        continue;
                    }
                }
            }
            // Not an allow-listed tag: emit the escaped `&lt;` verbatim.
            out.push_str("&lt;");
            i += 4;
            continue;
        }
        let ch = html[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Reverse the render pipeline's text escaping (`html::push_html`), which only
/// emits `&amp;`, `&lt;`, `&gt;`. `&amp;` is decoded last so it cannot manufacture
/// a spurious `&lt;`/`&gt;`.
fn undo_render_escape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Parse a single tag's inner text (between `<` and `>`, brackets excluded).
fn parse_tag(inner: &str) -> Option<Tag> {
    let inner = inner.trim();
    let closing = inner.starts_with('/');
    let name_start = if closing { 1 } else { 0 };
    let rest = &inner[name_start..];

    // Tag name: leading letters/digits.
    let name_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric()))
        .unwrap_or(rest.len());
    let name = rest[..name_end].to_ascii_lowercase();
    if name.is_empty() {
        return None;
    }
    let mut attrs = Vec::new();
    if !closing {
        let mut pos = name_start + name_end;
        let b = inner.as_bytes();
        while pos < inner.len() {
            // Skip whitespace and stray self-closing slashes.
            while pos < inner.len() && (b[pos].is_ascii_whitespace() || b[pos] == b'/') {
                pos += 1;
            }
            if pos >= inner.len() {
                break;
            }
            // Attribute name: letters, digits, '-'.
            let astart = pos;
            while pos < inner.len()
                && (b[pos].is_ascii_alphanumeric() || b[pos] == b'-' || b[pos] == b'_')
            {
                pos += 1;
            }
            if pos == astart {
                // Not a valid attribute char; bail to avoid infinite loops.
                pos += 1;
                continue;
            }
            let aname = inner[astart..pos].to_ascii_lowercase();
            // Optional `= value`.
            let mut ws = pos;
            while ws < inner.len() && b[ws].is_ascii_whitespace() {
                ws += 1;
            }
            if ws < inner.len() && b[ws] == b'=' {
                pos = ws + 1;
                while pos < inner.len() && b[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                let value = if pos < inner.len() && (b[pos] == b'"' || b[pos] == b'\'') {
                    let quote = b[pos];
                    pos += 1;
                    let vstart = pos;
                    while pos < inner.len() && b[pos] != quote {
                        pos += 1;
                    }
                    let v = &inner[vstart..pos.min(inner.len())];
                    if pos < inner.len() {
                        pos += 1; // consume closing quote
                    }
                    v
                } else {
                    let vstart = pos;
                    while pos < inner.len() && !b[pos].is_ascii_whitespace() && b[pos] != b'/' {
                        pos += 1;
                    }
                    &inner[vstart..pos]
                };
                attrs.push((aname, Some(value.to_string())));
            } else {
                attrs.push((aname, None));
            }
        }
    }
    Some(Tag {
        name,
        closing,
        attrs,
    })
}

/// Rebuild a validated tag string, or `None` to keep the escaped text as-is.
fn rebuild(tag: Tag) -> Option<String> {
    let name = tag.name.as_str();
    let void = matches!(name, "img" | "br");
    let allowed = matches!(
        name,
        "img" | "p" | "div" | "br" | "kbd" | "sub" | "sup" | "details" | "summary"
    );
    if !allowed {
        return None;
    }
    if tag.closing {
        // Void elements have no meaningful closing tag.
        if void {
            return None;
        }
        return Some(format!("</{name}>"));
    }

    let mut rebuilt = String::new();
    rebuilt.push('<');
    rebuilt.push_str(name);

    match name {
        "img" => {
            let mut src = None;
            let mut extra = String::new();
            for (attr, value) in &tag.attrs {
                match attr.as_str() {
                    "src" => {
                        let v = decode_entities(value.as_deref().unwrap_or(""));
                        // Reject unsafe schemes up front (javascript:, data:,
                        // protocol-relative). Local/relative and http(s) ride
                        // the existing post_process image path.
                        if v.trim().is_empty() || !safe_image_url(&v) {
                            return None; // src invalid → keep whole tag escaped
                        }
                        src = Some(v);
                    }
                    "alt" => {
                        let v = decode_entities(value.as_deref().unwrap_or(""));
                        extra.push_str(&format!(r#" alt="{}""#, escape_attr(&v)));
                    }
                    "width" | "height" => {
                        let v = decode_entities(value.as_deref().unwrap_or(""));
                        if !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()) {
                            extra.push_str(&format!(r#" {attr}="{v}""#));
                        }
                    }
                    _ => {} // drop every other attribute (style, class, on*, …)
                }
            }
            let src = src?; // no valid src → keep escaped
            rebuilt.push_str(&format!(r#" src="{}""#, escape_attr(&src)));
            rebuilt.push_str(&extra);
        }
        "p" | "div" => {
            for (attr, value) in &tag.attrs {
                if attr == "align" {
                    let v = decode_entities(value.as_deref().unwrap_or(""));
                    let v = v.trim().to_ascii_lowercase();
                    if matches!(v.as_str(), "center" | "left" | "right") {
                        rebuilt.push_str(&format!(r#" align="{v}""#));
                    }
                }
            }
        }
        "details" if tag.attrs.iter().any(|(a, _)| a == "open") => {
            rebuilt.push_str(" open");
        }
        // br, kbd, sub, sup, summary: no attributes allowed.
        _ => {}
    }

    rebuilt.push('>');
    Some(rebuilt)
}

/// Escape an attribute value for double-quoted output.
fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Decode the handful of HTML entities an attribute value may contain. Used only
/// while extracting a value; the result is re-escaped on output.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some(semi) = s[i + 1..].find(';') {
                let entity = &s[i + 1..i + 1 + semi];
                let decoded = match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ => decode_numeric(entity),
                };
                if let Some(c) = decoded {
                    out.push(c);
                    i += semi + 2;
                    continue;
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn decode_numeric(entity: &str) -> Option<char> {
    let code = entity.strip_prefix('#')?;
    let n = if let Some(hex) = code.strip_prefix(['x', 'X']) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        code.parse::<u32>().ok()?
    };
    char::from_u32(n)
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    // Feed a tag exactly as the render pipeline would escape it.
    fn esc(tag: &str) -> String {
        tag.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    #[test]
    fn 許可imgは実タグに再構築される() {
        let input = esc(r#"<img src="assets/icon.png" width="128">"#);
        let out = reconstruct_allowed(&input);
        assert_eq!(out, r#"<img src="assets/icon.png" width="128">"#);
    }

    #[test]
    fn p_alignは中央寄せ属性を保持する() {
        let input = esc(r#"<p align="center">x</p>"#);
        let out = reconstruct_allowed(&input);
        assert!(out.starts_with(r#"<p align="center">"#), "got: {out}");
        assert!(out.ends_with("</p>"), "got: {out}");
    }

    #[test]
    fn align不正値は属性を落としタグは残す() {
        let input = esc(r#"<div align="justify">x</div>"#);
        let out = reconstruct_allowed(&input);
        assert!(out.starts_with("<div>"), "got: {out}");
        assert!(!out.contains("align"), "got: {out}");
    }

    #[test]
    fn onerror属性は出力に現れない() {
        let input = esc(r#"<img src=x onerror=alert(1)>"#);
        let out = reconstruct_allowed(&input);
        assert!(!out.contains("onerror"), "got: {out}");
        // src=x is schemeless/relative → tag emitted, post_process decides.
        assert!(out.starts_with(r#"<img src="x">"#), "got: {out}");
    }

    #[test]
    fn javascript_srcは非描画でエスケープのまま() {
        let input = esc(r#"<img src="javascript:alert(1)">"#);
        let out = reconstruct_allowed(&input);
        assert!(!out.contains("<img"), "got: {out}");
        assert!(out.contains("&lt;img"), "got: {out}");
    }

    #[test]
    fn data_srcは非描画でエスケープのまま() {
        let input = esc(r#"<img src="data:text/html,boom">"#);
        let out = reconstruct_allowed(&input);
        assert!(!out.contains("<img"), "got: {out}");
        assert!(out.contains("&lt;img"), "got: {out}");
    }

    #[test]
    fn protocol_relative_srcは非描画() {
        let input = esc(r#"<img src="//evil.com/a.png">"#);
        let out = reconstruct_allowed(&input);
        assert!(!out.contains("<img"), "got: {out}");
    }

    #[test]
    fn scriptタグはエスケープのまま() {
        let input = esc(r#"<script>alert(1)</script>"#);
        let out = reconstruct_allowed(&input);
        assert_eq!(out, input);
        assert!(!out.contains("<script"), "got: {out}");
    }

    #[test]
    fn 未許可タグはエスケープのまま() {
        for tag in [
            "<iframe src=x>",
            "</iframe>",
            "<style>a{}</style>",
            "<svg><rect/></svg>",
        ] {
            let input = esc(tag);
            let out = reconstruct_allowed(&input);
            assert!(!out.contains('<'), "leaked real tag for {tag}: {out}");
        }
    }

    #[test]
    fn 許可タグの未許可属性は落ちる() {
        let input = esc(r#"<p align="center" style="color:red" class="x" onclick="y">t</p>"#);
        let out = reconstruct_allowed(&input);
        assert!(out.starts_with(r#"<p align="center">"#), "got: {out}");
        assert!(!out.contains("style"), "got: {out}");
        assert!(!out.contains("class"), "got: {out}");
        assert!(!out.contains("onclick"), "got: {out}");
    }

    #[test]
    fn imgのstyleやonloadは落ちる() {
        let input = esc(r#"<img src="a.png" style="x" onload="y" width="10">"#);
        let out = reconstruct_allowed(&input);
        assert!(!out.contains("style"), "got: {out}");
        assert!(!out.contains("onload"), "got: {out}");
        assert!(out.contains(r#"width="10""#), "got: {out}");
    }

    #[test]
    fn 非数値widthは落ちる() {
        let input = esc(r#"<img src="a.png" width="abc">"#);
        let out = reconstruct_allowed(&input);
        assert!(!out.contains("width"), "got: {out}");
    }

    #[test]
    fn kbd_sub_sup_brは属性なしで再構築される() {
        let input = esc(r#"Press <kbd>Ctrl</kbd><br>H<sub>2</sub>O x<sup>2</sup>"#);
        let out = reconstruct_allowed(&input);
        assert!(out.contains("<kbd>"), "got: {out}");
        assert!(out.contains("</kbd>"), "got: {out}");
        assert!(out.contains("<br>"), "got: {out}");
        assert!(out.contains("<sub>"), "got: {out}");
        assert!(out.contains("<sup>"), "got: {out}");
    }

    #[test]
    fn detailsのopenは許可されsummaryも通る() {
        let input = esc(r#"<details open><summary>s</summary>body</details>"#);
        let out = reconstruct_allowed(&input);
        assert!(out.contains("<details open>"), "got: {out}");
        assert!(out.contains("<summary>"), "got: {out}");
        assert!(out.contains("</details>"), "got: {out}");
    }

    #[test]
    fn detailsの未許可属性は落ちるがopenは残る() {
        let input = esc(r#"<details open onclick="x">"#);
        let out = reconstruct_allowed(&input);
        assert_eq!(out, "<details open>");
    }

    #[test]
    fn 絶対パスsrcはスキーム的には通りpost_processで処理される() {
        // safe_image_url allows schemeless "/etc/passwd"; post_process later
        // strips it (forbidden absolute component). Here we only assert the
        // reconstruction stage does not leak onerror-style attributes.
        let input = esc(r#"<img src="/etc/passwd">"#);
        let out = reconstruct_allowed(&input);
        assert_eq!(out, r#"<img src="/etc/passwd">"#);
    }

    #[test]
    fn 単なる不等号テキストはタグ扱いしない() {
        let input = esc("a < b and c > d");
        let out = reconstruct_allowed(&input);
        assert_eq!(out, input);
    }

    #[test]
    fn 実タグはそのまま素通しする() {
        // A real Markdown-produced tag (literal <) must be untouched.
        let html = r#"<p>hello <img src="data:image/png;base64,AAA" /></p>"#;
        let out = reconstruct_allowed(html);
        assert_eq!(out, html);
    }

    #[test]
    fn alt内エンティティは再エスケープされ属性を破壊しない() {
        // Original raw HTML alt contains an entity-encoded quote and ampersand.
        let input = esc(r#"<img src="a.png" alt="x &amp; y &quot;q&quot;">"#);
        let out = reconstruct_allowed(&input);
        assert!(out.starts_with(r#"<img src="a.png""#), "got: {out}");
        assert!(
            out.contains(r#"alt="x &amp; y &quot;q&quot;""#),
            "got: {out}"
        );
        // Exactly one src/alt pair, no attribute breakout.
        assert!(!out.contains("onerror"), "got: {out}");
    }

    #[test]
    fn src無しimgはエスケープのまま() {
        let input = esc(r#"<img alt="x">"#);
        let out = reconstruct_allowed(&input);
        assert!(!out.contains("<img"), "got: {out}");
    }

    #[test]
    fn 大文字タグや大文字属性も扱う() {
        let input = esc(r#"<IMG SRC="a.png" WIDTH="50">"#);
        let out = reconstruct_allowed(&input);
        assert_eq!(out, r#"<img src="a.png" width="50">"#);
    }
}
