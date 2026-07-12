//! Filesystem watching utilities for live reload and sidebar auto-refresh.
//!
//! Two watchers are exposed, both built on `notify-debouncer-full` and both
//! running on a dedicated thread that forwards debounced notifications over a
//! callback. The pure relevance predicates (which path/event should trigger a
//! reload) are unit-tested; the FS event plumbing is not.

use anyhow::{Context, Result};
use notify_debouncer_full::notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebouncedEvent};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

/// Debounce window for both watchers. Bursts of save/rename events within this
/// window collapse into a single notification.
const DEBOUNCE: Duration = Duration::from_millis(250);
const STOP_POLL: Duration = Duration::from_millis(100);

fn is_markdown(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()),
        Some("md") | Some("markdown")
    )
}

/// Noise directories whose contents must never trigger a sidebar refresh.
fn is_ignored_component(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "node_modules" | "target" | "dist" | "build")
}

/// Does a batch of changed paths warrant re-reading the active file `target`?
///
/// True when any changed path equals `target` (its content changed, was
/// re-created, or renamed into place).
pub fn active_file_affected(target: &Path, changed: &[PathBuf]) -> bool {
    changed.iter().any(|p| p == target)
}

/// Does a batch of changed paths warrant rebuilding the sidebar tree under
/// `root`? True when any changed path is a markdown file inside `root` that is
/// not within an ignored directory. (Add/remove/rename of an `.md` all surface
/// here; edits of an existing md also match but only cost one tree rebuild.)
pub fn tree_affected(root: &Path, changed: &[PathBuf]) -> bool {
    changed.iter().any(|p| is_relevant_md_path(root, p))
}

fn is_relevant_md_path(root: &Path, p: &Path) -> bool {
    if !is_markdown(p) {
        return false;
    }
    let Ok(rel) = p.strip_prefix(root) else {
        return false;
    };
    // Reject when any intermediate directory component is ignored.
    let mut comps: Vec<&str> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    comps.pop(); // drop the file name itself
    !comps.iter().any(|c| is_ignored_component(c))
}

fn collect_watch_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_watch_dirs_inner(root, 0, &mut out);
    out
}

fn collect_watch_dirs_inner(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if depth > 0 && (name.is_empty() || is_ignored_component(name)) {
        return;
    }
    out.push(dir.to_path_buf());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            collect_watch_dirs_inner(&path, depth + 1, out);
        }
    }
}

/// Collect all paths touched by a batch of debounced events.
fn paths_of(events: &[DebouncedEvent]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for ev in events {
        out.extend(ev.event.paths.iter().cloned());
    }
    out
}

pub fn watch_file_until<F>(file: &Path, stop: &Receiver<()>, mut on_change: F) -> Result<()>
where
    F: FnMut() -> bool,
{
    let target = file.to_path_buf();
    let dir = file
        .parent()
        .context("file has no parent dir")?
        .to_path_buf();

    let (tx, rx) = std::sync::mpsc::channel();
    let mut debouncer = new_debouncer(DEBOUNCE, None, tx)?;
    debouncer.watch(&dir, RecursiveMode::NonRecursive)?;

    loop {
        if stop_requested(stop) {
            break;
        }
        match rx.recv_timeout(STOP_POLL) {
            Ok(Ok(events)) => {
                let paths = paths_of(&events);
                if active_file_affected(&target, &paths) && !on_change() {
                    break;
                }
            }
            Ok(Err(_)) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }
    Ok(())
}

pub fn watch_tree_until<F>(root: &Path, stop: &Receiver<()>, mut on_change: F) -> Result<()>
where
    F: FnMut() -> bool,
{
    let root_buf = root.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut debouncer = new_debouncer(DEBOUNCE, None, tx)?;
    let mut watched = 0;
    for dir in collect_watch_dirs(&root_buf) {
        if debouncer.watch(&dir, RecursiveMode::NonRecursive).is_ok() {
            watched += 1;
        }
    }
    if watched == 0 {
        debouncer.watch(&root_buf, RecursiveMode::NonRecursive)?;
    }

    loop {
        if stop_requested(stop) {
            break;
        }
        match rx.recv_timeout(STOP_POLL) {
            Ok(Ok(events)) => {
                let paths = paths_of(&events);
                if tree_affected(&root_buf, &paths) && !on_change() {
                    break;
                }
            }
            Ok(Err(_)) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }
    Ok(())
}

fn stop_requested(stop: &Receiver<()>) -> bool {
    stop.try_recv().is_ok()
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn アクティブファイルと一致する変更は再読込対象() {
        let target = p("/proj/docs/a.md");
        let changed = vec![p("/proj/docs/b.md"), p("/proj/docs/a.md")];
        assert!(active_file_affected(&target, &changed));
    }

    #[test]
    fn 別ファイルの変更はアクティブ再読込しない() {
        let target = p("/proj/docs/a.md");
        let changed = vec![p("/proj/docs/b.md"), p("/proj/docs/c.md")];
        assert!(!active_file_affected(&target, &changed));
    }

    #[test]
    fn root下のmd追加はツリー更新対象() {
        let root = p("/proj");
        let changed = vec![p("/proj/docs/new.md")];
        assert!(tree_affected(&root, &changed));
    }

    #[test]
    fn markdown拡張子もツリー更新対象() {
        let root = p("/proj");
        let changed = vec![p("/proj/note.markdown")];
        assert!(tree_affected(&root, &changed));
    }

    #[test]
    fn 非mdファイルはツリー更新しない() {
        let root = p("/proj");
        let changed = vec![p("/proj/src/main.rs"), p("/proj/notes.txt")];
        assert!(!tree_affected(&root, &changed));
    }

    #[test]
    fn 無視ディレクトリ内のmdはツリー更新しない() {
        let root = p("/proj");
        let changed = vec![
            p("/proj/node_modules/pkg/x.md"),
            p("/proj/.git/y.md"),
            p("/proj/target/z.md"),
        ];
        assert!(!tree_affected(&root, &changed));
    }

    #[test]
    fn root外のmdはツリー更新しない() {
        let root = p("/proj");
        let changed = vec![p("/other/a.md")];
        assert!(!tree_affected(&root, &changed));
    }

    #[test]
    fn root直下のmdもツリー更新対象() {
        let root = p("/proj");
        let changed = vec![p("/proj/README.md")];
        assert!(tree_affected(&root, &changed));
    }

    #[test]
    fn watch_file_until_stops_without_waiting_for_file_event() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "# note").unwrap();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let result = watch_file_until(&file, &stop_rx, || true);
            done_tx.send(result.is_ok()).unwrap();
        });
        stop_tx.send(()).unwrap();

        // Generous timeout: this asserts the watcher stops without a file event,
        // not how fast it stops. FSEvents setup under a loaded machine can take
        // seconds, and a tight bound made this flake.
        assert!(done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("watcher did not stop"));
    }

    #[test]
    fn watch_tree_until_stops_without_waiting_for_file_event() {
        let dir = tempfile::tempdir().unwrap();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let root = dir.path().to_path_buf();

        std::thread::spawn(move || {
            let result = watch_tree_until(&root, &stop_rx, || true);
            done_tx.send(result.is_ok()).unwrap();
        });
        stop_tx.send(()).unwrap();

        assert!(done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("tree watcher did not stop"));
    }

    #[test]
    fn tree_watch_dirs_skip_noisy_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("docs/nested")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::create_dir_all(root.join(".git/objects")).unwrap();

        let watched = collect_watch_dirs(root);

        assert!(watched.iter().any(|p| p.ends_with("docs/nested")));
        assert!(!watched.iter().any(|p| p.ends_with("node_modules")));
        assert!(!watched.iter().any(|p| p.ends_with("pkg")));
        assert!(!watched.iter().any(|p| p.ends_with("target")));
        assert!(!watched.iter().any(|p| p.ends_with(".git")));
    }
}
