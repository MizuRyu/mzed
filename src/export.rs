//! Export the active document to a self-contained HTML file (markdown-pdf style).
//!
//! [`to_html`] wraps the *already rendered* markdown body (captured from the live
//! WebView, so Mermaid is inline SVG, KaTeX is laid out, and code is highlighted)
//! in a clean white-background document. github-markdown light CSS + a small page
//! wrapper give the VSCode "Markdown PDF" look. No app chrome CSS, no JS — the
//! output is static and scrolls normally. Images are already data URLs.

/// Stylesheets embedded into the binary for export (all *light* variants).
pub struct Assets {
    pub github_css: &'static str,
    pub highlight_css: &'static str,
    pub katex_css: &'static str,
}

/// Page wrapper: white background, centered readable column, sane print, and
/// plain (un-themed) Mermaid cards. Deliberately small — it does NOT pull in the
/// app's `mdo.css` (which locks `overflow: hidden` for the in-app layout and
/// would make a standalone file unscrollable).
const PAGE_CSS: &str = r#"
html, body { background: #ffffff; margin: 0; padding: 0; }
.markdown-body {
  box-sizing: border-box;
  max-width: 880px;
  margin: 0 auto;
  padding: 32px 40px 64px;
  background: #ffffff;
  color: #1f2328;
}
.mdo-mermaid { display: flex; justify-content: center; margin: 16px 0; }
.mdo-mermaid svg { max-width: 100%; height: auto; }
.frontmatter { margin-bottom: 16px; }
@media print { .markdown-body { max-width: none; } }
"#;

/// Build a self-contained, white-background HTML document from a rendered body.
pub fn to_html(body: &str, title: &str, assets: &Assets) -> String {
    let css = format!(
        "{}\n{}\n{}\n{}",
        assets.github_css, assets.highlight_css, assets.katex_css, PAGE_CSS
    );
    format!(
        "<!DOCTYPE html>\n\
<html lang=\"en\">\n\
<head>\n\
<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{title}</title>\n\
<style>\n{css}\n</style>\n\
</head>\n\
<body class=\"markdown-body\">\n\
{body}\n\
</body>\n\
</html>\n",
        title = escape(title),
        css = css,
        body = body,
    )
}

/// Escape text destined for the `<title>` element.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;
    use indoc::indoc;

    fn assets() -> Assets {
        Assets {
            github_css: ".markdown-body{color:#111}",
            highlight_css: ".hljs{background:#fff}",
            katex_css: ".katex{font-size:1em}",
        }
    }

    #[test]
    fn 完全なhtml文書として組み立てられる() {
        let out = to_html("<p>hi</p>", "doc", &assets());
        assert!(out.starts_with("<!DOCTYPE html>"));
        assert!(out.contains("<html"));
        assert!(out.trim_end().ends_with("</html>"));
    }

    #[test]
    fn styleタグに全cssがインライン化される() {
        let out = to_html("<p>hi</p>", "doc", &assets());
        assert!(out.contains("<style>"));
        assert!(out.contains(".markdown-body{color:#111}")); // github
        assert!(out.contains(".hljs{background:#fff}")); // highlight theme
        assert!(out.contains(".katex{font-size:1em}")); // katex
    }

    #[test]
    fn 白背景のページcssが含まれる() {
        let out = to_html("<p>hi</p>", "doc", &assets());
        assert!(out.contains("background: #ffffff"));
        assert!(out.contains("max-width: 880px"));
    }

    #[test]
    fn bodyにmarkdown_bodyクラスが付く() {
        let out = to_html("<p>hi</p>", "doc", &assets());
        assert!(out.contains(r#"<body class="markdown-body">"#));
    }

    #[test]
    fn 本文htmlが含まれる() {
        let body = indoc! {r#"
            <h1 id="t">Title</h1>
            <p>paragraph</p>
        "#};
        let out = to_html(body, "doc", &assets());
        assert!(out.contains(r#"<h1 id="t">Title</h1>"#));
        assert!(out.contains("<p>paragraph</p>"));
    }

    #[test]
    fn titleが含まれエスケープされる() {
        let out = to_html("<p>hi</p>", "a & <b>", &assets());
        assert!(out.contains("<title>a &amp; &lt;b&gt;</title>"));
    }
}
