//! Table-of-contents extraction and heading slug generation.
//!
//! `slugify` turns heading text into a URL anchor; `toc` walks the markdown and
//! returns one entry per heading. Both are shared with `render`, which uses the
//! same slug logic to emit `<h2 id="…">` so anchors line up with the ToC links.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;

/// A single heading in the table of contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    /// Heading level, 1..=6.
    pub level: u8,
    /// Visible heading text (inline markup stripped to plain text).
    pub text: String,
    /// Unique anchor matching the `id` attribute emitted by `render`.
    pub anchor: String,
}

/// Lowercase, keep ASCII alphanumerics and a few word chars, turn everything
/// else into `-`, collapse runs of `-`, and trim leading/trailing `-`.
pub fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_dash = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for low in ch.to_lowercase() {
                out.push(low);
            }
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Disambiguate repeated slugs the way GitHub does: `slug`, `slug-1`, `slug-2`…
/// `counts` tracks how many times each base slug has been seen so far.
pub fn unique_anchor(base: &str, counts: &mut HashMap<String, usize>) -> String {
    let base = if base.is_empty() {
        "section".to_string()
    } else {
        base.to_string()
    };
    let n = counts.entry(base.clone()).or_insert(0);
    let anchor = if *n == 0 {
        base.clone()
    } else {
        format!("{base}-{n}")
    };
    *n += 1;
    anchor
}

fn level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Extract the table of contents from markdown.
///
/// Anchors are generated with the same slug + de-duplication rules that
/// `render` applies, so a ToC link `#anchor` always finds its heading.
pub fn toc(input: &str) -> Vec<TocEntry> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);

    let parser = Parser::new_ext(input, options);

    let mut entries: Vec<TocEntry> = Vec::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut cur_level: Option<u8> = None;
    let mut cur_text = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                cur_level = Some(level_to_u8(level));
                cur_text.clear();
            }
            Event::Text(t) | Event::Code(t) if cur_level.is_some() => {
                cur_text.push_str(&t);
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = cur_level.take() {
                    let text = cur_text.trim().to_string();
                    let anchor = unique_anchor(&slugify(&text), &mut counts);
                    entries.push(TocEntry {
                        level,
                        text,
                        anchor,
                    });
                }
            }
            _ => {}
        }
    }
    entries
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn slugifyは小文字化し非英数をハイフンにする() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("Foo: Bar / Baz"), "foo-bar-baz");
        assert_eq!(slugify("  trim me  "), "trim-me");
    }

    #[test]
    fn slugifyは連続ハイフンを圧縮し前後をトリムする() {
        assert_eq!(slugify("a -- b"), "a-b");
        assert_eq!(slugify("!!!start"), "start");
        assert_eq!(slugify("end!!!"), "end");
    }

    #[test]
    fn slugifyは日本語をそのまま残す() {
        // is_alphanumeric は日本語も真。アンカーとして使えれば十分。
        assert_eq!(slugify("はじめに"), "はじめに");
        assert_eq!(slugify("第1章 概要"), "第1章-概要");
    }

    #[test]
    fn tocは見出しのみを抽出する() {
        let md = indoc! {r#"
            # Title

            本文は無視される。

            ## Section A

            - リストも無視

            ### Sub
        "#};
        let entries = toc(md);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].level, 1);
        assert_eq!(entries[0].text, "Title");
        assert_eq!(entries[0].anchor, "title");
        assert_eq!(entries[1].level, 2);
        assert_eq!(entries[1].anchor, "section-a");
        assert_eq!(entries[2].level, 3);
        assert_eq!(entries[2].text, "Sub");
    }

    #[test]
    fn toc同名見出しはアンカーが衝突しない() {
        let md = indoc! {r#"
            ## 概要

            ## 概要

            ## 概要
        "#};
        let entries = toc(md);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].anchor, "概要");
        assert_eq!(entries[1].anchor, "概要-1");
        assert_eq!(entries[2].anchor, "概要-2");
    }

    #[test]
    fn tocはインラインコードや装飾を含む見出しを平文化する() {
        let md = "## Use `foo()` **bold**\n";
        let entries = toc(md);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "Use foo() bold");
        assert_eq!(entries[0].anchor, "use-foo-bold");
    }
}
