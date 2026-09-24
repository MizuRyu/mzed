//! Reader notes: one JSON file per note under `~/.config/mzed/notes/`.
//!
//! mzed only ever creates note files and the agent that acts on them only ever
//! moves them away, so one file per note removes the need for any locking or
//! status field between the two.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Note file schema version. Bump when an existing field changes meaning.
const VERSION: u32 = 1;

/// why: a note points into a document rather than copying one, so a field past
/// these caps is a runaway selection, not content worth storing.
const QUOTE_MAX_CHARS: usize = 4_000;
const HEADING_MAX_CHARS: usize = 300;
const NOTE_MAX_CHARS: usize = 4_000;

const NAME_ATTEMPTS: usize = 16;

/// why: mixed into the file name so two notes written in the same second never
/// collide, even when their contents are identical.
static NOTE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// What the WebView reports about the current document selection. `pane`
/// (0=left, 1=right) says which pane's file the note belongs to.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub(crate) struct Selection {
    pub(crate) quote: String,
    #[serde(default)]
    pub(crate) heading: Option<String>,
    #[serde(default)]
    pub(crate) pane: u8,
}

/// One note, exactly as written to disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Note {
    pub(crate) version: u32,
    pub(crate) created_at: String,
    pub(crate) project_root: PathBuf,
    pub(crate) file: PathBuf,
    pub(crate) rel_path: String,
    pub(crate) heading: Option<String>,
    pub(crate) quote: String,
    pub(crate) note: String,
}

impl Note {
    /// Build a note about `file`. `created_at` is passed in (rather than read
    /// from the clock) so it also fixes the file name, which tests can assert.
    /// A file outside `project_root` — a dropped or IPC-opened standalone file —
    /// is anchored to its own parent, keeping `rel_path` short and meaningful.
    pub(crate) fn new(
        file: &Path,
        project_root: Option<&Path>,
        selection: &Selection,
        note: &str,
        created_at: String,
    ) -> Self {
        let root = project_root
            .filter(|root| file.starts_with(root))
            .map(Path::to_path_buf)
            .unwrap_or_else(|| file.parent().unwrap_or(file).to_path_buf());
        let rel_path = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .display()
            .to_string();
        Self {
            version: VERSION,
            created_at,
            project_root: root,
            file: file.to_path_buf(),
            rel_path,
            heading: selection
                .heading
                .as_deref()
                .map(str::trim)
                .filter(|heading| !heading.is_empty())
                .map(|heading| cut(heading, HEADING_MAX_CHARS)),
            quote: cut(selection.quote.trim(), QUOTE_MAX_CHARS),
            note: cut(note.trim(), NOTE_MAX_CHARS),
        }
    }
}

impl Note {
    /// A note read back from disk, cut to the caps [`Note::new`] applies when
    /// writing one (the file may have been edited by hand).
    fn clamped(mut self) -> Self {
        self.quote = cut(&self.quote, QUOTE_MAX_CHARS);
        self.heading = self.heading.map(|heading| cut(&heading, HEADING_MAX_CHARS));
        self.note = cut(&self.note, NOTE_MAX_CHARS);
        self
    }
}

/// Whether [`Note::new`] would have to cut something, so the caller can say so.
pub(crate) fn over_limit(selection: &Selection, note: &str) -> bool {
    let longer_than = |text: &str, max: usize| text.trim().chars().count() > max;
    longer_than(&selection.quote, QUOTE_MAX_CHARS)
        || selection
            .heading
            .as_deref()
            .is_some_and(|heading| longer_than(heading, HEADING_MAX_CHARS))
        || longer_than(note, NOTE_MAX_CHARS)
}

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// `~/.config/mzed/notes/`, created if missing (the palette's "Open Notes
/// Folder" hands it straight to Finder, which needs it to exist).
pub(crate) fn ensure_dir() -> Result<PathBuf> {
    let dir = crate::config::config_dir()
        .context("failed to determine the notes directory")?
        .join("notes");
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    Ok(dir)
}

/// Write `note` to the notes directory and return its path.
pub(crate) fn save(note: &Note) -> Result<PathBuf> {
    save_in(&ensure_dir()?, note)
}

/// [`save`] against an explicit directory, so tests never touch the real home.
///
/// why: writes a fresh name rather than replacing one — `persistence::atomic_write`
/// renames over whatever is already there, which here would delete someone's
/// note. The content still arrives whole (a `.json.tmp` renamed into place),
/// because an agent may be reading the directory at any moment.
pub(crate) fn save_in(dir: &Path, note: &Note) -> Result<PathBuf> {
    if note.note.trim().is_empty() {
        return Err(anyhow!("refusing to save an empty note"));
    }
    let json = serde_json::to_string_pretty(note).context("failed to serialize the note")?;
    std::fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
    for _ in 0..NAME_ATTEMPTS {
        let path = dir.join(file_name(note));
        if path.exists() {
            continue;
        }
        let temporary = path.with_extension("json.tmp");
        if let Err(error) = std::fs::write(&temporary, json.as_bytes()) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error).with_context(|| format!("failed to write {}", temporary.display()));
        }
        if path.exists() {
            let _ = std::fs::remove_file(&temporary);
            continue;
        }
        std::fs::rename(&temporary, &path)
            .with_context(|| format!("failed to move {} into place", path.display()))?;
        return Ok(path);
    }
    Err(anyhow!(
        "failed to find a free note file name in {}",
        dir.display()
    ))
}

/// The notes in the inbox about `file`, oldest first. Notes already moved into
/// `done/` are acted on, so they are not listed. Blocking: call off the UI thread.
pub(crate) fn pending_for(file: &Path) -> Vec<Note> {
    crate::config::config_dir()
        .map(|dir| pending_for_in(&dir.join("notes"), file))
        .unwrap_or_default()
}

/// why these caps: the inbox is read on every file switch and every inbox
/// change, and anything can be dropped into it. A note mzed writes is at most
/// ~12K characters (three 4,000-character fields), so a file past 64KB is not
/// one of ours; 1,000 files is far more than anyone leaves unhandled; 200
/// underlines is already more than a page can show legibly.
const NOTE_FILE_MAX_BYTES: u64 = 64 * 1024;
const INBOX_SCAN_MAX: usize = 1_000;
const PANE_NOTES_MAX: usize = 200;

/// [`pending_for`] against an explicit directory. A missing directory, and a
/// note file that cannot be read or parsed, count as no note. Only the newest
/// [`INBOX_SCAN_MAX`] files are read, and at most [`PANE_NOTES_MAX`] notes are
/// returned (the oldest ones).
pub(crate) fn pending_for_in(dir: &Path, file: &Path) -> Vec<Note> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    // The file name starts with the UTC stamp, so it sorts by age.
    paths.sort_unstable_by(|a, b| b.cmp(a));
    paths.truncate(INBOX_SCAN_MAX);
    let key = crate::watcher::path_key(file);
    let mut notes: Vec<Note> = paths
        .into_iter()
        .filter(|path| std::fs::metadata(path).is_ok_and(|meta| meta.len() <= NOTE_FILE_MAX_BYTES))
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .filter_map(|json| serde_json::from_str::<Note>(&json).ok())
        .filter(|note| crate::watcher::path_key(&note.file) == key)
        .map(Note::clamped)
        .collect();
    notes.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    notes.truncate(PANE_NOTES_MAX);
    notes
}

/// `<20260912T012205Z>-<8 hex>.json`: the UTC stamp sorts the directory in
/// creation order, the hash keeps two notes taken in the same second apart.
fn file_name(note: &Note) -> String {
    let stamp: String = note
        .created_at
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    format!("{stamp}-{}.json", digest(note))
}

// The sequence guards against a same-second collision within this process,
// the pid against another process writing at the same time, and the content
// hash against a file name from which the note's contents could be guessed.
fn digest(note: &Note) -> String {
    let mut hasher = std::hash::DefaultHasher::new();
    note.file.hash(&mut hasher);
    note.created_at.hash(&mut hasher);
    note.quote.hash(&mut hasher);
    note.note.hash(&mut hasher);
    NOTE_SEQUENCE
        .fetch_add(1, Ordering::Relaxed)
        .hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    format!("{:08x}", hasher.finish() as u32)
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn selection() -> Selection {
        Selection {
            quote: "選択した本文をそのまま".into(),
            heading: Some("## 実装方針".into()),
            pane: 0,
        }
    }

    fn note(created_at: &str) -> Note {
        Note::new(
            Path::new("/Users/me/dev/foo/docs/plan.md"),
            Some(Path::new("/Users/me/dev/foo")),
            &selection(),
            "ここは Orca 由来のイベントも対象にして",
            created_at.into(),
        )
    }

    #[test]
    fn 保存したJSONは全フィールドを持つ() {
        let dir = tempdir().unwrap();

        let path = save_in(dir.path(), &note("2026-09-12T01:22:05Z")).unwrap();

        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "version": 1,
                "created_at": "2026-09-12T01:22:05Z",
                "project_root": "/Users/me/dev/foo",
                "file": "/Users/me/dev/foo/docs/plan.md",
                "rel_path": "docs/plan.md",
                "heading": "## 実装方針",
                "quote": "選択した本文をそのまま",
                "note": "ここは Orca 由来のイベントも対象にして",
            })
        );
    }

    #[test]
    fn ファイル名は時刻スタンプとハッシュで時系列に並ぶ() {
        let dir = tempdir().unwrap();

        let first = save_in(dir.path(), &note("2026-09-12T01:22:05Z")).unwrap();
        let second = save_in(dir.path(), &note("2026-09-12T09:00:00Z")).unwrap();

        let name = |path: &Path| path.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            name(&first).starts_with("20260912T012205Z-"),
            "unexpected name: {}",
            name(&first)
        );
        assert!(name(&first).ends_with(".json"));
        let hex = name(&first)
            .trim_start_matches("20260912T012205Z-")
            .trim_end_matches(".json")
            .to_string();
        assert_eq!(hex.len(), 8);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(name(&first) < name(&second));
    }

    #[test]
    fn 同じ秒の同じ内容でも上書きしない() {
        let dir = tempdir().unwrap();
        let created_at = "2026-09-12T01:22:05Z";

        let first = save_in(dir.path(), &note(created_at)).unwrap();
        let second = save_in(dir.path(), &note(created_at)).unwrap();

        assert_ne!(first, second);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn 長すぎる引用と見出しとメモは上限で切られる() {
        let long = Note::new(
            Path::new("/Users/me/dev/foo/docs/plan.md"),
            Some(Path::new("/Users/me/dev/foo")),
            &Selection {
                quote: "あ".repeat(QUOTE_MAX_CHARS + 500),
                heading: Some("い".repeat(HEADING_MAX_CHARS + 50)),
                pane: 0,
            },
            &"う".repeat(NOTE_MAX_CHARS + 500),
            "2026-09-12T01:22:05Z".into(),
        );

        assert_eq!(long.quote.chars().count(), QUOTE_MAX_CHARS);
        assert_eq!(long.heading.unwrap().chars().count(), HEADING_MAX_CHARS);
        assert_eq!(long.note.chars().count(), NOTE_MAX_CHARS);
    }

    #[test]
    fn over_limitは上限を超えた入力だけを報告する() {
        let within = Selection {
            quote: "あ".repeat(QUOTE_MAX_CHARS),
            heading: Some("い".repeat(HEADING_MAX_CHARS)),
            pane: 0,
        };
        let quote_over = Selection {
            quote: "あ".repeat(QUOTE_MAX_CHARS + 1),
            ..within.clone()
        };
        let heading_over = Selection {
            heading: Some("い".repeat(HEADING_MAX_CHARS + 1)),
            ..within.clone()
        };

        assert!(!over_limit(&within, &"う".repeat(NOTE_MAX_CHARS)));
        assert!(over_limit(&quote_over, "短いメモ"));
        assert!(over_limit(&heading_over, "短いメモ"));
        assert!(over_limit(&within, &"う".repeat(NOTE_MAX_CHARS + 1)));
    }

    #[test]
    fn 同じ秒の別メモは別ファイルになる() {
        let dir = tempdir().unwrap();
        let created_at = "2026-09-12T01:22:05Z";
        let other = Note::new(
            Path::new("/Users/me/dev/foo/docs/plan.md"),
            Some(Path::new("/Users/me/dev/foo")),
            &selection(),
            "別の指示",
            created_at.into(),
        );

        let first = save_in(dir.path(), &note(created_at)).unwrap();
        let second = save_in(dir.path(), &other).unwrap();

        assert_ne!(first, second);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn root外のファイルは親ディレクトリをrootにする() {
        let built = Note::new(
            Path::new("/tmp/scratch/memo.md"),
            Some(Path::new("/Users/me/dev/foo")),
            &selection(),
            "直して",
            "2026-09-12T01:22:05Z".into(),
        );

        assert_eq!(built.project_root, PathBuf::from("/tmp/scratch"));
        assert_eq!(built.rel_path, "memo.md");
    }

    #[test]
    fn rootが無いファイルも親ディレクトリをrootにする() {
        let built = Note::new(
            Path::new("/tmp/scratch/memo.md"),
            None,
            &selection(),
            "直して",
            "2026-09-12T01:22:05Z".into(),
        );

        assert_eq!(built.project_root, PathBuf::from("/tmp/scratch"));
        assert_eq!(built.rel_path, "memo.md");
    }

    #[test]
    fn 空メモは保存しない() {
        let dir = tempdir().unwrap();
        let blank = Note::new(
            Path::new("/Users/me/dev/foo/docs/plan.md"),
            Some(Path::new("/Users/me/dev/foo")),
            &selection(),
            "   \n ",
            "2026-09-12T01:22:05Z".into(),
        );

        assert!(save_in(dir.path(), &blank).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn 受信箱から指定ファイルのメモだけを古い順に返す() {
        let dir = tempdir().unwrap();
        let docs = dir.path().join("Docs");
        std::fs::create_dir_all(&docs).unwrap();
        let plan = docs.join("plan.md");
        std::fs::write(&plan, "# plan").unwrap();
        let about = |file: &Path, note: &str, at: &str| {
            Note::new(file, Some(dir.path()), &selection(), note, at.into())
        };
        let inbox = dir.path().join("notes");
        save_in(&inbox, &about(&plan, "後", "2026-09-24T02:00:00Z")).unwrap();
        save_in(&inbox, &about(&plan, "先", "2026-09-24T01:00:00Z")).unwrap();
        save_in(
            &inbox,
            &about(&docs.join("other.md"), "別", "2026-09-24T01:30:00Z"),
        )
        .unwrap();
        // Handled notes, a half-written one and a broken one are all skipped.
        save_in(
            &inbox.join("done"),
            &about(&plan, "済", "2026-09-24T00:00:00Z"),
        )
        .unwrap();
        std::fs::write(inbox.join("x.json.tmp"), "{").unwrap();
        std::fs::write(inbox.join("broken.json"), "{").unwrap();

        // The same file spelt differently (case) still matches.
        let found = pending_for_in(&inbox, &dir.path().join("docs/PLAN.md"));

        let texts: Vec<_> = found.iter().map(|note| note.note.as_str()).collect();
        assert_eq!(texts, ["先", "後"]);
    }

    #[test]
    fn 大きすぎるメモファイルは読まない() {
        let dir = tempdir().unwrap();
        let plan = dir.path().join("plan.md");
        let inbox = dir.path().join("notes");
        let small = Note::new(
            &plan,
            None,
            &selection(),
            "小",
            "2026-09-24T01:00:00Z".into(),
        );
        save_in(&inbox, &small).unwrap();
        let mut big = serde_json::to_value(&small).unwrap();
        big["note"] = "大".repeat(NOTE_FILE_MAX_BYTES as usize).into();
        std::fs::write(inbox.join("big.json"), big.to_string()).unwrap();

        let found = pending_for_in(&inbox, &plan);

        let texts: Vec<_> = found.iter().map(|note| note.note.as_str()).collect();
        assert_eq!(texts, ["小"]);
    }

    #[test]
    fn 読み込み時も各フィールドを上限で切る() {
        let dir = tempdir().unwrap();
        let plan = dir.path().join("plan.md");
        let inbox = dir.path().join("notes");
        std::fs::create_dir_all(&inbox).unwrap();
        let mut edited = serde_json::to_value(Note::new(
            &plan,
            None,
            &selection(),
            "x",
            "2026-09-24T01:00:00Z".into(),
        ))
        .unwrap();
        edited["quote"] = "あ".repeat(QUOTE_MAX_CHARS + 10).into();
        std::fs::write(inbox.join("edited.json"), edited.to_string()).unwrap();

        let found = pending_for_in(&inbox, &plan);

        assert_eq!(found[0].quote.chars().count(), QUOTE_MAX_CHARS);
    }

    #[test]
    fn 走査は新しい順に上限件数まで_返すのは古い順に上限件数まで() {
        let dir = tempdir().unwrap();
        let plan = dir.path().join("plan.md");
        let inbox = dir.path().join("notes");
        std::fs::create_dir_all(&inbox).unwrap();
        let json = |i: usize| {
            serde_json::to_string(&Note::new(
                &plan,
                None,
                &selection(),
                &i.to_string(),
                format!("2026-09-24T00:00:00.{i:04}Z"),
            ))
            .unwrap()
        };
        // Written directly: `save_in` would hash thousands of names for nothing.
        for i in 0..INBOX_SCAN_MAX + 5 {
            std::fs::write(inbox.join(format!("{i:05}.json")), json(i)).unwrap();
        }

        let found = pending_for_in(&inbox, &plan);

        assert_eq!(found.len(), PANE_NOTES_MAX);
        // The 5 oldest files are past the scan cap; the rest start at 5.
        assert_eq!(found[0].note, "5");
        assert_eq!(
            found[PANE_NOTES_MAX - 1].note,
            (PANE_NOTES_MAX + 4).to_string()
        );
    }

    #[test]
    fn 受信箱が無ければメモ無し() {
        let dir = tempdir().unwrap();

        assert!(pending_for_in(&dir.path().join("missing"), Path::new("/a.md")).is_empty());
    }

    #[test]
    fn 見出しの無い選択はheadingがnullになる() {
        let built = Note::new(
            Path::new("/Users/me/dev/foo/docs/plan.md"),
            Some(Path::new("/Users/me/dev/foo")),
            &Selection {
                quote: "本文".into(),
                heading: None,
                pane: 0,
            },
            "直して",
            "2026-09-12T01:22:05Z".into(),
        );

        assert_eq!(built.heading, None);
    }
}
