//! GitHub alert blockquotes (`> [!NOTE]` …).

const ALERT_TYPES: &[&str] = &["note", "tip", "important", "warning", "caution"];

/// Convert GitHub alert blockquotes into styled HTML before parsing.
///
/// A blockquote that starts with `> [!NOTE]` (or TIP/IMPORTANT/WARNING/CAUTION,
/// case-insensitive) becomes `<div class="markdown-alert markdown-alert-…">`.
/// Plain blockquotes are left untouched.
pub fn preprocess_alerts(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let mut out = String::new();
    let mut i = 0;

    while i < lines.len() {
        if let Some(kind) = alert_kind(lines[i]) {
            i += 1;
            let mut body: Vec<String> = Vec::new();
            while i < lines.len() && (lines[i].starts_with("> ") || lines[i] == ">") {
                let content = lines[i]
                    .strip_prefix("> ")
                    .or_else(|| lines[i].strip_prefix('>'))
                    .unwrap_or(lines[i]);
                body.push(escape_alert_text(content));
                i += 1;
            }
            let title = title_case(&kind);
            let body_html = body.join("<br>");
            out.push_str(&format!(
                "<div class=\"markdown-alert markdown-alert-{kind}\">\
                 <p class=\"markdown-alert-title\">{title}</p>\
                 <p>{body_html}</p></div>\n\n"
            ));
        } else {
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;
        }
    }
    out
}

/// Detect a `> [!TYPE]` alert header line, returning the lowercase type.
fn alert_kind(line: &str) -> Option<String> {
    let rest = line.trim_end().strip_prefix("> [!")?.strip_suffix(']')?;
    let kind = rest.to_lowercase();
    ALERT_TYPES.contains(&kind.as_str()).then_some(kind)
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn escape_alert_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&super::escape_html(&rest[..pos]));
        let after_amp = &rest[pos + 1..];
        if let Some(end) = after_amp.find(';') {
            let entity = &after_amp[..end];
            if is_html_entity_like(entity) {
                out.push('&');
                out.push_str(entity);
                out.push(';');
                rest = &after_amp[end + 1..];
                continue;
            }
        }
        out.push_str("&amp;");
        rest = after_amp;
    }
    out.push_str(&super::escape_html(rest));
    out
}

fn is_html_entity_like(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    if let Some(hex) = s.strip_prefix("#x").or_else(|| s.strip_prefix("#X")) {
        return !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    if let Some(decimal) = s.strip_prefix('#') {
        return !decimal.is_empty() && decimal.chars().all(|c| c.is_ascii_digit());
    }
    s.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII (NOTE, div, …).
mod tests {
    use super::*;

    #[test]
    fn NOTEアラートをスタイル付きdivに変換する() {
        let html = preprocess_alerts("> [!NOTE]\n> これはノート\n");
        assert!(html.contains(r#"class="markdown-alert markdown-alert-note""#));
        assert!(html.contains("これはノート"));
    }

    #[test]
    fn アラート種別は大文字小文字を問わず認識する() {
        let html = preprocess_alerts("> [!warning]\n> 注意\n");
        assert!(html.contains("markdown-alert-warning"));
    }

    #[test]
    fn 通常の引用はアラートに変換しない() {
        let html = preprocess_alerts("> ただの引用\n");
        assert!(!html.contains("markdown-alert"));
        assert!(html.contains("> ただの引用"));
    }

    #[test]
    fn alert本文は既存entityを二重escapeしない() {
        let html = preprocess_alerts("> [!NOTE]\n> &lt;b&gt;x&lt;/b&gt; & raw\n");
        assert!(html.contains("&lt;b&gt;x&lt;/b&gt; &amp; raw"));
        assert!(!html.contains("&amp;lt;b"));
    }
}
