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
    matches!(
        p.extension().and_then(|e| e.to_str()),
        Some("md") | Some("markdown")
    )
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
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);

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
