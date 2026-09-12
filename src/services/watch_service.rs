use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::thread::JoinHandle;

use tokio::sync::mpsc;

use crate::sync::{SyncEvent, SyncOrigin};
use crate::{orca, watcher, zed};

pub(crate) struct WatchSubscription<T> {
    pub(crate) rx: mpsc::UnboundedReceiver<T>,
    stop_txs: Vec<std_mpsc::Sender<()>>,
    join_handles: Vec<JoinHandle<()>>,
}

impl<T> Drop for WatchSubscription<T> {
    fn drop(&mut self) {
        for stop_tx in self.stop_txs.drain(..) {
            let _ = stop_tx.send(());
        }
        let join_handles: Vec<_> = self.join_handles.drain(..).collect();
        if !join_handles.is_empty() {
            let _ = std::thread::Builder::new()
                .name("mzed-watch-reaper".into())
                .spawn(move || {
                    for join_handle in join_handles {
                        let _ = join_handle.join();
                    }
                });
        }
    }
}

/// Both project sources on one channel.
///
/// The watchers always report; which reports actually drive a switch is decided
/// on the UI side (`sync::accepts`, `SyncMode::decide`), the same way the Zed
/// watcher has always worked — policy can then change without restarting a
/// thread. A missing Zed DB ends that thread; the Orca watcher instead waits
/// for its state file to appear, since Orca may be installed or first launched
/// after mzed started.
pub(crate) fn project_sources() -> WatchSubscription<SyncEvent> {
    let (tx, rx) = mpsc::unbounded_channel::<SyncEvent>();
    let (zed_stop_tx, zed_stop_rx) = std_mpsc::channel::<()>();
    let (orca_stop_tx, orca_stop_rx) = std_mpsc::channel::<()>();

    let zed_tx = tx.clone();
    let zed_handle = std::thread::spawn(move || {
        // A watcher's first callback is the state it found, not a switch the
        // user made; the UI side lands on those by a different rule.
        let mut first = true;
        if let Some(db) = zed::default_zed_db_path() {
            let _ = zed::watch_until(&db, &zed_stop_rx, move |project| {
                let _ = zed_tx.send(SyncEvent {
                    origin: SyncOrigin::Zed,
                    project,
                    initial: std::mem::take(&mut first),
                });
            });
        }
    });
    let orca_handle = std::thread::spawn(move || {
        let mut first = true;
        let _ = orca::watch_active_project(&orca_stop_rx, move |project| {
            let _ = tx.send(SyncEvent {
                origin: SyncOrigin::Orca,
                project,
                initial: std::mem::take(&mut first),
            });
        });
    });

    WatchSubscription {
        rx,
        stop_txs: vec![zed_stop_tx, orca_stop_tx],
        join_handles: vec![zed_handle, orca_handle],
    }
}

/// Watch several files (the worktree-overlay candidates of one logical path)
/// through a single receiver. A change to any copy triggers a reload, which
/// re-resolves the freshest checkout.
pub(crate) fn files_changes(files: Vec<PathBuf>) -> WatchSubscription<()> {
    let (tx, rx) = mpsc::unbounded_channel::<()>();
    let mut stop_txs = Vec::new();
    let mut join_handles = Vec::new();
    for file in files {
        let tx = tx.clone();
        let (stop_tx, stop_rx) = std_mpsc::channel::<()>();
        stop_txs.push(stop_tx);
        let join_handle = std::thread::spawn(move || {
            let _ = watcher::watch_file_until(&file, &stop_rx, move || tx.send(()).is_ok());
        });
        join_handles.push(join_handle);
    }
    WatchSubscription {
        rx,
        stop_txs,
        join_handles,
    }
}

pub(crate) fn tree_changes(roots: Vec<PathBuf>) -> WatchSubscription<()> {
    let (tx, rx) = mpsc::unbounded_channel::<()>();
    let mut stop_txs = Vec::new();
    let mut join_handles = Vec::new();
    for root in roots {
        let tx = tx.clone();
        let (stop_tx, stop_rx) = std_mpsc::channel::<()>();
        stop_txs.push(stop_tx);
        let join_handle = std::thread::spawn(move || {
            let _ = watcher::watch_tree_until(&root, &stop_rx, move || tx.send(()).is_ok());
        });
        join_handles.push(join_handle);
    }
    WatchSubscription {
        rx,
        stop_txs,
        join_handles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn dropping_subscription_sends_stop_without_waiting_for_worker_join() {
        let (_tx, rx) = mpsc::unbounded_channel::<()>();
        let (stop_tx, stop_rx) = std_mpsc::channel::<()>();
        let (joined_tx, joined_rx) = std_mpsc::channel::<()>();
        let join_handle = std::thread::spawn(move || {
            stop_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("stop signal was not sent");
            std::thread::sleep(Duration::from_millis(500));
            joined_tx.send(()).expect("joined signal failed");
        });

        let started = std::time::Instant::now();
        {
            let _subscription = WatchSubscription {
                rx,
                stop_txs: vec![stop_tx],
                join_handles: vec![join_handle],
            };
        }

        assert!(
            started.elapsed() < Duration::from_millis(100),
            "drop blocked on worker join"
        );
        joined_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("worker was not joined asynchronously");
    }
}
