//! Markdown-focused project file tree for the sidebar.
//!
//! Only directories that contain at least one markdown file (directly or
//! nested) appear in the tree. Each directory carries a recursive markdown
//! count for an Obsidian-style badge.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A node in the markdown tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    /// For directories: number of markdown files in the subtree. For files: 1.
    pub md_count: usize,
    /// why: taken during the walk so the palette and the unread marker never
    /// stat again. Seconds since the epoch (0 when unreadable); for a directory,
    /// the newest mtime in its subtree.
    pub mtime: u64,
    /// Filled in by [`crate::app_state::unread`]; directories carry the count.
    pub unread: bool,
    pub unread_count: usize,
    pub children: Vec<TreeNode>,
}

const MAX_DEPTH: usize = 8;

pub(crate) fn is_markdown(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

/// True when `root` is a linked git worktree (or a submodule checkout): both
/// keep a `.git` *file* pointing at the real git dir, while a primary checkout
/// has a `.git` directory.
pub(crate) fn is_git_worktree(root: &Path) -> bool {
    root.join(".git").is_file()
}

/// Modification time in whole seconds. why: the only stat in the walk, and it
/// is paid for markdown files alone — directories take the max of their children.
fn entry_mtime(entry: &std::fs::DirEntry) -> u64 {
    entry
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Modification time in whole seconds, or `None` if `p` can't be stat-ed.
pub fn path_mtime(p: &Path) -> Option<u64> {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

fn is_ignored_dir(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "node_modules" | "target" | "dist" | "build")
}

/// Build the markdown tree under `root` (the root itself is not a node).
pub fn build_tree(root: &Path) -> Vec<TreeNode> {
    crate::perf::measure(
        "files.build_tree",
        &[("root", root.display().to_string())],
        || build_dir(root, 0),
    )
}

fn build_dir(dir: &Path, depth: usize) -> Vec<TreeNode> {
    if depth >= MAX_DEPTH {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut dirs: Vec<TreeNode> = Vec::new();
    let mut files: Vec<TreeNode> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()).map(String::from) else {
            continue;
        };
        // Use file_type() to avoid a stat() per entry (and macOS TCC prompts).
        // Symlinks report neither dir nor file there, so only for them fall
        // back to a following stat — a symlinked docs dir must still show up.
        // Cycles are bounded by MAX_DEPTH.
        let file_type = entry.file_type();
        let is_dir = match &file_type {
            Ok(t) if t.is_symlink() => path.is_dir(),
            Ok(t) => t.is_dir(),
            Err(_) => false,
        };

        if is_dir {
            if is_ignored_dir(&name) {
                continue;
            }
            let children = build_dir(&path, depth + 1);
            let md_count: usize = children.iter().map(|c| c.md_count).sum();
            // Hide directories that contain no markdown at all.
            if md_count > 0 {
                dirs.push(TreeNode {
                    path,
                    name,
                    is_dir: true,
                    md_count,
                    mtime: children.iter().map(|c| c.mtime).max().unwrap_or(0),
                    unread: false,
                    unread_count: 0,
                    children,
                });
            }
        } else if is_markdown(&path) {
            files.push(TreeNode {
                path,
                name,
                is_dir: false,
                md_count: 1,
                mtime: entry_mtime(&entry),
                unread: false,
                unread_count: 0,
                children: Vec::new(),
            });
        }
    }

    // Directories first, then files; each group sorted by name.
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    files.sort_by(|a, b| a.name.cmp(&b.name));
    dirs.into_iter().chain(files).collect()
}

/// Build the markdown tree for `root` with its linked worktrees overlaid:
/// every node keeps a *main-checkout* path (worktree-only files get the path
/// they would have on main), so tabs / highlights / session stay logical and
/// [`crate::worktrees::Overlay::resolve`] picks the copy to display.
pub fn build_tree_overlay(root: &Path, worktree_roots: &[&Path]) -> Vec<TreeNode> {
    let mut tree = build_tree(root);
    for wt in worktree_roots {
        let overlaid = remap_tree(build_tree(wt), wt, root);
        tree = crate::perf::measure(
            "files.merge_trees",
            &[
                ("root", root.display().to_string()),
                ("worktree", wt.display().to_string()),
            ],
            || merge_trees(tree, overlaid),
        );
    }
    tree
}

/// Rewrite every node path from the `from` prefix to the `to` prefix. Nodes
/// outside `from` (not possible for a `build_tree(from)` result) are kept.
fn remap_tree(nodes: Vec<TreeNode>, from: &Path, to: &Path) -> Vec<TreeNode> {
    nodes
        .into_iter()
        .map(|mut n| {
            if let Ok(rel) = n.path.strip_prefix(from) {
                n.path = to.join(rel);
            }
            n.children = remap_tree(n.children, from, to);
            n
        })
        .collect()
}

/// Union two sibling lists by (name, kind): directories merge recursively,
/// duplicate files collapse to one node (paths are equal after remapping).
fn merge_trees(a: Vec<TreeNode>, b: Vec<TreeNode>) -> Vec<TreeNode> {
    let mut merged = a;
    // why: a keyed lookup keeps the merge linear in the number of siblings.
    let mut index: HashMap<(String, bool), usize> = HashMap::with_capacity(merged.len());
    for (i, m) in merged.iter().enumerate() {
        index.entry((m.name.clone(), m.is_dir)).or_insert(i);
    }
    for node in b {
        match index
            .get(&(node.name.clone(), node.is_dir))
            .map(|&i| &mut merged[i])
        {
            Some(existing) if existing.is_dir => {
                let children = merge_trees(std::mem::take(&mut existing.children), node.children);
                existing.md_count = children.iter().map(|c| c.md_count).sum();
                existing.mtime = children.iter().map(|c| c.mtime).max().unwrap_or(0);
                existing.children = children;
            }
            // Same file in both checkouts → one logical node, carrying the
            // mtime of the copy `Overlay::resolve` will display (the freshest).
            Some(existing) => existing.mtime = existing.mtime.max(node.mtime),
            None => {
                index.insert((node.name.clone(), node.is_dir), merged.len());
                merged.push(node);
            }
        }
    }
    // Restore the sidebar order: directories first, then files, by name.
    merged.sort_by(|x, y| y.is_dir.cmp(&x.is_dir).then(x.name.cmp(&y.name)));
    merged
}

/// Flatten a markdown tree into the list of every markdown file path it holds,
/// in tree order. Used by the palette's file-search mode.
pub fn flatten_md(nodes: &[TreeNode]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_md(nodes, &mut out);
    out
}

fn collect_md(nodes: &[TreeNode], out: &mut Vec<PathBuf>) {
    for n in nodes {
        if n.is_dir {
            collect_md(&n.children, out);
        } else {
            out.push(n.path.clone());
        }
    }
}

/// The mtime already sitting in the tree for `path`, without stat-ing it again.
/// Used to mark a file "seen" at the moment it is opened or live-reloaded.
pub fn find_mtime(nodes: &[TreeNode], path: &Path) -> Option<u64> {
    for n in nodes {
        if n.path == path {
            return Some(n.mtime);
        }
        if n.is_dir && path.starts_with(&n.path) {
            if let Some(m) = find_mtime(&n.children, path) {
                return Some(m);
            }
        }
    }
    None
}

/// Apply freshly stat-ed mtimes to the files named in `updates`, bubbling each
/// change up through its ancestor directories (max of children), without
/// touching anything else in the tree. Used for a content-only fs event, where
/// a full rescan would be a full re-stat of the project for one saved file.
/// Returns whether anything actually changed.
pub fn update_mtimes(nodes: &mut [TreeNode], updates: &HashMap<PathBuf, u64>) -> bool {
    crate::perf::measure(
        "files.update_mtimes",
        &[("updates", updates.len().to_string())],
        || apply_mtimes(nodes, updates),
    )
}

fn apply_mtimes(nodes: &mut [TreeNode], updates: &HashMap<PathBuf, u64>) -> bool {
    let mut changed = false;
    for n in nodes {
        if n.is_dir {
            if apply_mtimes(&mut n.children, updates) {
                n.mtime = n.children.iter().map(|c| c.mtime).max().unwrap_or(0);
                changed = true;
            }
        } else if let Some(&mtime) = updates.get(&n.path) {
            if n.mtime != mtime {
                n.mtime = mtime;
                changed = true;
            }
        }
    }
    changed
}

/// A markdown file as the command palette sees it: the logical path plus the
/// root-relative label it is matched and displayed by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteFile {
    pub path: PathBuf,
    /// `docs/specs/05-zed.md`, prefixed with the root's name when multi-root.
    pub rel: String,
    pub name: String,
    pub mtime: u64,
    pub unread: bool,
}

impl PaletteFile {
    /// Directory part of `rel`, empty at the root. Shown dimmed after the name.
    pub fn dir(&self) -> &str {
        match self.rel.rfind('/') {
            Some(i) => &self.rel[..i],
            None => "",
        }
    }

    /// Everything a query may match: the relative path, the file name, and the
    /// name without its extension (so `plan` exactly matches `plan.md`).
    pub fn keys(&self) -> Vec<String> {
        let stem = self.name.rsplit_once('.').map(|(s, _)| s).unwrap_or("");
        let mut keys = vec![self.rel.clone(), self.name.clone()];
        if !stem.is_empty() && stem != self.name {
            keys.push(stem.to_string());
        }
        keys
    }
}

/// Flatten per-root markdown trees into palette candidates.
pub fn palette_files(trees: &[(PathBuf, Vec<TreeNode>)]) -> Vec<PaletteFile> {
    let multi = trees.len() > 1;
    let mut out = Vec::new();
    for (root, nodes) in trees {
        let prefix = if multi {
            root.file_name()
                .map(|s| format!("{}/", s.to_string_lossy()))
                .unwrap_or_default()
        } else {
            String::new()
        };
        collect_palette_files(nodes, root, &prefix, &mut out);
    }
    out
}

fn collect_palette_files(
    nodes: &[TreeNode],
    root: &Path,
    prefix: &str,
    out: &mut Vec<PaletteFile>,
) {
    for n in nodes {
        if n.is_dir {
            collect_palette_files(&n.children, root, prefix, out);
            continue;
        }
        let rel = n.path.strip_prefix(root).unwrap_or(&n.path);
        out.push(PaletteFile {
            path: n.path.clone(),
            rel: format!("{prefix}{}", rel.to_string_lossy()),
            name: n.name.clone(),
            mtime: n.mtime,
            unread: n.unread,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(unix)]
    #[test]
    fn symlinkのディレクトリとmdもツリーに載る() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Real content outside the tree root, reached only via symlinks.
        fs::create_dir_all(root.join("real-docs")).unwrap();
        fs::write(root.join("real-docs/guide.md"), "# g").unwrap();
        fs::write(root.join("note.md"), "# n").unwrap();

        let proj = root.join("proj");
        fs::create_dir_all(&proj).unwrap();
        std::os::unix::fs::symlink(root.join("real-docs"), proj.join("docs")).unwrap();
        std::os::unix::fs::symlink(root.join("note.md"), proj.join("note.md")).unwrap();

        let tree = build_tree(&proj);
        let docs = tree
            .iter()
            .find(|n| n.name == "docs")
            .expect("symlinked dir should appear");
        assert!(docs.is_dir);
        assert_eq!(docs.md_count, 1);
        assert!(tree.iter().any(|n| n.name == "note.md" && !n.is_dir));
    }

    #[test]
    fn is_git_worktreeはgitファイルのみ真() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // No .git at all → not a worktree.
        assert!(!is_git_worktree(root));

        // Primary checkout: .git is a directory.
        fs::create_dir_all(root.join("primary/.git")).unwrap();
        assert!(!is_git_worktree(&root.join("primary")));

        // Linked worktree: .git is a file pointing at the real git dir.
        fs::create_dir_all(root.join("wt")).unwrap();
        fs::write(root.join("wt/.git"), "gitdir: /repo/.git/worktrees/wt\n").unwrap();
        assert!(is_git_worktree(&root.join("wt")));
    }

    #[test]
    fn flatten_mdは全mdファイルを集める() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("README.md"), "# r").unwrap();
        fs::write(root.join("docs/guide.md"), "# g").unwrap();

        let tree = build_tree(root);
        let files = flatten_md(&tree);
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|p| p.ends_with("README.md")));
        assert!(files.iter().any(|p| p.ends_with("guide.md")));
    }

    #[test]
    fn treeはmdファイルのmtimeを持つ() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/guide.md"), "# g").unwrap();

        let tree = build_tree(root);
        let docs = &tree[0];
        let guide = &docs.children[0];
        assert!(guide.mtime > 0, "file mtime should be read during the walk");
        // ディレクトリは配下の最新 mtime を持つ。
        assert_eq!(docs.mtime, guide.mtime);
    }

    #[test]
    fn palette_filesは相対パスと名前を返す() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("docs/specs")).unwrap();
        fs::write(root.join("docs/specs/05-zed.md"), "# z").unwrap();
        fs::write(root.join("README.md"), "# r").unwrap();

        let trees = vec![(root.to_path_buf(), build_tree(root))];
        let files = palette_files(&trees);
        let zed = files.iter().find(|f| f.name == "05-zed.md").unwrap();
        assert_eq!(zed.rel, "docs/specs/05-zed.md");
        assert_eq!(zed.dir(), "docs/specs");
        assert_eq!(
            zed.keys(),
            vec!["docs/specs/05-zed.md", "05-zed.md", "05-zed"]
        );
        let readme = files.iter().find(|f| f.name == "README.md").unwrap();
        assert_eq!(readme.rel, "README.md");
        assert_eq!(readme.dir(), "");
    }

    #[test]
    fn palette_filesは複数rootでroot名を前置する() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("alpha");
        let b = dir.path().join("beta");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("x.md"), "# x").unwrap();
        fs::write(b.join("y.md"), "# y").unwrap();

        let trees = vec![(a.clone(), build_tree(&a)), (b.clone(), build_tree(&b))];
        let rels: Vec<String> = palette_files(&trees).into_iter().map(|f| f.rel).collect();
        assert_eq!(rels, vec!["alpha/x.md", "beta/y.md"]);
    }

    #[test]
    fn find_mtimeはツリーに既にある値を返す() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/guide.md"), "# g").unwrap();
        let tree = build_tree(root);

        let path = root.join("docs/guide.md");
        let mtime = find_mtime(&tree, &path).unwrap();
        assert!(mtime > 0);
        assert_eq!(find_mtime(&tree, &root.join("docs/missing.md")), None);
    }

    #[test]
    fn update_mtimesは対象ノードとその祖先だけ更新する() {
        let mut tree = vec![TreeNode {
            path: PathBuf::from("/p/docs"),
            name: "docs".into(),
            is_dir: true,
            md_count: 2,
            mtime: 100,
            unread: false,
            unread_count: 0,
            children: vec![
                TreeNode {
                    path: PathBuf::from("/p/docs/a.md"),
                    name: "a.md".into(),
                    is_dir: false,
                    md_count: 1,
                    mtime: 100,
                    unread: false,
                    unread_count: 0,
                    children: vec![],
                },
                TreeNode {
                    path: PathBuf::from("/p/docs/b.md"),
                    name: "b.md".into(),
                    is_dir: false,
                    md_count: 1,
                    mtime: 50,
                    unread: false,
                    unread_count: 0,
                    children: vec![],
                },
            ],
        }];
        let mut updates = HashMap::new();
        updates.insert(PathBuf::from("/p/docs/b.md"), 200);
        assert!(update_mtimes(&mut tree, &updates));
        assert_eq!(tree[0].children[1].mtime, 200);
        assert_eq!(tree[0].children[0].mtime, 100); // 触っていないファイルは不変
        assert_eq!(tree[0].mtime, 200); // 祖先ディレクトリへ最新値が伝播する

        // 変化の無い更新は false を返す（既に同じ mtime）。
        assert!(!update_mtimes(&mut tree, &updates));
    }

    #[test]
    fn build_tree_overlayはworktree限定のmdを主パスで合成する() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("repo");
        let wt = dir.path().join("wt");
        fs::create_dir_all(main.join("docs")).unwrap();
        fs::create_dir_all(wt.join("docs/specs")).unwrap();
        fs::write(main.join("docs/guide.md"), "# main").unwrap();
        fs::write(wt.join("docs/guide.md"), "# wt copy").unwrap(); // both sides
        fs::write(wt.join("docs/specs/new.md"), "# wt only").unwrap();
        fs::write(wt.join("wt-note.md"), "# wt top").unwrap();

        let tree = build_tree_overlay(&main, &[&wt]);
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["docs", "wt-note.md"]);

        // Worktree-only file carries the logical main path.
        assert_eq!(tree[1].path, main.join("wt-note.md"));

        let docs = &tree[0];
        // guide.md deduped, specs/ (worktree only) merged in.
        assert_eq!(docs.md_count, 2);
        let specs = docs.children.iter().find(|n| n.name == "specs").unwrap();
        assert_eq!(specs.children[0].path, main.join("docs/specs/new.md"));
        let guide = docs
            .children
            .iter()
            .filter(|n| n.name == "guide.md")
            .count();
        assert_eq!(guide, 1);
        assert_eq!(
            docs.children
                .iter()
                .find(|n| n.name == "guide.md")
                .unwrap()
                .path,
            main.join("docs/guide.md")
        );
    }

    #[test]
    fn merge_treesは広い兄弟を並び規則どおりに合成する() {
        fn file(name: String, mtime: u64) -> TreeNode {
            TreeNode {
                path: PathBuf::from(format!("/p/{name}")),
                name,
                is_dir: false,
                md_count: 1,
                mtime,
                unread: false,
                unread_count: 0,
                children: vec![],
            }
        }
        let side = |wt: u64| -> Vec<TreeNode> {
            // 半分は main と同名、半分は worktree 固有。
            (0..2_000)
                .map(|i| {
                    let name = if i % 2 == 0 {
                        format!("{i:04}.md")
                    } else {
                        format!("wt{wt}-{i:04}.md")
                    };
                    file(name, wt)
                })
                .collect()
        };
        let mut tree: Vec<TreeNode> = (0..2_000).map(|i| file(format!("{i:04}.md"), 0)).collect();
        for wt in 1..=10 {
            tree = merge_trees(tree, side(wt));
        }

        assert_eq!(tree.len(), 2_000 + 10 * 1_000);
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        // 同名ファイルは 1 つにまとまり、最新の mtime を持つ。
        assert_eq!(tree.iter().find(|n| n.name == "0000.md").unwrap().mtime, 10);
        assert_eq!(tree.iter().find(|n| n.name == "0001.md").unwrap().mtime, 0);
    }

    #[test]
    fn builds_md_tree_with_counts_and_skips_empty_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("docs/research")).unwrap();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::write(root.join("README.md"), "# r").unwrap();
        fs::write(root.join("docs/guide.md"), "# g").unwrap();
        fs::write(root.join("docs/research/deep.md"), "# d").unwrap();
        fs::write(root.join("node_modules/pkg/x.md"), "# x").unwrap(); // ignored
        fs::write(root.join("notes.txt"), "nope").unwrap(); // non-md ignored

        let tree = build_tree(root);

        // Top level: docs/ (dir) then README.md (file). empty/ and node_modules/ gone.
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["docs", "README.md"]);

        let docs = &tree[0];
        assert!(docs.is_dir);
        // docs/guide.md + docs/research/deep.md
        assert_eq!(docs.md_count, 2);

        // research/ nested under docs with its single md.
        let research = docs.children.iter().find(|n| n.name == "research").unwrap();
        assert_eq!(research.md_count, 1);
    }
}
