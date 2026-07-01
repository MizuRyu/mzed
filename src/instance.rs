//! Single-instance coordination over a Unix local socket.
//!
//! The first `mzed` to start becomes the *primary*: it binds a local socket and
//! listens for newline-delimited JSON messages. A later `mzed` connects to that
//! socket, requests a new window or sends an [`Msg`] describing what to open,
//! and exits. If no primary is reachable, the new process binds the socket
//! itself and becomes primary.
//!
//! The wire protocol (one JSON object per line) is pure logic and unit-tested
//! via [`encode`] / [`parse`]; socket I/O lives in [`try_send`] / [`serve`].

use crate::cli::Target;
use interprocess::local_socket::{
    prelude::*, GenericFilePath, Listener, ListenerOptions, Stream, ToFsName,
};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const MAX_MESSAGE_LEN: usize = 64 * 1024;
const MAX_MESSAGES_PER_CONNECTION: usize = 128;
const MAX_PATHS_PER_OPEN_MANY: usize = 128;
const READ_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_ACTIVE_CONNECTIONS: usize = 32;

/// A message sent from a secondary instance to the primary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Msg {
    /// Open another application window in the primary process.
    NewWindow,
    /// Open a markdown file in a new tab.
    Open { path: PathBuf },
    /// Open multiple markdown files in new tabs.
    OpenMany { paths: Vec<PathBuf> },
    /// Switch the project root to a directory.
    OpenDir { path: PathBuf },
}

impl Msg {
    /// Build the message(s) implied by a startup [`Target`].
    pub fn from_target(target: &Target) -> Vec<Msg> {
        match target {
            Target::Zed => Vec::new(),
            Target::Files(fs) => fs.iter().cloned().map(|path| Msg::Open { path }).collect(),
            Target::Dir(d) => vec![Msg::OpenDir { path: d.clone() }],
        }
    }

    /// Build the request a secondary process sends to the primary.
    pub fn for_secondary(target: &Target) -> Vec<Msg> {
        match target {
            Target::Zed => vec![Msg::NewWindow],
            Target::Files(files) => Self::file_messages_for_secondary(files),
            Target::Dir(_) => Self::from_target(target),
        }
    }

    fn file_messages_for_secondary(files: &[PathBuf]) -> Vec<Msg> {
        match files {
            [] => Vec::new(),
            [path] => vec![Msg::Open { path: path.clone() }],
            paths => {
                let mut messages = Vec::new();
                let mut chunk = Vec::new();
                let mut chunk_len = empty_open_many_payload_len();
                for path in paths {
                    let path_len = encoded_path_payload_len(path);
                    let separator_len = usize::from(!chunk.is_empty());
                    if !chunk.is_empty()
                        && (chunk.len() >= MAX_PATHS_PER_OPEN_MANY
                            || chunk_len + separator_len + path_len > MAX_MESSAGE_LEN)
                    {
                        if !chunk.is_empty() {
                            messages.push(Msg::OpenMany { paths: chunk });
                        }
                        chunk = Vec::new();
                        chunk_len = empty_open_many_payload_len();
                    }
                    if !chunk.is_empty() {
                        chunk_len += 1;
                    }
                    chunk_len += path_len;
                    chunk.push(path.clone());
                }
                if !chunk.is_empty() {
                    messages.push(Msg::OpenMany { paths: chunk });
                }
                messages
            }
        }
    }
}

/// Serialise a message to a single wire line (JSON + trailing newline).
pub fn encode(msg: &Msg) -> String {
    let mut s = serde_json::to_string(msg).expect("Msg serialises");
    s.push('\n');
    s
}

fn encoded_payload_len(msg: &Msg) -> usize {
    serde_json::to_string(msg).expect("Msg serialises").len()
}

fn encoded_path_payload_len(path: &Path) -> usize {
    serde_json::to_string(path).expect("Path serialises").len()
}

fn empty_open_many_payload_len() -> usize {
    r#"{"type":"open_many","paths":[]}"#.len()
}

#[cfg(test)]
fn encoded_open_many_payload_len(paths: &[PathBuf]) -> usize {
    empty_open_many_payload_len()
        + paths
            .iter()
            .map(|path| encoded_path_payload_len(path))
            .sum::<usize>()
        + paths.len().saturating_sub(1)
}

/// Parse one wire line into a [`Msg`]. Blank lines and malformed JSON yield
/// `None` so a noisy peer can't crash the primary's listen loop.
pub fn parse(line: &str) -> Option<Msg> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    serde_json::from_str(line).ok()
}

/// Filesystem path of this user's coordination socket. Lives under the runtime
/// dir (or `$TMPDIR`/`/tmp`) and is keyed by uid so different users don't
/// collide. macOS has no abstract sockets, so a real path is used on all
/// Unixes for predictable, debuggable behaviour.
pub fn socket_path() -> PathBuf {
    let uid = unsafe { libc_getuid() };
    let dir = dirs::runtime_dir()
        .or_else(|| std::env::var_os("TMPDIR").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    dir.join(format!("mzed-{uid}.sock"))
}

// uid lookup without pulling in the `libc` crate.
extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
    fn flock(fd: std::ffi::c_int, operation: std::ffi::c_int) -> std::ffi::c_int;
}

struct SocketInitLock {
    _file: File,
}

impl SocketInitLock {
    fn acquire(socket_path: &Path) -> io::Result<Self> {
        let mut lock_path = socket_path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        // LOCK_EX is released automatically when `file` is dropped, including
        // when a process exits during socket recovery.
        if unsafe { flock(file.as_raw_fd(), 2) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { _file: file })
    }
}

fn socket_name(path: &Path) -> io::Result<interprocess::local_socket::Name<'_>> {
    path.as_os_str().to_fs_name::<GenericFilePath>()
}

fn create_listener(path: &Path) -> io::Result<Listener> {
    ListenerOptions::new()
        .name(socket_name(path)?)
        .create_sync()
}

/// Bind a listener without replacing a reachable primary. An unreachable Unix
/// socket is treated as stale and reclaimed while holding the init lock.
pub fn bind_at(path: &Path) -> io::Result<Listener> {
    let _init_lock = SocketInitLock::acquire(path)?;
    let listener = match create_listener(path) {
        Ok(listener) => listener,
        Err(bind_err) if bind_err.kind() == io::ErrorKind::AddrInUse => {
            match Stream::connect(socket_name(path)?) {
                Ok(_) => return Err(bind_err),
                Err(connect_err)
                    if should_reclaim_stale_socket_after_connect_error(connect_err.kind()) => {}
                Err(_) => return Err(bind_err),
            }
            let metadata = std::fs::symlink_metadata(path)?;
            if !metadata.file_type().is_socket() {
                return Err(bind_err);
            }
            std::fs::remove_file(path)?;
            create_listener(path)?
        }
        Err(err) => return Err(err),
    };
    // Restrict the socket to the owning user (rwx for owner only).
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

fn should_reclaim_stale_socket_after_connect_error(kind: io::ErrorKind) -> bool {
    matches!(
        kind,
        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
    )
}

pub(crate) fn bind() -> io::Result<Listener> {
    bind_at(&socket_path())
}

/// Try to hand `msgs` to an already-running primary. Returns `Ok(true)` if a
/// primary accepted the connection (this process should exit), `Ok(false)` if
/// no primary is listening (this process should become primary).
pub fn try_send(msgs: &[Msg]) -> std::io::Result<bool> {
    try_send_to(&socket_path(), msgs)
}

pub fn try_send_to(path: &Path, msgs: &[Msg]) -> io::Result<bool> {
    match Stream::connect(socket_name(path)?) {
        Ok(mut conn) => {
            validate_outgoing_messages(msgs)?;
            for m in msgs {
                conn.write_all(encode(m).as_bytes())?;
            }
            conn.flush()?;
            Ok(true)
        }
        // No listener (or a stale socket file): we become primary.
        Err(_) => Ok(false),
    }
}

fn validate_outgoing_messages(msgs: &[Msg]) -> io::Result<()> {
    if msgs.len() > MAX_MESSAGES_PER_CONNECTION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "IPC request contains {} messages, max is {MAX_MESSAGES_PER_CONNECTION}",
                msgs.len()
            ),
        ));
    }
    for msg in msgs {
        validate_message_shape(msg)?;
        let len = encoded_payload_len(msg);
        if len > MAX_MESSAGE_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("IPC message is {len} bytes, max is {MAX_MESSAGE_LEN}"),
            ));
        }
    }
    Ok(())
}

fn validate_message_shape(msg: &Msg) -> io::Result<()> {
    if let Msg::OpenMany { paths } = msg {
        if paths.len() > MAX_PATHS_PER_OPEN_MANY {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "OpenMany contains {} paths, max is {MAX_PATHS_PER_OPEN_MANY}",
                    paths.len()
                ),
            ));
        }
    }
    Ok(())
}

fn handle_connection<F: Fn(Msg)>(conn: Stream, on_msg: &F) {
    let mut reader = BufReader::new(conn);
    let mut messages = 0;
    loop {
        if messages >= MAX_MESSAGES_PER_CONNECTION {
            break;
        }
        let mut bytes = Vec::new();
        let result = std::io::Read::by_ref(&mut reader)
            .take((MAX_MESSAGE_LEN + 2) as u64)
            .read_until(b'\n', &mut bytes);
        let Ok(read) = result else { break };
        if read == 0 {
            break;
        }
        if bytes.last() != Some(&b'\n') || bytes.len() - 1 > MAX_MESSAGE_LEN {
            break;
        }
        bytes.pop();
        messages += 1;
        if let Ok(line) = std::str::from_utf8(&bytes) {
            if let Some(msg) = parse(line) {
                if validate_message_shape(&msg).is_ok() {
                    on_msg(msg);
                }
            }
        }
    }
}

fn connection_slots_available(active: usize, max_active: usize) -> bool {
    active < max_active
}

fn serve_inner<F>(listener: Listener, max_connections: Option<usize>, on_msg: F) -> io::Result<()>
where
    F: Fn(Msg) + Send + Sync + 'static,
{
    let on_msg = Arc::new(on_msg);
    let active_connections = Arc::new(AtomicUsize::new(0));
    let mut finite_workers = Vec::new();
    for (accepted, conn) in listener.incoming().enumerate() {
        let conn = match conn {
            Ok(conn) => conn,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        if !connection_slots_available(
            active_connections.load(Ordering::Acquire),
            MAX_ACTIVE_CONNECTIONS,
        ) {
            continue;
        }
        active_connections.fetch_add(1, Ordering::AcqRel);
        let _ = conn.set_recv_timeout(Some(READ_TIMEOUT));
        let on_msg = Arc::clone(&on_msg);
        let active_connections_for_worker = Arc::clone(&active_connections);
        let active_connections_for_error = Arc::clone(&active_connections);
        let worker = match std::thread::Builder::new()
            .name("mzed-ipc-connection".into())
            .spawn(move || {
                handle_connection(conn, &*on_msg);
                active_connections_for_worker.fetch_sub(1, Ordering::AcqRel);
            }) {
            Ok(worker) => worker,
            Err(err) => {
                active_connections_for_error.fetch_sub(1, Ordering::AcqRel);
                eprintln!("mzed IPC connection worker failed to start: {err}");
                continue;
            }
        };
        if max_connections.is_some() {
            finite_workers.push(worker);
        }
        if max_connections.is_some_and(|max| accepted + 1 >= max) {
            for worker in finite_workers {
                let _ = worker.join();
            }
            return Ok(());
        }
    }
    Ok(())
}

/// Serve accepted connections on independent workers so one slow peer cannot
/// block the listener.
pub(crate) fn serve<F>(listener: Listener, on_msg: F) -> io::Result<()>
where
    F: Fn(Msg) + Send + Sync + 'static,
{
    serve_inner(listener, None, on_msg)
}

/// Serve at most `max_connections` connections, then return.
/// Exposed for integration tests and controlled benchmarks; not part of the
/// stable public API.
#[doc(hidden)]
pub fn serve_n<F>(listener: Listener, max_connections: usize, on_msg: F) -> io::Result<()>
where
    F: Fn(Msg) + Send + Sync + 'static,
{
    serve_inner(listener, Some(max_connections), on_msg)
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Duration;
    use tempfile::tempdir;

    fn raw_listener(path: &Path) -> interprocess::local_socket::Listener {
        let name = path
            .as_os_str()
            .to_fs_name::<GenericFilePath>()
            .expect("socket name");
        ListenerOptions::new()
            .name(name)
            .create_sync()
            .expect("bind test socket")
    }

    fn raw_connect(path: &Path) -> Stream {
        let name = path
            .as_os_str()
            .to_fs_name::<GenericFilePath>()
            .expect("socket name");
        Stream::connect(name).expect("connect test socket")
    }

    #[test]
    fn encodeは1行のJSONに改行を付ける() {
        let line = encode(&Msg::Open {
            path: PathBuf::from("/abs/x.md"),
        });
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
        assert!(line.contains("\"type\":\"open\""));
        assert!(line.contains("/abs/x.md"));
    }

    #[test]
    fn open_dirがsnake_caseでタグ付けされる() {
        let line = encode(&Msg::OpenDir {
            path: PathBuf::from("/abs/dir"),
        });
        assert!(line.contains("\"type\":\"open_dir\""));
    }

    #[test]
    fn new_windowがsnake_caseでタグ付けされる() {
        let line = encode(&Msg::NewWindow);
        assert_eq!(line, "{\"type\":\"new_window\"}\n");
        assert_eq!(parse(&line), Some(Msg::NewWindow));
    }

    #[test]
    fn open_manyがsnake_caseでタグ付けされる() {
        let msg = Msg::OpenMany {
            paths: vec![PathBuf::from("/a.md"), PathBuf::from("/b.md")],
        };
        let line = encode(&msg);

        assert!(line.contains("\"type\":\"open_many\""));
        assert_eq!(parse(&line), Some(msg));
    }

    #[test]
    fn open_manyの増分size計算はserde_jsonと一致する() {
        let paths = vec![
            PathBuf::from("/a space.md"),
            PathBuf::from("/quote\"name.md"),
            PathBuf::from("/日本語.md"),
        ];

        assert_eq!(
            encoded_open_many_payload_len(&paths),
            encoded_payload_len(&Msg::OpenMany { paths })
        );
    }

    #[test]
    fn parseはencodeの逆変換になる() {
        let m = Msg::Open {
            path: PathBuf::from("/abs/x.md"),
        };
        assert_eq!(parse(&encode(&m)), Some(m));
        let d = Msg::OpenDir {
            path: PathBuf::from("/abs/dir"),
        };
        assert_eq!(parse(&encode(&d)), Some(d));
    }

    #[test]
    fn parseは空行と不正JSONをNoneにする() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("   "), None);
        assert_eq!(parse("not json"), None);
        assert_eq!(parse("{\"type\":\"bogus\"}"), None);
    }

    #[test]
    fn from_targetはFilesを各Openへ展開する() {
        let t = Target::Files(vec![PathBuf::from("/a.md"), PathBuf::from("/b.md")]);
        assert_eq!(
            Msg::from_target(&t),
            vec![
                Msg::Open {
                    path: PathBuf::from("/a.md")
                },
                Msg::Open {
                    path: PathBuf::from("/b.md")
                },
            ]
        );
    }

    #[test]
    fn from_targetはDirをOpenDirにしZedは空にする() {
        assert_eq!(
            Msg::from_target(&Target::Dir(PathBuf::from("/d"))),
            vec![Msg::OpenDir {
                path: PathBuf::from("/d")
            }]
        );
        assert!(Msg::from_target(&Target::Zed).is_empty());
    }

    #[test]
    fn for_secondaryはZedをNewWindowにする() {
        assert_eq!(Msg::for_secondary(&Target::Zed), vec![Msg::NewWindow]);
        assert_eq!(
            Msg::for_secondary(&Target::Files(vec![PathBuf::from("/a.md")])),
            vec![Msg::Open {
                path: PathBuf::from("/a.md")
            }]
        );
    }

    #[test]
    fn for_secondaryは複数fileをopen_manyへまとめる() {
        let files: Vec<PathBuf> = (0..150)
            .map(|index| PathBuf::from(format!("/{index}.md")))
            .collect();
        let msgs = Msg::for_secondary(&Target::Files(files.clone()));
        let round_tripped: Vec<PathBuf> = msgs
            .iter()
            .flat_map(|msg| match msg {
                Msg::OpenMany { paths } => paths.clone(),
                other => panic!("unexpected message: {other:?}"),
            })
            .collect();

        assert_eq!(msgs.len(), 2);
        assert!(msgs.iter().all(|msg| match msg {
            Msg::OpenMany { paths } => paths.len() <= MAX_PATHS_PER_OPEN_MANY,
            _ => false,
        }));
        assert_eq!(round_tripped, files);
    }

    #[test]
    fn for_secondaryは長いfile一覧をmessage上限内へ分割する() {
        let files: Vec<PathBuf> = (0..150)
            .map(|index| PathBuf::from(format!("/{}-{index}.md", "x".repeat(1024))))
            .collect();

        let msgs = Msg::for_secondary(&Target::Files(files.clone()));
        let round_tripped: Vec<PathBuf> = msgs
            .iter()
            .flat_map(|msg| match msg {
                Msg::OpenMany { paths } => paths.clone(),
                other => panic!("unexpected message: {other:?}"),
            })
            .collect();

        assert!(msgs.len() > 1);
        assert!(msgs
            .iter()
            .all(|msg| encoded_payload_len(msg) <= MAX_MESSAGE_LEN));
        assert_eq!(round_tripped, files);
    }

    #[test]
    fn outgoing_validationは接続あたりmessage上限超過を拒否する() {
        let msgs = vec![Msg::NewWindow; MAX_MESSAGES_PER_CONNECTION + 1];

        let err = validate_outgoing_messages(&msgs).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn outgoing_validationはmessage_size上限超過を拒否する() {
        let msg = Msg::Open {
            path: PathBuf::from(format!("/{}.md", "x".repeat(MAX_MESSAGE_LEN))),
        };

        let err = validate_outgoing_messages(&[msg]).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn open_manyはpath数上限超過を送受信前に拒否する() {
        let msg = Msg::OpenMany {
            paths: (0..=MAX_PATHS_PER_OPEN_MANY)
                .map(|index| PathBuf::from(format!("/{index}.md")))
                .collect(),
        };

        let err = validate_message_shape(&msg).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn bind_atは稼働中primaryのsocketを奪取しない() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("primary.sock");
        let primary = raw_listener(&path);

        let err = bind_at(&path).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
        let _connection = raw_connect(&path);
        drop(primary);
    }

    #[test]
    fn bind_atはstale_socketだけ回収する() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("stale.sock");
        let mut stale = raw_listener(&path);
        stale.do_not_reclaim_name_on_drop();
        drop(stale);
        assert!(path.exists());

        let recovered = bind_at(&path).expect("recover stale socket");

        let _connection = raw_connect(&path);
        drop(recovered);
    }

    #[test]
    fn bind_atはsocket以外の既存pathを削除しない() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("keep.txt");
        std::fs::write(&path, "keep").unwrap();

        assert!(bind_at(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep");
    }

    #[test]
    fn bind_atは一時的なconnect失敗をstale扱いしない() {
        assert!(!should_reclaim_stale_socket_after_connect_error(
            std::io::ErrorKind::PermissionDenied
        ));
        assert!(should_reclaim_stale_socket_after_connect_error(
            std::io::ErrorKind::ConnectionRefused
        ));
        assert!(should_reclaim_stale_socket_after_connect_error(
            std::io::ErrorKind::NotFound
        ));
    }

    #[test]
    fn ipc_worker上限を超えた接続はdropされる() {
        assert!(!connection_slots_available(32, 32));
        assert!(connection_slots_available(31, 32));
    }

    #[test]
    fn 無改行peerが後続messageを停止させない() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonblocking.sock");
        let listener = bind_at(&path).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || serve_n(listener, 2, move |msg| tx.send(msg).unwrap()));

        let mut blocking_peer = raw_connect(&path);
        blocking_peer.write_all(b"{").unwrap();
        let expected = Msg::Open {
            path: PathBuf::from("/later.md"),
        };
        assert!(try_send_to(&path, std::slice::from_ref(&expected)).unwrap());

        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), expected);
    }

    #[test]
    fn 上限超過messageを破棄して後続messageを受信する() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bounded.sock");
        let listener = bind_at(&path).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || serve_n(listener, 2, move |msg| tx.send(msg).unwrap()));

        let mut oversized_peer = raw_connect(&path);
        oversized_peer
            .write_all(&vec![b'x'; MAX_MESSAGE_LEN + 1])
            .unwrap();
        oversized_peer.write_all(b"\n").unwrap();
        drop(oversized_peer);

        let expected = Msg::OpenDir {
            path: PathBuf::from("/later"),
        };
        assert!(try_send_to(&path, std::slice::from_ref(&expected)).unwrap());

        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), expected);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn ひとつの接続から受け取るmessage数を制限する() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("message-count.sock");
        let listener = bind_at(&path).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || serve_n(listener, 1, move |msg| tx.send(msg).unwrap()));

        let mut peer = raw_connect(&path);
        for index in 0..150 {
            let msg = Msg::Open {
                path: PathBuf::from(format!("/{index}.md")),
            };
            // サーバは上限到達で接続を切るため、以降の write 失敗は想定内
            if peer.write_all(encode(&msg).as_bytes()).is_err() {
                break;
            }
        }
        drop(peer);

        let mut received = 0;
        while rx.recv_timeout(Duration::from_millis(100)).is_ok() {
            received += 1;
        }

        assert_eq!(received, MAX_MESSAGES_PER_CONNECTION);
    }

    #[test]
    fn open_manyのpath数上限超過messageは配送しない() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("open-many-path-count.sock");
        let listener = bind_at(&path).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || serve_n(listener, 1, move |msg| tx.send(msg).unwrap()));

        let invalid = Msg::OpenMany {
            paths: (0..=MAX_PATHS_PER_OPEN_MANY)
                .map(|index| PathBuf::from(format!("/{index}.md")))
                .collect(),
        };
        let expected = Msg::Open {
            path: PathBuf::from("/valid.md"),
        };
        let mut peer = raw_connect(&path);
        peer.write_all(encode(&invalid).as_bytes()).unwrap();
        peer.write_all(encode(&expected).as_bytes()).unwrap();
        drop(peer);

        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), expected);
        assert!(rx.try_recv().is_err());
    }
}
