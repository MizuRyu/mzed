//! CLI argument parsing (pure logic).
//!
//! `clap` parses the raw process arguments into [`Cli`]. The pure function
//! [`resolve`] turns those parsed args into an [`Intent`] describing what the
//! app should open (files, a directory, or nothing) plus the requested sync
//! mode. FS access (canonicalize, existence checks) happens in `classify`,
//! which is exercised against real temp paths; the argument-to-intent mapping
//! itself is pure and unit-tested with plain strings.

use crate::theme::SyncMode;
use clap::Parser;
use std::path::PathBuf;

/// Parsed command line for `mzed`.
#[derive(Debug, Parser, Clone, PartialEq, Eq)]
#[command(
    name = "mzed",
    about = "Zed-linked Markdown viewer",
    version,
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Markdown files or directories to open. With none, mzed starts in
    /// Zed-linked mode.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Sync mode: how mzed follows Zed's focused project.
    #[arg(long, value_enum)]
    pub sync: Option<SyncArg>,
}

/// CLI surface for the sync mode (mirrors [`SyncMode`] with a `clap` enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SyncArg {
    Auto,
    #[value(name = "self")]
    SelfPinned,
    Off,
}

impl From<SyncArg> for SyncMode {
    fn from(a: SyncArg) -> Self {
        match a {
            SyncArg::Auto => SyncMode::Auto,
            SyncArg::SelfPinned => SyncMode::SelfPinned,
            SyncArg::Off => SyncMode::Off,
        }
    }
}

/// What the app should open at startup, derived purely from the parsed args.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// No path arguments: start in Zed-linked mode.
    Zed,
    /// One or more files to open as tabs.
    Files(Vec<PathBuf>),
    /// A single directory to use as the project root.
    Dir(PathBuf),
}

/// The resolved startup intent: what to open plus the requested sync mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    pub target: Target,
    pub sync: SyncMode,
    pub sync_overridden: bool,
}

/// A predicate over a path: is it an existing directory?
///
/// Injected so [`resolve_with`] stays pure and testable; production passes the
/// real FS check via [`resolve`].
pub type IsDir<'a> = dyn Fn(&std::path::Path) -> bool + 'a;

/// Resolve parsed args into an [`Intent`], using `is_dir` to classify paths.
///
/// Rules:
/// - no paths -> [`Target::Zed`]
/// - exactly one path that is a directory -> [`Target::Dir`]
/// - otherwise -> [`Target::Files`] (directories among multiple paths are
///   dropped; only file paths become tabs)
pub fn resolve_with(cli: &Cli, is_dir: &IsDir) -> Intent {
    let sync_overridden = cli.sync.is_some();
    let sync = cli.sync.unwrap_or(SyncArg::Auto).into();
    let target = match cli.paths.as_slice() {
        [] => Target::Zed,
        [only] if is_dir(only) => Target::Dir(only.clone()),
        paths => {
            let files: Vec<PathBuf> = paths.iter().filter(|p| !is_dir(p)).cloned().collect();
            if files.is_empty() {
                // All args were directories but more than one; fall back to the
                // first as the root rather than opening nothing.
                Target::Dir(paths[0].clone())
            } else {
                Target::Files(files)
            }
        }
    };
    Intent {
        target,
        sync,
        sync_overridden,
    }
}

/// Resolve against the real filesystem.
pub fn resolve(cli: &Cli) -> Intent {
    resolve_with(cli, &|p| p.is_dir())
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    fn cli(paths: &[&str], sync: Option<SyncArg>) -> Cli {
        Cli {
            paths: paths.iter().map(PathBuf::from).collect(),
            sync,
        }
    }

    fn never_dir(_: &std::path::Path) -> bool {
        false
    }

    #[test]
    fn 引数なしはZed連動になる() {
        let c = cli(&[], None);
        let intent = resolve_with(&c, &never_dir);
        assert_eq!(intent.target, Target::Zed);
        assert_eq!(intent.sync, SyncMode::Auto);
        assert!(!intent.sync_overridden);
    }

    #[test]
    fn 単一ファイルはFilesになる() {
        let c = cli(&["/x.md"], None);
        let intent = resolve_with(&c, &never_dir);
        assert_eq!(intent.target, Target::Files(vec![PathBuf::from("/x.md")]));
    }

    #[test]
    fn 単一ディレクトリはDirになる() {
        let c = cli(&["/proj"], None);
        let is_dir = |p: &std::path::Path| p == std::path::Path::new("/proj");
        let intent = resolve_with(&c, &is_dir);
        assert_eq!(intent.target, Target::Dir(PathBuf::from("/proj")));
    }

    #[test]
    fn 複数ファイルは順序を保ったFilesになる() {
        let c = cli(&["/a.md", "/b.md"], None);
        let intent = resolve_with(&c, &never_dir);
        assert_eq!(
            intent.target,
            Target::Files(vec![PathBuf::from("/a.md"), PathBuf::from("/b.md")])
        );
    }

    #[test]
    fn 複数指定でディレクトリは除外されファイルだけ開く() {
        let c = cli(&["/dir", "/a.md"], None);
        let is_dir = |p: &std::path::Path| p == std::path::Path::new("/dir");
        let intent = resolve_with(&c, &is_dir);
        assert_eq!(intent.target, Target::Files(vec![PathBuf::from("/a.md")]));
    }

    #[test]
    fn syncフラグがSyncModeへ変換される() {
        assert_eq!(
            resolve_with(&cli(&[], Some(SyncArg::SelfPinned)), &never_dir).sync,
            SyncMode::SelfPinned
        );
        assert_eq!(
            resolve_with(&cli(&[], Some(SyncArg::Off)), &never_dir).sync,
            SyncMode::Off
        );
    }

    #[test]
    fn syncフラグ指定の有無を保持する() {
        assert!(!resolve_with(&cli(&[], None), &never_dir).sync_overridden);
        assert!(resolve_with(&cli(&[], Some(SyncArg::Auto)), &never_dir).sync_overridden);
        assert!(resolve_with(&cli(&[], Some(SyncArg::Off)), &never_dir).sync_overridden);
    }

    #[test]
    fn clapがファイルとsyncをパースする() {
        let c = Cli::try_parse_from(["mzed", "a.md", "b.md", "--sync", "off"]).unwrap();
        assert_eq!(c.paths, vec![PathBuf::from("a.md"), PathBuf::from("b.md")]);
        assert_eq!(c.sync, Some(SyncArg::Off));
    }
}
