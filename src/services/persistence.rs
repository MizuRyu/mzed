use anyhow::{Context, Result};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
static WRITE_QUEUE: OnceLock<std::sync::mpsc::Sender<WriteRequest>> = OnceLock::new();

struct WriteRequest {
    path: PathBuf,
    contents: Vec<u8>,
    done: tokio::sync::oneshot::Sender<Result<()>>,
}

struct TemporaryFile {
    path: Option<PathBuf>,
}

impl TemporaryFile {
    fn path(&self) -> &Path {
        self.path.as_deref().expect("temporary path is present")
    }

    fn disarm(mut self) {
        self.path = None;
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = fs::remove_file(path);
        }
    }
}

pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory for {}", path.display()))?;

    let (mut file, temporary) = create_temporary_file(path, parent)?;
    file.write_all(contents)
        .with_context(|| format!("failed to write temporary file for {}", path.display()))?;
    file.flush()
        .with_context(|| format!("failed to flush temporary file for {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync temporary file for {}", path.display()))?;
    drop(file);

    fs::rename(temporary.path(), path)
        .with_context(|| format!("failed to atomically replace {}", path.display()))?;
    sync_parent_directory(parent, path)?;
    temporary.disarm();
    Ok(())
}

fn sync_parent_directory(parent: &Path, target: &Path) -> Result<()> {
    File::open(parent)
        .with_context(|| format!("failed to open parent directory for {}", target.display()))?
        .sync_all()
        .with_context(|| {
            format!(
                "failed to sync parent directory after replacing {}",
                target.display()
            )
        })
}

pub(crate) async fn atomic_write_queued(path: PathBuf, contents: Vec<u8>) -> Result<()> {
    let (done, result) = tokio::sync::oneshot::channel();
    let request = WriteRequest {
        path,
        contents,
        done,
    };
    write_queue()
        .send(request)
        .context("failed to enqueue persistence write")?;
    result
        .await
        .context("persistence write worker stopped before completing request")?
}

fn write_queue() -> &'static std::sync::mpsc::Sender<WriteRequest> {
    WRITE_QUEUE.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<WriteRequest>();
        std::thread::Builder::new()
            .name("mzed-persistence-writer".into())
            .spawn(move || {
                for request in rx {
                    let result = atomic_write(&request.path, &request.contents);
                    let _ = request.done.send(result);
                }
            })
            .expect("spawn persistence writer");
        tx
    })
}

fn create_temporary_file(path: &Path, parent: &Path) -> Result<(File, TemporaryFile)> {
    let file_name = path
        .file_name()
        .with_context(|| format!("target path has no file name: {}", path.display()))?;

    loop {
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut temporary_name = OsString::from(".");
        temporary_name.push(file_name);
        temporary_name.push(format!(".{}.{}.tmp", std::process::id(), counter));
        let temporary_path = parent.join(temporary_name);

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => {
                return Ok((
                    file,
                    TemporaryFile {
                        path: Some(temporary_path),
                    },
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to create temporary file for {}", path.display())
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::tempdir;

    #[test]
    fn saves_contents_to_requested_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("data.json");

        atomic_write(&path, br#"{"saved":true}"#).unwrap();

        assert_eq!(fs::read(&path).unwrap(), br#"{"saved":true}"#);
    }

    #[test]
    fn replaces_existing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("data.json");
        fs::write(&path, b"old contents").unwrap();

        atomic_write(&path, br#"{"new":true}"#).unwrap();

        assert_eq!(fs::read(&path).unwrap(), br#"{"new":true}"#);
    }

    #[test]
    fn failure_includes_target_path() {
        let dir = tempdir().unwrap();
        let parent_file = dir.path().join("not-a-directory");
        fs::write(&parent_file, b"blocker").unwrap();
        let path = parent_file.join("data.json");

        let error = atomic_write(&path, b"contents").unwrap_err();

        assert!(error.to_string().contains(&path.display().to_string()));
    }

    #[test]
    fn failed_rename_leaves_no_temporary_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("data.json");
        fs::create_dir(&path).unwrap();

        atomic_write(&path, b"contents").unwrap_err();

        let entries = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec![path.file_name().unwrap()]);
    }

    #[test]
    fn concurrent_saves_leave_complete_json() {
        let dir = tempdir().unwrap();
        let path = Arc::new(dir.path().join("data.json"));
        let barrier = Arc::new(Barrier::new(8));
        let mut workers = Vec::new();

        for worker in 0..8 {
            let path = Arc::clone(&path);
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                barrier.wait();
                for iteration in 0..25 {
                    let value = json!({
                        "worker": worker,
                        "iteration": iteration,
                        "payload": "x".repeat((worker + 1) * 257),
                    });
                    atomic_write(&path, value.to_string().as_bytes()).unwrap();
                }
            }));
        }

        for worker in workers {
            worker.join().unwrap();
        }

        let contents = fs::read_to_string(path.as_ref()).unwrap();
        let value: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert!(value["worker"].as_u64().unwrap() < 8);
        assert_eq!(value["iteration"], 24);
    }

    #[test]
    fn parent_directory_can_be_synced_after_replace() {
        let dir = tempdir().unwrap();

        sync_parent_directory(dir.path(), dir.path().join("data.json").as_path()).unwrap();
    }

    #[test]
    fn queued_writes_are_persisted_in_enqueue_order() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let dir = tempdir().unwrap();
            let path = dir.path().join("data.json");

            for value in 0..20u32 {
                atomic_write_queued(path.clone(), format!(r#"{{"value":{value}}}"#).into_bytes())
                    .await
                    .unwrap();
            }

            let contents = fs::read_to_string(path).unwrap();
            assert_eq!(contents, r#"{"value":19}"#);
        });
    }
}
