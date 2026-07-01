//! Markdown -> HTML pipeline.

use pulldown_cmark::{html, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;

use super::toc::{slugify, unique_anchor};
use super::{alerts, frontmatter, security};

fn markdown_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options
}

/// Render Markdown to an HTML fragment.
///
/// Pipeline: split YAML frontmatter, pre-process GitHub alerts, parse GFM, and
/// turn fenced ```mermaid blocks into `<pre class="mermaid">` for the JS layer.
/// Other fenced code keeps pulldown-cmark's `<pre><code class="language-…">`,
/// which highlight.js picks up in the WebView. Frontmatter, if any, is rendered
/// as a collapsible table before the body.
pub fn render(input: &str) -> String {
    let (front, body) = frontmatter::split_frontmatter(input);
    let options = markdown_options();
    let body = security::escape_user_html(&body, options);
    let body = alerts::preprocess_alerts(&body);

    let parser = Parser::new_ext(&body, options);

    let mut in_mermaid = false;
    let mut events: Vec<Event> = Vec::new();

    // Heading id injection: buffer a heading's inner events until its end so we
    // can derive the slug from the full text, then emit a raw <hN id="…"> open
    // tag. Anchors are de-duplicated exactly like `toc`, so links line up.
    let mut anchor_counts: HashMap<String, usize> = HashMap::new();
    let mut heading: Option<(pulldown_cmark::HeadingLevel, Vec<Event>, String)> = None;

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref lang)))
                if lang.as_ref() == "mermaid" =>
            {
                in_mermaid = true;
                events.push(Event::Html(r#"<pre class="mermaid">"#.into()));
            }
            Event::End(TagEnd::CodeBlock) if in_mermaid => {
                in_mermaid = false;
                events.push(Event::Html("</pre>".into()));
            }
            Event::Text(t) if in_mermaid => {
                events.push(Event::Text(t));
            }
            Event::Start(Tag::Heading { level, .. }) => {
                heading = Some((level, Vec::new(), String::new()));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, inner, text)) = heading.take() {
                    let anchor = unique_anchor(&slugify(text.trim()), &mut anchor_counts);
                    events.push(Event::Start(Tag::Heading {
                        level,
                        id: Some(anchor.into()),
                        classes: Vec::new(),
                        attrs: Vec::new(),
                    }));
                    events.extend(inner);
                    events.push(Event::End(TagEnd::Heading(level)));
                }
            }
            other => {
                let other = security::sanitize_url_event(other);
                if let Some((_, inner, text)) = heading.as_mut() {
                    if let Event::Text(t) | Event::Code(t) = &other {
                        text.push_str(t);
                    }
                    inner.push(other);
                } else {
                    events.push(other);
                }
            }
        }
    }

    let mut out = String::new();
    if let Some(front) = front {
        out.push_str(&frontmatter::frontmatter_to_html(&front));
    }
    html::push_html(&mut out, events.into_iter());
    out
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn renders_heading_and_table() {
        let md = indoc! {r#"
            # Title

            | a | b |
            |---|---|
            | 1 | 2 |
        "#};
        let html = render(md);
        assert!(html.contains(r#"<h1 id="title">Title</h1>"#));
        assert!(html.contains("<table>"));
        assert!(html.contains("<td>1</td>"));
    }

    #[test]
    fn 見出しにslug由来のidが付与される() {
        let md = indoc! {r#"
            ## Hello World

            ### Use `foo()`
        "#};
        let html = render(md);
        assert!(html.contains(r#"<h2 id="hello-world">"#));
        assert!(html.contains(r#"<h3 id="use-foo">"#));
    }

    #[test]
    fn 同名見出しのidは衝突しない() {
        let md = indoc! {r#"
            ## 概要

            ## 概要
        "#};
        let html = render(md);
        assert!(html.contains(r#"<h2 id="概要">"#));
        assert!(html.contains(r#"<h2 id="概要-1">"#));
    }

    #[test]
    fn mermaid_block_becomes_pre() {
        let md = indoc! {r#"
            ```mermaid
            graph TD; A-->B;
            ```
        "#};
        let html = render(md);
        assert!(html.contains(r#"<pre class="mermaid">"#));
        assert!(html.contains("graph TD"));
        assert!(!html.contains("<code"));
    }

    #[test]
    fn コードブロックはhighlight用のlanguageクラスを持つ() {
        // mermaid 以外のコードは pulldown-cmark の language クラスを残し、
        // WebView 側の highlight.js が拾えるようにする
        let md = indoc! {r#"
            ```rust
            fn main() {}
            ```
        "#};
        let html = render(md);
        assert!(html.contains(r#"class="language-rust""#));
    }

    #[test]
    fn render時にフロントマターとアラートが両方反映される() {
        let md = "---\ntitle: T\n---\n\n> [!WARNING]\n> 注意文\n";
        let html = render(md);
        assert!(html.contains("<details")); // フロントマター
        assert!(html.contains("markdown-alert-warning")); // アラート
        assert!(html.contains("注意文"));
    }

    #[test]
    fn raw_html_is_rendered_as_text_not_executed() {
        let html = render(r#"<script>alert(1)</script><img src=x onerror=alert(2)>"#);

        assert!(!html.contains("<script>"));
        assert!(!html.contains("<img src=x"));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("&lt;img src=x onerror=alert(2)&gt;"));
    }

    #[test]
    fn unsafe_markdown_urls_are_replaced_before_html_rendering() {
        let html = render(
            r#"[x](javascript:alert(1)) [y](data:text/html,<script>) ![z](file:///tmp/a.png)"#,
        );

        assert!(!html.contains("javascript:"));
        assert!(!html.contains("data:text/html"));
        assert!(!html.contains("file:///"));
        assert!(html.contains(r##"<a href="#">x</a>"##));
        assert!(html.contains(r#"<img src="" alt="z" />"#));
    }

    #[test]
    fn mermaid_block_escapes_html_breakout_text() {
        let html = render(indoc! {r#"
            ```mermaid
            </pre><img src=x onerror=alert(1)>
            ```
        "#});

        assert!(html.contains(r#"<pre class="mermaid">"#));
        assert!(html.contains("&lt;/pre&gt;&lt;img src=x onerror=alert(1)&gt;"));
        assert!(!html.contains(r#"</pre><img"#));
    }

    #[test]
    fn alert_body_raw_html_is_not_double_escaped() {
        let html = render("> [!NOTE]\n> <b>x</b>\n");

        assert!(html.contains("&lt;b&gt;x&lt;/b&gt;"));
        assert!(!html.contains("&amp;lt;b&amp;gt;"));
    }
}
