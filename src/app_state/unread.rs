//! Which markdown files changed since the user last read them.
//!
//! A file is unread when its mtime is newer than the moment it was last opened
//! in mzed. A file that was never opened falls back to when the project itself
//! was first opened, so opening a project does not paint every file green.
//! "Mark all as read" raises that floor to now and drops the per-file records —
//! that is what keeps `state.json` from growing with the size of the project.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::files::TreeNode;
use crate::session::PerProjectHistory;

/// Seconds since the epoch, the unit every stamp here is in (mtime granularity).
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Key a file is remembered by: its path relative to the project root, or the
/// absolute path when it lives under a secondary root of a multi-root workspace.
pub fn seen_key(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

/// `mtime` is newer than the newest moment this file counts as read: when it was
/// last opened, or — never having been opened — the later of the project's first
/// open and the last "mark all as read".
pub fn is_unread(mtime: u64, seen_at: Option<u64>, first_opened_at: u64, read_all_at: u64) -> bool {
    mtime > seen_at.unwrap_or(first_opened_at).max(read_all_at)
}

fn is_unread_in(hist: &PerProjectHistory, key: &str, mtime: u64) -> bool {
    is_unread(
        mtime,
        hist.seen.get(key).copied(),
        hist.first_opened_at,
        hist.read_all_at,
    )
}

/// Flag every file under `nodes` and roll the counts up into the directories,
/// then reorder each directory's children (see [`order_unread_first`]).
/// Returns the total, for the toolbar badge. Pure: the mtimes come from the
/// tree walk, so marking never stats.
pub fn mark(nodes: &mut [TreeNode], root: &Path, hist: &PerProjectHistory) -> usize {
    let mut total = 0;
    for node in nodes.iter_mut() {
        if node.is_dir {
            node.unread_count = mark(&mut node.children, root, hist);
            node.unread = false;
        } else {
            node.unread = is_unread_in(hist, &seen_key(root, &node.path), node.mtime);
            node.unread_count = usize::from(node.unread);
        }
        total += node.unread_count;
    }
    order_unread_first(nodes);
    total
}

/// Reorder one directory's children the way the sidebar shows them:
/// subdirectories first (name order), then unread files newest-first
/// (mtime descending, ties by name), then read files in name order.
/// Pure and non-recursive by itself — `mark` calls it at every level as it
/// unwinds, so nested directories end up ordered too.
pub fn order_unread_first(children: &mut [TreeNode]) {
    children.sort_by(|a, b| {
        let rank = |n: &TreeNode| -> u8 {
            if n.is_dir {
                0
            } else if n.unread {
                1
            } else {
                2
            }
        };
        rank(a).cmp(&rank(b)).then_with(|| {
            if !a.is_dir && a.unread && !b.is_dir && b.unread {
                b.mtime.cmp(&a.mtime).then_with(|| a.name.cmp(&b.name))
            } else {
                a.name.cmp(&b.name)
            }
        })
    });
}

/// Record that `path` was read at the mtime it carries now. Redundant records
/// (a file already covered by the project's floor) are not stored, which is what
/// bounds the map to the files that actually went from unread to read.
pub fn mark_read(hist: &mut PerProjectHistory, root: &Path, path: &Path, mtime: u64) {
    let key = seen_key(root, path);
    if is_unread_in(hist, &key, mtime) {
        hist.seen.insert(key, mtime);
    }
}

/// Read everything in the project as of now.
pub fn mark_all_read(hist: &mut PerProjectHistory) {
    hist.read_all_at = now();
    hist.seen.clear();
}

/// Stamp a project as opened; the first open also becomes the unread floor.
pub fn record_open(hist: &mut PerProjectHistory) {
    let at = now();
    if hist.first_opened_at == 0 {
        hist.first_opened_at = at;
    }
    hist.last_opened_at = at;
}

/// The history as it should be written to disk: records for files that are no
/// longer in `tree` are dropped, so a project that churns through file names
/// does not accumulate them. `tree` may cover several roots (a multi-root
/// workspace); each root's own records are checked against its own subtree.
/// A root in `history` but not in `tree` (a parked project) is left untouched
/// — call this once `tree` actually holds every current root, or a still-
/// loading/transitional tree would look like every file just vanished.
pub fn pruned(
    history: &HashMap<PathBuf, PerProjectHistory>,
    tree: &[(PathBuf, Vec<TreeNode>)],
) -> HashMap<PathBuf, PerProjectHistory> {
    let mut out = history.clone();
    for (root, nodes) in tree {
        let Some(hist) = out.get_mut(root) else {
            continue;
        };
        if hist.seen.is_empty() {
            continue;
        }
        let mut live = HashSet::new();
        collect_keys(nodes, root, &mut live);
        hist.seen.retain(|k, _| live.contains(k));
    }
    out
}

fn collect_keys(nodes: &[TreeNode], root: &Path, out: &mut HashSet<String>) {
    for n in nodes {
        if n.is_dir {
            collect_keys(&n.children, root, out);
        } else {
            out.insert(seen_key(root, &n.path));
        }
    }
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    fn hist(first: u64, read_all: u64, seen: &[(&str, u64)]) -> PerProjectHistory {
        PerProjectHistory {
            first_opened_at: first,
            last_opened_at: first,
            read_all_at: read_all,
            seen: seen.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        }
    }

    #[test]
    fn 記録が無ければプロジェクト初回オープン時刻で判定する() {
        // 初回オープン前からあるファイルは既読扱い（全部が緑にならない）。
        assert!(!is_unread(50, None, 100, 0));
        assert!(!is_unread(100, None, 100, 0));
        // 開いた後に作られた / 更新されたものは未読。
        assert!(is_unread(101, None, 100, 0));
    }

    #[test]
    fn 記録があればその時刻で判定する() {
        assert!(!is_unread(200, Some(200), 100, 0));
        assert!(is_unread(201, Some(200), 100, 0));
        // 記録は初回オープン時刻より優先される（開いた後に更新されたら未読）。
        assert!(is_unread(150, Some(120), 300, 0));
    }

    #[test]
    fn 一括既読後は以前のmtimeがすべて既読になる() {
        assert!(!is_unread(400, None, 100, 500));
        assert!(!is_unread(400, Some(200), 100, 500));
        assert!(is_unread(501, None, 100, 500));
        assert!(is_unread(501, Some(200), 100, 500));
    }

    #[test]
    fn 表示中に更新されたら既読記録を更新して未読に戻らない() {
        let mut h = hist(100, 0, &[]);
        let root = Path::new("/p");
        let file = Path::new("/p/docs/a.md");
        mark_read(&mut h, root, file, 150);
        assert_eq!(h.seen.get("docs/a.md"), Some(&150));
        // ライブリロードで mtime が進んだら、その時刻で上書きされる。
        mark_read(&mut h, root, file, 180);
        assert_eq!(h.seen.get("docs/a.md"), Some(&180));
        assert!(!is_unread_in(&h, "docs/a.md", 180));
    }

    #[test]
    fn 既読のファイルは記録を増やさない() {
        // 初回オープンより古い = すでに既読。記録する意味が無い。
        let mut h = hist(100, 0, &[]);
        mark_read(&mut h, Path::new("/p"), Path::new("/p/old.md"), 50);
        assert!(h.seen.is_empty());
    }

    #[test]
    fn 一括既読は記録を捨てる() {
        let mut h = hist(100, 0, &[("a.md", 150)]);
        mark_all_read(&mut h);
        assert!(h.seen.is_empty());
        assert!(h.read_all_at > 0);
    }

    #[test]
    fn record_openは初回だけ基準時刻を決める() {
        let mut h = PerProjectHistory::default();
        record_open(&mut h);
        let first = h.first_opened_at;
        assert!(first > 0);
        record_open(&mut h);
        assert_eq!(h.first_opened_at, first);
        assert!(h.last_opened_at >= first);
    }

    fn file_node(path: &str, mtime: u64) -> TreeNode {
        TreeNode {
            path: PathBuf::from(path),
            name: Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string(),
            is_dir: false,
            md_count: 1,
            mtime,
            unread: false,
            unread_count: 0,
            children: Vec::new(),
        }
    }

    fn dir_node(path: &str, children: Vec<TreeNode>) -> TreeNode {
        TreeNode {
            path: PathBuf::from(path),
            name: Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string(),
            is_dir: true,
            md_count: children.len(),
            mtime: children.iter().map(|c| c.mtime).max().unwrap_or(0),
            unread: false,
            unread_count: 0,
            children,
        }
    }

    #[test]
    fn markはファイルに印を付けフォルダに件数を積む() {
        let mut tree = vec![
            dir_node(
                "/p/docs",
                vec![
                    file_node("/p/docs/a.md", 150),
                    file_node("/p/docs/b.md", 50),
                ],
            ),
            file_node("/p/c.md", 200),
        ];
        let total = mark(&mut tree, Path::new("/p"), &hist(100, 0, &[]));
        assert_eq!(total, 2);
        assert_eq!(tree[0].unread_count, 1);
        assert!(tree[0].children[0].unread);
        assert!(!tree[0].children[1].unread);
        assert!(tree[1].unread);
    }

    #[test]
    fn markは未読ファイルを先頭にmtime降順でまとめる() {
        let mut tree = vec![
            file_node("/p/old-read.md", 50),  // 既読（初回オープンより前）
            file_node("/p/z-unread.md", 300), // 未読、最新
            file_node("/p/a-unread.md", 200), // 未読
            file_node("/p/m-read.md", 60),    // 既読
        ];
        mark(&mut tree, Path::new("/p"), &hist(100, 0, &[]));
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        // 未読(mtime降順) → 既読(名前順)。
        assert_eq!(
            names,
            vec!["z-unread.md", "a-unread.md", "m-read.md", "old-read.md"]
        );
    }

    #[test]
    fn markは未読の同mtimeを名前順にする() {
        let mut tree = vec![file_node("/p/b.md", 200), file_node("/p/a.md", 200)];
        mark(&mut tree, Path::new("/p"), &hist(100, 0, &[]));
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["a.md", "b.md"]);
    }

    #[test]
    fn markはディレクトリを常に未読ファイルより前に名前順で置く() {
        let mut tree = vec![
            file_node("/p/z-unread.md", 999), // 未読・最新mtimeでもディレクトリより後
            dir_node("/p/zeta", vec![file_node("/p/zeta/x.md", 10)]),
            dir_node("/p/alpha", vec![file_node("/p/alpha/x.md", 10)]),
        ];
        mark(&mut tree, Path::new("/p"), &hist(100, 0, &[]));
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zeta", "z-unread.md"]);
    }

    #[test]
    fn markはネストしたフォルダの中も並べ替える() {
        let mut tree = vec![dir_node(
            "/p/docs",
            vec![
                file_node("/p/docs/old.md", 50),
                file_node("/p/docs/new.md", 300),
                file_node("/p/docs/mid.md", 200),
            ],
        )];
        mark(&mut tree, Path::new("/p"), &hist(100, 0, &[]));
        let names: Vec<&str> = tree[0].children.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["new.md", "mid.md", "old.md"]);
    }

    #[test]
    fn prunedはツリーに無い記録を落とす() {
        let tree = vec![(PathBuf::from("/p"), vec![file_node("/p/a.md", 150)])];
        let mut history = HashMap::new();
        history.insert(
            PathBuf::from("/p"),
            hist(100, 0, &[("a.md", 150), ("gone.md", 120)]),
        );
        let out = pruned(&history, &tree);
        let seen = &out[Path::new("/p")].seen;
        assert!(seen.contains_key("a.md"));
        assert!(!seen.contains_key("gone.md"));
    }

    #[test]
    fn prunedは複数rootをそれぞれ自分のツリーで掃除する() {
        let tree = vec![
            (PathBuf::from("/a"), vec![file_node("/a/x.md", 150)]),
            (PathBuf::from("/b"), vec![file_node("/b/y.md", 150)]),
        ];
        let mut history = HashMap::new();
        history.insert(
            PathBuf::from("/a"),
            hist(100, 0, &[("x.md", 150), ("gone-a.md", 120)]),
        );
        history.insert(
            PathBuf::from("/b"),
            hist(100, 0, &[("y.md", 150), ("gone-b.md", 120)]),
        );
        let out = pruned(&history, &tree);
        assert!(out[Path::new("/a")].seen.contains_key("x.md"));
        assert!(!out[Path::new("/a")].seen.contains_key("gone-a.md"));
        assert!(out[Path::new("/b")].seen.contains_key("y.md"));
        assert!(!out[Path::new("/b")].seen.contains_key("gone-b.md"));
    }

    #[test]
    fn prunedはtreeに無いrootの記録は触らない() {
        // 走査中の空ツリーやパーク済みプロジェクトでは掃除しない。
        let tree: Vec<(PathBuf, Vec<TreeNode>)> = vec![];
        let mut history = HashMap::new();
        history.insert(PathBuf::from("/parked"), hist(100, 0, &[("a.md", 150)]));
        let out = pruned(&history, &tree);
        assert!(out[Path::new("/parked")].seen.contains_key("a.md"));
    }
}
