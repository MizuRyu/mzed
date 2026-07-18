//! Zed workspace database access (read-only) for the mzed prototype.

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

/// The most-recently-active Zed project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveProject {
    pub paths: String,
    pub timestamp: String,
}

impl ActiveProject {
    /// Split the raw `paths` string into individual workspace roots.
    #[allow(dead_code)] // Used by the mzed bin, not the zed_watch bin.
    ///
    /// Zed stores a multi-root workspace's roots in one TEXT column, joined by
    /// a newline (`util::path_list::PathList::serialize`). Single-root
    /// workspaces are just one path. Empty/blank segments are dropped.
    pub fn roots(&self) -> Vec<PathBuf> {
        parse_roots(&self.paths)
    }
}

/// Parse Zed's newline-joined `paths` column into root paths (blank-trimmed,
/// empties dropped). Pure for testability.
#[allow(dead_code)] // Used by the mzed bin, not the zed_watch bin.
pub fn parse_roots(paths: &str) -> Vec<PathBuf> {
    paths
        .split('\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Query the project the user is currently focused on in Zed.
///
/// Zed records the active project per window in
/// `scoped_kv_store(namespace='multi_workspace_state')` as `active_workspace_id`,
/// and the window focus order in `kv_store('session_window_stack')` (front-most
/// first). We resolve focused window -> active_workspace_id -> paths. This
/// reflects the project the user actually has in front and ignores background
/// `workspaces.timestamp` churn (LSP, autosave, opening a file).
///
/// Falls back to the most-recently-touched workspace when those keys are absent
/// (older Zed, or a session that never wrote the multi-workspace state).
pub fn query_active_project(db_path: &Path) -> Result<Option<ActiveProject>> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open zed db: {}", db_path.display()))?;

    if let Some(p) = query_focused_project(&conn) {
        return Ok(Some(p));
    }
    query_latest_project(&conn)
}

/// Precise path: front-most window -> its active workspace -> paths.
/// Best-effort: any missing table/column/row yields `None` so the caller can
/// fall back. (Errors are swallowed via `.ok()` because a minimal/old DB simply
/// lacks these tables.)
fn query_focused_project(conn: &Connection) -> Option<ActiveProject> {
    // Front-most window id = first element of the focus stack.
    let window_id: i64 = conn
        .query_row(
            "SELECT json_extract(value, '$[0]') \
             FROM kv_store WHERE key = 'session_window_stack'",
            [],
            |r| r.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten()?;

    // Active workspace id for that window.
    let active_ws: i64 = conn
        .query_row(
            "SELECT json_extract(value, '$.active_workspace_id') \
             FROM scoped_kv_store \
             WHERE namespace = 'multi_workspace_state' AND key = ?1",
            [window_id.to_string()],
            |r| r.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten()?;

    // Resolve that workspace's path + timestamp.
    conn.query_row(
        "SELECT paths, timestamp FROM workspaces \
         WHERE workspace_id = ?1 AND paths IS NOT NULL AND paths != ''",
        [active_ws],
        |r| {
            Ok(ActiveProject {
                paths: r.get(0)?,
                timestamp: r.get(1)?,
            })
        },
    )
    .ok()
}

/// Fallback: the most-recently-touched workspace (string timestamp order).
fn query_latest_project(conn: &Connection) -> Result<Option<ActiveProject>> {
    let mut stmt = conn.prepare(
        "SELECT paths, timestamp FROM workspaces \
         WHERE paths IS NOT NULL AND paths != '' \
         ORDER BY timestamp DESC LIMIT 1",
    )?;

    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        Ok(Some(ActiveProject {
            paths: row.get(0)?,
            timestamp: row.get(1)?,
        }))
    } else {
        Ok(None)
    }
}

/// Recent Zed workspace roots, most-recently-used first, de-duplicated.
///
/// Reads up to 30 of the newest `workspaces` rows (by string timestamp) and
/// splits each multi-root `paths` column via [`parse_roots`], preserving order
/// while dropping repeats. Read-only connection. Errors yield an empty list so
/// the caller (the project-switch dropdown) degrades gracefully.
#[allow(dead_code)] // Used by the mzed bin, not the zed_watch bin.
pub fn recent_workspaces(db_path: &Path) -> Vec<PathBuf> {
    let Ok(conn) = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT paths FROM workspaces \
         WHERE paths IS NOT NULL AND paths != '' \
         ORDER BY timestamp DESC LIMIT 30",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for paths in rows.flatten() {
        for root in parse_roots(&paths) {
            if !out.contains(&root) {
                out.push(root);
            }
        }
    }
    out
}

/// Resolve the default Zed stable DB path on macOS.
pub fn default_zed_db_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let db = home.join("Library/Application Support/Zed/db/0-stable/db.sqlite");
    if db.exists() {
        Some(db)
    } else {
        None
    }
}

use notify_debouncer_full::new_debouncer;
use notify_debouncer_full::notify::RecursiveMode;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// Watch the Zed DB for changes and invoke `on_change` with the active project
/// whenever it changes. Blocks the calling thread.
#[allow(dead_code)] // Used by src/bin/zed_watch.rs.
pub fn watch<F>(db_path: &Path, mut on_change: F) -> Result<()>
where
    F: FnMut(Option<ActiveProject>),
{
    let (_stop_tx, stop_rx) = mpsc::channel();
    watch_until(db_path, &stop_rx, &mut on_change)
}

pub fn watch_until<F>(db_path: &Path, stop: &Receiver<()>, mut on_change: F) -> Result<()>
where
    F: FnMut(Option<ActiveProject>),
{
    // Zed uses WAL mode; writes land in the -wal file. Watch the db directory
    // so both db.sqlite and db.sqlite-wal changes are observed.
    let watch_dir = db_path
        .parent()
        .context("db path has no parent dir")?
        .to_path_buf();

    let (tx, rx) = mpsc::channel();
    // notify-debouncer-full 0.4: `Debouncer` exposes `watch()` directly;
    // the old `.watcher()` accessor is deprecated and returns `()`.
    let mut debouncer = new_debouncer(Duration::from_millis(300), None, tx)?;
    debouncer.watch(&watch_dir, RecursiveMode::NonRecursive)?;

    // Emit the initial state once.
    let mut last = query_active_project(db_path).unwrap_or(None);
    on_change(last.clone());

    // Hybrid detection: react to notify events, but also re-query on a fixed
    // poll interval. SQLite WAL writes don't always trigger a filesystem event
    // that FSEvents/notify reliably reports, so polling guarantees we catch a
    // project switch within `POLL` even when the event is missed.
    const POLL: Duration = Duration::from_millis(1500);
    const STOP_POLL: Duration = Duration::from_millis(100);
    let mut last_poll = std::time::Instant::now();
    loop {
        if stop.try_recv().is_ok() {
            break;
        }
        // Wake frequently for explicit stop, while keeping DB polling at POLL.
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
        let current = query_active_project(db_path).unwrap_or(None);
        // Compare by project path only: Zed bumps the active workspace's
        // timestamp periodically even without a switch, and we don't want to
        // re-render the same project on those no-op updates.
        let changed = current.as_ref().map(|p| &p.paths) != last.as_ref().map(|p| &p.paths);
        if changed {
            last = current.clone();
            on_change(current);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use rusqlite::Connection;

    fn make_test_db(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(indoc! {r#"
            CREATE TABLE workspaces (
                workspace_id INTEGER PRIMARY KEY,
                paths TEXT,
                timestamp TEXT NOT NULL
            );
            INSERT INTO workspaces (workspace_id, paths, timestamp) VALUES
                (1, '/Users/me/projectA', '2026-06-20 10:00:00'),
                (2, '/Users/me/projectB', '2026-06-21 12:30:00'),
                (3, NULL,                 '2026-06-22 09:00:00');
        "#})
            .unwrap();
    }

    #[test]
    fn returns_latest_non_null_project() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db.sqlite");
        make_test_db(&db);

        let active = query_active_project(&db).unwrap();
        assert_eq!(
            active,
            Some(ActiveProject {
                paths: "/Users/me/projectB".to_string(),
                timestamp: "2026-06-21 12:30:00".to_string(),
            })
        );
    }

    #[test]
    fn focused_window_overrides_latest_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db.sqlite");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(indoc! {r#"
            CREATE TABLE workspaces (
                workspace_id INTEGER PRIMARY KEY,
                paths TEXT,
                timestamp TEXT NOT NULL
            );
            INSERT INTO workspaces (workspace_id, paths, timestamp) VALUES
                (7,  '/Users/me/focused', '2026-06-20 10:00:00'),
                (13, '/Users/me/newest',  '2026-06-24 12:00:00');

            CREATE TABLE kv_store (key TEXT PRIMARY KEY, value TEXT);
            INSERT INTO kv_store (key, value) VALUES
                ('session_window_stack', '[111,222]');

            CREATE TABLE scoped_kv_store (namespace TEXT, key TEXT, value TEXT);
            INSERT INTO scoped_kv_store (namespace, key, value) VALUES
                ('multi_workspace_state', '111', '{"active_workspace_id":7}'),
                ('multi_workspace_state', '222', '{"active_workspace_id":13}');
        "#})
            .unwrap();

        // Front-most window 111 -> workspace 7 -> /Users/me/focused, even though
        // workspace 13 has a newer timestamp.
        let active = query_active_project(&db).unwrap().unwrap();
        assert_eq!(active.paths, "/Users/me/focused");
    }

    #[test]
    fn watch_until_stops_without_waiting_for_db_event() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db.sqlite");
        make_test_db(&db);
        let (stop_tx, stop_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result = watch_until(&db, &stop_rx, |_| {});
            done_tx.send(result.is_ok()).unwrap();
        });
        stop_tx.send(()).unwrap();

        assert!(done_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("zed watcher did not stop"));
    }

    #[test]
    fn watch_until_stops_quickly_after_initial_poll() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db.sqlite");
        make_test_db(&db);
        let (stop_tx, stop_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();

        std::thread::spawn(move || {
            let mut first = true;
            let result = watch_until(&db, &stop_rx, |_| {
                if first {
                    first = false;
                    ready_tx.send(()).unwrap();
                }
            });
            done_tx.send(result.is_ok()).unwrap();
        });
        // Generous setup timeout: under full-suite parallel load the watcher
        // thread's first poll can take well over a second (flaked at 1s).
        ready_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("initial watcher callback did not run");
        std::thread::sleep(Duration::from_millis(50));
        stop_tx.send(()).unwrap();

        // "Quickly" = within STOP_POLL granularity, i.e. under a full POLL
        // (1500ms). 1400ms is the widest bound that still proves the watcher
        // didn't sleep through a whole poll; anything tighter flakes under
        // full-suite parallel load.
        assert!(done_rx
            .recv_timeout(Duration::from_millis(1400))
            .expect("zed watcher did not stop quickly"));
    }

    #[allow(non_snake_case)]
    mod recent {
        use super::super::*;
        use indoc::indoc;
        use rusqlite::Connection;
        use std::path::Path;

        fn make_db(path: &Path) {
            let conn = Connection::open(path).unwrap();
            conn.execute_batch(indoc! {r#"
                CREATE TABLE workspaces (
                    workspace_id INTEGER PRIMARY KEY,
                    paths TEXT,
                    timestamp TEXT NOT NULL
                );
                INSERT INTO workspaces (workspace_id, paths, timestamp) VALUES
                    (1, '/Users/me/old',   '2026-06-20 10:00:00'),
                    (2, '/Users/me/new',   '2026-06-23 10:00:00'),
                    (3, '/Users/me/mid',   '2026-06-22 10:00:00'),
                    (4, NULL,              '2026-06-24 10:00:00'),
                    (5, '',                '2026-06-25 10:00:00');
            "#})
                .unwrap();
        }

        #[test]
        fn timestamp降順で返す() {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("db.sqlite");
            make_db(&db);
            assert_eq!(
                recent_workspaces(&db),
                vec![
                    PathBuf::from("/Users/me/new"),
                    PathBuf::from("/Users/me/mid"),
                    PathBuf::from("/Users/me/old"),
                ]
            );
        }

        #[test]
        fn 複数ルートを分解し重複は除く() {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("db.sqlite");
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(indoc! {r#"
                CREATE TABLE workspaces (
                    workspace_id INTEGER PRIMARY KEY,
                    paths TEXT,
                    timestamp TEXT NOT NULL
                );
                INSERT INTO workspaces (workspace_id, paths, timestamp) VALUES
                    (1, '/a' || char(10) || '/b', '2026-06-23 10:00:00'),
                    (2, '/b' || char(10) || '/c', '2026-06-22 10:00:00');
            "#})
                .unwrap();
            // /b は最初の行で既出なので 2 行目では除外。
            assert_eq!(
                recent_workspaces(&db),
                vec![
                    PathBuf::from("/a"),
                    PathBuf::from("/b"),
                    PathBuf::from("/c"),
                ]
            );
        }

        #[test]
        fn 存在しないdbは空ベクタ() {
            assert!(recent_workspaces(Path::new("/no/such/db.sqlite")).is_empty());
        }
    }

    #[allow(non_snake_case)]
    mod roots {
        use super::super::*;
        use std::path::PathBuf;

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
    }
}
