//! T3 Code thread state access (read-only).
//!
//! T3 Code (an agent-thread manager) does not record which thread is on screen.
//! It does append a `thread.visited` event to its SQLite event log when a
//! thread is opened, and again whenever the open thread catches up with new
//! output, so the newest such event names the thread the user is looking at.
//! This is an **undocumented internal database**: the shape below was read off
//! a live install (2026-11-13, v0.1.6) and T3 Code may change it without
//! notice, so anything we fail to read leaves the current project alone.
//!
//! The thread's own `lastVisitedAt` cannot be used instead: it holds the
//! thread's `updatedAt` at the visit, not the time of the visit.

use crate::logging;
use crate::sync::ActiveProject;
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The log's newest sequence, read first so a recreated (shorter) log is
/// noticed.
const NEWEST_SEQUENCE: &str = "SELECT COALESCE(MAX(sequence), 0) FROM orchestration_events";

/// The newest visit in `(?1, ?2]`. `sequence` is the rowid, so the range keeps
/// the scan to rows not yet seen (`event_type` has no index).
const NEWEST_VISIT: &str = "\
    SELECT json_extract(e.payload_json, '$.worktreePath'), p.workspace_root, e.occurred_at \
    FROM orchestration_events e \
    LEFT JOIN projection_projects p \
      ON p.project_id = json_extract(e.payload_json, '$.projectId') \
    WHERE e.event_type = 'thread.visited' AND e.sequence > ?1 AND e.sequence <= ?2 \
    ORDER BY e.sequence DESC LIMIT 1";

/// One `thread.visited` row.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Visit {
    /// `null` for a thread that works in the project checkout itself.
    worktree: Option<String>,
    project_root: Option<String>,
    at: String,
}

/// Why a query told us nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    Open,
    Schema,
    Read,
}

/// The newest visit after sequence `after`, and the sequence the next query
/// should start from.
fn query_visit(db: &Path, after: i64) -> Result<(i64, Option<Visit>), Failure> {
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| Failure::Open)?;
    let mut newest = conn.prepare(NEWEST_SEQUENCE).map_err(|_| Failure::Schema)?;
    let mut visit = conn.prepare(NEWEST_VISIT).map_err(|_| Failure::Schema)?;
    let newest: i64 = newest
        .query_row([], |r| r.get(0))
        .map_err(|_| Failure::Read)?;
    // A log that went backwards was recreated; everything in it is unseen.
    let after = if newest < after { 0 } else { after };
    let visit = visit
        .query_row([after, newest], |r| {
            Ok(Visit {
                worktree: r.get(0)?,
                project_root: r.get(1)?,
                at: r.get(2)?,
            })
        })
        .optional()
        .map_err(|_| Failure::Read)?;
    Ok((newest, visit))
}

/// The directory a visit names, canonicalized: T3 Code stores the same home
/// directory with different letter case across threads, and mzed compares
/// roots byte for byte.
///
/// `None` when it is not a directory on this disk (a deleted worktree, or a
/// thread from another machine): replacing the user's project with it would be
/// worse than staying put.
fn visit_dir(visit: &Visit) -> Option<PathBuf> {
    let raw = visit.worktree.as_ref().or(visit.project_root.as_ref())?;
    let path = Path::new(raw);
    if !path.is_absolute() {
        return None;
    }
    std::fs::canonicalize(path).ok().filter(|p| p.is_dir())
}

/// Outcome of one poll of the database.
enum Poll {
    /// Nothing to apply: the files had not changed, there was no new visit, or
    /// what we read told us nothing usable.
    Unchanged,
    /// The newest visit names this project.
    Read(ActiveProject),
}

/// What we last wrote to the log about the database. The poll loop revisits
/// the same condition every 1500ms, so only a transition is worth a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Health {
    Ok,
    NotFound,
    WatchFailed,
    Unreadable,
    UnexpectedSchema,
    Unresolvable,
}

/// mtimes of the database and its WAL. T3 Code writes through the WAL, and a
/// checkpoint rewrites the main file, so either moving means new rows.
type Stamp = (Option<SystemTime>, Option<SystemTime>);

fn stamp(db: &Path) -> Stamp {
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    (mtime(db), mtime(Path::new(&wal)))
}

/// What the watcher remembers between polls.
#[derive(Default)]
struct Watcher {
    /// File mtimes at the last successful query.
    stamp: Option<Stamp>,
    /// Newest event sequence already looked at.
    sequence: i64,
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

    /// Query for a new visit only when the database files changed.
    ///
    /// A failed query clears `stamp` so the next tick tries again even if the
    /// files stay still.
    fn poll(&mut self, db: &Path) -> Poll {
        let stamp = stamp(db);
        if self.stamp == Some(stamp) {
            return Poll::Unchanged;
        }
        let (sequence, visit) = match query_visit(db, self.sequence) {
            Ok(read) => read,
            Err(failure) => {
                self.stamp = None;
                let (health, why) = match failure {
                    Failure::Schema => (Health::UnexpectedSchema, "unexpected schema in"),
                    Failure::Open | Failure::Read => (Health::Unreadable, "cannot read"),
                };
                self.report(health, || {
                    format!("t3code: {why} {}; keeping current project", db.display())
                });
                return Poll::Unchanged;
            }
        };
        self.stamp = Some(stamp);
        self.sequence = sequence;
        let Some(visit) = visit else {
            self.report(Health::Ok, || format!("t3code: reading {}", db.display()));
            return Poll::Unchanged;
        };
        let Some(dir) = visit_dir(&visit) else {
            self.report(Health::Unresolvable, || {
                format!(
                    "t3code: the last visited thread is not in a directory here ({}); keeping current project",
                    visit.worktree.as_ref().or(visit.project_root.as_ref()).map_or("none", String::as_str)
                )
            });
            return Poll::Unchanged;
        };
        self.report(Health::Ok, || format!("t3code: reading {}", db.display()));
        Poll::Read(ActiveProject {
            paths: dir.to_string_lossy().into_owned(),
            timestamp: visit.at,
        })
    }
}

/// Resolve T3 Code's state database.
///
/// `MZED_T3_DB` overrides the location. It exists so the follow can be
/// exercised against a *copy* of the database — T3 Code's own file is
/// read-only to us and must never be edited.
pub fn default_t3code_db_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MZED_T3_DB") {
        return Some(PathBuf::from(path));
    }
    let db = dirs::home_dir()?.join(".t3/userdata/statev2.sqlite");
    db.exists().then_some(db)
}

use notify_debouncer_full::new_debouncer;
use notify_debouncer_full::notify::RecursiveMode;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const POLL: Duration = Duration::from_millis(1500);
const STOP_POLL: Duration = Duration::from_millis(100);

/// Follow the thread last visited in T3 Code, resolving the database lazily.
///
/// The database can be absent when mzed starts (T3 Code not installed, or not
/// yet launched), so we keep looking for it on the poll interval rather than
/// ending the thread. Blocks the calling thread until `stop` fires or its
/// sender is dropped.
pub fn watch_active_project<F>(stop: &Receiver<()>, mut on_change: F) -> Result<()>
where
    F: FnMut(Option<ActiveProject>),
{
    let mut watcher = Watcher::default();
    loop {
        match default_t3code_db_path() {
            Some(path) => match watch_path(&path, stop, &mut watcher, &mut on_change) {
                Ok(()) => return Ok(()),
                Err(err) => watcher.report(Health::WatchFailed, || {
                    format!("t3code: cannot watch {}: {err}", path.display())
                }),
            },
            None => watcher.report(Health::NotFound, || {
                "t3code: no state db found; waiting for one to appear".to_string()
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

/// Watch one known database and invoke `on_change` whenever the visited
/// project changes. Blocks the calling thread. Mirrors `orca::watch_path`.
fn watch_path(
    db_path: &Path,
    stop: &Receiver<()>,
    watcher: &mut Watcher,
    on_change: &mut dyn FnMut(Option<ActiveProject>),
) -> Result<()> {
    // Watch the directory so both the database and its WAL are observed.
    let watch_dir = db_path
        .parent()
        .context("t3code db path has no parent dir")?
        .to_path_buf();

    let (tx, rx) = mpsc::channel();
    let mut debouncer = new_debouncer(Duration::from_millis(300), None, tx)?;
    debouncer.watch(&watch_dir, RecursiveMode::NonRecursive)?;

    let mut last = match watcher.poll(db_path) {
        Poll::Read(active) => Some(active),
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
        let Poll::Read(current) = watcher.poll(db_path) else {
            continue;
        };
        // Compare by path only: an open thread is re-visited every ~10s as it
        // catches up with new output, and only a project change should
        // re-render.
        if last.as_ref().map(|p| &p.paths) != Some(&current.paths) {
            last = Some(current.clone());
            on_change(Some(current));
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

    /// Drive one known path, the way `watch_active_project` does internally.
    fn watch_until(
        path: &Path,
        stop: &Receiver<()>,
        mut on_change: impl FnMut(Option<ActiveProject>),
    ) -> Result<()> {
        watch_path(path, stop, &mut Watcher::default(), &mut on_change)
    }

    /// The two tables we read, with only the columns we read.
    fn make_db(path: &Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE orchestration_events (
                 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                 event_type TEXT NOT NULL,
                 occurred_at TEXT NOT NULL,
                 payload_json TEXT NOT NULL
             );
             CREATE TABLE projection_projects (
                 project_id TEXT PRIMARY KEY,
                 workspace_root TEXT NOT NULL
             );",
        )
        .unwrap();
        conn
    }

    fn add_project(conn: &Connection, id: &str, root: &Path) {
        conn.execute(
            "INSERT INTO projection_projects VALUES (?1, ?2)",
            (id, root.to_string_lossy()),
        )
        .unwrap();
    }

    fn add_event(conn: &Connection, event_type: &str, project_id: &str, worktree: Option<&Path>) {
        let payload = serde_json::json!({
            "projectId": project_id,
            "worktreePath": worktree.map(|p| p.to_string_lossy()),
        });
        conn.execute(
            "INSERT INTO orchestration_events (event_type, occurred_at, payload_json) \
             VALUES (?1, '2026-11-13T00:00:00.000Z', ?2)",
            (event_type, payload.to_string()),
        )
        .unwrap();
    }

    fn visit(conn: &Connection, project_id: &str, worktree: Option<&Path>) {
        add_event(conn, "thread.visited", project_id, worktree);
    }

    /// A real directory (the resolution canonicalizes against the disk).
    fn dir_in(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::create_dir_all(&path).unwrap();
        path.canonicalize().unwrap()
    }

    fn read(watcher: &mut Watcher, db: &Path) -> Option<String> {
        // Force a query: tests write faster than mtimes are guaranteed to move.
        watcher.stamp = None;
        match watcher.poll(db) {
            Poll::Read(p) => Some(p.paths),
            Poll::Unchanged => None,
        }
    }

    fn path_string(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn 最新のvisitのworktreePathを採る() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (dir_in(tmp.path(), "a"), dir_in(tmp.path(), "b"));
        let db = tmp.path().join("statev2.sqlite");
        let conn = make_db(&db);
        add_project(&conn, "p", tmp.path());
        visit(&conn, "p", Some(&a));
        visit(&conn, "p", Some(&b));

        assert_eq!(read(&mut Watcher::default(), &db), Some(path_string(&b)));
    }

    #[test]
    fn worktreePathがnullならプロジェクトのルートを採る() {
        let tmp = tempfile::tempdir().unwrap();
        let root = dir_in(tmp.path(), "repo");
        let db = tmp.path().join("statev2.sqlite");
        let conn = make_db(&db);
        add_project(&conn, "p", &root);
        visit(&conn, "p", None);

        assert_eq!(read(&mut Watcher::default(), &db), Some(path_string(&root)));
    }

    #[test]
    fn visit以外のイベントは無視する() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (dir_in(tmp.path(), "a"), dir_in(tmp.path(), "b"));
        let db = tmp.path().join("statev2.sqlite");
        let conn = make_db(&db);
        visit(&conn, "p", Some(&a));
        // An agent working in another thread writes events of other types.
        add_event(&conn, "thread.metadata-updated", "p", Some(&b));

        assert_eq!(read(&mut Watcher::default(), &db), Some(path_string(&a)));
    }

    #[test]
    fn 新しいvisitが無ければ報告しない() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (dir_in(tmp.path(), "a"), dir_in(tmp.path(), "b"));
        let db = tmp.path().join("statev2.sqlite");
        let conn = make_db(&db);
        visit(&conn, "p", Some(&a));

        let mut watcher = Watcher::default();
        assert_eq!(read(&mut watcher, &db), Some(path_string(&a)));
        add_event(&conn, "turn-item.updated", "p", Some(&b));
        assert_eq!(read(&mut watcher, &db), None);
        visit(&conn, "p", Some(&b));
        assert_eq!(read(&mut watcher, &db), Some(path_string(&b)));
    }

    #[test]
    fn ファイルが変わらなければ問い合わせない() {
        let tmp = tempfile::tempdir().unwrap();
        let a = dir_in(tmp.path(), "a");
        let db = tmp.path().join("statev2.sqlite");
        let conn = make_db(&db);
        visit(&conn, "p", Some(&a));
        drop(conn);

        let mut watcher = Watcher::default();
        assert!(matches!(watcher.poll(&db), Poll::Read(_)));
        // Rewind the cursor: a query now would report the same visit again.
        watcher.sequence = 0;
        assert!(matches!(watcher.poll(&db), Poll::Unchanged));
    }

    #[test]
    fn パスを正規化する() {
        let tmp = tempfile::tempdir().unwrap();
        let real = dir_in(tmp.path(), "real");
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let db = tmp.path().join("statev2.sqlite");
        let conn = make_db(&db);
        visit(&conn, "p", Some(&link));

        assert_eq!(read(&mut Watcher::default(), &db), Some(path_string(&real)));
    }

    #[test]
    fn ディレクトリでないパスは前回値を維持する() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("note.md");
        std::fs::write(&file, "x").unwrap();
        let db = tmp.path().join("statev2.sqlite");
        let conn = make_db(&db);

        let mut watcher = Watcher::default();
        for gone in [tmp.path().join("gone"), file, PathBuf::from("relative")] {
            visit(&conn, "p", Some(&gone));
            assert_eq!(read(&mut watcher, &db), None, "{}", gone.display());
        }
        // Neither a worktree nor a known project.
        visit(&conn, "unknown", None);
        assert_eq!(read(&mut watcher, &db), None);
        assert_eq!(watcher.reported, Some(Health::Unresolvable));
    }

    #[test]
    fn 表が無いDBは前回値を維持する() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("statev2.sqlite");
        Connection::open(&db)
            .unwrap()
            .execute_batch("CREATE TABLE other (x)")
            .unwrap();

        let mut watcher = Watcher::default();
        assert!(matches!(watcher.poll(&db), Poll::Unchanged));
        assert_eq!(watcher.reported, Some(Health::UnexpectedSchema));
        // Cleared so the next tick asks again.
        assert_eq!(watcher.stamp, None);
    }

    #[test]
    fn DBが作り直されたら先頭から読み直す() {
        let tmp = tempfile::tempdir().unwrap();
        let a = dir_in(tmp.path(), "a");
        let db = tmp.path().join("statev2.sqlite");
        let conn = make_db(&db);
        visit(&conn, "p", Some(&a));

        let mut watcher = Watcher {
            sequence: 1_000,
            ..Watcher::default()
        };
        assert_eq!(read(&mut watcher, &db), Some(path_string(&a)));
        assert_eq!(watcher.sequence, 1);
    }

    #[test]
    fn DBが無くてもパニックしない() {
        let mut watcher = Watcher::default();
        assert!(matches!(
            watcher.poll(Path::new("/no/such/statev2.sqlite")),
            Poll::Unchanged
        ));
        assert_eq!(watcher.reported, Some(Health::Unreadable));
    }

    #[test]
    fn watch_untilはDB不在でも停止できる() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("statev2.sqlite");
        let (stop_tx, stop_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result = watch_until(&db, &stop_rx, |_| {});
            done_tx.send(result.is_ok()).unwrap();
        });
        stop_tx.send(()).unwrap();

        // Generous bound: this asserts the watcher stops without a file event,
        // not how fast (same reasoning as the Zed watcher stop test).
        assert!(done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("t3code watcher did not stop"));
    }

    #[test]
    fn watch_untilは別のスレッドのvisitで切替を通知する() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (dir_in(tmp.path(), "a"), dir_in(tmp.path(), "b"));
        let db = tmp.path().join("statev2.sqlite");
        let conn = make_db(&db);
        visit(&conn, "p", Some(&a));
        let (stop_tx, stop_rx) = mpsc::channel();
        let (seen_tx, seen_rx) = mpsc::channel();

        let watched = db.clone();
        std::thread::spawn(move || {
            let _ = watch_until(&watched, &stop_rx, move |active| {
                let _ = seen_tx.send(active.map(|p| p.paths));
            });
        });
        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            Some(path_string(&a))
        );

        // Re-visiting the same thread is not a switch.
        visit(&conn, "p", Some(&a));
        visit(&conn, "p", Some(&b));
        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            Some(path_string(&b))
        );
        stop_tx.send(()).unwrap();
    }

    // The MZED_T3_DB tests set a process-wide env var; nextest runs every test
    // in its own process, so they cannot leak into another test.
    #[test]
    fn 環境変数でDBの場所を差し替えられる() {
        std::env::set_var("MZED_T3_DB", "/tmp/mzed-test/statev2.sqlite");
        let path = default_t3code_db_path().unwrap();
        std::env::remove_var("MZED_T3_DB");
        assert_eq!(path, PathBuf::from("/tmp/mzed-test/statev2.sqlite"));
    }

    #[test]
    fn 後から現れたDBを拾う() {
        let tmp = tempfile::tempdir().unwrap();
        let a = dir_in(tmp.path(), "a");
        let db = tmp.path().join("late/statev2.sqlite");
        std::env::set_var("MZED_T3_DB", &db);
        let (stop_tx, stop_rx) = mpsc::channel();
        let (seen_tx, seen_rx) = mpsc::channel();

        std::thread::spawn(move || {
            let _ = watch_active_project(&stop_rx, move |active| {
                if active.is_some() {
                    let _ = seen_tx.send(active.map(|p| p.paths));
                }
            });
        });

        // Nothing to watch yet: the parent dir does not even exist.
        assert!(seen_rx.recv_timeout(Duration::from_millis(500)).is_err());
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        let conn = make_db(&db);
        visit(&conn, "p", Some(&a));

        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            Some(path_string(&a))
        );
        stop_tx.send(()).unwrap();
    }
}
