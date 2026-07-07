use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::{export, markdown};

#[derive(Debug, Clone)]
pub(crate) struct DocumentSnapshot {
    path: Option<PathBuf>,
    source: String,
    rendered_html: String,
    raw_html: String,
    lower_source: String,
    toc: Vec<markdown::TocEntry>,
}

impl DocumentSnapshot {
    pub(crate) fn loading(path: Option<PathBuf>) -> Self {
        match path {
            Some(path) => Self {
                path: Some(path),
                source: String::new(),
                rendered_html: loading_html().to_string(),
                raw_html: loading_html().to_string(),
                lower_source: String::new(),
                toc: Vec::new(),
            },
            None => Self::empty(),
        }
    }

    pub(crate) fn empty() -> Self {
        Self {
            path: None,
            source: String::new(),
            rendered_html: select_file_html(),
            raw_html: select_file_html(),
            lower_source: String::new(),
            toc: Vec::new(),
        }
    }

    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(crate) fn rendered_html(&self) -> &str {
        &self.rendered_html
    }

    pub(crate) fn raw_html(&self) -> &str {
        &self.raw_html
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn toc(&self) -> &[markdown::TocEntry] {
        &self.toc
    }

    pub(crate) fn find_count(&self, query: &str) -> usize {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            0
        } else {
            self.lower_source.matches(&needle).count()
        }
    }
}

pub(crate) fn loading_html() -> &'static str {
    "<p>Loading...</p>"
}

pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Pick a representative markdown file for a project root.
/// Prefers a README, then any `.md` found by a depth-bounded walk of `docs/`,
/// then the project root.
pub(crate) fn pick_markdown(root: &Path) -> Option<PathBuf> {
    for cand in [root.join("README.md"), root.join("docs/README.md")] {
        if cand.exists() {
            return Some(cand);
        }
    }
    find_markdown(&root.join("docs"), 3).or_else(|| find_markdown(root, 2))
}

/// Depth-bounded search for the first `.md` file under `dir`. Files in a
/// directory are checked before descending; noisy dirs are skipped.
pub(crate) fn find_markdown(dir: &Path, depth: usize) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();

    for p in &entries {
        if p.is_file() && p.extension().map(|x| x == "md").unwrap_or(false) {
            return Some(p.clone());
        }
    }
    if depth == 0 {
        return None;
    }
    for p in &entries {
        if !p.is_dir() {
            continue;
        }
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.starts_with('.') || matches!(name, "node_modules" | "target") {
            continue;
        }
        if let Some(found) = find_markdown(p, depth - 1) {
            return Some(found);
        }
    }
    None
}

/// Directories from `file`'s parent up to and including `root`, for auto-expand.
pub(crate) fn ancestor_dirs(root: &Path, file: &Path) -> HashSet<PathBuf> {
    let mut set = HashSet::new();
    let mut cur = file.parent();
    while let Some(d) = cur {
        if !d.starts_with(root) {
            break;
        }
        set.insert(d.to_path_buf());
        if d == root {
            break;
        }
        cur = d.parent();
    }
    set
}

/// Like [`ancestor_dirs`] but tries each root, returning the expansion set for
/// whichever root contains `file` (empty if none does).
pub(crate) fn ancestor_dirs_multi(roots: &[PathBuf], file: &Path) -> HashSet<PathBuf> {
    for r in roots {
        if file.starts_with(r) {
            return ancestor_dirs(r, file);
        }
    }
    HashSet::new()
}

pub(crate) fn load_document(file: Option<PathBuf>, roots: &[PathBuf]) -> DocumentSnapshot {
    let Some(path) = file else {
        return DocumentSnapshot::empty();
    };

    let source = match crate::perf::measure(
        "file.read_markdown",
        &[("path", path.display().to_string())],
        || std::fs::read_to_string(&path),
    ) {
        Ok(source) => source,
        Err(_) => {
            let failed_html = failed_read_html(&path);
            return DocumentSnapshot {
                rendered_html: failed_html.clone(),
                path: Some(path),
                source: String::new(),
                raw_html: failed_html,
                lower_source: String::new(),
                toc: Vec::new(),
            };
        }
    };
    let base = path.parent().unwrap_or(Path::new("."));
    let allowed_roots = allowed_roots_for_file(base, roots);
    // Wikilink pre-processing expands `[[...]]` to standard Markdown links.
    // We keep the original `source` for clipboard/raw display and use the
    // expanded form only for rendering.
    let source_for_render = crate::perf::measure(
        "markdown.wikilinks",
        &[("input_bytes", source.len().to_string())],
        || markdown::preprocess_wikilinks(&source, base, &allowed_roots),
    );
    let rendered = crate::perf::measure(
        "markdown.render",
        &[("input_bytes", source_for_render.len().to_string())],
        || markdown::render(&source_for_render),
    );
    let rendered_html = crate::perf::measure(
        "markdown.post_process",
        &[
            ("input_bytes", rendered.len().to_string()),
            ("allowed_roots", allowed_roots.len().to_string()),
        ],
        || markdown::post_process(&rendered, base, &allowed_roots),
    );
    let raw_html = crate::perf::measure(
        "markdown.raw_html",
        &[("input_bytes", source.len().to_string())],
        || format!("<pre><code>{}</code></pre>", html_escape(&source)),
    );
    let lower_source = crate::perf::measure(
        "markdown.find_index",
        &[("input_bytes", source.len().to_string())],
        || source.to_lowercase(),
    );
    let toc = crate::perf::measure(
        "markdown.toc",
        &[("input_bytes", source.len().to_string())],
        || markdown::toc(&source),
    );

    DocumentSnapshot {
        path: Some(path),
        source,
        rendered_html,
        raw_html,
        lower_source,
        toc,
    }
}

pub(crate) fn allowed_roots_for_file(base_dir: &Path, roots: &[PathBuf]) -> Vec<PathBuf> {
    if roots.is_empty() {
        return vec![base_dir.to_path_buf()];
    }

    let mut allowed = roots.to_vec();
    let base_inside_root = base_dir.canonicalize().ok().is_some_and(|base| {
        roots
            .iter()
            .filter_map(|root| root.canonicalize().ok())
            .any(|root| base.starts_with(root))
    });
    if !base_inside_root {
        allowed.push(base_dir.to_path_buf());
    }
    allowed
}

pub(crate) fn allowed_roots_for_active_files(
    roots: &[PathBuf],
    active: Option<&PathBuf>,
    active_r: Option<&PathBuf>,
) -> Vec<PathBuf> {
    let mut allowed = roots.to_vec();
    for base in [active, active_r]
        .into_iter()
        .flatten()
        .filter_map(|path| path.parent())
    {
        for root in allowed_roots_for_file(base, &allowed) {
            if !allowed.iter().any(|existing| existing == &root) {
                allowed.push(root);
            }
        }
    }
    if allowed.is_empty() {
        [active, active_r]
            .into_iter()
            .flatten()
            .filter_map(|path| path.parent().map(Path::to_path_buf))
            .collect()
    } else {
        allowed
    }
}

pub(crate) fn path_inside_roots(path: &Path, roots: &[PathBuf]) -> bool {
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    roots
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| path.starts_with(root))
}

pub(crate) fn rename_destination(target: &Path, new_stem: &str) -> Option<PathBuf> {
    let name = new_stem.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return None;
    }
    if !matches!(
        Path::new(name).components().next(),
        Some(Component::Normal(_))
    ) || Path::new(name).components().count() != 1
    {
        return None;
    }
    let parent = target.parent()?;
    let dest = match target.extension().and_then(|e| e.to_str()) {
        Some(ext) => parent.join(format!("{name}.{ext}")),
        None => parent.join(name),
    };
    if dest.exists() {
        return None;
    }
    Some(dest)
}

pub(crate) fn rename_preserving_extension(
    target: &Path,
    new_stem: &str,
) -> std::io::Result<Option<PathBuf>> {
    match rename_destination(target, new_stem) {
        Some(dest) => std::fs::rename(target, &dest).map(|_| Some(dest)),
        None => Ok(None),
    }
}

pub(crate) fn canonical_display(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

pub(crate) fn canonical_path_or_original(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

/// Find the markdown file with the most recent modification time under `root`.
/// Performs a depth-bounded walk (same depth limit as [`find_markdown`]) and
/// returns `None` when the directory is empty or unreadable.
pub(crate) fn latest_markdown(root: &Path) -> Option<PathBuf> {
    latest_markdown_in(root, 0)
}

fn latest_markdown_in(dir: &Path, depth: usize) -> Option<PathBuf> {
    const MAX_DEPTH: usize = 8;
    if depth >= MAX_DEPTH {
        return None;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            // Skip the same noisy directories as the sidebar tree.
            if name.starts_with('.') || matches!(name, "node_modules" | "target" | "dist" | "build")
            {
                continue;
            }
            if let Some((t, p)) = latest_markdown_in(&path, depth + 1)
                .and_then(|p| p.metadata().and_then(|m| m.modified()).ok().map(|t| (t, p)))
            {
                if best.as_ref().is_none_or(|(bt, _)| t > *bt) {
                    best = Some((t, p));
                }
            }
        } else if crate::files::is_markdown(&path) {
            if let Ok(mtime) = path.metadata().and_then(|m| m.modified()) {
                if best.as_ref().is_none_or(|(bt, _)| mtime > *bt) {
                    best = Some((mtime, path));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Wrap a rendered body in the markdown-pdf-style document and write it into
/// `dir`. Always uses the light theme for a white page. Returns the output path.
pub(crate) fn write_export_html(
    file: &Path,
    rendered_body: &str,
    dir: &Path,
    assets: &export::Assets,
) -> std::io::Result<PathBuf> {
    crate::perf::measure(
        "export.write_html",
        &[
            ("source", file.display().to_string()),
            ("body_bytes", rendered_body.len().to_string()),
            ("dir", dir.display().to_string()),
        ],
        || write_export_html_inner(file, rendered_body, dir, assets),
    )
}

const MAX_EXPORT_ATTEMPTS: usize = 100;

/// Return a path under `dir` that does not yet exist.
/// Tries `{title}.html`, then `{title} (1).html` … `{title} (N).html`.
fn unique_export_path(dir: &Path, title: &str) -> std::io::Result<PathBuf> {
    let base = dir.join(format!("{title}.html"));
    if !base.exists() {
        return Ok(base);
    }
    for n in 1..=MAX_EXPORT_ATTEMPTS {
        let candidate = dir.join(format!("{title} ({n}).html"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("export collision: exceeded {MAX_EXPORT_ATTEMPTS} attempts for \"{title}.html\""),
    ))
}

fn write_export_html_inner(
    file: &Path,
    rendered_body: &str,
    dir: &Path,
    assets: &export::Assets,
) -> std::io::Result<PathBuf> {
    let title = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document")
        .to_string();
    let doc = export::to_html(rendered_body, &title, assets);
    std::fs::create_dir_all(dir)?;
    let out = unique_export_path(dir, &title)?;
    std::fs::write(&out, doc).map(|_| out)
}

fn select_file_html() -> String {
    "<p>Select a markdown file in the sidebar…</p>".to_string()
}

fn failed_read_html(path: &Path) -> String {
    format!(
        "<p>Failed to read {}</p>",
        html_escape(&path.display().to_string())
    )
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn allowed_roots_for_file_adds_standalone_file_parent_to_existing_roots() {
        let project = tempdir().unwrap();
        let standalone = tempdir().unwrap();
        let roots = vec![project.path().to_path_buf()];

        let allowed = allowed_roots_for_file(standalone.path(), &roots);

        assert!(allowed.iter().any(|root| root == project.path()));
        assert!(allowed.iter().any(|root| root == standalone.path()));
    }

    #[test]
    fn allowed_roots_for_file_does_not_duplicate_existing_containing_root() {
        let project = tempdir().unwrap();
        let docs = project.path().join("docs");
        fs::create_dir(&docs).unwrap();
        let roots = vec![project.path().to_path_buf()];

        let allowed = allowed_roots_for_file(&docs, &roots);

        assert_eq!(allowed.len(), 1);
        assert_eq!(allowed[0], project.path());
    }

    #[test]
    fn path_inside_roots_rejects_outside_path() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("a.md");
        fs::write(&outside_file, "# outside").unwrap();

        assert!(!path_inside_roots(
            &outside_file,
            &[root.path().to_path_buf()]
        ));
    }

    #[test]
    fn document_snapshot_raw_html_escapes_markdown_text() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("a.md");
        fs::write(&file, "<script>alert(1)</script>").unwrap();

        let snapshot = load_document(Some(file), &[dir.path().to_path_buf()]);
        let html = snapshot.raw_html();

        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn document_snapshot_derives_render_raw_toc_and_find_from_one_source() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("a.md");
        fs::write(&file, "# Title\n\nneedle <script>x</script>").unwrap();

        let snapshot = load_document(Some(file.clone()), &[dir.path().to_path_buf()]);

        assert_eq!(snapshot.path(), Some(file.as_path()));
        assert!(snapshot.rendered_html().contains(r#"<h1 id="title">"#));
        assert!(snapshot.raw_html().contains("&lt;script&gt;"));
        assert_eq!(snapshot.toc().len(), 1);
        assert_eq!(snapshot.find_count("needle"), 1);
    }

    #[test]
    fn loading_snapshot_matches_target_path_without_stale_source() {
        let file = PathBuf::from("/tmp/loading.md");

        let snapshot = DocumentSnapshot::loading(Some(file.clone()));

        assert_eq!(snapshot.path(), Some(file.as_path()));
        assert!(snapshot.source().is_empty());
        assert!(snapshot.rendered_html().contains("Loading"));
        assert_eq!(snapshot.find_count("anything"), 0);
    }

    #[test]
    fn non_success_snapshots_keep_raw_placeholders() {
        let loading = DocumentSnapshot::loading(Some(PathBuf::from("/tmp/loading.md")));
        assert!(loading.raw_html().contains("Loading"));

        let empty = DocumentSnapshot::empty();
        assert!(empty.raw_html().contains("Select a markdown file"));

        let missing = PathBuf::from("/definitely/missing.md");
        let failed = load_document(Some(missing), &[]);
        assert!(failed.raw_html().contains("Failed to read"));
    }

    #[test]
    fn rename_destination_preserves_original_extension() {
        let path = Path::new("/tmp/old.md");

        let dest = rename_destination(path, "new name").unwrap();

        assert_eq!(dest, PathBuf::from("/tmp/new name.md"));
    }

    #[test]
    fn rename_destination_rejects_parent_components() {
        let path = Path::new("/tmp/old.md");

        assert!(rename_destination(path, "../escape").is_none());
    }

    #[test]
    fn rename_destination_rejects_path_separators() {
        let path = Path::new("/tmp/old.md");

        assert!(rename_destination(path, "nested/name").is_none());
        assert!(rename_destination(path, "nested\\name").is_none());
    }

    #[test]
    fn rename_destination_rejects_existing_destination() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("old.md");
        let existing = dir.path().join("taken.md");
        fs::write(&file, "# old").unwrap();
        fs::write(&existing, "# taken").unwrap();

        assert!(rename_destination(&file, "taken").is_none());
    }

    #[test]
    fn rename_preserving_extension_reports_blank_name_as_noop() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("old.md");
        fs::write(&file, "# old").unwrap();

        let renamed = rename_preserving_extension(&file, "   ").unwrap();

        assert_eq!(renamed, None);
        assert!(file.exists());
    }

    #[test]
    fn rename_preserving_extension_returns_destination_after_success() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("old.md");
        let expected = dir.path().join("new.md");
        fs::write(&file, "# old").unwrap();

        let renamed = rename_preserving_extension(&file, "new").unwrap();

        assert_eq!(renamed, Some(expected.clone()));
        assert!(!file.exists());
        assert!(expected.exists());
    }

    #[test]
    fn canonical_path_or_original_keeps_missing_path() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("missing.md");

        assert_eq!(canonical_path_or_original(missing.clone()), missing);
    }

    #[test]
    fn unique_export_pathは衝突がなければbase名を返す() {
        let dir = tempdir().unwrap();
        let path = unique_export_path(dir.path(), "report").unwrap();
        assert_eq!(path, dir.path().join("report.html"));
    }

    #[test]
    fn unique_export_pathは既存ファイルに連番を付与する() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("report.html"), "v1").unwrap();

        let path = unique_export_path(dir.path(), "report").unwrap();
        assert_eq!(path, dir.path().join("report (1).html"));
    }

    #[test]
    fn unique_export_pathは連続した衝突をスキップする() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("report.html"), "v1").unwrap();
        fs::write(dir.path().join("report (1).html"), "v2").unwrap();
        fs::write(dir.path().join("report (2).html"), "v3").unwrap();

        let path = unique_export_path(dir.path(), "report").unwrap();
        assert_eq!(path, dir.path().join("report (3).html"));
    }

    #[test]
    fn unique_export_pathは上限超過でエラーを返す() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("report.html"), "v0").unwrap();
        for n in 1..=MAX_EXPORT_ATTEMPTS {
            fs::write(dir.path().join(format!("report ({n}).html")), "vn").unwrap();
        }

        let err = unique_export_path(dir.path(), "report").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    }

    /// B1: opening a deeply nested file from the palette/search should expand
    /// all ancestor directories up to (and including) the project root.
    #[test]
    fn ancestor_dirs_multiはネストしたファイルの全祖先を返す() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let sub = root.join("docs").join("deep");
        fs::create_dir_all(&sub).unwrap();
        let file = sub.join("target.md");
        fs::write(&file, "# target").unwrap();

        let expanded = ancestor_dirs_multi(std::slice::from_ref(&root), &file);

        // Must contain docs/ and docs/deep/ (the parent chain up to root).
        assert!(
            expanded.contains(&root.join("docs")),
            "docs/ should be in expanded set"
        );
        assert!(
            expanded.contains(&root.join("docs").join("deep")),
            "docs/deep/ should be in expanded set"
        );
        // Root itself is included too.
        assert!(expanded.contains(&root));
    }

    #[test]
    fn latest_markdownは最終更新が最も新しいファイルを返す() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("sub")).unwrap();

        // Write files with a deliberate ordering: older first.
        let older = root.join("older.md");
        let newer = root.join("sub").join("newer.md");
        fs::write(&older, "# old").unwrap();
        // Bump newer's mtime by sleeping briefly so the OS records a later time.
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&newer, "# new").unwrap();

        let result = latest_markdown(root).unwrap();
        assert_eq!(
            result, newer,
            "should return the most recently modified file"
        );
    }

    #[test]
    fn latest_markdownは空ディレクトリでNoneを返す() {
        let dir = tempdir().unwrap();
        assert!(latest_markdown(dir.path()).is_none());
    }

    #[test]
    fn latest_markdownはignored_dirをスキップする() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Place a markdown in an ignored dir only — should still return None.
        fs::create_dir_all(root.join("node_modules")).unwrap();
        fs::write(root.join("node_modules").join("pkg.md"), "# pkg").unwrap();

        assert!(
            latest_markdown(root).is_none(),
            "files inside node_modules should be ignored"
        );
    }

    #[test]
    fn write_export_html_reports_directory_creation_failure() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("source.md");
        let not_dir = dir.path().join("not-dir");
        fs::write(&source, "# source").unwrap();
        fs::write(&not_dir, "file").unwrap();
        let assets = export::Assets {
            github_css: "",
            highlight_css: "",
            katex_css: "",
        };

        let result = write_export_html(&source, "<p>body</p>", &not_dir, &assets);

        assert!(result.is_err());
    }
}
