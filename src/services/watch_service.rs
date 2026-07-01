use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::thread::JoinHandle;

use tokio::sync::mpsc;

use crate::{watcher, zed};

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

pub(crate) fn zed_projects() -> WatchSubscription<Option<zed::ActiveProject>> {
    let (tx, rx) = mpsc::unbounded_channel::<Option<zed::ActiveProject>>();
    let (stop_tx, stop_rx) = std_mpsc::channel::<()>();
    let join_handle = std::thread::spawn(move || {
        if let Some(db) = zed::default_zed_db_path() {
            let _ = zed::watch_until(&db, &stop_rx, move |active| {
                let _ = tx.send(active);
            });
        }
    });
    WatchSubscription {
        rx,
        stop_txs: vec![stop_tx],
        join_handles: vec![join_handle],
    }
}

pub(crate) fn file_changes(file: PathBuf) -> WatchSubscription<()> {
    let (tx, rx) = mpsc::unbounded_channel::<()>();
    let (stop_tx, stop_rx) = std_mpsc::channel::<()>();
    let join_handle = std::thread::spawn(move || {
        let _ = watcher::watch_file_until(&file, &stop_rx, move || tx.send(()).is_ok());
    });
    WatchSubscription {
        rx,
        stop_txs: vec![stop_tx],
        join_handles: vec![join_handle],
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
