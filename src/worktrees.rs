//! Git worktree overlay: show a worktree's fresher docs on the main checkout.
//!
//! The user keeps mzed on the main repository while editing in a linked
//! worktree (Zed + `sync_skip_worktrees`). Every path mzed holds (tabs,
//! sidebar, session) stays a *logical* main-checkout path; this module maps a
//! logical path to whichever checkout — main or any linked worktree — has the
//! most recently modified copy, purely for display. Nothing is ever written
//! or copied between checkouts.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Linked worktrees of a primary checkout, discovered from
/// `.git/worktrees/<name>/gitdir` (each holds the path to `<wt>/.git`).
/// Stale registrations (worktree directory gone) are skipped.
pub fn linked_worktrees(root: &Path) -> Vec<PathBuf> {
    let worktrees_dir = root.join(".git/worktrees");
    let Ok(entries) = std::fs::read_dir(&worktrees_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Ok(content) = std::fs::read_to_string(entry.path().join("gitdir")) else {
            continue;
        };
        // gitdir holds "<worktree>/.git"; its parent is the worktree root.
        let Some(wt) = Path::new(content.trim()).parent().map(Path::to_path_buf) else {
            continue;
        };
        if wt.is_dir() && wt != root {
            out.push(wt);
        }
    }
    out.sort();
    out
}

/// Main-checkout root of a linked worktree, from its `.git` file
/// (`gitdir: <main>/.git/worktrees/<name>`). `None` when `root` is not a
/// linked worktree or the pointer is malformed.
pub fn main_root_of(root: &Path) -> Option<PathBuf> {
    let git_file = root.join(".git");
    if !git_file.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(&git_file).ok()?;
    let gitdir = content.strip_prefix("gitdir:")?.trim();
    // <main>/.git/worktrees/<name> → <main>
    let main = Path::new(gitdir).ancestors().nth(3)?;
    main.is_dir().then(|| main.to_path_buf())
}

/// The overlay for one window's project roots: each main root paired with its
/// linked worktrees. Equality is by the discovered path sets, so memoisation
/// only invalidates downstream state when worktrees appear or disappear.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overlay {
    pairs: Vec<(PathBuf, Vec<PathBuf>)>,
}

impl Overlay {
    /// Discover linked worktrees for every root. Roots that are themselves
    /// worktrees or have none get an empty pair (cheap: a couple of small
    /// file reads per root).
    pub fn discover(roots: &[PathBuf]) -> Self {
        let pairs = roots
            .iter()
            .map(|r| (r.clone(), linked_worktrees(r)))
            .collect();
        Self { pairs }
    }

    /// Worktrees of one specific root.
    pub fn worktrees_of(&self, root: &Path) -> &[PathBuf] {
        self.pairs
            .iter()
            .find(|(r, _)| r == root)
            .map(|(_, wts)| wts.as_slice())
            .unwrap_or(&[])
    }

    /// Every worktree root across all pairs (render containment, watching).
    pub fn worktree_roots(&self) -> Vec<PathBuf> {
        self.pairs
            .iter()
            .flat_map(|(_, wts)| wts.iter().cloned())
            .collect()
    }

    /// All copies a logical path could live at: the logical path itself plus
    /// the same relative path inside each linked worktree (existing or not —
    /// callers filter). Paths outside every main root map to themselves only.
    pub fn candidates(&self, logical: &Path) -> Vec<PathBuf> {
        let mut out = vec![logical.to_path_buf()];
        for (root, wts) in &self.pairs {
            if let Ok(rel) = logical.strip_prefix(root) {
                out.extend(wts.iter().map(|wt| wt.join(rel)));
            }
        }
        out
    }

    /// The freshest existing copy of a logical path (highest mtime wins).
    /// Falls back to the logical path itself when nothing exists.
    pub fn resolve(&self, logical: &Path) -> PathBuf {
        self.candidates(logical)
            .into_iter()
            .filter_map(|p| mtime(&p).map(|t| (t, p)))
            .max_by_key(|(t, _)| *t)
            .map(|(_, p)| p)
            .unwrap_or_else(|| logical.to_path_buf())
    }

    /// Map a worktree path back to its logical main-checkout path (identity
    /// for anything not under a known worktree). Keeps tabs, session and
    /// sidebar highlights on main paths when links inside a worktree copy
    /// are followed.
    pub fn to_logical(&self, path: &Path) -> PathBuf {
        for (root, wts) in &self.pairs {
            for wt in wts {
                if let Ok(rel) = path.strip_prefix(wt) {
                    return root.join(rel);
                }
            }
        }
        path.to_path_buf()
    }
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;
    use std::fs;

    /// Lay out a fake main checkout + one registered worktree.
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("repo");
        let wt = dir.path().join("wt-feature");
        fs::create_dir_all(main.join(".git/worktrees/wt-feature")).unwrap();
        fs::create_dir_all(&wt).unwrap();
        fs::write(
            main.join(".git/worktrees/wt-feature/gitdir"),
            format!("{}\n", wt.join(".git").display()),
        )
        .unwrap();
        fs::write(
            wt.join(".git"),
            format!(
                "gitdir: {}\n",
                main.join(".git/worktrees/wt-feature").display()
            ),
        )
        .unwrap();
        (dir, main, wt)
    }

    #[test]
    fn linked_worktreesは登録済みworktreeを返す() {
        let (_dir, main, wt) = fixture();
        assert_eq!(linked_worktrees(&main), vec![wt]);
    }

    #[test]
    fn linked_worktreesは消えたworktreeを除外する() {
        let (_dir, main, wt) = fixture();
        fs::remove_dir_all(&wt).unwrap();
        assert!(linked_worktrees(&main).is_empty());
    }

    #[test]
    fn main_root_ofはworktreeから主checkoutを引く() {
        let (_dir, main, wt) = fixture();
        assert_eq!(main_root_of(&wt), Some(main.clone()));
        assert_eq!(main_root_of(&main), None);
    }

    #[test]
    fn resolveはmtimeが新しい側を選ぶ() {
        let (_dir, main, wt) = fixture();
        fs::create_dir_all(main.join("docs")).unwrap();
        fs::create_dir_all(wt.join("docs")).unwrap();
        let logical = main.join("docs/a.md");
        fs::write(&logical, "# main").unwrap();
        fs::write(wt.join("docs/a.md"), "# wt").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let f = fs::File::open(&logical).unwrap();
        f.set_modified(old).unwrap();

        let ov = Overlay::discover(&[main.clone()]);
        assert_eq!(ov.resolve(&logical), wt.join("docs/a.md"));

        // Freshen main again: it wins back.
        fs::File::open(&logical)
            .unwrap()
            .set_modified(std::time::SystemTime::now())
            .unwrap();
        fs::File::open(wt.join("docs/a.md"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(ov.resolve(&logical), logical);
    }

    #[test]
    fn resolveはworktreeにしかないファイルも引く() {
        let (_dir, main, wt) = fixture();
        fs::create_dir_all(wt.join("docs")).unwrap();
        fs::write(wt.join("docs/only.md"), "# wt only").unwrap();

        let ov = Overlay::discover(&[main.clone()]);
        let logical = main.join("docs/only.md");
        assert_eq!(ov.resolve(&logical), wt.join("docs/only.md"));
        // Nothing exists anywhere → logical path unchanged.
        assert_eq!(ov.resolve(&main.join("nope.md")), main.join("nope.md"));
    }

    #[test]
    fn to_logicalはworktreeパスを主checkoutへ写す() {
        let (_dir, main, wt) = fixture();
        let ov = Overlay::discover(&[main.clone()]);
        assert_eq!(ov.to_logical(&wt.join("docs/a.md")), main.join("docs/a.md"));
        // Main paths and unrelated paths pass through.
        assert_eq!(ov.to_logical(&main.join("x.md")), main.join("x.md"));
        assert_eq!(
            ov.to_logical(Path::new("/etc/hosts")),
            PathBuf::from("/etc/hosts")
        );
    }

    #[test]
    fn overlayの等価性はworktree集合で決まる() {
        let (_dir, main, _wt) = fixture();
        let a = Overlay::discover(&[main.clone()]);
        let b = Overlay::discover(&[main.clone()]);
        assert_eq!(a, b);
    }
}
