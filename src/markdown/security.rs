//! Security policy applied before rendered Markdown reaches the WebView.

use pulldown_cmark::{CowStr, Event, Options, Parser, Tag};

pub(super) fn escape_user_html(input: &str, options: Options) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;

    for (event, range) in Parser::new_ext(input, options).into_offset_iter() {
        if !matches!(event, Event::Html(_) | Event::InlineHtml(_)) || range.start < cursor {
            continue;
        }
        out.push_str(&input[cursor..range.start]);
        out.push_str(&super::escape_html(&input[range.clone()]));
        cursor = range.end;
    }
    out.push_str(&input[cursor..]);
    out
}

fn has_scheme(url: &str) -> Option<&str> {
    let trimmed = url.trim_start_matches(|c: char| c.is_ascii_whitespace() || c.is_control());
    let scheme_end = trimmed.find(':')?;
    let first_separator = trimmed.find(['/', '?', '#']).unwrap_or(trimmed.len());
    (scheme_end < first_separator).then_some(&trimmed[..scheme_end])
}

pub(super) fn safe_link_url(url: &str) -> bool {
    let trimmed = url.trim_start();
    if trimmed.starts_with("//") {
        return false;
    }
    match has_scheme(trimmed).map(str::to_ascii_lowercase) {
        Some(scheme) => matches!(scheme.as_str(), "http" | "https" | "mailto"),
        None => true,
    }
}

pub(super) fn safe_image_url(url: &str) -> bool {
    let trimmed = url.trim_start();
    if trimmed.starts_with("//") {
        return false;
    }
    match has_scheme(trimmed).map(str::to_ascii_lowercase) {
        Some(scheme) => matches!(scheme.as_str(), "http" | "https"),
        None => true,
    }
}

pub(super) fn sanitize_url_event(event: Event) -> Event {
    match event {
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let dest_url = if safe_link_url(&dest_url) {
                dest_url
            } else {
                CowStr::from("#")
            };
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            })
        }
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let dest_url = if safe_image_url(&dest_url) {
                dest_url
            } else {
                CowStr::from("")
            };
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            })
        }
        other => other,
    }
}
