//! Which projects the Cmd+O switcher offers (pure logic).
//!
//! Sources pile up for years — Zed keeps every workspace it ever opened — so
//! the list is pruned on the fly: folders that are gone, linked worktrees that
//! a pick would redirect to their parent anyway, and, while nothing is typed,
//! projects not used lately. Nothing here is persisted; a typed query still
//! reaches every project that exists.

use crate::sync::WorktreeSwitch;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// While nothing is typed, only projects used within this window are listed…
pub const RECENT_WINDOW_SECS: u64 = 30 * 86_400;
/// …and at most this many of them (exempt rows come on top).
pub const RECENT_LIMIT: usize = 15;

/// One switcher row and when it was last used (Unix seconds; `None` = unknown).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub path: PathBuf,
    pub last_used: Option<u64>,
}

/// Everything the list is built from, snapshotted off the UI state.
pub struct Sources<'a> {
    /// Roots on screen: always listed, never folded or dropped.
    pub current: &'a [PathBuf],
    /// Projects with parked tabs.
    pub parked: &'a [PathBuf],
    /// Zed's recent roots, newest first, with Zed's own timestamp.
    pub zed: &'a [(PathBuf, Option<u64>)],
    /// Named projects: an explicit "keep this reachable", so never folded.
    pub aliased: &'a [PathBuf],
    /// Rows the user hid with ✕.
    pub hidden: &'a [PathBuf],
    /// mzed's own `last_opened_at` per root.
    pub opened: &'a HashMap<PathBuf, u64>,
    pub worktree: WorktreeSwitch,
}

/// Build the full candidate list. `is_dir` and `main_root_of` touch the disk,
/// so this runs off the UI thread.
pub fn collect(
    src: &Sources,
    is_dir: &dyn Fn(&Path) -> bool,
    main_root_of: &dyn Fn(&Path) -> Option<PathBuf>,
) -> Vec<Candidate> {
    let zed_time: HashMap<&Path, u64> = src
        .zed
        .iter()
        .filter_map(|(p, t)| t.map(|t| (p.as_path(), t)))
        .collect();
    let used = |p: &Path| {
        let mzed = src.opened.get(p).copied().filter(|t| *t > 0);
        mzed.max(zed_time.get(p).copied())
    };

    let ordered = src
        .current
        .iter()
        .chain(src.parked)
        .chain(src.zed.iter().map(|(p, _)| p))
        .chain(src.aliased);
    let mut out: Vec<Candidate> = Vec::new();
    for path in ordered {
        let pinned = src.current.contains(path);
        if !pinned && (src.hidden.contains(path) || !is_dir(path)) {
            continue;
        }
        // why: under `main` a pick on a worktree opens its parent, so the
        // worktree's own row only duplicates the parent's. A row the user
        // named keeps its path so the name stays on it.
        let target = match src.worktree {
            WorktreeSwitch::Main if !pinned && !src.aliased.contains(path) => {
                main_root_of(path).unwrap_or_else(|| path.clone())
            }
            _ => path.clone(),
        };
        if &target != path && !src.current.contains(&target) && src.hidden.contains(&target) {
            continue;
        }
        let last_used = used(path);
        match out.iter_mut().find(|c| c.path == target) {
            Some(c) => c.last_used = c.last_used.max(last_used),
            None => {
                let last_used = last_used.max(used(&target));
                out.push(Candidate {
                    path: target,
                    last_used,
                });
            }
        }
    }
    out
}

/// Rows to offer. With a query, every candidate (the fuzzy ranking narrows
/// them); without one, the exempt rows plus the [`RECENT_LIMIT`] most recently
/// used within [`RECENT_WINDOW_SECS`] of `now`.
pub fn visible(
    candidates: &[Candidate],
    exempt: &[PathBuf],
    query_empty: bool,
    now: u64,
) -> Vec<PathBuf> {
    if !query_empty {
        return candidates.iter().map(|c| c.path.clone()).collect();
    }
    let mut recent: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| !exempt.contains(&c.path))
        .filter(|c| {
            c.last_used
                .is_some_and(|t| t <= now && now - t <= RECENT_WINDOW_SECS)
        })
        .collect();
    recent.sort_by(|a, b| b.last_used.cmp(&a.last_used).then(a.path.cmp(&b.path)));
    recent.truncate(RECENT_LIMIT);
    candidates
        .iter()
        .filter(|c| exempt.contains(&c.path) || recent.iter().any(|r| r.path == c.path))
        .map(|c| c.path.clone())
        .collect()
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000_000;
    const DAY: u64 = 86_400;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn every_dir(_: &Path) -> bool {
        true
    }

    fn no_parent(_: &Path) -> Option<PathBuf> {
        None
    }

    /// `/repo-wt*` are worktrees of `/repo`.
    fn wt_parent(path: &Path) -> Option<PathBuf> {
        path.to_string_lossy()
            .starts_with("/repo-wt")
            .then(|| p("/repo"))
    }

    struct Fixture {
        current: Vec<PathBuf>,
        parked: Vec<PathBuf>,
        zed: Vec<(PathBuf, Option<u64>)>,
        aliased: Vec<PathBuf>,
        hidden: Vec<PathBuf>,
        opened: HashMap<PathBuf, u64>,
        worktree: WorktreeSwitch,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                current: Vec::new(),
                parked: Vec::new(),
                zed: Vec::new(),
                aliased: Vec::new(),
                hidden: Vec::new(),
                opened: HashMap::new(),
                worktree: WorktreeSwitch::Main,
            }
        }

        fn collect_with(
            &self,
            is_dir: &dyn Fn(&Path) -> bool,
            parent: &dyn Fn(&Path) -> Option<PathBuf>,
        ) -> Vec<Candidate> {
            collect(
                &Sources {
                    current: &self.current,
                    parked: &self.parked,
                    zed: &self.zed,
                    aliased: &self.aliased,
                    hidden: &self.hidden,
                    opened: &self.opened,
                    worktree: self.worktree,
                },
                is_dir,
                parent,
            )
        }

        fn paths(&self, parent: &dyn Fn(&Path) -> Option<PathBuf>) -> Vec<PathBuf> {
            self.collect_with(&every_dir, parent)
                .into_iter()
                .map(|c| c.path)
                .collect()
        }
    }

    #[test]
    fn 消えたフォルダは外すが表示中のものは残す() {
        let mut f = Fixture::new();
        f.current = vec![p("/gone-current")];
        f.zed = vec![(p("/gone"), None), (p("/here"), None)];
        let exists = |q: &Path| q == Path::new("/here");
        let got: Vec<PathBuf> = f
            .collect_with(&exists, &no_parent)
            .into_iter()
            .map(|c| c.path)
            .collect();
        assert_eq!(got, vec![p("/gone-current"), p("/here")]);
    }

    #[test]
    fn mainではworktreeを親に畳む() {
        let mut f = Fixture::new();
        f.zed = vec![
            (p("/repo-wt1"), Some(NOW - DAY)),
            (p("/other"), None),
            (p("/repo-wt2"), Some(NOW)),
        ];
        let got = f.collect_with(&every_dir, &wt_parent);
        assert_eq!(
            got,
            vec![
                Candidate {
                    path: p("/repo"),
                    last_used: Some(NOW),
                },
                Candidate {
                    path: p("/other"),
                    last_used: None,
                },
            ]
        );
    }

    #[test]
    fn main以外と表示中と別名付きは畳まない() {
        let mut f = Fixture::new();
        f.zed = vec![(p("/repo-wt1"), None)];
        for mode in [WorktreeSwitch::Skip, WorktreeSwitch::Follow] {
            f.worktree = mode;
            assert_eq!(f.paths(&wt_parent), vec![p("/repo-wt1")], "{mode:?}");
        }
        f.worktree = WorktreeSwitch::Main;
        f.current = vec![p("/repo-wt1")];
        assert_eq!(f.paths(&wt_parent), vec![p("/repo-wt1")]);
        f.current.clear();
        f.aliased = vec![p("/repo-wt1")];
        assert_eq!(f.paths(&wt_parent), vec![p("/repo-wt1")]);
    }

    #[test]
    fn 隠した行は畳む前と後の両方で除く() {
        let mut f = Fixture::new();
        f.zed = vec![(p("/repo-wt1"), None), (p("/repo-wt2"), None)];
        // Hiding one worktree does not hide its parent via a sibling.
        f.hidden = vec![p("/repo-wt1")];
        assert_eq!(f.paths(&wt_parent), vec![p("/repo")]);
        // Hiding the parent hides what folds into it.
        f.hidden = vec![p("/repo")];
        assert!(f.paths(&wt_parent).is_empty());
    }

    #[test]
    fn 使った時刻はmzedとZedの新しい方() {
        let mut f = Fixture::new();
        f.zed = vec![(p("/a"), Some(NOW - DAY)), (p("/b"), Some(NOW - DAY))];
        f.opened = HashMap::from([(p("/a"), NOW), (p("/b"), 0)]);
        let got = f.collect_with(&every_dir, &no_parent);
        assert_eq!(got[0].last_used, Some(NOW));
        // 0 is "never", not the epoch.
        assert_eq!(got[1].last_used, Some(NOW - DAY));
    }

    fn cand(path: &str, last_used: Option<u64>) -> Candidate {
        Candidate {
            path: p(path),
            last_used,
        }
    }

    #[test]
    fn 空検索は30日以内だけ() {
        let c = vec![
            cand("/edge", Some(NOW - RECENT_WINDOW_SECS)),
            cand("/stale", Some(NOW - RECENT_WINDOW_SECS - 1)),
            cand("/unknown", None),
            cand("/future", Some(NOW + 1)),
        ];
        assert_eq!(visible(&c, &[], true, NOW), vec![p("/edge")]);
    }

    #[test]
    fn 空検索は新しい順に上限まで_免除は枠外() {
        let mut c: Vec<Candidate> = (0..RECENT_LIMIT as u64 + 1)
            .map(|i| cand(&format!("/p{i:02}"), Some(NOW - i)))
            .collect();
        c.push(cand("/old-current", None));
        let exempt = [p("/old-current")];
        let got = visible(&c, &exempt, true, NOW);
        assert_eq!(got.len(), RECENT_LIMIT + 1);
        assert!(got.contains(&p("/old-current")));
        // The oldest of the recent ones is the one cut.
        assert!(!got.contains(&p(&format!("/p{RECENT_LIMIT:02}"))));
    }

    #[test]
    fn 検索中は期限も上限もない() {
        let c = vec![cand("/stale", Some(0)), cand("/unknown", None)];
        assert_eq!(
            visible(&c, &[], false, NOW),
            vec![p("/stale"), p("/unknown")]
        );
    }
}
