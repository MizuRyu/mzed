//! Git worktree overlay: show a worktree's fresher docs on the main checkout.
//!
//! The user keeps mzed on the main repository while editing in a linked
//! worktree ([`redirect`] puts it there). Every path mzed holds (tabs,
//! sidebar, session) stays a *logical* main-checkout path; this module maps a
//! logical path to whichever checkout — main or any linked worktree — has the
//! most recently modified copy, purely for display. Nothing is ever written
//! or copied between checkouts.

use crate::sync::WorktreeSwitch;
use std::ffi::OsStr;
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
/// linked worktree or the pointer no longer describes that layout.
///
/// why: taking the third ancestor on trust turns a dangling or foreign pointer
/// into a "main repository" that merely happens to exist — `gitdir: /tmp/gone/x`
/// would answer `/tmp`, and a submodule's `<super>/.git/modules/<name>` would
/// answer the superproject, whose overlay cannot see the submodule's docs. So
/// every level of git's own layout is demanded, on disk.
pub fn main_root_of(root: &Path) -> Option<PathBuf> {
    let git_file = root.join(".git");
    if !git_file.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(&git_file).ok()?;
    let gitdir = Path::new(content.strip_prefix("gitdir:")?.trim());
    if !gitdir.is_dir() {
        return None;
    }
    let mut up = gitdir.ancestors().skip(1);
    let worktrees = up.next()?;
    let git_dir = up.next()?;
    let main = up.next()?;
    let laid_out = worktrees.file_name() == Some(OsStr::new("worktrees"))
        && git_dir.file_name() == Some(OsStr::new(".git"))
        && git_dir.is_dir();
    laid_out.then(|| main.to_path_buf())
}

/// Rewrite a project switch target under `mode`, before the app acts on it.
/// Under [`WorktreeSwitch::Main`] every root that is a linked worktree becomes
/// its main checkout (duplicates collapse: `[main, wt-of-main]` is one root);
/// the other modes pass `roots` through untouched. Roots with no main checkout
/// — an ordinary clone, or a broken `gitdir` pointer — stay as they are, so
/// this never leaves the app with a project that isn't there.
///
/// The primary is always `roots[0]`; when `roots` is empty there is nothing to
/// derive it from, so it is returned unchanged (empty).
pub fn redirect(roots: Vec<PathBuf>, mode: WorktreeSwitch) -> (PathBuf, Vec<PathBuf>) {
    if mode != WorktreeSwitch::Main {
        let primary = roots.first().cloned().unwrap_or_default();
        return (primary, roots);
    }
    let mut mapped: Vec<PathBuf> = Vec::with_capacity(roots.len());
    for root in roots {
        let root = main_root_of(&root).unwrap_or(root);
        if !mapped.contains(&root) {
            mapped.push(root);
        }
    }
    let primary = mapped.first().cloned().unwrap_or_default();
    (primary, mapped)
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

    /// Register one more worktree on the same main checkout.
    fn add_worktree(main: &Path, name: &str) -> PathBuf {
        let admin = main.join(".git/worktrees").join(name);
        let wt = main.parent().unwrap().join(name);
        fs::create_dir_all(&admin).unwrap();
        fs::create_dir_all(&wt).unwrap();
        fs::write(
            admin.join("gitdir"),
            format!("{}\n", wt.join(".git").display()),
        )
        .unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
        wt
    }

    /// An ordinary directory: no `.git` at all, so never redirected.
    fn plain_dir(under: &Path, name: &str) -> PathBuf {
        let p = under.join(name);
        fs::create_dir_all(&p).unwrap();
        p
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
    fn main_root_ofは壊れたgitdirを親と誤認しない() {
        let (dir, _main, wt) = fixture();
        let broken = dir.path().join("broken");
        fs::create_dir_all(&broken).unwrap();

        // Target gone, but its third ancestor exists: /tmp must not become
        // the main repository.
        fs::write(wt.join(".git"), "gitdir: /tmp/mzed-no-such-wt/x/y\n").unwrap();
        assert_eq!(main_root_of(&wt), None);

        // Target exists but is not git's layout (no `.git`/`worktrees` levels).
        let foreign = plain_dir(dir.path(), "a/b/c");
        fs::write(wt.join(".git"), format!("gitdir: {}\n", foreign.display())).unwrap();
        assert_eq!(main_root_of(&wt), None);

        // A submodule checkout points at `<super>/.git/modules/<name>`; the
        // superproject's overlay cannot see its docs, so it is not a parent.
        let modules = plain_dir(dir.path(), "super/.git/modules/sub");
        fs::write(wt.join(".git"), format!("gitdir: {}\n", modules.display())).unwrap();
        assert_eq!(main_root_of(&wt), None);

        // Not a pointer at all.
        fs::write(wt.join(".git"), "something else\n").unwrap();
        assert_eq!(main_root_of(&wt), None);
        assert_eq!(main_root_of(&broken), None);
    }

    #[test]
    fn redirectは混在rootsの順序とprimaryを保つ() {
        let (dir, main, wt) = fixture();
        let a = plain_dir(dir.path(), "plain-a");
        let b = plain_dir(dir.path(), "plain-b");

        // Non-worktree head keeps the primary; the worktree in the middle is
        // the only root rewritten.
        assert_eq!(
            redirect(vec![a.clone(), wt.clone(), b.clone()], WorktreeSwitch::Main),
            (a.clone(), vec![a.clone(), main.clone(), b.clone()])
        );
        // Worktree head: the primary becomes its main checkout.
        assert_eq!(
            redirect(vec![wt.clone(), a.clone()], WorktreeSwitch::Main),
            (main.clone(), vec![main.clone(), a.clone()])
        );
    }

    #[test]
    fn redirectは同じ親の別worktreeを同じ選択に写す() {
        let (_dir, main, wt) = fixture();
        let wt2 = add_worktree(&main, "wt-other");
        let expected = (main.clone(), vec![main.clone()]);
        assert_eq!(redirect(vec![wt], WorktreeSwitch::Main), expected);
        assert_eq!(redirect(vec![wt2], WorktreeSwitch::Main), expected);
    }

    #[test]
    fn redirectはmainモードでworktreeを親に付け替える() {
        let (_dir, main, wt) = fixture();
        assert_eq!(
            redirect(vec![wt.clone()], WorktreeSwitch::Main),
            (main.clone(), vec![main.clone()])
        );
    }

    #[test]
    fn redirectはskipとfollowでは何もしない() {
        let (_dir, _main, wt) = fixture();
        for mode in [WorktreeSwitch::Skip, WorktreeSwitch::Follow] {
            assert_eq!(
                redirect(vec![wt.clone()], mode),
                (wt.clone(), vec![wt.clone()])
            );
        }
    }

    #[test]
    fn redirectは通常checkoutを付け替えない() {
        let (_dir, main, _wt) = fixture();
        let plain = PathBuf::from("/nonexistent/plain");
        for mode in [
            WorktreeSwitch::Main,
            WorktreeSwitch::Skip,
            WorktreeSwitch::Follow,
        ] {
            assert_eq!(
                redirect(vec![main.clone()], mode),
                (main.clone(), vec![main.clone()])
            );
            assert_eq!(
                redirect(vec![plain.clone()], mode),
                (plain.clone(), vec![plain.clone()])
            );
        }
    }

    #[test]
    fn redirectは同じ親になったrootを1つに畳む() {
        let (_dir, main, wt) = fixture();
        let other = PathBuf::from("/nonexistent/other");
        assert_eq!(
            redirect(
                vec![wt.clone(), main.clone(), other.clone()],
                WorktreeSwitch::Main
            ),
            (main.clone(), vec![main.clone(), other])
        );
    }

    #[test]
    fn worktree_switchはsnake_caseでシリアライズされる() {
        assert_eq!(
            serde_json::to_string(&WorktreeSwitch::Main).unwrap(),
            "\"main\""
        );
        assert_eq!(
            serde_json::from_str::<WorktreeSwitch>("\"follow\"").unwrap(),
            WorktreeSwitch::Follow
        );
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
