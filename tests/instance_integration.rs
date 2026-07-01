//! Integration tests for single-instance coordination (`mzed::instance`).
//!
//! Scenarios covered:
//!   1. primary receives an `Open` message forwarded by a secondary
//!   2. `try_send_to` returns `Ok(true)` when primary is alive (secondary exit path)
//!      and `Ok(false)` when no primary exists (caller becomes primary)
//!   3. stale socket (file present, no listener) is reclaimed → caller becomes primary
//!   4. socket file mode is 0600
//!   5. invalid JSON / oversized messages do not crash the primary

use interprocess::local_socket::{prelude::*, GenericFilePath, ListenerOptions, Stream, ToFsName};
use mzed::instance::{bind_at, serve_n, try_send_to, Msg};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use tempfile::tempdir;

/// Convert a filesystem path to an interprocess socket name.
fn sock_name(path: &Path) -> interprocess::local_socket::Name<'_> {
    path.as_os_str()
        .to_fs_name::<GenericFilePath>()
        .expect("valid socket path")
}

// ── Scenario 1 ────────────────────────────────────────────────────────────────

#[test]
fn primary_receives_open_message_forwarded_by_secondary() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("fwd.sock");

    let listener = bind_at(&path).unwrap();
    let (tx, rx) = mpsc::channel::<Msg>();
    std::thread::spawn(move || serve_n(listener, 1, move |msg| tx.send(msg).unwrap()));

    let expected = Msg::Open {
        path: PathBuf::from("/integration/note.md"),
    };
    let reached_primary = try_send_to(&path, std::slice::from_ref(&expected)).unwrap();

    assert!(
        reached_primary,
        "try_send_to must return true when primary is reachable"
    );
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        expected,
        "primary must receive the forwarded Open message"
    );
}

// ── Scenario 2 ────────────────────────────────────────────────────────────────

#[test]
fn try_send_returns_true_so_secondary_knows_to_exit() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("exit.sock");

    let listener = bind_at(&path).unwrap();
    // Serve one connection then finish; secondary sends NewWindow.
    std::thread::spawn(move || serve_n(listener, 1, |_| {}));

    let should_exit = try_send_to(&path, &[Msg::NewWindow]).unwrap();
    assert!(
        should_exit,
        "secondary must receive Ok(true) → should exit, not become primary"
    );
}

#[test]
fn try_send_returns_false_when_no_primary_exists() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("no-primary.sock");

    // Nothing listening on this path.
    let should_become_primary = try_send_to(&path, &[Msg::NewWindow]).unwrap();
    assert!(
        !should_become_primary,
        "try_send_to must return false → caller should become primary"
    );
}

// ── Scenario 3 ────────────────────────────────────────────────────────────────

#[test]
fn stale_socket_is_reclaimed_and_caller_becomes_primary() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("stale.sock");

    // Create a listener directly (bypassing bind_at) then abandon the file.
    {
        let mut stale = ListenerOptions::new()
            .name(sock_name(&path))
            .create_sync()
            .expect("bind stale socket");
        stale.do_not_reclaim_name_on_drop();
        // Drop: process gone, socket file remains.
    }
    assert!(path.exists(), "stale socket file must survive the drop");

    // bind_at should detect the unreachable socket and reclaim it.
    let recovered = bind_at(&path).expect("must reclaim stale socket and bind");

    // Recovered listener must be functional.
    let (tx, rx) = mpsc::channel::<Msg>();
    std::thread::spawn(move || serve_n(recovered, 1, move |msg| tx.send(msg).unwrap()));

    let expected = Msg::OpenDir {
        path: PathBuf::from("/reclaimed"),
    };
    assert!(try_send_to(&path, std::slice::from_ref(&expected)).unwrap());
    assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), expected);
}

// ── Scenario 4 ────────────────────────────────────────────────────────────────

#[test]
fn socket_file_permissions_are_0600() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let path = dir.path().join("perms.sock");

    let _listener = bind_at(&path).unwrap();

    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(
        mode & 0o777,
        0o600,
        "socket must be owner-only (0600), got {mode:#o}"
    );
}

// ── Scenario 5 ────────────────────────────────────────────────────────────────

#[test]
fn invalid_json_does_not_crash_primary() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("garbled.sock");

    let listener = bind_at(&path).unwrap();
    let (tx, rx) = mpsc::channel::<Msg>();
    std::thread::spawn(move || serve_n(listener, 2, move |msg| tx.send(msg).unwrap()));

    // Peer 1: sends invalid JSON terminated with a newline.
    {
        let mut peer = Stream::connect(sock_name(&path)).unwrap();
        peer.write_all(b"this is not json\n").unwrap();
    }

    // Primary must still accept a subsequent valid message from peer 2.
    let expected = Msg::Open {
        path: PathBuf::from("/after-garbage.md"),
    };
    assert!(try_send_to(&path, std::slice::from_ref(&expected)).unwrap());
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        expected,
        "primary must survive invalid JSON and deliver the next valid message"
    );
    assert!(rx.try_recv().is_err(), "no unexpected extra messages");
}

#[test]
fn oversized_message_does_not_crash_primary() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("huge.sock");

    let listener = bind_at(&path).unwrap();
    let (tx, rx) = mpsc::channel::<Msg>();
    std::thread::spawn(move || serve_n(listener, 2, move |msg| tx.send(msg).unwrap()));

    // Peer 1: sends a message body larger than MAX_MESSAGE_LEN (64 KiB).
    {
        let mut peer = Stream::connect(sock_name(&path)).unwrap();
        let body: Vec<u8> = vec![b'x'; 64 * 1024 + 1];
        peer.write_all(&body).unwrap();
        peer.write_all(b"\n").unwrap();
    }

    // Primary must still accept a valid message from peer 2.
    let expected = Msg::OpenDir {
        path: PathBuf::from("/after-oversized"),
    };
    assert!(try_send_to(&path, std::slice::from_ref(&expected)).unwrap());
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        expected,
        "primary must survive an oversized message and deliver the next valid one"
    );
    assert!(rx.try_recv().is_err(), "no unexpected extra messages");
}
