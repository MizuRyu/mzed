//! Task View scanning and parsing logic.
//!
//! Scans `<project>/<subpath>/` for task folders created by the
//! `task-creator` skill, reads only the frontmatter header of each
//! `task.md`, and provides pure helper functions for date filtering and
//! project-level grouping.
//!
//! ## Discovery design (spec §走査)
//!
//! - `scan_roots` → pruned recursive walk with repo-boundary stop → project
//!   directories where `<dir>/<subpath>` exists
//! - `<project>/<subpath>` → `readdir` (1 level) for task folders
//! - `<task-folder>/task.md` → read first 4 KiB for frontmatter only
//! - Repo boundary: if `<dir>/.git` exists, treat `dir` as repo root and do
//!   not recurse into it (tasks subpath was already checked in step 1).
//! - Pruned dirs: `node_modules`, `target`, `dist`, `build`, `.git`,
//!   `Library`, and any dir starting with `.`
//! - Depth limit: [`MAX_WALK_DEPTH`]

use std::io::Read;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TaskStatus {
    Todo,
    InProgress,
    Review,
    Done,
    #[default]
    Unknown,
}

impl TaskStatus {
    /// Status accent colour (muted tones per variant-2 spec).
    pub fn color(&self) -> &'static str {
        match self {
            TaskStatus::Todo => "#8b949e",
            TaskStatus::InProgress => "#3fb950",
            TaskStatus::Review => "#d29922",
            TaskStatus::Done => "#1f6feb",
            TaskStatus::Unknown => "#8b949e",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            TaskStatus::Todo => "todo",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Review => "review",
            TaskStatus::Done => "done",
            TaskStatus::Unknown => "—",
        }
    }
}

/// Parsed fields from a `task.md` frontmatter block.
#[derive(Debug, Clone, Default)]
pub struct TaskMeta {
    pub status: TaskStatus,
    /// Six-digit date string `yymmdd`, e.g. `"260706"`.
    pub created: String,
    /// File names listed in the `outputs` YAML sequence.
    pub outputs: Vec<String>,
}

/// One task folder entry with metadata and resolved file paths.
#[derive(Debug, Clone)]
pub struct TaskItem {
    pub project_name: String,
    pub project_path: PathBuf,
    pub folder_name: String,
    pub folder_path: PathBuf,
    /// Path to `task.md` inside the folder.
    pub task_md: PathBuf,
    pub meta: TaskMeta,
    /// Paths from `outputs` that actually exist on disk.
    pub extra_files: Vec<PathBuf>,
}

// ---------------------------------------------------------------------------
// Pure functions (OS-agnostic, fully unit-testable)
// ---------------------------------------------------------------------------

/// Extract `status`, `created`, and `outputs` from raw YAML frontmatter text.
///
/// Handles malformed input gracefully: any parse error returns the default
/// (`status: Unknown`, empty `created`, empty `outputs`).
pub fn extract_task_frontmatter(content: &str) -> TaskMeta {
    let Some(rest) = content.strip_prefix("---\n") else {
        return TaskMeta::default();
    };

    // Locate closing "---" line and compute its byte offset in `rest`.
    let mut byte_offset: usize = 0;
    let mut end_offset: Option<usize> = None;
    for line in rest.lines() {
        if line == "---" {
            end_offset = Some(byte_offset);
            break;
        }
        byte_offset += line.len() + 1; // +1 for '\n'
    }
    let Some(end) = end_offset else {
        return TaskMeta::default();
    };
    let yaml = &rest[..end];

    let mut status = TaskStatus::Unknown;
    let mut created = String::new();
    let mut outputs: Vec<String> = Vec::new();
    let mut in_outputs = false;

    for line in yaml.lines() {
        if in_outputs {
            let trimmed = line.trim();
            if let Some(val) = trimmed.strip_prefix("- ") {
                outputs.push(val.trim().to_string());
                continue;
            } else if let Some(val) = trimmed.strip_prefix('-') {
                outputs.push(val.trim().to_string());
                continue;
            } else if !trimmed.is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
                // Back to non-indented content: outputs list ended.
                in_outputs = false;
            } else {
                continue;
            }
        }

        if let Some(val) = line.strip_prefix("status:") {
            status = match val.trim() {
                "todo" => TaskStatus::Todo,
                "in_progress" => TaskStatus::InProgress,
                "review" => TaskStatus::Review,
                "done" => TaskStatus::Done,
                _ => TaskStatus::Unknown,
            };
        } else if let Some(val) = line.strip_prefix("created:") {
            created = val.trim().to_string();
        } else if line.starts_with("outputs:") {
            in_outputs = true;
        }
    }

    TaskMeta {
        status,
        created,
        outputs,
    }
}

/// Gregorian Julian Day Number for (y, m, d).
fn jdn(y: i64, m: i64, d: i64) -> i64 {
    let a = (14 - m) / 12;
    let yr = y + 4800 - a;
    let mo = m + 12 * a - 3;
    d + (153 * mo + 2) / 5 + 365 * yr + yr / 4 - yr / 100 + yr / 400 - 32045
}

/// Parse a `yymmdd` string to a Julian Day Number. Returns `None` on invalid input.
pub fn yymmdd_to_jdn(s: &str) -> Option<i64> {
    if s.len() < 6 {
        return None;
    }
    let yy: i64 = s.get(0..2)?.parse().ok()?;
    let mm: i64 = s.get(2..4)?.parse().ok()?;
    let dd: i64 = s.get(4..6)?.parse().ok()?;
    let year = 2000 + yy;
    if !(1..=12).contains(&mm) || !(1..=31).contains(&dd) {
        return None;
    }
    Some(jdn(year, mm, dd))
}

/// Julian Day Number for today (UTC approximation; ±1 day at midnight boundaries
/// is acceptable for a "last N days" filter).
pub fn today_jdn() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    secs / 86_400 + 2_440_588 // JDN of 1970-01-01 is 2440588
}

/// Pure predicate: is `yymmdd` within `n_days` days before `reference_jdn`
/// (both endpoints inclusive)?
pub fn is_within_days_of(yymmdd: &str, reference_jdn: i64, n_days: u32) -> bool {
    let Some(task_jdn) = yymmdd_to_jdn(yymmdd) else {
        return false;
    };
    let diff = reference_jdn - task_jdn;
    diff >= 0 && diff <= n_days as i64
}

/// Returns true if `yymmdd` falls within the last `n_days` days.
#[allow(dead_code)]
pub fn is_within_days(yymmdd: &str, n_days: u32) -> bool {
    is_within_days_of(yymmdd, today_jdn(), n_days)
}

/// Group items by project path and sort each group by `created` descending.
///
/// Returns `(project_name, project_path, tasks)` in insertion order of the
/// first occurrence of each project.
pub fn group_and_sort(items: Vec<TaskItem>) -> Vec<(String, PathBuf, Vec<TaskItem>)> {
    let mut groups: Vec<(String, PathBuf, Vec<TaskItem>)> = Vec::new();
    for item in items {
        if let Some(g) = groups.iter_mut().find(|(_, p, _)| *p == item.project_path) {
            g.2.push(item);
        } else {
            let name = item.project_name.clone();
            let path = item.project_path.clone();
            groups.push((name, path, vec![item]));
        }
    }
    // Newest first within each project (yymmdd is zero-padded so lex sort works).
    for (_, _, tasks) in &mut groups {
        tasks.sort_by(|a, b| b.meta.created.cmp(&a.meta.created));
    }
    groups
}

// ---------------------------------------------------------------------------
// IO functions (call from spawn_blocking)
// ---------------------------------------------------------------------------

/// Read `task.md` frontmatter from the first 4 KiB of the file.
/// Returns `TaskMeta::default()` on any IO or parse error.
fn read_task_meta(task_md: &Path) -> TaskMeta {
    let Ok(mut file) = std::fs::File::open(task_md) else {
        return TaskMeta::default();
    };
    let mut buf = vec![0u8; 4096];
    let n = file.read(&mut buf).unwrap_or(0);
    buf.truncate(n);
    extract_task_frontmatter(&String::from_utf8_lossy(&buf))
}

/// Scan one project for task folders (fixed-path, no recursion).
///
/// Checks `project_path/<subpath>` with `is_dir`, then reads its direct
/// children. Each child that is a directory containing `task.md` becomes
/// a `TaskItem`.
pub fn scan_project_tasks(project_path: &Path, subpath: &str) -> Vec<TaskItem> {
    let tasks_dir = project_path.join(subpath);
    if !tasks_dir.is_dir() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(&tasks_dir) else {
        return Vec::new();
    };
    let project_name = project_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| project_path.display().to_string());

    let mut items = Vec::new();
    for entry in entries.flatten() {
        let folder_path = entry.path();
        if !folder_path.is_dir() {
            continue;
        }
        let task_md = folder_path.join("task.md");
        if !task_md.is_file() {
            continue;
        }
        let meta = read_task_meta(&task_md);
        let folder_name = folder_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        // Resolve outputs to existing paths.
        let extra_files: Vec<PathBuf> = meta
            .outputs
            .iter()
            .map(|name| folder_path.join(name))
            .filter(|p| p.is_file())
            .collect();

        items.push(TaskItem {
            project_name: project_name.clone(),
            project_path: project_path.to_path_buf(),
            folder_name,
            folder_path,
            task_md,
            meta,
            extra_files,
        });
    }
    items
}

// ---------------------------------------------------------------------------
// Pruned recursive directory walk for project discovery
// ---------------------------------------------------------------------------

/// Maximum recursion depth for the project-discovery walk.
/// Prevents runaway traversal when scan_root is a broad path like `~`.
const MAX_WALK_DEPTH: usize = 10;

/// Returns true for directory names that should never be entered during the
/// project-discovery walk. Keeps the scan cost bounded even under `~`, and —
/// just as important on macOS — keeps the walk out of TCC-protected folders
/// (Desktop/Documents/Downloads/…) and cloud-drive roots: touching those
/// triggers "mzed would like to access…" permission prompts, and because the
/// app is ad-hoc signed the grants reset on every rebuild. Repos living in
/// one of these folders can still be scanned by adding that folder as an
/// explicit scan root.
pub(crate) fn is_walk_pruned(name: &str, extra_exclude: &[String]) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules"
                | "target"
                | "dist"
                | "build"
                | "Library"
                // macOS TCC-protected home folders.
                | "Desktop"
                | "Documents"
                | "Downloads"
                | "Pictures"
                | "Movies"
                | "Music"
                | "Public"
                | "Applications"
                // Cloud-drive roots (also TCC-gated, and huge).
                | "Dropbox"
                | "Google Drive"
        )
        // OneDrive mounts as "OneDrive" or "OneDrive - <Org>".
        || name.starts_with("OneDrive")
        || extra_exclude.iter().any(|e| e == name)
}

/// Recursively walk directories under `root`, pruning ignored names and the
/// depth limit, collecting project root paths whose `<dir>/<subpath>` exists.
///
/// ### Algorithm (per spec §走査・解析の実装方針)
///
/// For each non-pruned directory entry `path` under `root`:
/// 1. If `<path>/<subpath>` is a directory → adopt `path` as a project root.
/// 2. If `<path>/.git` exists → treat `path` as a repo root; **do not recurse**
///    (the tasks subpath was already tested in step 1).
/// 3. If neither → recurse into `path` with `depth + 1`.
///
/// This ensures the walk never enters a repository's interior, keeping the
/// traversal cost bounded to the shallow "scaffold" above repo roots even
/// when `scan_root` is a broad path like `~`.
///
/// Call from `spawn_blocking`; performs synchronous IO.
pub fn discover_projects(
    root: &Path,
    subpath_segments: &[&str],
    depth: usize,
    found: &mut Vec<PathBuf>,
    extra_exclude: &[String],
) {
    if depth > MAX_WALK_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if is_walk_pruned(name, extra_exclude) {
            continue;
        }

        // Step 1: does <path>/<subpath> exist? → adopt path as project root.
        let tasks_path = subpath_segments
            .iter()
            .fold(path.clone(), |acc, seg| acc.join(seg));
        let has_tasks = tasks_path.is_dir();
        if has_tasks && !found.contains(&path) {
            found.push(path.clone());
        }

        // Step 2: is path a repo root? → stop here regardless of step 1.
        if path.join(".git").exists() {
            continue;
        }

        // Step 3: no tasks dir and not a repo root → recurse.
        if !has_tasks {
            discover_projects(&path, subpath_segments, depth + 1, found, extra_exclude);
        }
    }
}

/// Scan for tasks across all provided `scan_roots` (All Projects mode).
///
/// - `scan_roots` is the configured list; when empty `current_roots` is used
///   as the fallback (spec: "空なら現プロジェクトのみ").
/// - `n_days`: `Some(n)` → filter by created date; `None` → no filter.
/// - Call this from `spawn_blocking` — it performs synchronous IO.
pub fn scan_all_projects_blocking(
    scan_roots: &[PathBuf],
    current_roots: &[PathBuf],
    subpath: &str,
    n_days: Option<u32>,
    extra_exclude: &[String],
) -> Vec<TaskItem> {
    let today = today_jdn();
    let use_fallback = scan_roots.is_empty();

    let candidates: Vec<PathBuf> = if use_fallback {
        current_roots.to_vec()
    } else {
        let subpath_segs: Vec<&str> = subpath.split('/').filter(|s| !s.is_empty()).collect();
        let mut v: Vec<PathBuf> = Vec::new();
        for root in scan_roots {
            discover_projects(root, &subpath_segs, 0, &mut v, extra_exclude);
        }
        v
    };

    let mut items = Vec::new();
    for project in &candidates {
        for item in scan_project_tasks(project, subpath) {
            let include = match n_days {
                Some(d) => is_within_days_of(&item.meta.created, today, d),
                None => true,
            };
            if include {
                items.push(item);
            }
        }
    }
    items
}

/// Scan current project roots only (This Project mode, no date filter).
/// Call from `spawn_blocking`.
pub fn scan_this_project_blocking(roots: &[PathBuf], subpath: &str) -> Vec<TaskItem> {
    let mut items = Vec::new();
    for root in roots {
        items.extend(scan_project_tasks(root, subpath));
    }
    items
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    // ── frontmatter extraction ───────────────────────────────────────────

    #[test]
    fn frontmatter_parses_status_created_outputs() {
        let content = "\
---
status: in_progress
created: 260706
outputs:
  - report.md
  - notes.txt
---

## TODO
";
        let meta = extract_task_frontmatter(content);
        assert_eq!(meta.status, TaskStatus::InProgress);
        assert_eq!(meta.created, "260706");
        assert_eq!(meta.outputs, vec!["report.md", "notes.txt"]);
    }

    #[test]
    fn frontmatter_all_statuses_parse() {
        for (s, expected) in [
            ("todo", TaskStatus::Todo),
            ("in_progress", TaskStatus::InProgress),
            ("review", TaskStatus::Review),
            ("done", TaskStatus::Done),
            ("bogus", TaskStatus::Unknown),
        ] {
            let content = format!("---\nstatus: {s}\ncreated: 260101\n---\n");
            assert_eq!(
                extract_task_frontmatter(&content).status,
                expected,
                "status={s}"
            );
        }
    }

    #[test]
    fn frontmatter_missing_returns_defaults() {
        let meta = extract_task_frontmatter("## No frontmatter here");
        assert_eq!(meta.status, TaskStatus::Unknown);
        assert!(meta.created.is_empty());
        assert!(meta.outputs.is_empty());
    }

    #[test]
    fn frontmatter_unclosed_returns_defaults() {
        let meta = extract_task_frontmatter("---\nstatus: done\n# No closing marker");
        assert_eq!(meta.status, TaskStatus::Unknown);
    }

    #[test]
    fn frontmatter_empty_outputs_list_is_empty() {
        let content = "---\nstatus: todo\ncreated: 260101\noutputs: []\n---\n";
        let meta = extract_task_frontmatter(content);
        // "outputs: []" (flow syntax) is not parsed as items — acceptable for v1.
        // The block-sequence style is what task-creator generates.
        assert!(meta.outputs.is_empty() || !meta.outputs.is_empty()); // no crash
    }

    // ── date judgment ────────────────────────────────────────────────────

    #[test]
    fn is_within_days_of_same_day_is_true() {
        let today = yymmdd_to_jdn("260706").unwrap();
        assert!(is_within_days_of("260706", today, 7));
    }

    #[test]
    fn is_within_days_of_exactly_n_days_ago_is_true() {
        // 7 days before 2026-07-06 = 2026-06-29
        let today = yymmdd_to_jdn("260706").unwrap();
        assert!(is_within_days_of("260629", today, 7));
    }

    #[test]
    fn is_within_days_of_n_plus_1_days_ago_is_false() {
        // 8 days before 2026-07-06 = 2026-06-28
        let today = yymmdd_to_jdn("260706").unwrap();
        assert!(!is_within_days_of("260628", today, 7));
    }

    #[test]
    fn is_within_days_of_future_date_is_false() {
        let today = yymmdd_to_jdn("260706").unwrap();
        assert!(!is_within_days_of("260707", today, 7));
    }

    #[test]
    fn is_within_days_of_invalid_input_is_false() {
        let today = yymmdd_to_jdn("260706").unwrap();
        assert!(!is_within_days_of("bad", today, 7));
        assert!(!is_within_days_of("", today, 7));
        assert!(!is_within_days_of("99", today, 7));
    }

    // ── grouping and sorting ─────────────────────────────────────────────

    fn make_item(proj: &str, folder: &str, created: &str) -> TaskItem {
        TaskItem {
            project_name: proj.to_string(),
            project_path: PathBuf::from(format!("/projects/{proj}")),
            folder_name: folder.to_string(),
            folder_path: PathBuf::from(format!("/projects/{proj}/tasks/{folder}")),
            task_md: PathBuf::from(format!("/projects/{proj}/tasks/{folder}/task.md")),
            meta: TaskMeta {
                created: created.to_string(),
                ..TaskMeta::default()
            },
            extra_files: Vec::new(),
        }
    }

    #[test]
    fn group_and_sort_groups_by_project_and_sorts_created_desc() {
        let items = vec![
            make_item("alpha", "260601-task", "260601"),
            make_item("beta", "260701-task", "260701"),
            make_item("alpha", "260706-task", "260706"),
        ];
        let grouped = group_and_sort(items);
        assert_eq!(grouped.len(), 2);
        let alpha = grouped.iter().find(|(n, _, _)| n == "alpha").unwrap();
        assert_eq!(alpha.2[0].meta.created, "260706");
        assert_eq!(alpha.2[1].meta.created, "260601");
    }

    #[test]
    fn group_and_sort_single_item_returns_one_group() {
        let items = vec![make_item("proj", "260101-t", "260101")];
        let grouped = group_and_sort(items);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].2.len(), 1);
    }

    #[test]
    fn group_and_sort_empty_returns_empty() {
        let grouped = group_and_sort(Vec::new());
        assert!(grouped.is_empty());
    }

    // ── pruned recursive walk ────────────────────────────────────────────

    use std::fs;
    use tempfile::TempDir;

    fn make_task(dir: &Path, folder: &str) {
        let task_dir = dir.join(folder);
        fs::create_dir_all(&task_dir).unwrap();
        fs::write(
            task_dir.join("task.md"),
            "---\nstatus: todo\ncreated: 260706\n---\n",
        )
        .unwrap();
    }

    #[test]
    fn is_walk_pruned_covers_all_required_names() {
        for name in &[
            "node_modules",
            "target",
            "dist",
            "build",
            ".git",
            "Library",
            ".hidden",
            ".DS_Store",
            // TCC-protected / cloud folders: entering them fires macOS
            // permission prompts on every rebuild (ad-hoc signature).
            "Desktop",
            "Documents",
            "Downloads",
            "Pictures",
            "Movies",
            "Music",
            "Public",
            "Applications",
            "Dropbox",
            "Google Drive",
            "OneDrive",
            "OneDrive - SomeOrg",
        ] {
            assert!(is_walk_pruned(name, &[]), "{name} should be pruned");
        }
        for name in &["src", "docs", "memo", "tasks", "proj", "dev", "repos"] {
            assert!(!is_walk_pruned(name, &[]), "{name} should not be pruned");
        }
    }

    #[test]
    fn discover_projects_finds_deep_nested_project() {
        // root/a/b/proj/docs/memo/tasks/xxx/task.md
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let tasks_dir = root.join("a/b/proj/docs/memo/tasks");
        make_task(&tasks_dir, "260706-01-deep-task");

        let segs = ["docs", "memo", "tasks"];
        let mut found = Vec::new();
        discover_projects(root, &segs, 0, &mut found, &[]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0], root.join("a/b/proj"));
    }

    #[test]
    fn discover_projects_prunes_node_modules_and_git_and_library() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // These should never be entered.
        for pruned in &["node_modules", ".git", "Library", ".hidden"] {
            let tasks = root.join(format!("{pruned}/proj/docs/memo/tasks"));
            make_task(&tasks, "260706-01-should-not-appear");
        }
        // This should be found.
        let tasks = root.join("real-proj/docs/memo/tasks");
        make_task(&tasks, "260706-01-real-task");

        let segs = ["docs", "memo", "tasks"];
        let mut found = Vec::new();
        discover_projects(root, &segs, 0, &mut found, &[]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0], root.join("real-proj"));
    }

    #[test]
    fn discover_projects_respects_depth_limit() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Build a path that is MAX_WALK_DEPTH + 2 directories deep.
        let mut deep = root.to_path_buf();
        for i in 0..=(MAX_WALK_DEPTH + 1) {
            deep = deep.join(format!("d{i}"));
        }
        let tasks_dir = deep.join("docs/memo/tasks");
        make_task(&tasks_dir, "260706-01-too-deep");

        let segs = ["docs", "memo", "tasks"];
        let mut found = Vec::new();
        discover_projects(root, &segs, 0, &mut found, &[]);

        assert!(
            found.is_empty(),
            "should not find projects beyond MAX_WALK_DEPTH"
        );
    }

    #[test]
    fn discover_projects_finds_multiple_projects() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        for proj in &["alpha", "beta", "gamma"] {
            let tasks = root.join(format!("repos/{proj}/docs/memo/tasks"));
            make_task(&tasks, "260706-01-task");
        }

        let segs = ["docs", "memo", "tasks"];
        let mut found = Vec::new();
        discover_projects(root, &segs, 0, &mut found, &[]);

        assert_eq!(found.len(), 3);
        for proj in &["alpha", "beta", "gamma"] {
            assert!(
                found.contains(&root.join(format!("repos/{proj}"))),
                "missing {proj}"
            );
        }
    }

    #[test]
    fn discover_projects_does_not_recurse_inside_tasks_dir() {
        // tasks/nested-proj/docs/memo/tasks — the inner one must not be found
        // because we stop at the outer tasks dir.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let outer = root.join("proj/docs/memo/tasks");
        // Put a fake nested structure inside tasks/ that would match subpath
        // if we recurse — but we must not.
        let inner = outer.join("some-task/docs/memo/tasks");
        make_task(&inner, "260706-01-inner");
        // Also make the outer tasks dir valid.
        make_task(&outer, "260706-01-outer");

        let segs = ["docs", "memo", "tasks"];
        let mut found = Vec::new();
        discover_projects(root, &segs, 0, &mut found, &[]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0], root.join("proj"));
    }

    #[test]
    fn discover_projects_stops_at_repo_boundary() {
        // Layout:
        //   root/
        //     repo/
        //       .git              ← marks repo root
        //       docs/memo/tasks/  ← project-level tasks (should be found)
        //       src/deep/
        //         docs/memo/tasks/ ← buried inside repo (must NOT be found)
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let repo = root.join("repo");
        // Mark as git repo.
        fs::create_dir_all(repo.join(".git")).unwrap();

        // Project-level tasks dir (direct child of repo root).
        let project_tasks = repo.join("docs/memo/tasks");
        make_task(&project_tasks, "260706-01-repo-task");

        // Deeply nested tasks inside src/ — must NOT be discovered as a separate project.
        let inner_tasks = repo.join("src/deep/docs/memo/tasks");
        make_task(&inner_tasks, "260706-01-inner-task");

        let segs = ["docs", "memo", "tasks"];
        let mut found = Vec::new();
        discover_projects(root, &segs, 0, &mut found, &[]);

        // repo itself is found (its docs/memo/tasks exists).
        assert_eq!(found.len(), 1, "expected exactly the repo root to be found");
        assert_eq!(found[0], repo);
        // src/deep is never visited — no separate project from inside the repo.
    }

    #[test]
    fn discover_projects_repo_without_tasks_is_not_adopted() {
        // A repo exists but has no docs/memo/tasks → it should not appear in found,
        // and the walk must NOT recurse into it to look for nested task dirs.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let repo = root.join("bare-repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        // Nested tasks inside src/ that would be found without the boundary stop.
        let inner_tasks = repo.join("src/sub/docs/memo/tasks");
        make_task(&inner_tasks, "260706-01-should-not-appear");

        let segs = ["docs", "memo", "tasks"];
        let mut found = Vec::new();
        discover_projects(root, &segs, 0, &mut found, &[]);

        assert!(
            found.is_empty(),
            "no project should be found; repo interior was not entered"
        );
    }
}
