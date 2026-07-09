//! Markdown rendering.
//!
//! Split into submodules per concern (arto-style): frontmatter extraction,
//! GitHub alerts pre-processing, and the GFM -> HTML pipeline.

mod alerts;
mod frontmatter;
mod post_process;
mod raw_html;
mod render;
mod security;
mod toc;
mod wikilink;

pub use post_process::post_process;
pub use render::render;
pub use toc::{toc, TocEntry};
pub use wikilink::preprocess_wikilinks;

/// Normalize a source file before Markdown processing:
/// strip a leading UTF-8 BOM and convert CRLF / lone CR to LF.
///
/// Windows-authored notes (`\r\n`) and BOM-prefixed files otherwise break
/// frontmatter detection (`strip_prefix("---\n")`) and other line-oriented
/// pre-processing. Kept separate from the original source so raw/clipboard
/// views stay byte-faithful.
pub fn normalize_source(input: &str) -> String {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    if !input.contains('\r') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
#[allow(non_snake_case)]
mod normalize_tests {
    use super::normalize_source;

    #[test]
    fn 先頭BOMを除去する() {
        assert_eq!(normalize_source("\u{feff}# H"), "# H");
    }

    #[test]
    fn CRLFをLFに変換する() {
        assert_eq!(normalize_source("a\r\nb\r\n"), "a\nb\n");
    }

    #[test]
    fn 単独CRもLFに変換する() {
        assert_eq!(normalize_source("a\rb"), "a\nb");
    }

    #[test]
    fn BOMとCRLFを同時に処理する() {
        assert_eq!(
            normalize_source("\u{feff}---\r\ntitle: T\r\n---\r\n"),
            "---\ntitle: T\n---\n"
        );
    }

    #[test]
    fn CRが無ければ割り当てを避けても内容は同じ() {
        assert_eq!(normalize_source("plain\ntext"), "plain\ntext");
    }
}

/// Minimal HTML text escaping shared by the submodules.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod security_contract_tests {
    use super::security;

    #[test]
    fn security_policy_rejects_executable_and_local_schemes() {
        assert!(!security::safe_link_url("javascript:alert(1)"));
        assert!(!security::safe_link_url("file:///tmp/secret"));
        assert!(!security::safe_image_url("data:text/html,boom"));
    }

    #[test]
    fn security_policy_allows_product_link_schemes() {
        assert!(security::safe_link_url("https://example.com"));
        assert!(security::safe_link_url("mailto:user@example.com"));
        assert!(security::safe_link_url("../guide.md"));
        assert!(security::safe_image_url("./image.png"));
    }
}
