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
//!   directories where `<dir>/<subpath>` exists (a scan root itself is also
//!   checked, and adopting a non-repo directory does not stop the walk —
//!   nested projects underneath are still discovered)
//! - `<project>/<subpath>` → `readdir` (1 level) for task folders
//! - `<task-folder>/task.md` → read first 4 KiB for frontmatter only
//! - `<task-folder>/*` → all other files listed as-is (1 level, dot-files
//!   excluded, sorted by name)
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

    /// Frontmatter value / config key. Unlike [`label`], `Unknown` gets a real
    /// key so it can be ordered and addressed like any other status.
    pub fn key(&self) -> &'static str {
        match self {
            TaskStatus::Unknown => "unknown",
            other => other.label(),
        }
    }

    /// Heading text for the Task View status groups.
    pub fn heading(&self) -> &'static str {
        match self {
            TaskStatus::Todo => "未着手",
            TaskStatus::InProgress => "対応中",
            TaskStatus::Review => "レビュー",
            TaskStatus::Done => "完了",
            TaskStatus::Unknown => "その他",
        }
    }

    /// Parse a config/frontmatter key back into a status.
    pub fn from_key(key: &str) -> TaskStatus {
        match key {
            "todo" => TaskStatus::Todo,
            "in_progress" => TaskStatus::InProgress,
            "review" => TaskStatus::Review,
            "done" => TaskStatus::Done,
            _ => TaskStatus::Unknown,
        }
    }
}

/// Parsed fields from a `task.md` frontmatter block.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskMeta {
    pub status: TaskStatus,
    /// Six-digit date string `yymmdd`, e.g. `"260706"`.
    pub created: String,
}

/// One task folder entry with metadata and resolved file paths.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskItem {
    pub project_name: String,
    pub project_path: PathBuf,
    pub folder_name: String,
    pub folder_path: PathBuf,
    /// Path to `task.md` inside the folder.
    pub task_md: PathBuf,
    pub meta: TaskMeta,
    /// Everything else in the task folder as a small tree (subdirectories
    /// included, dot-entries excluded, dirs first then files by name).
    pub extra_files: Vec<TaskFileNode>,
}

/// One entry inside a task folder: a file, or a subdirectory with children.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskFileNode {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub children: Vec<TaskFileNode>,
}

// ---------------------------------------------------------------------------
// Pure functions (OS-agnostic, fully unit-testable)
// ---------------------------------------------------------------------------

/// Extract `status` and `created` from raw YAML frontmatter text.
///
/// Handles malformed input gracefully: any parse error returns the default
/// (`status: Unknown`, empty `created`).
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

    for line in yaml.lines() {
        if let Some(val) = line.strip_prefix("status:") {
            status = TaskStatus::from_key(val.trim());
        } else if let Some(val) = line.strip_prefix("created:") {
            created = val.trim().to_string();
        }
    }

    TaskMeta { status, created }
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

/// What a group heading in the Task View tree stands for.
#[derive(Debug, Clone, PartialEq)]
pub enum GroupKey {
    Project { name: String, path: PathBuf },
    Status(TaskStatus),
}

/// One heading in the Task View tree. Either it nests further groups
/// (`children`) or it holds the tasks themselves (`tasks`) — never both.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskGroup {
    pub key: GroupKey,
    /// Stable identity for expand/collapse state, unique across the tree.
    pub id: String,
    pub children: Vec<TaskGroup>,
    pub tasks: Vec<TaskItem>,
}

impl TaskGroup {
    /// Tasks under this group, including those in nested groups.
    pub fn task_count(&self) -> usize {
        self.tasks.len() + self.children.iter().map(|c| c.task_count()).sum::<usize>()
    }
}

/// Sort tasks in place by `created` (yymmdd is zero-padded, so lexical order
/// is chronological). Ties keep their scan order.
fn sort_tasks(tasks: &mut [TaskItem], date_desc: bool) {
    if date_desc {
        tasks.sort_by(|a, b| b.meta.created.cmp(&a.meta.created));
    } else {
        tasks.sort_by(|a, b| a.meta.created.cmp(&b.meta.created));
    }
}

/// Rank of a status in the configured heading order. Statuses missing from
/// the configured order (and `Unknown`) sort after every configured one.
fn status_rank(status: &TaskStatus, status_order: &[String]) -> usize {
    status_order
        .iter()
        .position(|s| s == status.key())
        .unwrap_or(usize::MAX)
}

/// Group tasks by project, preserving first-seen project order.
fn by_project(items: Vec<TaskItem>) -> Vec<(String, PathBuf, Vec<TaskItem>)> {
    let mut groups: Vec<(String, PathBuf, Vec<TaskItem>)> = Vec::new();
    for item in items {
        if let Some(g) = groups.iter_mut().find(|(_, p, _)| *p == item.project_path) {
            g.2.push(item);
        } else {
            let (name, path) = (item.project_name.clone(), item.project_path.clone());
            groups.push((name, path, vec![item]));
        }
    }
    groups
}

/// Group tasks by status, ordered by `status_order` (unconfigured statuses last,
/// then by the enum's own order so the result is deterministic).
fn by_status(items: Vec<TaskItem>, status_order: &[String]) -> Vec<(TaskStatus, Vec<TaskItem>)> {
    let mut groups: Vec<(TaskStatus, Vec<TaskItem>)> = Vec::new();
    for item in items {
        let status = item.meta.status.clone();
        if let Some(g) = groups.iter_mut().find(|(s, _)| *s == status) {
            g.1.push(item);
        } else {
            groups.push((status, vec![item]));
        }
    }
    groups.sort_by_key(|(s, _)| (status_rank(s, status_order), s.key()));
    groups
}

/// Build the Task View tree.
///
/// - `group_by_status == false` → one level: project → tasks.
/// - `project_first` → project → status → tasks.
/// - otherwise → status → project → tasks.
///
/// Projects keep their first-seen order; statuses follow `status_order`
/// (unconfigured ones last); tasks sort by `created` per `date_desc`.
pub fn build_groups(
    items: Vec<TaskItem>,
    group_by_status: bool,
    project_first: bool,
    status_order: &[String],
    date_desc: bool,
) -> Vec<TaskGroup> {
    if !group_by_status {
        return by_project(items)
            .into_iter()
            .map(|(name, path, mut tasks)| {
                sort_tasks(&mut tasks, date_desc);
                TaskGroup {
                    id: path.display().to_string(),
                    key: GroupKey::Project { name, path },
                    children: Vec::new(),
                    tasks,
                }
            })
            .collect();
    }

    if project_first {
        by_project(items)
            .into_iter()
            .map(|(name, path, tasks)| {
                let id = path.display().to_string();
                let children = by_status(tasks, status_order)
                    .into_iter()
                    .map(|(status, mut tasks)| {
                        sort_tasks(&mut tasks, date_desc);
                        TaskGroup {
                            id: format!("{id}\u{1}{}", status.key()),
                            key: GroupKey::Status(status),
                            children: Vec::new(),
                            tasks,
                        }
                    })
                    .collect();
                TaskGroup {
                    id,
                    key: GroupKey::Project { name, path },
                    children,
                    tasks: Vec::new(),
                }
            })
            .collect()
    } else {
        by_status(items, status_order)
            .into_iter()
            .map(|(status, tasks)| {
                let id = format!("status\u{1}{}", status.key());
                let children = by_project(tasks)
                    .into_iter()
                    .map(|(name, path, mut tasks)| {
                        sort_tasks(&mut tasks, date_desc);
                        TaskGroup {
                            id: format!("{id}\u{1}{}", path.display()),
                            key: GroupKey::Project { name, path },
                            children: Vec::new(),
                            tasks,
                        }
                    })
                    .collect();
                TaskGroup {
                    id,
                    key: GroupKey::Status(status),
                    children,
                    tasks: Vec::new(),
                }
            })
            .collect()
    }
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

/// Recursion limit for the task-folder listing. Task folders are shallow by
/// convention; this only guards against symlink cycles and runaway nesting.
const TASK_FILES_MAX_DEPTH: usize = 6;

/// List everything in a task folder except `task.md` (top level only) and
/// dot-entries, as a tree: subdirectories recurse, empty ones are dropped.
/// Dirs first, then files, each sorted by name.
fn list_task_files(folder_path: &Path) -> Vec<TaskFileNode> {
    fn walk(dir: &Path, depth: usize, skip_task_md: bool) -> Vec<TaskFileNode> {
        if depth >= TASK_FILES_MAX_DEPTH {
            return Vec::new();
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut dirs: Vec<TaskFileNode> = Vec::new();
        let mut files: Vec<TaskFileNode> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()).map(String::from) else {
                continue;
            };
            if name.starts_with('.') || (skip_task_md && name == "task.md") {
                continue;
            }
            if path.is_dir() {
                let children = walk(&path, depth + 1, false);
                if !children.is_empty() {
                    dirs.push(TaskFileNode {
                        name,
                        path,
                        is_dir: true,
                        children,
                    });
                }
            } else if path.is_file() {
                files.push(TaskFileNode {
                    name,
                    path,
                    is_dir: false,
                    children: Vec::new(),
                });
            }
        }
        dirs.sort_by(|a, b| a.name.cmp(&b.name));
        files.sort_by(|a, b| a.name.cmp(&b.name));
        dirs.into_iter().chain(files).collect()
    }
    walk(folder_path, 0, true)
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
        let extra_files = list_task_files(&folder_path);

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
/// For each non-pruned directory entry `path` under `root` (except the one
/// named `skip_child`, if any):
/// 1. If `<path>/<subpath>` is a directory → adopt `path` as a project root.
/// 2. If `<path>/.git` exists → treat `path` as a repo root; **do not recurse**
///    (the tasks subpath was already tested in step 1).
/// 3. Otherwise recurse into `path` with `depth + 1` — even when `path` was
///    adopted, so nested projects under a "scaffold" project are found. In
///    that case the child matching the first subpath segment is skipped so
///    the walk never enters the adopted project's own tasks tree.
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
    skip_child: Option<&str>,
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
        if is_walk_pruned(name, extra_exclude) || Some(name) == skip_child {
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

        // Step 3: recurse. If path was adopted, keep going (nested projects)
        // but never descend into its own subpath scaffold.
        let skip = if has_tasks {
            subpath_segments.first().copied()
        } else {
            None
        };
        discover_projects(
            &path,
            subpath_segments,
            depth + 1,
            found,
            extra_exclude,
            skip,
        );
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
            // The scan root itself may be a project (e.g. a project root
            // passed directly instead of a parent directory).
            let root_tasks = subpath_segs
                .iter()
                .fold(root.clone(), |acc, seg| acc.join(seg));
            let root_has_tasks = root_tasks.is_dir();
            if root_has_tasks && !v.contains(root) {
                v.push(root.clone());
            }
            let skip = if root_has_tasks {
                subpath_segs.first().copied()
            } else {
                None
            };
            discover_projects(root, &subpath_segs, 0, &mut v, extra_exclude, skip);
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
    fn frontmatter_parses_status_and_created() {
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
    }

    #[test]
    fn frontmatter_unclosed_returns_defaults() {
        let meta = extract_task_frontmatter("---\nstatus: done\n# No closing marker");
        assert_eq!(meta.status, TaskStatus::Unknown);
    }

    #[test]
    fn list_task_files_lists_all_but_task_md_and_dotfiles() {
        let tmp = TempDir::new().unwrap();
        let folder = tmp.path().join("260710-01-task");
        make_task(tmp.path(), "260710-01-task");
        for name in &["b-notes.txt", "a-report.md", "image.png", ".DS_Store"] {
            fs::write(folder.join(name), b"x").unwrap();
        }
        fs::create_dir(folder.join("empty-subdir")).unwrap();

        let names: Vec<String> = list_task_files(&folder)
            .iter()
            .map(|n| n.name.clone())
            .collect();
        // task.md, dot-files, and empty directories are excluded; name order.
        assert_eq!(names, vec!["a-report.md", "b-notes.txt", "image.png"]);
    }

    #[test]
    fn list_task_files_recurses_into_subdirectories() {
        let tmp = TempDir::new().unwrap();
        let folder = tmp.path().join("260718-01-task");
        make_task(tmp.path(), "260718-01-task");
        fs::write(folder.join("root.md"), b"x").unwrap();
        fs::create_dir_all(folder.join("mvp/assets")).unwrap();
        fs::write(folder.join("mvp/DESIGN.md"), b"x").unwrap();
        fs::write(folder.join("mvp/assets/logo.png"), b"x").unwrap();
        // A nested task.md is NOT special below the top level.
        fs::write(folder.join("mvp/task.md"), b"x").unwrap();

        let entries = list_task_files(&folder);
        // Dirs first, then files.
        assert_eq!(entries[0].name, "mvp");
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].name, "root.md");

        let mvp = &entries[0];
        let names: Vec<&str> = mvp.children.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["assets", "DESIGN.md", "task.md"]);
        assert_eq!(mvp.children[0].children[0].name, "logo.png");
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
        make_item_st(proj, folder, created, TaskStatus::Todo)
    }

    fn make_item_st(proj: &str, folder: &str, created: &str, status: TaskStatus) -> TaskItem {
        TaskItem {
            project_name: proj.to_string(),
            project_path: PathBuf::from(format!("/projects/{proj}")),
            folder_name: folder.to_string(),
            folder_path: PathBuf::from(format!("/projects/{proj}/tasks/{folder}")),
            task_md: PathBuf::from(format!("/projects/{proj}/tasks/{folder}/task.md")),
            meta: TaskMeta {
                created: created.to_string(),
                status,
            },
            extra_files: Vec::new(),
        }
    }

    fn default_order() -> Vec<String> {
        crate::config::default_task_view_status_order()
    }

    fn project_name(g: &TaskGroup) -> &str {
        match &g.key {
            GroupKey::Project { name, .. } => name,
            GroupKey::Status(_) => panic!("expected a project group"),
        }
    }

    fn status_of(g: &TaskGroup) -> &TaskStatus {
        match &g.key {
            GroupKey::Status(s) => s,
            GroupKey::Project { .. } => panic!("expected a status group"),
        }
    }

    #[test]
    fn build_groups_flat_groups_by_project_and_sorts_created_desc() {
        let items = vec![
            make_item("alpha", "260601-task", "260601"),
            make_item("beta", "260701-task", "260701"),
            make_item("alpha", "260706-task", "260706"),
        ];
        let groups = build_groups(items, false, true, &default_order(), true);
        assert_eq!(groups.len(), 2);
        let alpha = groups.iter().find(|g| project_name(g) == "alpha").unwrap();
        assert!(alpha.children.is_empty());
        assert_eq!(alpha.tasks[0].meta.created, "260706");
        assert_eq!(alpha.tasks[1].meta.created, "260601");
    }

    #[test]
    fn build_groups_flat_ascending_reverses_task_order() {
        let items = vec![
            make_item("alpha", "260706-task", "260706"),
            make_item("alpha", "260601-task", "260601"),
        ];
        let groups = build_groups(items, false, true, &default_order(), false);
        assert_eq!(groups[0].tasks[0].meta.created, "260601");
        assert_eq!(groups[0].tasks[1].meta.created, "260706");
    }

    #[test]
    fn build_groups_project_first_nests_status_in_configured_order() {
        let items = vec![
            make_item_st("alpha", "260701-d", "260701", TaskStatus::Done),
            make_item_st("alpha", "260702-t", "260702", TaskStatus::Todo),
            make_item_st("alpha", "260703-p", "260703", TaskStatus::InProgress),
        ];
        let groups = build_groups(items, true, true, &default_order(), true);
        assert_eq!(groups.len(), 1);
        let statuses: Vec<&TaskStatus> = groups[0].children.iter().map(status_of).collect();
        assert_eq!(
            statuses,
            vec![
                &TaskStatus::Todo,
                &TaskStatus::InProgress,
                &TaskStatus::Done
            ]
        );
        assert_eq!(groups[0].task_count(), 3);
        assert!(groups[0].tasks.is_empty(), "parent holds no tasks directly");
    }

    #[test]
    fn build_groups_honours_custom_status_order() {
        let items = vec![
            make_item_st("alpha", "260701-t", "260701", TaskStatus::Todo),
            make_item_st("alpha", "260702-d", "260702", TaskStatus::Done),
        ];
        let order = vec!["done".to_string(), "todo".to_string()];
        let groups = build_groups(items, true, true, &order, true);
        let statuses: Vec<&TaskStatus> = groups[0].children.iter().map(status_of).collect();
        assert_eq!(statuses, vec![&TaskStatus::Done, &TaskStatus::Todo]);
    }

    #[test]
    fn build_groups_puts_unconfigured_status_last() {
        let items = vec![
            make_item_st("alpha", "260701-u", "260701", TaskStatus::Unknown),
            make_item_st("alpha", "260702-t", "260702", TaskStatus::Todo),
        ];
        // "unknown" is absent from the default order.
        let groups = build_groups(items, true, true, &default_order(), true);
        let statuses: Vec<&TaskStatus> = groups[0].children.iter().map(status_of).collect();
        assert_eq!(statuses, vec![&TaskStatus::Todo, &TaskStatus::Unknown]);
    }

    #[test]
    fn build_groups_status_first_nests_projects() {
        let items = vec![
            make_item_st("alpha", "260701-t", "260701", TaskStatus::Todo),
            make_item_st("beta", "260702-t", "260702", TaskStatus::Todo),
            make_item_st("alpha", "260703-d", "260703", TaskStatus::Done),
        ];
        let groups = build_groups(items, true, false, &default_order(), true);
        assert_eq!(groups.len(), 2);
        assert_eq!(status_of(&groups[0]), &TaskStatus::Todo);
        assert_eq!(groups[0].task_count(), 2);
        let projects: Vec<&str> = groups[0].children.iter().map(project_name).collect();
        assert_eq!(projects, vec!["alpha", "beta"]);
        assert_eq!(status_of(&groups[1]), &TaskStatus::Done);
        assert_eq!(groups[1].task_count(), 1);
    }

    #[test]
    fn build_groups_ids_are_unique_across_the_tree() {
        let items = vec![
            make_item_st("alpha", "260701-t", "260701", TaskStatus::Todo),
            make_item_st("beta", "260702-t", "260702", TaskStatus::Todo),
            make_item_st("alpha", "260703-d", "260703", TaskStatus::Done),
        ];
        for project_first in [true, false] {
            let groups = build_groups(items.clone(), true, project_first, &default_order(), true);
            let mut ids: Vec<&str> = Vec::new();
            for g in &groups {
                ids.push(&g.id);
                for c in &g.children {
                    ids.push(&c.id);
                }
            }
            let unique: std::collections::HashSet<&&str> = ids.iter().collect();
            assert_eq!(unique.len(), ids.len(), "duplicate group ids: {ids:?}");
        }
    }

    #[test]
    fn build_groups_empty_returns_empty() {
        assert!(build_groups(Vec::new(), true, true, &default_order(), true).is_empty());
        assert!(build_groups(Vec::new(), false, true, &default_order(), true).is_empty());
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
        discover_projects(root, &segs, 0, &mut found, &[], None);

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
        discover_projects(root, &segs, 0, &mut found, &[], None);

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
        discover_projects(root, &segs, 0, &mut found, &[], None);

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
        discover_projects(root, &segs, 0, &mut found, &[], None);

        assert_eq!(found.len(), 3);
        for proj in &["alpha", "beta", "gamma"] {
            assert!(
                found.contains(&root.join(format!("repos/{proj}"))),
                "missing {proj}"
            );
        }
    }

    #[test]
    fn discover_projects_finds_nested_project_under_adopted_dir() {
        // hoge/docs/memo/tasks + hoge/workspace/docs/memo/tasks — both must
        // be found (adopting a non-repo dir does not stop the walk).
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        make_task(&root.join("hoge/docs/memo/tasks"), "260710-01-outer");
        make_task(
            &root.join("hoge/workspace/docs/memo/tasks"),
            "260710-01-nested",
        );

        let segs = ["docs", "memo", "tasks"];
        let mut found = Vec::new();
        discover_projects(root, &segs, 0, &mut found, &[], None);

        assert_eq!(found.len(), 2);
        assert!(found.contains(&root.join("hoge")));
        assert!(found.contains(&root.join("hoge/workspace")));
    }

    #[test]
    fn discover_projects_nested_stops_at_repo_boundary() {
        // hoge adopted, hoge/repo has .git → repo's own tasks found via
        // step 1, but nothing deeper inside repo.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        make_task(&root.join("hoge/docs/memo/tasks"), "260710-01-outer");
        fs::create_dir_all(root.join("hoge/repo/.git")).unwrap();
        make_task(&root.join("hoge/repo/docs/memo/tasks"), "260710-01-repo");
        make_task(
            &root.join("hoge/repo/src/deep/docs/memo/tasks"),
            "260710-01-buried",
        );

        let segs = ["docs", "memo", "tasks"];
        let mut found = Vec::new();
        discover_projects(root, &segs, 0, &mut found, &[], None);

        assert_eq!(found.len(), 2);
        assert!(found.contains(&root.join("hoge")));
        assert!(found.contains(&root.join("hoge/repo")));
    }

    #[test]
    fn scan_all_projects_adopts_scan_root_itself() {
        // scan_roots pointing directly at a project root must pick it up.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        make_task(&root.join("docs/memo/tasks"), "260710-01-root-task");

        let items = scan_all_projects_blocking(
            std::slice::from_ref(&root),
            &[],
            "docs/memo/tasks",
            None,
            &[],
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].project_path, root);
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
        discover_projects(root, &segs, 0, &mut found, &[], None);

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
        discover_projects(root, &segs, 0, &mut found, &[], None);

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
        discover_projects(root, &segs, 0, &mut found, &[], None);

        assert!(
            found.is_empty(),
            "no project should be found; repo interior was not entered"
        );
    }
}
