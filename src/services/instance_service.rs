use crate::instance::{self, Msg};

pub(crate) fn start_server(
    tx: std::sync::mpsc::SyncSender<Msg>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    start_server_with_listener(instance::bind()?, tx)
}

fn start_server_with_listener(
    listener: interprocess::local_socket::Listener,
    tx: std::sync::mpsc::SyncSender<Msg>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("mzed-ipc-listener".into())
        .spawn(move || {
            if let Err(err) = instance::serve(listener, move |msg| {
                let _ = tx.try_send(msg);
            }) {
                eprintln!("mzed IPC server stopped: {err}");
            }
        })
}

#[cfg(test)]
fn start_server_at(
    tx: std::sync::mpsc::SyncSender<Msg>,
    path: &std::path::Path,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    start_server_with_listener(instance::bind_at(path)?, tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn start_server_atはbind失敗を同期的に返す() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("occupied");
        std::fs::write(&path, "keep").unwrap();
        let (tx, _rx) = std::sync::mpsc::sync_channel(1);

        assert!(start_server_at(tx, &path).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "keep");
    }
}
