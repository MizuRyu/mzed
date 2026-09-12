//! Orca workspace state access (read-only).
//!
//! Orca (worktree / terminal manager) keeps its whole session in one JSON file.
//! This is an **undocumented internal file**: the shape below was read off a
//! live install (2026-09-09) and Orca may change it without notice, so every
//! field is optional and anything we fail to understand leaves the current
//! project alone instead of clearing it.
//!
//! The file is ~500KB and Orca rewrites it constantly, so we gate on mtime and
//! only parse when it actually changed.

use crate::logging;
use crate::sync::ActiveProject;
use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The subset of `orca-data.json` we read. Everything is optional: a shape
/// change in Orca must not take the watcher down.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrcaData {
    #[serde(default)]
    workspace_session: WorkspaceSession,
    /// Plain folders opened as workspaces (no git repo behind them).
    #[serde(default)]
    folder_workspaces: Vec<FolderWorkspace>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceSession {
    /// Absent key vs explicit `null` are different answers ("Orca's shape
    /// moved" vs "nothing is open"), so the outer `Option` tracks presence.
    #[serde(default, deserialize_with = "present_option")]
    active_worktree_id: Option<Option<String>>,
    /// `"local|<worktreeId>" -> ms epoch`. Only used when there is no active id.
    #[serde(default)]
    last_visited_at_by_worktree_id: HashMap<String, f64>,
}

/// Deserialize a field into `Some(..)` whenever the key is present, so a
/// missing key (`None`) stays distinguishable from an explicit `null`.
fn present_option<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::deserialize(d).map(Some)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderWorkspace {
    #[serde(default)]
    id: String,
    #[serde(default)]
    folder_path: Option<String>,
}

/// What one reading of `orca-data.json` tells us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// A project path we could resolve.
    Project(ActiveProject),
    /// The file says nothing is open.
    NoProject,
    /// The keys we need are absent or name something we cannot resolve. Orca's
    /// shape may have moved under us; keeping the current project is better
    /// than clearing it on a guess.
    Unknown,
}

/// Resolve one Orca worktree id to a project path.
///
/// Two id forms exist: `"<repoUuid>::<absolute path>"` carries the path
/// inline, and `"folder:<uuid>"` refers to a `folderWorkspaces` entry.
///
/// The result must be an absolute directory that exists: Orca keeps entries for
/// worktrees it has since deleted, and replacing the user's project with a path
/// that is gone (or is a file) is worse than staying put.
fn worktree_path(data: &OrcaData, id: &str, is_dir: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let path = if let Some(uuid) = id.strip_prefix("folder:") {
        data.folder_workspaces
            .iter()
            .find(|f| f.id == uuid)
            .and_then(|f| f.folder_path.clone())
            .map(PathBuf::from)?
    } else {
        PathBuf::from(id.split_once("::")?.1)
    };
    (path.is_absolute() && is_dir(&path)).then_some(path)
}

/// The worktree Orca last visited, used when `activeWorktreeId` is missing or
/// unresolvable (e.g. it names a worktree Orca has since dropped).
fn last_visited(data: &OrcaData) -> Option<(String, f64)> {
    data.workspace_session
        .last_visited_at_by_worktree_id
        .iter()
        // Keys are host-scoped (`local|<worktreeId>`); the worktree id is the
        // part after the host.
        .map(|(key, at)| {
            let id = key.split_once('|').map_or(key.as_str(), |(_, id)| id);
            (id.to_string(), *at)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// Parse `orca-data.json` text into the project Orca has active.
pub fn parse_active_project(json: &str) -> Result<Resolved> {
    parse_active_project_with(json, &|path| path.is_dir())
}

/// [`parse_active_project`] with the directory check injected, so the parsing
/// and resolution rules stay testable without laying down real directories.
pub fn parse_active_project_with(json: &str, is_dir: &dyn Fn(&Path) -> bool) -> Result<Resolved> {
    let data: OrcaData = serde_json::from_str(json).context("parse orca-data.json")?;
    let active = data.workspace_session.active_worktree_id.clone();
    let resolved = active
        .clone()
        .flatten()
        .and_then(|id| worktree_path(&data, &id, is_dir).map(|path| (id, path)))
        .or_else(|| {
            let (id, _) = last_visited(&data)?;
            worktree_path(&data, &id, is_dir).map(|path| (id, path))
        });
    let Some((id, path)) = resolved else {
        let explicitly_none = matches!(active, Some(None))
            && data
                .workspace_session
                .last_visited_at_by_worktree_id
                .is_empty();
        return Ok(if explicitly_none {
            Resolved::NoProject
        } else {
            Resolved::Unknown
        });
    };
    let timestamp = data
        .workspace_session
        .last_visited_at_by_worktree_id
        .iter()
        .find(|(key, _)| key.ends_with(&id))
        .map(|(_, at)| format!("{at:.0}"))
        .unwrap_or_default();
    Ok(Resolved::Project(ActiveProject {
        paths: path.to_string_lossy().into_owned(),
        timestamp,
    }))
}

/// Outcome of one poll of the state file.
enum Poll {
    /// Nothing to apply: the mtime had not moved (so the JSON was never
    /// parsed), or what we read told us nothing usable.
    Unchanged,
    /// The file was re-read; this is the project it names (`None` = none open).
    Read(Option<ActiveProject>),
}

/// What we last wrote to the log about the state file. The poll loop revisits
/// the same condition every 1500ms, so only a transition is worth a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Health {
    Ok,
    NotFound,
    WatchFailed,
    Unreadable,
    Unparsable,
    NoActiveKeys,
}

/// What the watcher remembers between polls.
#[derive(Default)]
struct Watcher {
    /// mtime of the last text we actually parsed.
    mtime: Option<SystemTime>,
    reported: Option<Health>,
}

impl Watcher {
    /// Log `message` only when the watcher's condition actually changed.
    fn report(&mut self, health: Health, message: impl FnOnce() -> String) {
        if self.reported == Some(health) {
            return;
        }
        self.reported = Some(health);
        logging::app(message());
    }

    /// Read and parse the state file only when its mtime moved.
    ///
    /// An unreadable or half-written file keeps the caller's previous value;
    /// `mtime` is cleared in that case so the next successful read is never
    /// skipped (the file may come back with an older mtime, e.g. after being
    /// renamed away and back).
    fn poll(&mut self, path: &Path) -> Poll {
        let Ok(mtime) = std::fs::metadata(path).and_then(|m| m.modified()) else {
            self.mtime = None;
            self.report(Health::Unreadable, || {
                format!(
                    "orca: cannot read {}; keeping current project",
                    path.display()
                )
            });
            return Poll::Unchanged;
        };
        if self.mtime == Some(mtime) {
            return Poll::Unchanged;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            self.mtime = None;
            self.report(Health::Unreadable, || {
                format!(
                    "orca: cannot read {}; keeping current project",
                    path.display()
                )
            });
            return Poll::Unchanged;
        };
        match parse_active_project(&text) {
            Ok(Resolved::Project(active)) => {
                self.mtime = Some(mtime);
                self.report(Health::Ok, || format!("orca: reading {}", path.display()));
                Poll::Read(Some(active))
            }
            Ok(Resolved::NoProject) => {
                self.mtime = Some(mtime);
                self.report(Health::Ok, || format!("orca: reading {}", path.display()));
                Poll::Read(None)
            }
            // Parsed, but nothing we recognise. Record the mtime anyway: the
            // file has not changed, so re-parsing 500KB every tick would buy
            // nothing.
            Ok(Resolved::Unknown) => {
                self.mtime = Some(mtime);
                self.report(Health::NoActiveKeys, || {
                    format!(
                        "orca: no active worktree in {}; keeping current project",
                        path.display()
                    )
                });
                Poll::Unchanged
            }
            // Orca rewrites the whole file, so a read can land mid-write. Retry
            // on the next tick instead of reporting a spurious "no project".
            Err(_) => {
                self.mtime = None;
                self.report(Health::Unparsable, || {
                    format!(
                        "orca: {} is not valid JSON (mid-write?); keeping current project",
                        path.display()
                    )
                });
                Poll::Unchanged
            }
        }
    }
}

/// Resolve Orca's state file.
///
/// `MZED_ORCA_DATA` overrides the location. It exists so the Orca follow can be
/// exercised against a *copy* of the state file — Orca's own file is read-only
/// to us and must never be edited.
pub fn default_orca_data_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MZED_ORCA_DATA") {
        return Some(PathBuf::from(path));
    }
    // Usually a single `local-default` profile; if an install has several, the
    // most recently written one is the one being used.
    let profiles = dirs::home_dir()?.join("Library/Application Support/orca/profiles");
    std::fs::read_dir(profiles)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join("orca-data.json"))
        .filter_map(|path| {
            let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
            Some((mtime, path))
        })
        .max_by_key(|(mtime, _)| *mtime)
        .map(|(_, path)| path)
}

use notify_debouncer_full::new_debouncer;
use notify_debouncer_full::notify::RecursiveMode;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const POLL: Duration = Duration::from_millis(1500);
const STOP_POLL: Duration = Duration::from_millis(100);

/// Follow Orca's active project, resolving the state file lazily.
///
/// The file can be absent when mzed starts (Orca not installed, or not yet
/// launched for the first time), so we keep looking for it on the poll
/// interval rather than ending the thread — a `stat` every 1.5s is cheap
/// enough to pay in an Orca-less environment. Blocks the calling thread until
/// `stop` fires or its sender is dropped.
pub fn watch_active_project<F>(stop: &Receiver<()>, mut on_change: F) -> Result<()>
where
    F: FnMut(Option<ActiveProject>),
{
    let mut watcher = Watcher::default();
    loop {
        match default_orca_data_path() {
            Some(path) => match watch_path(&path, stop, &mut watcher, &mut on_change) {
                Ok(()) => return Ok(()),
                Err(err) => watcher.report(Health::WatchFailed, || {
                    format!("orca: cannot watch {}: {err}", path.display())
                }),
            },
            None => watcher.report(Health::NotFound, || {
                "orca: no state file found; waiting for one to appear".to_string()
            }),
        }
        if stop_requested(stop.recv_timeout(POLL)) {
            return Ok(());
        }
    }
}

/// A stop signal, or a stop sender that has gone away, both end the watcher.
fn stop_requested(recv: Result<(), mpsc::RecvTimeoutError>) -> bool {
    !matches!(recv, Err(mpsc::RecvTimeoutError::Timeout))
}

/// Watch one known state file and invoke `on_change` whenever the active
/// project changes. Blocks the calling thread. Mirrors
/// [`crate::zed::watch_until`], plus the shared [`Watcher`] so log lines stay
/// deduplicated across a re-resolve.
fn watch_path(
    data_path: &Path,
    stop: &Receiver<()>,
    watcher: &mut Watcher,
    on_change: &mut dyn FnMut(Option<ActiveProject>),
) -> Result<()> {
    // Orca replaces the file rather than writing in place, so watch the
    // containing directory: the inode we started on stops receiving events.
    let watch_dir = data_path
        .parent()
        .context("orca data path has no parent dir")?
        .to_path_buf();

    let (tx, rx) = mpsc::channel();
    let mut debouncer = new_debouncer(Duration::from_millis(300), None, tx)?;
    debouncer.watch(&watch_dir, RecursiveMode::NonRecursive)?;

    let mut last = match watcher.poll(data_path) {
        Poll::Read(active) => active,
        Poll::Unchanged => None,
    };
    on_change(last.clone());

    let mut last_poll = std::time::Instant::now();
    loop {
        if stop_now(stop) {
            break;
        }
        let event_received = match rx.recv_timeout(STOP_POLL) {
            Ok(_) => true,
            Err(mpsc::RecvTimeoutError::Timeout) => false,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let poll_due = last_poll.elapsed() >= POLL;
        if !event_received && !poll_due {
            continue;
        }
        if poll_due {
            last_poll = std::time::Instant::now();
        }
        let Poll::Read(current) = watcher.poll(data_path) else {
            continue;
        };
        // Compare by path only, like the Zed watcher: Orca touches the file on
        // any UI activity, and only a project change should re-render.
        let changed = current.as_ref().map(|p| &p.paths) != last.as_ref().map(|p| &p.paths);
        if changed {
            last = current.clone();
            on_change(current);
        }
    }
    Ok(())
}

/// Non-blocking check: has the watcher been told to stop, or has its stop
/// sender been dropped?
fn stop_now(stop: &Receiver<()>) -> bool {
    !matches!(stop.try_recv(), Err(mpsc::TryRecvError::Empty))
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;
    use std::io::Write;

    /// Drive one known path, the way `watch_active_project` does internally.
    fn watch_until(
        path: &Path,
        stop: &Receiver<()>,
        mut on_change: impl FnMut(Option<ActiveProject>),
    ) -> Result<()> {
        watch_path(path, stop, &mut Watcher::default(), &mut on_change)
    }

    const REPO_FORM: &str = include_str!("../tests/fixtures/orca-data-repo.json");
    const FOLDER_FORM: &str = include_str!("../tests/fixtures/orca-data-folder.json");
    const FALLBACK_FORM: &str = include_str!("../tests/fixtures/orca-data-fallback.json");

    fn any_dir(_: &Path) -> bool {
        true
    }

    /// A state file naming `path` as the active worktree. The watcher tests use
    /// real directories because `poll` applies the real `is_dir` check.
    fn state_json(path: &Path) -> String {
        format!(
            r#"{{"workspaceSession":{{"activeWorktreeId":"uuid::{}"}}}}"#,
            path.display()
        )
    }

    /// A directory named `name` under `dir`, and a state file pointing at it.
    fn project_dir(dir: &Path, name: &str) -> (PathBuf, String) {
        let path = dir.join(name);
        std::fs::create_dir_all(&path).unwrap();
        let json = state_json(&path);
        (path, json)
    }

    fn resolved(json: &str) -> Resolved {
        parse_active_project_with(json, &any_dir).unwrap()
    }

    fn project(json: &str) -> ActiveProject {
        match resolved(json) {
            Resolved::Project(p) => p,
            other => panic!("expected a project, got {other:?}"),
        }
    }

    #[test]
    fn 実在するディレクトリだけが採用される() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("project");
        std::fs::create_dir(&real).unwrap();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "x").unwrap();
        let json = |path: &Path| {
            format!(
                r#"{{"workspaceSession":{{"activeWorktreeId":"uuid::{}"}}}}"#,
                path.display()
            )
        };
        // The real check (`Path::is_dir`), not an injected one.
        assert!(matches!(
            parse_active_project(&json(&real)).unwrap(),
            Resolved::Project(_)
        ));
        assert_eq!(
            parse_active_project(&json(&file)).unwrap(),
            Resolved::Unknown
        );
        assert_eq!(
            parse_active_project(&json(&dir.path().join("absent"))).unwrap(),
            Resolved::Unknown
        );
    }

    #[test]
    fn repoUuid付きidはコロン2つ以降をパスにする() {
        let active = project(REPO_FORM);
        assert_eq!(active.paths, "/tmp/projects/alpha");
        assert_eq!(active.timestamp, "1788000002000");
    }

    #[test]
    fn folder形式はfolderWorkspacesのfolderPathを引く() {
        assert_eq!(project(FOLDER_FORM).paths, "/tmp/notes");
    }

    #[test]
    fn activeが無ければ最終訪問が新しいworktreeを使う() {
        assert_eq!(project(FALLBACK_FORM).paths, "/tmp/projects/beta");
    }

    #[test]
    fn 不正JSONはエラーになる() {
        assert!(parse_active_project("{\"workspaceSession\":").is_err());
    }

    #[test]
    fn 未知の形のidは判断を保留する() {
        let json = r#"{"workspaceSession":{"activeWorktreeId":"nonsense"}}"#;
        assert_eq!(resolved(json), Resolved::Unknown);
    }

    #[test]
    fn 空のJSONオブジェクトは判断を保留する() {
        assert_eq!(resolved("{}"), Resolved::Unknown);
    }

    #[test]
    fn 明示的なnullは開いていない状態として扱う() {
        let json =
            r#"{"workspaceSession":{"activeWorktreeId":null,"lastVisitedAtByWorktreeId":{}}}"#;
        assert_eq!(resolved(json), Resolved::NoProject);
    }

    #[test]
    fn 消えたディレクトリは判断を保留する() {
        // A worktree Orca still lists but that is gone from disk, and a path
        // that resolves to a file.
        assert_eq!(
            parse_active_project_with(REPO_FORM, &|_| false).unwrap(),
            Resolved::Unknown
        );
        assert_eq!(
            parse_active_project_with(FOLDER_FORM, &|_| false).unwrap(),
            Resolved::Unknown
        );
    }

    #[test]
    fn 相対パスは判断を保留する() {
        let json = r#"{"workspaceSession":{"activeWorktreeId":"uuid::relative/path"}}"#;
        assert_eq!(resolved(json), Resolved::Unknown);
    }

    #[test]
    fn 解決できないactiveは最終訪問へフォールバックする() {
        // The active id names a directory that is gone; the last-visited one is
        // still there.
        let json = r#"{
            "workspaceSession": {
                "activeWorktreeId": "uuid::/gone",
                "lastVisitedAtByWorktreeId": { "local|uuid::/here": 1788000000000 }
            }
        }"#;
        let alive = |p: &Path| p == Path::new("/here");
        match parse_active_project_with(json, &alive).unwrap() {
            Resolved::Project(p) => assert_eq!(p.paths, "/here"),
            other => panic!("expected a project, got {other:?}"),
        }
    }

    #[test]
    fn mtimeが変わらなければJSONを読み直さない() {
        let dir = tempfile::tempdir().unwrap();
        let (_, json) = project_dir(dir.path(), "alpha");
        let path = dir.path().join("orca-data.json");
        std::fs::write(&path, &json).unwrap();

        let mut watcher = Watcher::default();
        assert!(matches!(watcher.poll(&path), Poll::Read(Some(_))));
        // Same mtime: the 500KB parse must not run again.
        assert!(matches!(watcher.poll(&path), Poll::Unchanged));

        // A rewrite with a distinct mtime is picked up.
        let newer = std::time::SystemTime::now() + Duration::from_secs(2);
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(newer).unwrap();
        assert!(matches!(watcher.poll(&path), Poll::Read(Some(_))));
    }

    #[test]
    fn キー欠落は前回値を維持し再パースもしない() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orca-data.json");
        std::fs::write(&path, "{}").unwrap();

        let mut watcher = Watcher::default();
        assert!(matches!(watcher.poll(&path), Poll::Unchanged));
        // The mtime is still recorded, so the next tick skips the parse.
        assert!(watcher.mtime.is_some());
        assert!(matches!(watcher.poll(&path), Poll::Unchanged));
    }

    #[test]
    fn 壊れたJSONは前回値を維持し次回再読込する() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orca-data.json");
        std::fs::write(&path, "{ half-written").unwrap();

        let mut watcher = Watcher {
            mtime: Some(SystemTime::now()),
            reported: None,
        };
        assert!(matches!(watcher.poll(&path), Poll::Unchanged));
        // Cleared so a repaired file is re-read even if its mtime went backwards.
        assert_eq!(watcher.mtime, None);
    }

    #[test]
    fn ファイルが無くてもパニックしない() {
        let mut watcher = Watcher::default();
        assert!(matches!(
            watcher.poll(Path::new("/no/such/orca-data.json")),
            Poll::Unchanged
        ));
    }

    #[test]
    fn watch_untilはファイル不在でも停止できる() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orca-data.json");
        let (stop_tx, stop_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result = watch_until(&path, &stop_rx, |_| {});
            done_tx.send(result.is_ok()).unwrap();
        });
        stop_tx.send(()).unwrap();

        // Generous bound: this asserts the watcher stops without a file event,
        // not how fast (same reasoning as the Zed watcher stop test).
        assert!(done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("orca watcher did not stop"));
    }

    #[test]
    fn stop送信側をdropしても終了する() {
        let dir = tempfile::tempdir().unwrap();
        let (_, json) = project_dir(dir.path(), "alpha");
        let path = dir.path().join("orca-data.json");
        std::fs::write(&path, &json).unwrap();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (done_tx, done_rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result = watch_until(&path, &stop_rx, |_| {});
            done_tx.send(result.is_ok()).unwrap();
        });
        drop(stop_tx);

        assert!(done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("orca watcher did not stop when its stop sender was dropped"));
    }

    #[test]
    fn watch_untilはファイル書き換えで切替を通知する() {
        let dir = tempfile::tempdir().unwrap();
        let (alpha, alpha_json) = project_dir(dir.path(), "alpha");
        let (beta, beta_json) = project_dir(dir.path(), "beta");
        let path = dir.path().join("orca-data.json");
        std::fs::write(&path, &alpha_json).unwrap();
        let (stop_tx, stop_rx) = mpsc::channel();
        let (seen_tx, seen_rx) = mpsc::channel();

        let watched = path.clone();
        std::thread::spawn(move || {
            let _ = watch_until(&watched, &stop_rx, move |active| {
                let _ = seen_tx.send(active.map(|p| p.paths));
            });
        });

        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            Some(alpha.to_string_lossy().into_owned())
        );

        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(beta_json.as_bytes()).unwrap();
        file.sync_all().unwrap();
        drop(file);

        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            Some(beta.to_string_lossy().into_owned())
        );
        stop_tx.send(()).unwrap();
    }

    // The MZED_ORCA_DATA tests set a process-wide env var; nextest runs every
    // test in its own process, so they cannot leak into another test.
    #[test]
    fn 環境変数で状態ファイルの場所を差し替えられる() {
        std::env::set_var("MZED_ORCA_DATA", "/tmp/mzed-test/orca-data.json");
        let path = default_orca_data_path().unwrap();
        std::env::remove_var("MZED_ORCA_DATA");
        assert_eq!(path, PathBuf::from("/tmp/mzed-test/orca-data.json"));
    }

    #[test]
    fn 後から現れた状態ファイルを拾う() {
        let dir = tempfile::tempdir().unwrap();
        let (alpha, alpha_json) = project_dir(dir.path(), "alpha");
        let path = dir.path().join("late/orca-data.json");
        std::env::set_var("MZED_ORCA_DATA", &path);
        let (stop_tx, stop_rx) = mpsc::channel();
        let (seen_tx, seen_rx) = mpsc::channel();

        std::thread::spawn(move || {
            let _ = watch_active_project(&stop_rx, move |active| {
                let _ = seen_tx.send(active.map(|p| p.paths));
            });
        });

        // Nothing to watch yet: the parent dir does not even exist.
        assert!(seen_rx.recv_timeout(Duration::from_millis(500)).is_err());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &alpha_json).unwrap();

        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            Some(alpha.to_string_lossy().into_owned())
        );
        stop_tx.send(()).unwrap();
    }
}
