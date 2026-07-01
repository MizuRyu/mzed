//! YAML frontmatter handling.

/// Split leading YAML frontmatter (`---` ... `---`) from the body.
/// Returns `(Some(yaml), body)` when present, otherwise `(None, input)`.
pub fn split_frontmatter(input: &str) -> (Option<String>, String) {
    let Some(rest) = input.strip_prefix("---\n") else {
        return (None, input.to_string());
    };
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.strip_suffix('\n').unwrap_or(line);
        if trimmed == "---" {
            let fm = rest[..offset].trim_end_matches('\n').to_string();
            let body = rest[offset + line.len()..]
                .trim_start_matches('\n')
                .to_string();
            return (Some(fm), body);
        }
        offset += line.len();
    }
    (None, input.to_string())
}

/// Render frontmatter mo-style: a collapsible "Metadata" disclosure wrapping
/// the raw YAML in a syntax-highlighted code block (highlight.js picks up
/// `language-yaml`). Keeps the original YAML verbatim rather than flattening it
/// into a table, so nested/list values stay readable.
pub(crate) fn frontmatter_to_html(yaml: &str) -> String {
    let body = super::escape_html(yaml.trim_end());
    format!(
        "<details class=\"frontmatter\" open><summary>Metadata</summary>\
<pre><code class=\"language-yaml\">{body}</code></pre></details>\n\n"
    )
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII (HTML, …).
mod tests {
    use super::*;

    #[test]
    fn 先頭のフロントマターを本文から分離する() {
        // 先頭が `---` で囲まれた YAML をフロントマターとして取り出し、本文と分ける
        let (fm, body) = split_frontmatter("---\ntitle: Hello\n---\n\n# 本文");
        assert_eq!(fm.as_deref(), Some("title: Hello"));
        assert_eq!(body, "# 本文");
    }

    #[test]
    fn フロントマターが無ければ入力をそのまま返す() {
        let input = "# 見出し\n本文";
        let (fm, body) = split_frontmatter(input);
        assert!(fm.is_none());
        assert_eq!(body, input);
    }

    #[test]
    fn フロントマターは折りたたみ表としてHTML化される() {
        // details で折りたため、キーと値が表に入る
        let html = frontmatter_to_html("title: 設計\nstatus: WIP");
        assert!(html.contains("<details"));
        assert!(html.contains("title"));
        assert!(html.contains("設計"));
        assert!(html.contains("status"));
        assert!(html.contains("WIP"));
    }
}
