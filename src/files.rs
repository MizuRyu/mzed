//! Markdown-focused project file tree for the sidebar.
//!
//! Only directories that contain at least one markdown file (directly or
//! nested) appear in the tree. Each directory carries a recursive markdown
//! count for an Obsidian-style badge.

use std::path::{Path, PathBuf};

/// A node in the markdown tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    /// For directories: number of markdown files in the subtree. For files: 1.
    pub md_count: usize,
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
                    children,
                });
            }
        } else if is_markdown(&path) {
            files.push(TreeNode {
                path,
                name,
                is_dir: false,
                md_count: 1,
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
        tree = merge_trees(tree, overlaid);
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
    for node in b {
        match merged
            .iter_mut()
            .find(|m| m.name == node.name && m.is_dir == node.is_dir)
        {
            Some(existing) if existing.is_dir => {
                let children = merge_trees(std::mem::take(&mut existing.children), node.children);
                existing.md_count = children.iter().map(|c| c.md_count).sum();
                existing.children = children;
            }
            Some(_) => {} // same file in both checkouts → one logical node
            None => merged.push(node),
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
