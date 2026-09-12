//! Filesystem watching utilities for live reload and sidebar auto-refresh.
//!
//! Two watchers are exposed, both built on `notify-debouncer-full` and both
//! running on a dedicated thread that forwards debounced notifications over a
//! callback. The pure relevance predicates (which path/event should trigger a
//! reload) are unit-tested; the FS event plumbing is not.

use anyhow::{Context, Result};
use notify_debouncer_full::notify::event::{EventKind, ModifyKind};
use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, new_debouncer_opt, DebouncedEvent, NoCache};
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

/// Resolve a path the way FSEvents reports one, so the two can be compared.
///
/// FSEvents always reports the *fully resolved* path — symlinks followed and
/// macOS firmlinks expanded (`/tmp` → `/private/tmp`, `/var` → `/private/var`)
/// — in the volume's own spelling. The app, meanwhile, holds whatever spelling
/// the CLI argument, the Zed database, or a symlinked sidebar directory gave
/// it. Comparing those two verbatim silently drops every event, so live reload
/// and sidebar refresh just stop working for such paths.
///
/// The file itself may be gone (a delete or rename-away), so canonicalise the
/// parent — which still exists — and re-attach the file name.
fn resolved(p: &Path) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(p) {
        return c;
    }
    match (p.parent(), p.file_name()) {
        (Some(parent), Some(name)) => match std::fs::canonicalize(parent) {
            Ok(dir) => dir.join(name),
            Err(_) => p.to_path_buf(),
        },
        _ => p.to_path_buf(),
    }
}

/// Case-insensitive comparison key. `realpath(3)` resolves symlinks but does
/// *not* correct the spelling, and macOS volumes are case-insensitive by
/// default, so a home directory written `~alice` and `~Alice` can name the
/// same file while comparing unequal.
fn compare_key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

/// Does a batch of changed paths warrant re-reading the active file `target`?
///
/// True when any changed path is `target` (its content changed, was re-created,
/// or renamed into place), compared on the resolved path — see [`resolved`].
pub fn active_file_affected(target: &Path, changed: &[PathBuf]) -> bool {
    let target_key = compare_key(&resolved(target));
    changed
        .iter()
        .any(|p| compare_key(&resolved(p)) == target_key)
}

/// How a batch of changes affects the tree under `root`: its shape changed
/// (create/remove/rename — needs a full rescan), or only markdown file bodies
/// changed (their mtimes can be refreshed in place, no rescan needed).
pub enum TreeChange {
    Structural,
    Content(Vec<PathBuf>),
}

/// Classify a batch of changes for `root`'s tree. `None` when nothing in the
/// batch is relevant. A structural event anywhere in the batch wins outright.
pub fn classify_tree_change<'a>(
    root: &Path,
    changed: impl IntoIterator<Item = (EventKind, &'a Path)>,
) -> Option<TreeChange> {
    let root = resolved(root);
    let mut content_paths = Vec::new();
    for (kind, p) in changed {
        if !is_relevant_tree_path(&root, p) {
            continue;
        }
        if is_structural(&kind) {
            return Some(TreeChange::Structural);
        }
        if matches!(
            kind,
            EventKind::Modify(ModifyKind::Data(_)) | EventKind::Modify(ModifyKind::Metadata(_))
        ) {
            content_paths.push(p.to_path_buf());
        }
    }
    content_paths.sort();
    content_paths.dedup();
    if content_paths.is_empty() {
        None
    } else {
        Some(TreeChange::Content(content_paths))
    }
}

/// Does a batch of changes warrant rebuilding the sidebar tree under `root`?
/// True only for a shape change (create/remove/rename); see
/// [`classify_tree_change`] for content-only changes. why: `watch_tree_until`
/// now calls `classify_tree_change` directly, so this is test-only — kept as a
/// readable bool predicate for the existing structural-change test suite.
#[cfg_attr(not(test), allow(dead_code))]
pub fn tree_affected<'a>(
    root: &Path,
    changed: impl IntoIterator<Item = (EventKind, &'a Path)>,
) -> bool {
    matches!(
        classify_tree_change(root, changed),
        Some(TreeChange::Structural)
    )
}

/// Creates, deletes and renames reshape the tree; `Modify(Data)` (a save) and
/// `Modify(Metadata)` (a `touch`, an xattr write) leave it identical.
fn is_structural(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
    )
}

/// Is `p` inside `root` and part of what the sidebar tree shows?
///
/// `root` must already be [`resolved`]. Ordered so the common rejection — one
/// of the thousands of events a `cargo build` produces under `target/` — costs
/// no syscall at all: FSEvents already reports resolved paths, so the raw path
/// normally strips cleanly and [`resolved`] is only the spelling fallback.
fn is_relevant_tree_path(root: &Path, p: &Path) -> bool {
    let resolved_p;
    let rel = match strip_prefix_ignoring_case(p, root) {
        Some(rel) => rel,
        None => {
            resolved_p = resolved(p);
            match strip_prefix_ignoring_case(&resolved_p, root) {
                Some(rel) => rel,
                None => return false,
            }
        }
    };
    let mut components = rel.components();
    let Some(mut last) = components.next() else {
        return false; // the root itself
    };
    for component in components {
        if is_ignored_component(&last.as_os_str().to_string_lossy()) {
            return false;
        }
        last = component;
    }
    match std::fs::metadata(p) {
        // A directory's own name has to pass the ignore rule too, or creating
        // `.cache.md/` would read as a markdown file and refresh the sidebar.
        Ok(meta) if meta.is_dir() => !is_ignored_component(&last.as_os_str().to_string_lossy()),
        Ok(_) => is_markdown(p),
        // The path is already gone (a delete, or the source half of a rename,
        // which FSEvents reports without any file/folder hint). Guessing from
        // the extension would silently miss `docs.v1/` being moved out of the
        // project; one extra rescan when a non-markdown file is deleted is the
        // cheaper mistake.
        Err(_) => true,
    }
}

/// `Path::strip_prefix` that tolerates a spelling difference in the prefix
/// (see [`compare_key`]). Components must still match one-for-one.
fn strip_prefix_ignoring_case<'a>(path: &'a Path, prefix: &Path) -> Option<&'a Path> {
    if let Ok(rel) = path.strip_prefix(prefix) {
        return Some(rel);
    }
    let mut path_comps = path.components();
    for want in prefix.components() {
        let got = path_comps.next()?;
        if compare_key(Path::new(got.as_os_str())) != compare_key(Path::new(want.as_os_str())) {
            return None;
        }
    }
    Some(path_comps.as_path())
}

/// Collect all paths touched by a batch of debounced events.
fn paths_of(events: &[DebouncedEvent]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for ev in events {
        out.extend(ev.event.paths.iter().cloned());
    }
    out
}

/// Same, borrowed and paired with each path's event kind — the tree predicate
/// needs to tell a create/rename apart from a save.
fn changes_of(events: &[DebouncedEvent]) -> impl Iterator<Item = (EventKind, &Path)> {
    events
        .iter()
        .flat_map(|ev| ev.event.paths.iter().map(|p| (ev.event.kind, p.as_path())))
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

/// Watch a project root for tree-shape changes.
///
/// One recursive watcher covers subdirectories created after startup; the noise
/// directories are dropped by [`tree_affected`] rather than by not watching
/// them, which no static directory list could keep up with.
///
/// `NoCache` replaces the default file-id map because that map `stat`s the
/// whole tree when a recursive root is added — ~10s of solid IO on a Rust
/// project with a populated `target/`. Its only job is to stitch a rename's two
/// halves together, and either half already means "the tree changed".
pub fn watch_tree_until<F>(root: &Path, stop: &Receiver<()>, mut on_change: F) -> Result<()>
where
    F: FnMut(TreeChange) -> bool,
{
    let root_buf = root.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
        DEBOUNCE,
        None,
        tx,
        NoCache::new(),
        notify_debouncer_full::notify::Config::default(),
    )?;
    debouncer.watch(&root_buf, RecursiveMode::Recursive)?;

    loop {
        if stop_requested(stop) {
            break;
        }
        match rx.recv_timeout(STOP_POLL) {
            Ok(Ok(events)) => {
                if let Some(change) = classify_tree_change(&root_buf, changes_of(&events)) {
                    if !on_change(change) {
                        break;
                    }
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
    use notify_debouncer_full::notify::event::{
        CreateKind, DataChange, MetadataKind, RemoveKind, RenameMode,
    };

    const CREATE: EventKind = EventKind::Create(CreateKind::Any);
    const REMOVE_FILE: EventKind = EventKind::Remove(RemoveKind::File);

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

    // ── path rules that need no filesystem ───────────────────────────────

    #[test]
    fn root外のパスはツリー更新しない() {
        assert!(!tree_affected(
            Path::new("/proj"),
            [(CREATE, Path::new("/other/a.md"))]
        ));
    }

    #[test]
    fn root自体のイベントはツリー更新しない() {
        assert!(!tree_affected(
            Path::new("/proj"),
            [(CREATE, Path::new("/proj"))]
        ));
    }

    #[test]
    fn 無視ディレクトリ配下はツリー更新しない() {
        let changed = [
            (CREATE, Path::new("/proj/node_modules/pkg/x.md")),
            (CREATE, Path::new("/proj/.git/y.md")),
            (CREATE, Path::new("/proj/target/z.md")),
        ];
        assert!(!tree_affected(Path::new("/proj"), changed));
    }

    // ── event kind: only structural changes redraw the sidebar ───────────

    #[test]
    fn mdの内容変更はツリー更新しない() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.md");
        std::fs::write(&file, "# a").unwrap();
        let saved = EventKind::Modify(ModifyKind::Data(DataChange::Content));
        let touched = EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any));
        assert!(!tree_affected(dir.path(), [(saved, file.as_path())]));
        assert!(!tree_affected(dir.path(), [(touched, file.as_path())]));
    }

    #[test]
    fn mdの内容変更はcontentとして分類される() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.md");
        std::fs::write(&file, "# a").unwrap();
        let saved = EventKind::Modify(ModifyKind::Data(DataChange::Content));
        match classify_tree_change(dir.path(), [(saved, file.as_path())]) {
            Some(TreeChange::Content(paths)) => assert_eq!(paths, vec![file.clone()]),
            _ => panic!("expected Content"),
        }
    }

    #[test]
    fn 構造変化があればcontentより優先してstructuralになる() {
        let dir = tempfile::tempdir().unwrap();
        let saved_file = dir.path().join("a.md");
        std::fs::write(&saved_file, "# a").unwrap();
        let gone = dir.path().join("b.md");
        let saved = EventKind::Modify(ModifyKind::Data(DataChange::Content));
        let changed = [(saved, saved_file.as_path()), (REMOVE_FILE, gone.as_path())];
        assert!(matches!(
            classify_tree_change(dir.path(), changed),
            Some(TreeChange::Structural)
        ));
    }

    #[test]
    fn 無関係な変更はcontentとして分類しない() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.rs");
        std::fs::write(&file, "x").unwrap();
        let saved = EventKind::Modify(ModifyKind::Data(DataChange::Content));
        assert!(classify_tree_change(dir.path(), [(saved, file.as_path())]).is_none());
    }

    #[test]
    fn mdの削除とリネームはツリー更新対象() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        let gone = dir.path().join("docs/a.md");
        let renamed = EventKind::Modify(ModifyKind::Name(RenameMode::Any));
        assert!(tree_affected(dir.path(), [(REMOVE_FILE, gone.as_path())]));
        assert!(tree_affected(dir.path(), [(renamed, gone.as_path())]));
    }

    // ── what the tree is built from: markdown files and directories ──────

    #[test]
    fn mdの追加はツリー更新対象() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        for name in ["README.md", "docs/new.md", "docs/note.markdown"] {
            let file = dir.path().join(name);
            std::fs::write(&file, "# x").unwrap();
            assert!(
                tree_affected(dir.path(), [(CREATE, file.as_path())]),
                "{name}"
            );
        }
    }

    #[test]
    fn 既存の非mdファイルはツリー更新しない() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["main.rs", "notes.txt", "LICENSE"] {
            let file = dir.path().join(name);
            std::fs::write(&file, "x").unwrap();
            assert!(
                !tree_affected(dir.path(), [(CREATE, file.as_path())]),
                "{name}"
            );
        }
    }

    #[test]
    fn フォルダの作成はツリー更新対象() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("docs/sub");
        std::fs::create_dir_all(&sub).unwrap();
        let created_dir = EventKind::Create(CreateKind::Folder);
        assert!(tree_affected(dir.path(), [(created_dir, sub.as_path())]));
    }

    #[test]
    fn 拡張子付きの無視ディレクトリの作成はツリー更新しない() {
        // `.cache.md/` is a directory, not a markdown file: the ignore rule has
        // to be applied to the last component before its extension is read.
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join(".cache.md");
        std::fs::create_dir_all(&cache).unwrap();
        let created_dir = EventKind::Create(CreateKind::Folder);
        assert!(!tree_affected(dir.path(), [(created_dir, cache.as_path())]));
    }

    // ── vanished paths: never judged by their extension ──────────────────

    #[test]
    fn 拡張子付きディレクトリの無視領域への移動を取り逃がさない() {
        // `docs.v1/` moved to `target/docs.v1/`: the destination is ignored, so
        // only the source half can save the sidebar — and FSEvents gives a
        // rename no folder hint.
        let dir = tempfile::tempdir().unwrap();
        let renamed = EventKind::Modify(ModifyKind::Name(RenameMode::Any));
        let from = dir.path().join("docs.v1");
        let to = dir.path().join("target/docs.v1");
        assert!(tree_affected(
            dir.path(),
            [(renamed, from.as_path()), (renamed, to.as_path())]
        ));
    }

    #[test]
    fn 消えた非mdファイルは余分に再スキャンしてよい() {
        // The counterpart of the rule above: a vanished path is never rejected
        // on its extension, so deleting `LICENSE` costs one extra rescan.
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("LICENSE");
        assert!(tree_affected(dir.path(), [(REMOVE_FILE, gone.as_path())]));
    }

    // ── path spelling: FSEvents reports the fully resolved real path ─────
    // These cover the silent-no-reload class of bug: the app holds one
    // spelling, the watcher reports another, and every event is dropped.

    #[cfg(unix)]
    #[test]
    fn symlink越しのアクティブファイルも再読込対象() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("RealDir");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("a.md"), "# a").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        // The app opened the file through the symlink; FSEvents reports the
        // real directory (and, under /tmp, the /private prefix).
        let target = link.join("a.md");
        let reported = std::fs::canonicalize(real.join("a.md")).unwrap();
        assert!(active_file_affected(&target, &[reported]));
    }

    #[cfg(unix)]
    #[test]
    fn 大文字小文字違いのアクティブファイルも再読込対象() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("Docs");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("a.md"), "# a").unwrap();

        // macOS volumes are case-insensitive: the app may hold "docs" while
        // the watcher reports the on-disk "Docs".
        let target = dir.path().join("docs/a.md");
        let reported = std::fs::canonicalize(sub.join("a.md")).unwrap();
        assert!(active_file_affected(&target, &[reported]));
    }

    #[cfg(unix)]
    #[test]
    fn symlink越しのrootでもツリー更新対象() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("RealProj");
        std::fs::create_dir_all(real.join("docs")).unwrap();
        std::fs::write(real.join("docs/new.md"), "# n").unwrap();
        let link = dir.path().join("proj-link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let reported = std::fs::canonicalize(real.join("docs/new.md")).unwrap();
        assert!(tree_affected(&link, [(CREATE, reported.as_path())]));
        // The ignored-directory rule still applies after resolving.
        std::fs::create_dir_all(real.join("node_modules/pkg")).unwrap();
        std::fs::write(real.join("node_modules/pkg/x.md"), "# x").unwrap();
        let noise = std::fs::canonicalize(real.join("node_modules/pkg/x.md")).unwrap();
        assert!(!tree_affected(&link, [(CREATE, noise.as_path())]));
    }

    #[test]
    fn 存在しないパス同士は従来どおり文字列比較する() {
        // Deleted/never-created files can't be canonicalised; the raw
        // comparison must still work so tests and edge cases behave.
        let target = p("/proj/docs/a.md");
        assert!(active_file_affected(&target, &[p("/proj/docs/a.md")]));
        assert!(!active_file_affected(&target, &[p("/proj/docs/b.md")]));
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
            let result = watch_tree_until(&root, &stop_rx, |_| true);
            done_tx.send(result.is_ok()).unwrap();
        });
        stop_tx.send(()).unwrap();

        assert!(done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("tree watcher did not stop"));
    }

    #[test]
    fn 再帰監視でも既存ディレクトリのノイズは判定側で捨てる() {
        // The recursive watcher delivers events from every subdirectory, so the
        // ignore rule has to hold on real (stat-able) paths too.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("docs/nested")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::create_dir_all(root.join(".git/objects")).unwrap();
        let nested = root.join("docs/nested");
        let noise = [root.join("node_modules/pkg"), root.join(".git/objects")];

        assert!(tree_affected(root, [(CREATE, nested.as_path())]));
        assert!(!tree_affected(
            root,
            noise.iter().map(|p| (CREATE, p.as_path()))
        ));
    }
}
