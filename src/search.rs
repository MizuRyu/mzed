//! Full-text search across a project's markdown files (pure logic).
//!
//! [`search_in_files`] is the testable core for already-loaded files.
//! [`search_paths_with_policy`] adds bounded filesystem reads, result limits,
//! and cooperative cancellation.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

const DEFAULT_MAX_HITS: usize = 500;
const DEFAULT_MAX_FILE_BYTES: usize = 4 * 1024 * 1024;

/// One search hit: a file, a 1-based line number, and the matching line text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    /// 1-based line number, or 0 when the match is on the file name only.
    pub line: usize,
    /// The matched line (trimmed snippet), or the file name for a name match.
    pub snippet: String,
}

/// Resource limits applied to a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchPolicy {
    pub max_hits: usize,
    pub max_file_bytes: usize,
}

impl Default for SearchPolicy {
    fn default() -> Self {
        Self {
            max_hits: DEFAULT_MAX_HITS,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
        }
    }
}

/// Search results and completion state for metrics and callers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchReport {
    pub hits: Vec<Hit>,
    pub cancelled: bool,
    /// True when a hit or file byte limit omitted searchable input.
    pub truncated: bool,
}

/// Search `files` for `query`, returning hits in file-then-line order.
///
/// - Case-insensitive substring match.
/// - Each content line that contains the query yields one hit.
/// - A file whose name contains the query yields a single `line == 0` hit
///   (only when no content line already matched, to avoid noise).
/// - A blank/whitespace query yields no hits.
#[cfg(test)]
pub fn search_in_files(files: &[(PathBuf, String)], query: &str) -> Vec<Hit> {
    search_in_files_with_policy(files, query, SearchPolicy::default(), || false).hits
}

/// Search already-loaded files with result limits and cooperative cancellation.
///
/// `should_cancel` is checked between files and before each content line.
#[cfg(test)]
pub fn search_in_files_with_policy<F>(
    files: &[(PathBuf, String)],
    query: &str,
    policy: SearchPolicy,
    mut should_cancel: F,
) -> SearchReport
where
    F: FnMut() -> bool,
{
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return SearchReport::default();
    }
    if policy.max_hits == 0 {
        return SearchReport {
            truncated: true,
            ..SearchReport::default()
        };
    }

    let mut report = SearchReport::default();
    for (path, content) in files {
        if should_cancel() {
            report.cancelled = true;
            break;
        }

        match search_content(
            path,
            content,
            &needle,
            policy.max_hits,
            &mut report.hits,
            &mut should_cancel,
        ) {
            SearchStop::Complete => {}
            SearchStop::Cancelled => {
                report.cancelled = true;
                break;
            }
            SearchStop::HitLimit => {
                report.truncated = true;
                break;
            }
        }
    }
    report
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchStop {
    Complete,
    Cancelled,
    HitLimit,
}

fn search_content<F>(
    path: &Path,
    content: &str,
    needle: &str,
    max_hits: usize,
    hits: &mut Vec<Hit>,
    should_cancel: &mut F,
) -> SearchStop
where
    F: FnMut() -> bool,
{
    let mut matched_content = false;
    for (i, line) in content.lines().enumerate() {
        if should_cancel() {
            return SearchStop::Cancelled;
        }
        if line.to_lowercase().contains(needle) {
            matched_content = true;
            if hits.len() >= max_hits {
                return SearchStop::HitLimit;
            }
            hits.push(Hit {
                path: path.to_path_buf(),
                line: i + 1,
                snippet: snippet_of(line),
            });
            if hits.len() >= max_hits {
                return SearchStop::HitLimit;
            }
        }
    }

    if !matched_content {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if name.to_lowercase().contains(needle) {
            if hits.len() >= max_hits {
                return SearchStop::HitLimit;
            }
            hits.push(Hit {
                path: path.to_path_buf(),
                line: 0,
                snippet: name.to_string(),
            });
            if hits.len() >= max_hits {
                return SearchStop::HitLimit;
            }
        }
    }
    SearchStop::Complete
}

/// Trim a line into a bounded single-line snippet for display.
fn snippet_of(line: &str) -> String {
    const MAX: usize = 160;
    let t = line.trim();
    if t.chars().count() > MAX {
        let s: String = t.chars().take(MAX).collect();
        format!("{s}…")
    } else {
        t.to_string()
    }
}

/// Convenience: load `paths` and search them in one call.
#[cfg(test)]
pub fn search_paths(paths: &[PathBuf], query: &str) -> Vec<Hit> {
    crate::perf::measure(
        "search.search_paths",
        &[
            ("files", paths.len().to_string()),
            ("query_bytes", query.len().to_string()),
        ],
        || search_paths_with_policy(paths, query, SearchPolicy::default(), || false).hits,
    )
}

/// Search paths with bounded reads and cooperative cancellation.
///
/// Each file is read and searched before the next file is opened.
/// `should_cancel` is checked between files, after reads, and before each line.
pub fn search_paths_with_policy<F>(
    paths: &[PathBuf],
    query: &str,
    policy: SearchPolicy,
    mut should_cancel: F,
) -> SearchReport
where
    F: FnMut() -> bool,
{
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return SearchReport::default();
    }
    if policy.max_hits == 0 {
        return SearchReport {
            truncated: true,
            ..SearchReport::default()
        };
    }

    let mut report = SearchReport::default();
    for path in paths {
        if should_cancel() {
            report.cancelled = true;
            break;
        }
        let Some((content, file_truncated)) = read_file_limited(path, policy.max_file_bytes) else {
            continue;
        };
        report.truncated |= file_truncated;

        if should_cancel() {
            report.cancelled = true;
            break;
        }
        match search_content(
            path,
            &content,
            &needle,
            policy.max_hits,
            &mut report.hits,
            &mut should_cancel,
        ) {
            SearchStop::Complete => {}
            SearchStop::Cancelled => {
                report.cancelled = true;
                break;
            }
            SearchStop::HitLimit => {
                report.truncated = true;
                break;
            }
        }
    }
    report
}

fn read_file_limited(path: &Path, max_bytes: usize) -> Option<(String, bool)> {
    let file = File::open(path).ok()?;
    let max_bytes_u64 = u64::try_from(max_bytes).unwrap_or(u64::MAX);
    let truncated = file
        .metadata()
        .map(|metadata| metadata.len() > max_bytes_u64)
        .unwrap_or(false);
    let mut bytes = Vec::new();
    file.take(max_bytes_u64).read_to_end(&mut bytes).ok()?;
    Some((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}

/// Display the hit's path relative to `root` when possible, else absolute.
pub fn relative_display(path: &Path, root: Option<&Path>) -> String {
    match root {
        Some(r) => path.strip_prefix(r).unwrap_or(path).display().to_string(),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;
    use indoc::indoc;
    use std::io::Write;

    fn f(path: &str, content: &str) -> (PathBuf, String) {
        (PathBuf::from(path), content.to_string())
    }

    #[test]
    fn 空クエリは何もヒットしない() {
        let files = vec![f("/a.md", "hello world")];
        assert!(search_in_files(&files, "").is_empty());
        assert!(search_in_files(&files, "   ").is_empty());
    }

    #[test]
    fn 大文字小文字を無視してマッチする() {
        let files = vec![f("/a.md", "Hello World")];
        let hits = search_in_files(&files, "world");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 1);
        assert_eq!(hits[0].snippet, "Hello World");
    }

    #[test]
    fn 同一ファイル内の複数行ヒットを行番号付きで返す() {
        let content = indoc! {"
            alpha line
            beta here
            alpha again
        "};
        let files = vec![f("/doc.md", content)];
        let hits = search_in_files(&files, "alpha");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].line, 1);
        assert_eq!(hits[1].line, 3);
        assert_eq!(hits[1].snippet, "alpha again");
    }

    #[test]
    fn 複数ファイルを横断して検索する() {
        let files = vec![
            f("/a.md", "needle in a"),
            f("/b.md", "nothing here"),
            f("/c.md", "needle in c"),
        ];
        let hits = search_in_files(&files, "needle");
        let paths: Vec<&PathBuf> = hits.iter().map(|h| &h.path).collect();
        assert_eq!(hits.len(), 2);
        assert_eq!(
            paths,
            vec![&PathBuf::from("/a.md"), &PathBuf::from("/c.md")]
        );
    }

    #[test]
    fn ファイル名にマッチすると行0で1件返る() {
        let files = vec![f("/notes/architecture.md", "no body match")];
        let hits = search_in_files(&files, "architecture");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 0);
        assert_eq!(hits[0].snippet, "architecture.md");
    }

    #[test]
    fn 本文がヒットすればファイル名ヒットは重複させない() {
        // File name also contains "guide" but body matches too: only body hits.
        let files = vec![f("/guide.md", "the guide says")];
        let hits = search_in_files(&files, "guide");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 1);
    }

    #[test]
    fn 長い行はスニペットが切り詰められる() {
        let long = "x ".repeat(200);
        let files = vec![f("/a.md", &format!("x {long}"))];
        let hits = search_in_files(&files, "x");
        assert!(hits[0].snippet.ends_with('…'));
    }

    #[test]
    fn default_policyは製品向け上限を持つ() {
        let policy = SearchPolicy::default();
        assert_eq!(policy.max_hits, 500);
        assert_eq!(policy.max_file_bytes, 4 * 1024 * 1024);
    }

    #[test]
    fn hit上限に到達したらその場で切り詰めを通知する() {
        let files = vec![f("/a.md", "hit\nhit")];
        let report = search_in_files_with_policy(
            &files,
            "hit",
            SearchPolicy {
                max_hits: 2,
                ..SearchPolicy::default()
            },
            || false,
        );

        assert_eq!(report.hits.len(), 2);
        assert!(report.truncated);
    }

    #[test]
    fn hit上限0件なら走査せず切り詰めを通知する() {
        let files = vec![f("/a.md", "hit")];
        let report = search_in_files_with_policy(
            &files,
            "hit",
            SearchPolicy {
                max_hits: 0,
                ..SearchPolicy::default()
            },
            || false,
        );

        assert!(report.hits.is_empty());
        assert!(report.truncated);
        assert!(!report.cancelled);
    }

    #[test]
    fn 読込上限ちょうどのファイルは切り詰めない() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"needle").unwrap();
        let report = search_paths_with_policy(
            &[file.path().to_path_buf()],
            "needle",
            SearchPolicy {
                max_file_bytes: 6,
                ..SearchPolicy::default()
            },
            || false,
        );

        assert_eq!(report.hits.len(), 1);
        assert!(!report.truncated);
    }

    #[test]
    fn 巨大ファイルは上限までしか検索せず切り詰めを通知する() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"needle\npadding\nneedle").unwrap();
        let report = search_paths_with_policy(
            &[file.path().to_path_buf()],
            "needle",
            SearchPolicy {
                max_file_bytes: 14,
                ..SearchPolicy::default()
            },
            || false,
        );

        assert_eq!(report.hits.len(), 1);
        assert_eq!(report.hits[0].line, 1);
        assert!(report.truncated);
    }

    #[test]
    fn ファイル間でcancelを確認して後続ファイルを検索しない() {
        let files = vec![f("/a.md", "hit"), f("/b.md", "hit")];
        let mut checks = 0;
        let report = search_in_files_with_policy(&files, "hit", SearchPolicy::default(), || {
            checks += 1;
            checks >= 3
        });

        assert_eq!(report.hits.len(), 1);
        assert_eq!(report.hits[0].path, PathBuf::from("/a.md"));
        assert!(report.cancelled);
        assert!(!report.truncated);
    }

    #[test]
    fn 行走査中にcancelを確認して速やかに終了する() {
        let files = vec![f("/a.md", "hit\nhit\nhit\nhit")];
        let mut checks = 0;
        let report = search_in_files_with_policy(&files, "hit", SearchPolicy::default(), || {
            checks += 1;
            checks >= 4
        });

        assert_eq!(report.hits.len(), 2);
        assert!(report.cancelled);
        assert!(!report.truncated);
    }

    #[test]
    fn 既存search_pathsはdefault_policyのhit上限を使う() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all("hit\n".repeat(501).as_bytes()).unwrap();

        let hits = search_paths(&[file.path().to_path_buf()], "hit");

        assert_eq!(hits.len(), SearchPolicy::default().max_hits);
    }
}
