//! Project-follow primitives shared by the Zed and Orca watchers (pure logic).
//!
//! Both watchers report the same thing — "the project the user is now looking
//! at" — so [`ActiveProject`] lives here rather than in either watcher.
//! [`SyncSource`] is the user's choice of *whom* to follow; `SyncMode` in
//! `theme.rs` stays the upper-level policy of *how much* to follow.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The project a source reports as currently active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveProject {
    pub paths: String,
    pub timestamp: String,
}

impl ActiveProject {
    /// Split the raw `paths` string into individual workspace roots.
    ///
    /// Zed stores a multi-root workspace's roots in one TEXT column, joined by
    /// a newline (`util::path_list::PathList::serialize`). Single-root
    /// workspaces are just one path. Empty/blank segments are dropped.
    pub fn roots(&self) -> Vec<PathBuf> {
        parse_roots(&self.paths)
    }
}

/// Parse a newline-joined `paths` value into root paths (blank-trimmed,
/// empties dropped). Pure for testability.
pub fn parse_roots(paths: &str) -> Vec<PathBuf> {
    paths
        .split('\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Which app an active-project event came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOrigin {
    Zed,
    Orca,
}

/// One "the active project is now X" report, tagged with who said it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncEvent {
    pub origin: SyncOrigin,
    pub project: Option<ActiveProject>,
    /// A watcher's very first report: the state it found when it started, not
    /// a switch the user just made.
    pub initial: bool,
}

/// What the follow loop has landed on so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Landing {
    #[default]
    Nothing,
    /// One watcher's startup report, from this source.
    Startup(SyncOrigin),
    /// A switch the user actually made.
    Switch,
}

/// Which app mzed follows. `Auto` follows whichever reported last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum SyncSource {
    #[default]
    Auto,
    Zed,
    Orca,
}

impl SyncSource {
    /// Name of what is being followed, for toast/menu wording.
    pub fn label(self) -> &'static str {
        match self {
            SyncSource::Auto => "Zed & Orca",
            SyncSource::Zed => "Zed",
            SyncSource::Orca => "Orca",
        }
    }
}

/// The follow policy a burst is judged against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub source: SyncSource,
    /// Ignore Zed switches into a linked git worktree.
    pub skip_worktrees: bool,
}

/// Whether an event from `origin` may drive a project switch.
pub fn accepts(source: SyncSource, origin: SyncOrigin) -> bool {
    match source {
        SyncSource::Auto => true,
        SyncSource::Zed => origin == SyncOrigin::Zed,
        SyncSource::Orca => origin == SyncOrigin::Orca,
    }
}

/// Whether `event` may replace what is on screen, and what that leaves us
/// landed on.
///
/// A real switch always wins — it is the last thing the user did. A startup
/// report is not a user action, and the two watchers race on startup (reading
/// a JSON file against opening a SQLite DB), so their arrival order means
/// nothing: Zed's report may still claim a landing that only Orca's report
/// made, but never one a real switch made, and Orca's report only lands when
/// nothing else has. mzed followed Zed alone before Orca existed, and starting
/// up in some other project would read as a regression.
pub fn admit(landing: Landing, event: &SyncEvent) -> Option<Landing> {
    // No project means nothing to switch *to*; such an event must not mask a
    // real one or count as a landing.
    event.project.as_ref()?;
    if !event.initial {
        return Some(Landing::Switch);
    }
    let allowed = matches!(
        (landing, event.origin),
        (Landing::Nothing, _) | (Landing::Startup(SyncOrigin::Orca), SyncOrigin::Zed)
    );
    allowed.then_some(Landing::Startup(event.origin))
}

/// Whether this event is a Zed switch into a linked worktree, which the follow
/// ignores.
///
/// With docs kept on the main checkout, a worktree switch would swap the viewer
/// to a tree that has nothing to show. Orca is exempt — it is a worktree
/// manager, so its switches are worktree switches by definition and skipping
/// them would disable the follow entirely.
///
/// `is_worktree` touches the disk, so it is asked at most once per Zed event.
fn skipped(policy: Policy, event: &SyncEvent, is_worktree: &dyn Fn(&Path) -> bool) -> bool {
    if !policy.skip_worktrees || event.origin != SyncOrigin::Zed {
        return false;
    }
    event
        .project
        .as_ref()
        .and_then(|p| p.roots().first().cloned())
        .is_some_and(|primary| is_worktree(&primary))
}

/// Reduce a burst of queued events to the single switch to apply, advancing
/// `landing` as it goes.
///
/// A switch can queue several events before the UI wakes, so applying every
/// one would switch the project two or three times over. Each event is offered
/// to [`admit`] in arrival order and the last one admitted is what the user
/// sees, so a burst settles exactly as the same events would across separate
/// bursts.
///
/// Events the policy rejects are dropped *before* [`admit`] sees them: a source
/// the user is not following, or a worktree Zed moved into, must not claim the
/// landing and take a good event from the other source down with it.
pub fn admit_burst(
    landing: &mut Landing,
    policy: Policy,
    is_worktree: &dyn Fn(&Path) -> bool,
    events: Vec<SyncEvent>,
) -> Option<SyncEvent> {
    let mut chosen = None;
    for event in events {
        if !accepts(policy.source, event.origin) || skipped(policy, &event, is_worktree) {
            continue;
        }
        if let Some(next) = admit(*landing, &event) {
            *landing = next;
            chosen = Some(event);
        }
    }
    chosen
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    #[test]
    fn 単一ルートは1要素() {
        assert_eq!(parse_roots("/a/b"), vec![PathBuf::from("/a/b")]);
    }

    #[test]
    fn 改行区切りの複数ルートを分解する() {
        assert_eq!(
            parse_roots("/a\n/b/c\n/d"),
            vec![
                PathBuf::from("/a"),
                PathBuf::from("/b/c"),
                PathBuf::from("/d"),
            ]
        );
    }

    #[test]
    fn 空白や空セグメントは無視される() {
        assert_eq!(
            parse_roots("  /a  \n\n /b "),
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    #[test]
    fn 空文字列は空ベクタ() {
        assert!(parse_roots("").is_empty());
    }

    #[test]
    fn autoは両方のソースを受け入れる() {
        assert!(accepts(SyncSource::Auto, SyncOrigin::Zed));
        assert!(accepts(SyncSource::Auto, SyncOrigin::Orca));
    }

    #[test]
    fn 固定ソースは他方を拒否する() {
        assert!(accepts(SyncSource::Zed, SyncOrigin::Zed));
        assert!(!accepts(SyncSource::Zed, SyncOrigin::Orca));
        assert!(accepts(SyncSource::Orca, SyncOrigin::Orca));
        assert!(!accepts(SyncSource::Orca, SyncOrigin::Zed));
    }

    fn make(origin: SyncOrigin, paths: Option<&str>, initial: bool) -> SyncEvent {
        SyncEvent {
            origin,
            project: paths.map(|paths| ActiveProject {
                paths: paths.to_string(),
                timestamp: String::new(),
            }),
            initial,
        }
    }

    /// A switch the user made.
    fn event(origin: SyncOrigin, paths: Option<&str>) -> SyncEvent {
        make(origin, paths, false)
    }

    /// A watcher's startup report.
    fn startup(origin: SyncOrigin, paths: Option<&str>) -> SyncEvent {
        make(origin, paths, true)
    }

    /// Follow everything, and nothing is a worktree.
    fn follow_all() -> Policy {
        Policy {
            source: SyncSource::Auto,
            skip_worktrees: true,
        }
    }

    fn never_worktree(_: &Path) -> bool {
        false
    }

    /// Land a burst from scratch and report the project it settles on.
    fn collapsed(events: Vec<SyncEvent>) -> Option<String> {
        admit_burst(
            &mut Landing::default(),
            follow_all(),
            &never_worktree,
            events,
        )
        .map(|e| e.project.unwrap().paths)
    }

    #[test]
    fn 到着順で最後のイベントを採る() {
        let events = vec![
            event(SyncOrigin::Zed, Some("/a")),
            event(SyncOrigin::Orca, Some("/b")),
            event(SyncOrigin::Zed, Some("/c")),
        ];
        assert_eq!(collapsed(events), Some("/c".to_string()));
    }

    #[test]
    fn プロジェクト無しのイベントは後続でも採らない() {
        assert_eq!(
            collapsed(vec![
                event(SyncOrigin::Zed, Some("/a")),
                event(SyncOrigin::Orca, None),
            ]),
            Some("/a".to_string())
        );
        assert_eq!(
            collapsed(vec![
                event(SyncOrigin::Orca, Some("/b")),
                event(SyncOrigin::Zed, None),
            ]),
            Some("/b".to_string())
        );
    }

    #[test]
    fn 全てプロジェクト無しなら何もしない() {
        let mut landing = Landing::default();
        assert_eq!(
            admit_burst(
                &mut landing,
                follow_all(),
                &never_worktree,
                vec![event(SyncOrigin::Zed, None), event(SyncOrigin::Orca, None),]
            ),
            None
        );
        assert_eq!(
            admit_burst(&mut landing, follow_all(), &never_worktree, Vec::new()),
            None
        );
        // A dropped event must not count as a landing.
        assert_eq!(landing, Landing::Nothing);
    }

    /// Feed events one burst at a time, as the follow loop does, and report
    /// the project left on screen.
    fn landed(bursts: Vec<Vec<SyncEvent>>) -> Option<String> {
        landed_with(follow_all(), &never_worktree, bursts)
    }

    fn landed_with(
        policy: Policy,
        is_worktree: &dyn Fn(&Path) -> bool,
        bursts: Vec<Vec<SyncEvent>>,
    ) -> Option<String> {
        let mut landing = Landing::default();
        let mut shown = None;
        for burst in bursts {
            if let Some(event) = admit_burst(&mut landing, policy, is_worktree, burst) {
                shown = Some(event.project.unwrap().paths);
            }
        }
        shown
    }

    #[test]
    fn 起動報告はZedが勝つ() {
        // Whichever order the two startup reports arrive in, and whether they
        // share a burst or not.
        for bursts in [
            vec![
                vec![startup(SyncOrigin::Orca, Some("/o"))],
                vec![startup(SyncOrigin::Zed, Some("/z"))],
            ],
            vec![
                vec![startup(SyncOrigin::Zed, Some("/z"))],
                vec![startup(SyncOrigin::Orca, Some("/o"))],
            ],
            vec![vec![
                startup(SyncOrigin::Orca, Some("/o")),
                startup(SyncOrigin::Zed, Some("/z")),
            ]],
            vec![vec![
                startup(SyncOrigin::Zed, Some("/z")),
                startup(SyncOrigin::Orca, Some("/o")),
            ]],
        ] {
            assert_eq!(landed(bursts), Some("/z".to_string()));
        }
    }

    #[test]
    fn Zedの起動報告が無ければOrcaに着地する() {
        assert_eq!(
            landed(vec![vec![startup(SyncOrigin::Orca, Some("/o"))]]),
            Some("/o".to_string())
        );
        // A Zed report with no project is not a landing place either.
        assert_eq!(
            landed(vec![
                vec![startup(SyncOrigin::Zed, None)],
                vec![startup(SyncOrigin::Orca, Some("/o"))],
            ]),
            Some("/o".to_string())
        );
    }

    #[test]
    fn 遅れて来た起動報告は実際の切替を上書きしない() {
        assert_eq!(
            landed(vec![
                vec![startup(SyncOrigin::Zed, Some("/z"))],
                vec![event(SyncOrigin::Orca, Some("/o"))],
                vec![startup(SyncOrigin::Zed, Some("/z2"))],
            ]),
            Some("/o".to_string())
        );
    }

    #[test]
    fn 実際の切替は常に適用される() {
        assert_eq!(
            landed(vec![
                vec![event(SyncOrigin::Orca, Some("/o"))],
                vec![event(SyncOrigin::Zed, Some("/z"))],
            ]),
            Some("/z".to_string())
        );
    }

    #[test]
    fn ソースによる優先はない() {
        // The same pair in either order yields whichever arrived last.
        assert_eq!(
            collapsed(vec![
                event(SyncOrigin::Zed, Some("/z")),
                event(SyncOrigin::Orca, Some("/o")),
            ]),
            Some("/o".to_string())
        );
        assert_eq!(
            collapsed(vec![
                event(SyncOrigin::Orca, Some("/o")),
                event(SyncOrigin::Zed, Some("/z")),
            ]),
            Some("/z".to_string())
        );
    }

    #[test]
    fn スキップされたworktreeは着地を奪わない() {
        let is_worktree = |p: &Path| p == Path::new("/w");
        // Same burst: the skipped Zed event must not consume Orca's.
        assert_eq!(
            landed_with(
                follow_all(),
                &is_worktree,
                vec![vec![
                    event(SyncOrigin::Orca, Some("/b")),
                    event(SyncOrigin::Zed, Some("/w")),
                ]]
            ),
            Some("/b".to_string())
        );
        // At startup: a skipped Zed report must not block Orca's.
        assert_eq!(
            landed_with(
                follow_all(),
                &is_worktree,
                vec![vec![
                    startup(SyncOrigin::Zed, Some("/w")),
                    startup(SyncOrigin::Orca, Some("/b")),
                ]]
            ),
            Some("/b".to_string())
        );
        // …across bursts too.
        assert_eq!(
            landed_with(
                follow_all(),
                &is_worktree,
                vec![
                    vec![startup(SyncOrigin::Zed, Some("/w"))],
                    vec![startup(SyncOrigin::Orca, Some("/b"))],
                ]
            ),
            Some("/b".to_string())
        );
    }

    #[test]
    fn worktreeスキップはOrcaには効かない() {
        let is_worktree = |p: &Path| p == Path::new("/w");
        assert_eq!(
            landed_with(
                follow_all(),
                &is_worktree,
                vec![vec![event(SyncOrigin::Orca, Some("/w"))]]
            ),
            Some("/w".to_string())
        );
        // And not at all when the setting is off.
        let policy = Policy {
            source: SyncSource::Auto,
            skip_worktrees: false,
        };
        assert_eq!(
            landed_with(
                policy,
                &is_worktree,
                vec![vec![event(SyncOrigin::Zed, Some("/w"))]]
            ),
            Some("/w".to_string())
        );
    }

    #[test]
    fn 追従しないソースは着地を奪わない() {
        let policy = Policy {
            source: SyncSource::Zed,
            skip_worktrees: true,
        };
        assert_eq!(
            landed_with(
                policy,
                &never_worktree,
                vec![vec![
                    startup(SyncOrigin::Orca, Some("/o")),
                    startup(SyncOrigin::Zed, Some("/z")),
                ]]
            ),
            Some("/z".to_string())
        );
    }

    #[test]
    fn sync_sourceはlowercaseでシリアライズされる() {
        let json = serde_json::to_string(&SyncSource::Orca).unwrap();
        assert_eq!(json, "\"orca\"");
        assert_eq!(
            serde_json::from_str::<SyncSource>("\"zed\"").unwrap(),
            SyncSource::Zed
        );
    }
}
