//! Markdown -> HTML pipeline.

use pulldown_cmark::{html, CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};
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

    let events = autolink_pass(events);

    let mut out = String::new();
    if let Some(front) = front {
        out.push_str(&frontmatter::frontmatter_to_html(&front));
    }
    html::push_html(&mut out, events.into_iter());
    out
}

/// GFM-style autolink pass for bare `http(s)://…` URLs in prose.
///
/// Runs over the final event stream because pulldown-cmark splits `Text`
/// runs at emphasis-candidate characters (`_`, `*`, …), which would cut
/// URLs mid-way if linkified per event: consecutive `Text` events outside
/// code blocks and links are coalesced first, then linkified as one run.
fn autolink_pass(events: Vec<Event<'_>>) -> Vec<Event<'static>> {
    let mut out: Vec<Event<'static>> = Vec::new();
    let mut in_code_block = false;
    let mut in_link = false;
    let mut text_buf = String::new();

    fn flush(buf: &mut String, out: &mut Vec<Event<'static>>) {
        if buf.is_empty() {
            return;
        }
        if buf.contains("http://")
            || buf.contains("https://")
            || buf.contains("www.")
            || buf.contains('@')
        {
            out.extend(linkify_bare_urls(buf));
        } else {
            out.push(Event::Text(std::mem::take(buf).into()));
        }
        buf.clear();
    }

    for event in events {
        match &event {
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            Event::Start(Tag::Link { .. }) => in_link = true,
            Event::End(TagEnd::Link) => in_link = false,
            _ => {}
        }
        match event {
            Event::Text(t) if !in_code_block && !in_link => text_buf.push_str(&t),
            other => {
                flush(&mut text_buf, &mut out);
                out.push(own_event(other));
            }
        }
    }
    flush(&mut text_buf, &mut out);
    out
}

/// Convert a borrowed event into an owned (`'static`) one.
fn own_event(event: Event<'_>) -> Event<'static> {
    fn own(s: CowStr<'_>) -> CowStr<'static> {
        s.into_string().into()
    }
    match event {
        Event::Text(t) => Event::Text(own(t)),
        Event::Code(t) => Event::Code(own(t)),
        Event::Html(t) => Event::Html(own(t)),
        Event::InlineHtml(t) => Event::InlineHtml(own(t)),
        Event::InlineMath(t) => Event::InlineMath(own(t)),
        Event::DisplayMath(t) => Event::DisplayMath(own(t)),
        Event::FootnoteReference(t) => Event::FootnoteReference(own(t)),
        Event::SoftBreak => Event::SoftBreak,
        Event::HardBreak => Event::HardBreak,
        Event::Rule => Event::Rule,
        Event::TaskListMarker(b) => Event::TaskListMarker(b),
        Event::Start(tag) => Event::Start(own_tag(tag)),
        Event::End(end) => Event::End(end),
    }
}

fn own_tag(tag: Tag<'_>) -> Tag<'static> {
    fn own(s: CowStr<'_>) -> CowStr<'static> {
        s.into_string().into()
    }
    match tag {
        Tag::Paragraph => Tag::Paragraph,
        Tag::Heading {
            level,
            id,
            classes,
            attrs,
        } => Tag::Heading {
            level,
            id: id.map(own),
            classes: classes.into_iter().map(own).collect(),
            attrs: attrs
                .into_iter()
                .map(|(k, v)| (own(k), v.map(own)))
                .collect(),
        },
        Tag::BlockQuote(kind) => Tag::BlockQuote(kind),
        Tag::CodeBlock(kind) => Tag::CodeBlock(match kind {
            CodeBlockKind::Indented => CodeBlockKind::Indented,
            CodeBlockKind::Fenced(l) => CodeBlockKind::Fenced(own(l)),
        }),
        Tag::HtmlBlock => Tag::HtmlBlock,
        Tag::List(n) => Tag::List(n),
        Tag::Item => Tag::Item,
        Tag::FootnoteDefinition(l) => Tag::FootnoteDefinition(own(l)),
        Tag::DefinitionList => Tag::DefinitionList,
        Tag::DefinitionListTitle => Tag::DefinitionListTitle,
        Tag::DefinitionListDefinition => Tag::DefinitionListDefinition,
        Tag::Table(a) => Tag::Table(a),
        Tag::TableHead => Tag::TableHead,
        Tag::TableRow => Tag::TableRow,
        Tag::TableCell => Tag::TableCell,
        Tag::Emphasis => Tag::Emphasis,
        Tag::Strong => Tag::Strong,
        Tag::Strikethrough => Tag::Strikethrough,
        Tag::Superscript => Tag::Superscript,
        Tag::Subscript => Tag::Subscript,
        Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        } => Tag::Link {
            link_type,
            dest_url: own(dest_url),
            title: own(title),
            id: own(id),
        },
        Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        } => Tag::Image {
            link_type,
            dest_url: own(dest_url),
            title: own(title),
            id: own(id),
        },
        Tag::MetadataBlock(k) => Tag::MetadataBlock(k),
    }
}

/// Split a prose text run into Text/Link events, turning bare `http(s)://…`
/// substrings into autolinks (GFM-flavoured, simplified).
///
/// - URL start requires a word boundary (start of text, or a preceding char
///   that is not alphanumeric / `/` / `.` / `-` / `_`).
/// - URL runs over ASCII URL characters only, so it naturally stops at
///   whitespace and at CJK prose that follows a pasted URL without a space
///   (raw non-ASCII URLs are out of scope; browsers copy them
///   percent-encoded).
/// - Trailing ASCII punctuation is trimmed; a trailing `)` is kept only
///   while the URL has an unmatched `(`.
fn linkify_bare_urls(text: &str) -> Vec<Event<'static>> {
    fn is_url_char(c: char) -> bool {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '-' | '.'
                    | '_'
                    | '~'
                    | ':'
                    | '/'
                    | '?'
                    | '#'
                    | '['
                    | ']'
                    | '@'
                    | '!'
                    | '$'
                    | '&'
                    | '\''
                    | '('
                    | ')'
                    | '*'
                    | '+'
                    | ','
                    | ';'
                    | '='
                    | '%'
            )
    }
    fn trim_trailing(mut url: &str) -> &str {
        while let Some(last) = url.chars().last() {
            let trim = match last {
                '.' | ',' | ':' | ';' | '!' | '?' | '\'' | '*' | '_' | '~' => true,
                ')' => url.matches('(').count() < url.matches(')').count(),
                _ => false,
            };
            if !trim {
                break;
            }
            url = &url[..url.len() - last.len_utf8()];
        }
        url
    }
    // Word boundary before a scheme/www match.
    fn boundary_before(s: &str, i: usize) -> bool {
        match s[..i].chars().last() {
            None => true,
            Some(prev) => !prev.is_alphanumeric() && !matches!(prev, '/' | '.' | '-' | '_' | '@'),
        }
    }
    // The earliest valid http(s):// or www. match in `s`: (start, literal, dest).
    fn find_url(s: &str) -> Option<(usize, &str, String)> {
        let mut starts: Vec<usize> = s
            .match_indices("http")
            .filter(|(i, _)| {
                let tail = &s[*i..];
                (tail.starts_with("http://") || tail.starts_with("https://"))
                    && boundary_before(s, *i)
            })
            .map(|(i, _)| i)
            .collect();
        starts.extend(
            s.match_indices("www.")
                .filter(|(i, _)| boundary_before(s, *i))
                .map(|(i, _)| i),
        );
        starts.sort_unstable();
        for start in starts {
            let tail = &s[start..];
            let end = tail.find(|c: char| !is_url_char(c)).unwrap_or(tail.len());
            let url = trim_trailing(&tail[..end]);
            // A bare scheme / bare "www." with no host is not a link.
            if url.is_empty() || url == "http://" || url == "https://" || url == "www." {
                continue;
            }
            let dest = if url.starts_with("www.") {
                // GFM www autolinks assume http.
                format!("http://{url}")
            } else {
                url.to_string()
            };
            return Some((start, url, dest));
        }
        None
    }
    // The earliest bare e-mail in `s` (GFM-flavoured, simplified).
    fn find_email(s: &str) -> Option<(usize, &str, String)> {
        fn is_local(c: char) -> bool {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
        }
        fn is_domain(c: char) -> bool {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '-')
        }
        for (at, _) in s.match_indices('@') {
            let local_start = s[..at]
                .char_indices()
                .rev()
                .take_while(|(_, c)| is_local(*c))
                .last()
                .map(|(i, _)| i);
            let Some(local_start) = local_start else {
                continue;
            };
            if !boundary_before(s, local_start) {
                continue;
            }
            let after = &s[at + 1..];
            let dom_end = after.find(|c: char| !is_domain(c)).unwrap_or(after.len());
            let mut domain = &after[..dom_end];
            while domain.ends_with('.') || domain.ends_with('-') {
                domain = &domain[..domain.len() - 1];
            }
            if domain.is_empty() || !domain.contains('.') {
                continue;
            }
            let literal = &s[local_start..at + 1 + domain.len()];
            return Some((local_start, literal, format!("mailto:{literal}")));
        }
        None
    }

    let mut out = Vec::new();
    let mut rest = text;
    loop {
        let url = find_url(rest);
        let email = find_email(rest);
        let m = match (url, email) {
            (Some(u), Some(e)) => Some(if u.0 <= e.0 { u } else { e }),
            (u, e) => u.or(e),
        };
        let Some((start, literal, dest)) = m else {
            break;
        };
        if start > 0 {
            out.push(Event::Text(rest[..start].to_string().into()));
        }
        out.push(Event::Start(Tag::Link {
            link_type: LinkType::Autolink,
            dest_url: dest.into(),
            title: "".into(),
            id: "".into(),
        }));
        out.push(Event::Text(literal.to_string().into()));
        out.push(Event::End(TagEnd::Link));
        rest = &rest[start + literal.len()..];
    }
    if !rest.is_empty() {
        out.push(Event::Text(rest.to_string().into()));
    }
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
    fn 裸のURLが自動リンクになる() {
        let html = render("参照: https://example.com/path?q=1 を見て。");
        assert!(html.contains(
            r#"<a href="https://example.com/path?q=1">https://example.com/path?q=1</a>"#
        ));
    }

    #[test]
    fn 裸URLの末尾句読点はリンクに含めない() {
        let html = render("See https://example.com. Also https://example.com/a、次へ");
        assert!(html.contains(r#"<a href="https://example.com">"#));
        assert!(html.contains(r#"<a href="https://example.com/a">"#));
        assert!(!html.contains(r#"href="https://example.com.""#));
    }

    #[test]
    fn 括弧内の裸URLは閉じ括弧を含めない() {
        let html = render("(https://example.com/x) と (https://en.wikipedia.org/wiki/Foo_(bar))");
        assert!(html.contains(r#"<a href="https://example.com/x">"#));
        // バランスした括弧は URL の一部として保持（Wikipedia 形式）
        assert!(html.contains(r#"<a href="https://en.wikipedia.org/wiki/Foo_(bar)">"#));
    }

    #[test]
    fn コードブロックとインラインコード内のURLはリンク化しない() {
        let md = "`https://inline.example` \n\n```\nhttps://block.example\n```\n";
        let html = render(md);
        assert!(!html.contains(r#"href="https://inline.example""#));
        assert!(!html.contains(r#"href="https://block.example""#));
    }

    #[test]
    fn 既存リンクのラベル内URLは二重リンク化しない() {
        let html = render("[https://example.com](https://example.com)");
        assert_eq!(html.matches("<a href=").count(), 1);
    }

    #[test]
    fn wwwドット始まりが自動リンクになる() {
        let html = render("www.example.com を見て。xwww.example.com は境界なし");
        assert!(html.contains(r#"<a href="http://www.example.com">www.example.com</a>"#));
        assert!(!html.contains(r#"href="http://xwww"#));
        assert_eq!(html.matches("<a href=").count(), 1);
    }

    #[test]
    fn 裸のメールアドレスがmailtoリンクになる() {
        let html = render(
            "連絡は a.b+c@example.co.jp まで。user@localhost はドメインにドットがないので対象外",
        );
        assert!(html.contains(r#"<a href="mailto:a.b+c@example.co.jp">a.b+c@example.co.jp</a>"#));
        assert!(!html.contains(r#"mailto:user@localhost"#));
    }

    #[test]
    fn URL内のアットマークはメールとして誤検出しない() {
        let html = render("https://example.com/user@name/path を参照");
        assert!(html.contains(r#"<a href="https://example.com/user@name/path">"#));
        assert!(!html.contains("mailto:"));
    }

    #[test]
    fn コード内のメールとwwwはリンク化しない() {
        let html = render("`a@b.com` と `www.example.com`");
        assert!(!html.contains("<a href="));
    }

    #[test]
    fn 単語に埋め込まれたhttpはリンク化しない() {
        let html = render("xhttps://example.com は境界がないのでリンクにしない");
        assert!(!html.contains("<a href="));
    }

    #[test]
    fn BOM付きファイルは正規化後にフロントマターと見出しが効く() {
        // A raw BOM breaks frontmatter/heading parsing; normalize_source strips it.
        let raw = "\u{feff}---\ntitle: T\n---\n\n# 見出し\n";
        let broken = render(raw);
        assert!(!broken.contains("<details"), "BOM未処理でも壊れない前提");

        let fixed = render(&super::super::normalize_source(raw));
        assert!(fixed.contains("<details"), "got: {fixed}");
        assert!(
            fixed.contains(r#"<h1 id="見出し">見出し</h1>"#),
            "got: {fixed}"
        );
    }

    #[test]
    fn CRLFファイルは正規化後にフロントマターが効く() {
        let raw = "---\r\ntitle: T\r\n---\r\n\r\n# 本文\r\n";
        let fixed = render(&super::super::normalize_source(raw));
        assert!(fixed.contains("<details"), "got: {fixed}");
        assert!(fixed.contains(r#"<h1 id="本文">"#), "got: {fixed}");
    }

    #[test]
    fn alert_body_raw_html_is_not_double_escaped() {
        let html = render("> [!NOTE]\n> <b>x</b>\n");

        assert!(html.contains("&lt;b&gt;x&lt;/b&gt;"));
        assert!(!html.contains("&amp;lt;b&amp;gt;"));
    }
}
