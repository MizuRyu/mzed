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
