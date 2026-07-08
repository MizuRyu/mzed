use crate::domain::action::AppCommand;
use crate::tabs::Tabs;
use crate::{
    app_state, cli, config, export, files, instance, js, palette, perf, search, services, session,
    theme, ui, zed,
};
use clap::Parser;
use dioxus::dioxus_core::Task;
use dioxus::html::HasFileData;
use dioxus::prelude::*;
use instance::Msg;
use services::file_service;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use ui::*;

// github-markdown / hljs theme CSS is emitted as inline <style> (see the App
// render) rather than as <link> assets, so only the runtime JS/KaTeX assets and
// MDO_CSS are referenced as `Asset`s here. The themed CSS strings live in the
// `EXPORT_*` `include_str!` constants below.
pub(crate) const MDO_CSS: Asset = asset!("/assets/mdo.css");
const HLJS_JS: Asset = asset!("/assets/highlight.min.js");
pub(crate) const MERMAID_JS: Asset = asset!("/assets/mermaid.min.js");
const KATEX_CSS: Asset = asset!("/assets/katex/katex.min.css");
const KATEX_JS: Asset = asset!("/assets/katex/katex.min.js");
const KATEX_AUTO: Asset = asset!("/assets/katex/auto-render.min.js");

// CSS/JS embedded into the binary for self-contained HTML export. Using
// `include_str!` (rather than reading asset paths at runtime) keeps export
// working from any working directory and bundles everything into one file.
const EXPORT_GITHUB_CSS: &str = include_str!("../assets/github-markdown.css");
const EXPORT_GITHUB_DARK_CSS: &str = include_str!("../assets/github-markdown-dark.css");
const EXPORT_HLJS_CSS: &str = include_str!("../assets/highlight-github.css");
const EXPORT_HLJS_DARK_CSS: &str = include_str!("../assets/highlight-github-dark.css");
const EXPORT_KATEX_CSS: &str = include_str!("../assets/katex/katex.min.css");

/// The startup intent (CLI-derived), consumed by the first `App` on mount.
static INITIAL: OnceLock<Mutex<Option<cli::Intent>>> = OnceLock::new();

/// Per-window message senders. The process-level IPC router forwards secondary
/// instance requests to the latest live window, pruning closed windows lazily.
static WINDOW_ROUTER: OnceLock<Mutex<WindowMessageRouter>> = OnceLock::new();
static FIRST_POST_RENDER_LOGGED: AtomicBool = AtomicBool::new(false);
/// Monotonically-increasing count of `App` instances mounted in this process.
/// The first window (count == 0 before increment) is the *base* window that
/// tracks Zed; all subsequent windows default to `SelfPinned` and skip the
/// Zed watcher.
static WINDOW_COUNT: AtomicUsize = AtomicUsize::new(0);

const IPC_ROUTER_CAPACITY: usize = 128;
const WINDOW_CHANNEL_CAPACITY: usize = 128;
const WINDOW_ROUTER_PENDING_CAPACITY: usize = 128;
const IPC_NEW_WINDOW_MIN_INTERVAL: Duration = Duration::from_millis(750);

#[derive(Default)]
struct WindowMessageRouter {
    senders: Vec<mpsc::Sender<Msg>>,
    pending: Vec<Msg>,
    last_ipc_new_window: Option<Instant>,
}

impl WindowMessageRouter {
    fn register(&mut self) -> mpsc::Receiver<Msg> {
        let (tx, rx) = mpsc::channel(WINDOW_CHANNEL_CAPACITY);
        for msg in self.pending.drain(..) {
            let _ = tx.try_send(msg);
        }
        self.senders.push(tx);
        rx
    }

    fn route(&mut self, msg: Msg) {
        self.route_at(msg, Instant::now());
    }

    fn route_at(&mut self, msg: Msg, now: Instant) {
        if matches!(msg, Msg::NewWindow) && !self.accept_ipc_new_window(now) {
            return;
        }

        let mut index = self.senders.len();
        while index > 0 {
            index -= 1;
            if self.senders[index].is_closed() {
                self.senders.remove(index);
                continue;
            }
            match self.senders[index].try_send(msg.clone()) {
                Ok(()) => return,
                Err(mpsc::error::TrySendError::Full(_)) => return,
                Err(_) => {
                    self.senders.remove(index);
                }
            }
        }
        if self.pending.len() < WINDOW_ROUTER_PENDING_CAPACITY {
            self.pending.push(msg);
        }
    }

    fn accept_ipc_new_window(&mut self, now: Instant) -> bool {
        if self.pending.iter().any(|msg| matches!(msg, Msg::NewWindow)) {
            return false;
        }
        if self
            .last_ipc_new_window
            .is_some_and(|last| now.duration_since(last) < IPC_NEW_WINDOW_MIN_INTERVAL)
        {
            return false;
        }
        self.last_ipc_new_window = Some(now);
        true
    }
}

fn window_router() -> &'static Mutex<WindowMessageRouter> {
    WINDOW_ROUTER.get_or_init(|| Mutex::new(WindowMessageRouter::default()))
}

fn register_window_message_receiver() -> mpsc::Receiver<Msg> {
    window_router()
        .lock()
        .expect("window message router lock")
        .register()
}

fn route_window_message(msg: Msg) {
    window_router()
        .lock()
        .expect("window message router lock")
        .route(msg);
}

fn start_ipc_router(
    rx: std::sync::mpsc::Receiver<Msg>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("mzed-ipc-router".into())
        .spawn(move || {
            while let Ok(msg) = rx.recv() {
                route_window_message(msg);
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_msg(path: &str) -> Msg {
        Msg::Open {
            path: PathBuf::from(path),
        }
    }

    #[test]
    fn window_router_drains_pending_messages_to_first_registered_window() {
        let mut router = WindowMessageRouter::default();
        let expected = open_msg("/pending.md");

        router.route(expected.clone());
        let mut rx = router.register();

        assert_eq!(rx.try_recv().unwrap(), expected);
    }

    #[test]
    fn window_router_routes_to_latest_live_window() {
        let mut router = WindowMessageRouter::default();
        let mut first = router.register();
        let mut second = router.register();
        let expected = open_msg("/latest.md");

        router.route(expected.clone());

        assert!(first.try_recv().is_err());
        assert_eq!(second.try_recv().unwrap(), expected);
    }

    #[test]
    fn window_router_prunes_closed_sender_and_uses_previous_live_window() {
        let mut router = WindowMessageRouter::default();
        let mut first = router.register();
        let second = router.register();
        drop(second);
        let expected = open_msg("/fallback.md");

        router.route(expected.clone());

        assert_eq!(first.try_recv().unwrap(), expected);
        assert_eq!(router.senders.len(), 1);
    }

    #[test]
    fn window_router_caps_pending_messages_without_registered_window() {
        let mut router = WindowMessageRouter::default();

        for index in 0..150 {
            router.route(open_msg(&format!("/{index}.md")));
        }

        assert_eq!(router.pending.len(), WINDOW_ROUTER_PENDING_CAPACITY);
    }

    #[test]
    fn window_router_rate_limits_ipc_new_window_messages() {
        let mut router = WindowMessageRouter::default();
        let mut rx = router.register();
        let now = Instant::now();

        router.route_at(Msg::NewWindow, now);
        router.route_at(Msg::NewWindow, now);

        assert_eq!(rx.try_recv().unwrap(), Msg::NewWindow);
        assert!(rx.try_recv().is_err());
    }
}

pub fn run() {
    perf::mark_process_start();
    // Parse CLI and absolutise any path arguments so messages sent to another
    // instance (and our own initial state) are unambiguous regardless of cwd.
    let mut parsed = cli::Cli::parse();
    parsed.paths = parsed
        .paths
        .into_iter()
        .map(file_service::canonical_path_or_original)
        .collect();
    let intent = cli::resolve(&parsed);

    // Surface unusable path arguments on stderr instead of letting the IPC
    // layer drop them silently (the primary ignores non-markdown paths).
    let intent = match intent.target {
        cli::Target::Files(files) => {
            let (valid, invalid): (Vec<_>, Vec<_>) = files
                .into_iter()
                .partition(|f| f.is_file() && files::is_markdown(f));
            for f in &invalid {
                if !f.exists() {
                    eprintln!("mzed: no such file: {}", f.display());
                } else {
                    eprintln!("mzed: not a markdown file: {}", f.display());
                }
            }
            if valid.is_empty() {
                std::process::exit(2);
            }
            cli::Intent {
                target: cli::Target::Files(valid),
                ..intent
            }
        }
        _ => intent,
    };

    // Single instance: hand the startup request to an existing primary. A
    // pathless start is a typed request for another native window.
    let msgs = Msg::for_secondary(&intent.target);
    match instance::try_send(&msgs) {
        Ok(true) => std::process::exit(0),
        Ok(false) => {}
        Err(err) => {
            eprintln!("mzed: failed to send IPC request: {err}");
            std::process::exit(1);
        }
    }

    // We are the primary. Socket workers forward into a process-level router,
    // which then delivers to the latest live window.
    let (tx, rx) = std::sync::mpsc::sync_channel::<Msg>(IPC_ROUTER_CAPACITY);
    let _ipc_router = match start_ipc_router(rx) {
        Ok(router) => router,
        Err(err) => {
            eprintln!("mzed: failed to start IPC router: {err}");
            std::process::exit(1);
        }
    };
    let _ipc_server = match services::instance_service::start_server(tx) {
        Ok(server) => server,
        Err(err) => {
            eprintln!(
                "mzed: failed to start IPC server at {}: {err}",
                instance::socket_path().display()
            );
            std::process::exit(1);
        }
    };

    let _ = INITIAL.set(Mutex::new(Some(intent)));

    // Launch with a custom menu that deliberately omits any View/Zoom items.
    // macOS otherwise reserves Cmd+=/-/0 as native zoom accelerators, swallowing
    // the keydown before the webview sees it, so our in-app font-size zoom never
    // fires. We keep a minimal Edit menu (undo/redo/cut/copy/paste/select-all) so
    // standard clipboard shortcuts still work, plus a Window menu for quit/close.
    use dioxus::desktop::Config;
    // Explicitly opt out of always-on-top so the window behaves like a normal
    // app window (can go behind others). Also lower the minimum inner size well
    // below the macOS default so the window can be shrunk for side-by-side use.
    // Initial window size comes from the persisted config (General settings).
    let saved = config::load();
    let window = services::platform::main_window_builder(saved.window_width, saved.window_height);
    dioxus::LaunchBuilder::desktop()
        .with_cfg(Config::new().with_menu(build_menu()).with_window(window))
        .launch(App);
}

/// Build the application menu bar. Keeps standard Window + Edit menus and adds a
/// View menu whose Zoom items carry Cmd+=/-/0 accelerators. macOS routes those
/// key equivalents to the menu (consuming them before the webview's own zoom),
/// and we handle the resulting menu events to drive the in-app font-size zoom.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub(crate) fn build_menu() -> dioxus::desktop::muda::Menu {
    use dioxus::desktop::muda::accelerator::{Accelerator, Code, Modifiers};
    use dioxus::desktop::muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

    let menu = Menu::new();

    let window_menu = Submenu::new("Window", true);
    let _ = window_menu.append_items(&[
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::show_all(None),
        &PredefinedMenuItem::minimize(None),
        &PredefinedMenuItem::close_window(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ]);

    // Edit menu keeps the standard clipboard items so Cmd+C/V/X/A/Z keep working.
    let edit_menu = Submenu::new("Edit", true);
    let _ = edit_menu.append_items(&[
        &PredefinedMenuItem::undo(None),
        &PredefinedMenuItem::redo(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::cut(None),
        &PredefinedMenuItem::copy(None),
        &PredefinedMenuItem::paste(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::select_all(None),
    ]);

    // View menu: Zoom items with Cmd+=/-/0 accelerators. Their ids are matched
    // in `App` (drained from `MENU_RX`) to apply the font-size zoom.
    let view_menu = Submenu::new("View", true);
    let zoom_in = MenuItem::with_id(
        "zoom_in",
        "Zoom In",
        true,
        Some(Accelerator::new(Some(Modifiers::META), Code::Equal)),
    );
    let zoom_out = MenuItem::with_id(
        "zoom_out",
        "Zoom Out",
        true,
        Some(Accelerator::new(Some(Modifiers::META), Code::Minus)),
    );
    let zoom_reset = MenuItem::with_id(
        "zoom_reset",
        "Actual Size",
        true,
        Some(Accelerator::new(Some(Modifiers::META), Code::Digit0)),
    );
    let _ = view_menu.append_items(&[&zoom_in, &zoom_out, &zoom_reset]);

    let _ = menu.append_items(&[&window_menu, &edit_menu, &view_menu]);

    #[cfg(target_os = "macos")]
    window_menu.set_as_windows_menu_for_nsapp();

    menu
}

/// Build a self-contained HTML document for `file` (rendered `body`, themed by
/// `appearance`) and write it via a native save dialog. Default file name is the
/// markdown stem with a `.html` extension. No-ops if no file is active or the
/// dialog is cancelled. Images are already data URLs (post_process), so the
/// output has no external dependencies.
/// The configured export directory, or the OS Downloads folder, or home.
pub(crate) fn export_dir(cfg: &Option<PathBuf>) -> PathBuf {
    cfg.clone()
        .or_else(dirs::download_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Right-click target on a sidebar row: screen position + the path and kind.
#[derive(Clone, PartialEq)]
pub(crate) struct CtxMenu {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) path: PathBuf,
    pub(crate) is_dir: bool,
}

#[component]
#[allow(clippy::collapsible_match, clippy::redundant_closure)]
pub(crate) fn App() -> Element {
    // Load persisted settings + last session up front, so they seed the initial
    // signal values before CLI/Zed apply. CLI args (handled later) take priority.
    let initial_intent = use_hook(|| INITIAL.get().and_then(|intent| intent.lock().ok()?.take()));
    let saved_config = use_hook(config::load);
    let saved_session = use_hook(session::load);
    use_hook(|| {
        perf::log_since_process_start("app.mounted", &[]);
    });
    // True only for the first window opened in this process. Subsequent windows
    // (Cmd+N) default to SelfPinned and skip the Zed watcher entirely so only
    // the base window follows Zed's active project.
    let is_base_window = use_hook(|| WINDOW_COUNT.fetch_add(1, Ordering::Relaxed) == 0);

    let mut root = use_signal(|| None::<PathBuf>);
    // All sidebar roots (multi-root Zed workspaces aggregate here). `root` stays
    // the primary/first root for headers and relative paths.
    let mut roots = use_signal(Vec::<PathBuf>::new);
    let mut tabs = use_signal(Tabs::default);
    // Split view: a second (right) pane with its own independent tab set. `split`
    // toggles its visibility; `active_pane` (0=left, 1=right) is the focused pane
    // that new files open into.
    let mut tabs_r = use_signal(Tabs::default);
    let mut split = use_signal(|| false);
    let mut active_pane = use_signal(|| 0u8);
    // The focused pane's tab signal (right only when the split is showing).
    // `Signal` is Copy, so returning it lets every tab op target the right pane.
    let act_tabs = move || {
        if split() && active_pane() == 1 {
            tabs_r
        } else {
            tabs
        }
    };
    // Route an open into the focused pane.
    let open_active = move |path: PathBuf| {
        act_tabs().write().open(path);
    };
    // Per-project parked tab sets (keyed by primary root). `tabs` holds the
    // current project's live set; switching projects parks/restores via this map.
    // Seed from the persisted session so each project's last-active file survives
    // restarts.
    let initial_pt = saved_session.restore_project_tabs();
    let mut project_tabs = use_signal(move || initial_pt);
    let mut expanded = use_signal(HashSet::<PathBuf>::new);
    // Open a file and reveal it in the sidebar (expand its ancestor folders).
    // Used by palette/search so the user can see where the file lives.
    let mut open_and_reveal = move |path: PathBuf| {
        let exp = file_service::ancestor_dirs_multi(&roots(), &path);
        if !exp.is_empty() {
            expanded.write().extend(exp);
        }
        act_tabs().write().open(path);
    };
    // Sidebar right-click menu + inline-ish rename (a small centered prompt).
    let mut ctx_menu = use_signal(|| None::<CtxMenu>);
    let mut rename_target = use_signal(|| None::<PathBuf>);
    let mut rename_buf = use_signal(String::new);
    let mut sidebar_width = use_signal(|| saved_session.sidebar_width);
    // Bumped by the active-file watcher to force a re-read/re-render of the
    // currently displayed file when it changes on disk.
    let mut reload = use_signal(|| 0u32);
    // Bumped by the project-root watcher to force a sidebar tree rebuild when
    // markdown files are added/removed/renamed.
    let mut tree_refresh = use_signal(|| 0u32);
    let mut file_watch_task = use_signal(|| None::<Task>);
    let mut file_watch_task_r = use_signal(|| None::<Task>);
    let mut tree_watch_task = use_signal(|| None::<Task>);
    let mut tree_generation = use_signal(app_state::generation::Generation::default);
    let mut document_generation = use_signal(app_state::generation::Generation::default);
    let mut document_generation_r = use_signal(app_state::generation::Generation::default);
    let mut trees = use_signal(Vec::<(PathBuf, Vec<files::TreeNode>)>::new);
    let mut document = use_signal(file_service::DocumentSnapshot::empty);
    let mut document_r = use_signal(file_service::DocumentSnapshot::empty);
    // Quick Access favorites (files and project dirs), arto-style: a single
    // ordered path list, persisted to config. Folder vs file is told apart by
    // `is_dir` at render time.
    let mut favorites = use_signal(|| saved_config.favorites.clone());

    // Chunk4: command palette, theme, sync mode and zoom state.
    // Theme/zoom seed from the saved config; sync mode seeds from config too but
    // a CLI `--sync` flag (when not the default Auto) overrides it.
    let mut theme = use_signal(|| saved_config.theme);
    let initial_sync = initial_intent.as_ref().map(|intent| intent.sync);
    let mut sync_mode = use_signal(|| {
        if !is_base_window {
            // Secondary windows don't track Zed; they stay on whichever project
            // they were last showing. SelfPinned prevents accidental Zed switches.
            theme::SyncMode::SelfPinned
        } else {
            match initial_sync {
                // CLI passed an explicit non-default mode -> honour it over the config.
                Some(m)
                    if initial_intent
                        .as_ref()
                        .is_some_and(|intent| intent.sync_overridden) =>
                {
                    m
                }
                _ => saved_config.sync_mode,
            }
        }
    });
    let mut persisted_sync_mode = use_signal(|| saved_config.sync_mode);
    let mut zed_sync_on = use_signal(|| true);
    let mut zoom = use_signal(|| saved_config.zoom);
    // Toggle a path's favorite status (add if absent, remove if present).
    let mut toggle_fav = move |path: PathBuf| {
        let mut f = favorites.write();
        if let Some(i) = f.iter().position(|p| p == &path) {
            f.remove(i);
        } else {
            f.push(path);
        }
    };
    // Drive the webview's native page zoom so the entire UI (sidebar, folder
    // tabs, tab bar, content) scales together and reflows like a browser.
    {
        let window = dioxus::desktop::use_window();
        use_effect(move || {
            let _ = window.webview.zoom(zoom() as f64);
        });
    }
    // ToC starts collapsed; toggled via the content toolbar.
    let toc_open = use_signal(|| false);
    // Raw view: show the active file's source markdown in a <pre><code> instead
    // of rendered HTML.
    let raw_view = use_signal(|| false);
    let mut palette_open = use_signal(|| false);
    // Settings modal (Cmd+,) and sidebar visibility toggle (Cmd+B).
    let mut settings_open = use_signal(|| false);
    // Active category in the two-pane settings modal.
    let settings_tab = use_signal(|| SettingsTab::Appearance);
    let mut sidebar_visible = use_signal(|| saved_config.sidebar_visible_default);
    // General settings (config-backed): default window size, startup behavior,
    // sidebar default, external-link policy, and the code-block font.
    let mut win_w = use_signal(|| saved_config.window_width);
    let mut win_h = use_signal(|| saved_config.window_height);
    // Last known window position (physical pixels). None until the window is
    // moved for the first time or a saved position exists.
    let mut win_x = use_signal(|| saved_config.window_x);
    let mut win_y = use_signal(|| saved_config.window_y);

    // Capture Moved/Resized events to keep position & size signals current.
    // Guards filter out anomalous values produced by minimize/maximize.
    {
        let window_ev = dioxus::desktop::use_window();
        dioxus::desktop::use_wry_event_handler(move |event, _| {
            use dioxus::desktop::tao::event::{Event, WindowEvent};
            match event {
                Event::WindowEvent {
                    event: WindowEvent::Moved(pos),
                    ..
                } => {
                    // Skip values that indicate the window is off-screen or
                    // being hidden (e.g. macOS minimise sends extreme coords).
                    if pos.x > -30_000 && pos.y > -30_000 {
                        win_x.set(Some(pos.x));
                        win_y.set(Some(pos.y));
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::Resized(size),
                    ..
                } => {
                    // Convert physical → logical before persisting so the
                    // existing window_width/height semantics are preserved.
                    if size.width > 0 && size.height > 0 {
                        let sf = window_ev.scale_factor();
                        win_w.set((size.width as f64 / sf).round() as i32);
                        win_h.set((size.height as f64 / sf).round() as i32);
                    }
                }
                _ => {}
            }
        });
    }

    // Restore saved position after the first render (once). Validates that the
    // title-bar area of the saved window rect intersects at least one connected
    // monitor; skips restoration silently when the window would be off-screen.
    {
        let init_x = saved_config.window_x;
        let init_y = saved_config.window_y;
        let init_w = saved_config.window_width;
        let init_h = saved_config.window_height;
        let window_pos = dioxus::desktop::use_window();
        use_effect(move || {
            if let (Some(x), Some(y)) = (init_x, init_y) {
                use dioxus::desktop::tao::dpi::PhysicalPosition;
                // Build monitor rects in physical pixels.
                let sf = window_pos
                    .current_monitor()
                    .map(|m| m.scale_factor())
                    .unwrap_or(1.0);
                let monitors: Vec<services::platform::Rect> = window_pos
                    .available_monitors()
                    .map(|m| {
                        let mpos = m.position();
                        let msz = m.size();
                        services::platform::Rect::new(
                            mpos.x,
                            mpos.y,
                            msz.width as i32,
                            msz.height as i32,
                        )
                    })
                    .collect();
                // Convert logical w/h to physical for the intersection check.
                let phys_w = (init_w as f64 * sf).round() as i32;
                let phys_h = (init_h as f64 * sf).round() as i32;
                let saved_rect = services::platform::Rect::new(x, y, phys_w, phys_h);
                if let Some((sx, sy)) = services::platform::clamp_to_monitors(saved_rect, &monitors)
                {
                    window_pos.set_outer_position(PhysicalPosition::new(sx, sy));
                }
            }
        });
    }
    let startup_behavior = use_signal(|| saved_config.startup);
    let sidebar_default = use_signal(|| saved_config.sidebar_visible_default);
    let external_links_in_browser = use_signal(|| saved_config.external_links_in_browser);
    let code_font = use_signal(|| saved_config.code_font.clone());
    let code_font_size = use_signal(|| saved_config.code_font_size);
    let line_height = use_signal(|| saved_config.line_height);
    let open_latest_on_project_open = use_signal(|| saved_config.open_latest_on_project_open);
    // Rebindable shortcuts, merged with defaults (fills new actions, drops stale).
    let keymap = use_signal(|| config::merged_keybindings(&saved_config.keybindings));
    // Export destination + feature flags (extension-like toggles).
    let export_dir_sig = use_signal(|| saved_config.export_dir.clone());
    let feature_katex = use_signal(|| saved_config.feature_katex);
    let feature_html_export = use_signal(|| saved_config.feature_html_export);
    let feature_pdf_export = use_signal(|| saved_config.feature_pdf_export);
    // Task View feature flags and configuration.
    let feature_task_view = use_signal(|| saved_config.feature_task_view);
    let task_view_tasks_subpath = use_signal(|| saved_config.task_view_tasks_subpath.clone());
    let task_view_scan_roots = use_signal(|| saved_config.task_view_scan_roots.clone());
    let task_view_scan_exclude = use_signal(|| saved_config.task_view_scan_exclude.clone());
    let task_view_days = use_signal(|| saved_config.task_view_days);
    let mut task_view_open = use_signal(|| false);
    // Transient top-right toast (e.g. "Copied!"). Auto-hides after ~1.5s; a
    // generation counter ensures only the latest toast clears itself.
    let mut toast = use_signal(|| None::<String>);
    let mut toast_gen = use_signal(|| 0u32);
    let mut show_toast = move |msg: String| {
        let g = toast_gen() + 1;
        toast_gen.set(g);
        toast.set(Some(msg));
        spawn(async move {
            // Await the Eval future (script completion), not `recv` — the script
            // sends no message, so `recv` would block forever and never clear.
            let _ = document::eval("await new Promise(r => setTimeout(r, 1500));").await;
            if toast_gen() == g {
                toast.set(None);
            }
        });
    };
    // Commit an inline rename: rename `rename_target`'s stem to `rename_buf`,
    // re-attaching the original extension, then clear the editing state.
    let commit_rename = move |_: ()| {
        if let Some(target) = rename_target() {
            let name = rename_buf();
            match file_service::rename_preserving_extension(&target, &name) {
                Ok(Some(dest)) => {
                    app_state::rename::apply_renamed_path(
                        &mut tabs.write(),
                        &mut tabs_r.write(),
                        &mut favorites.write(),
                        &target,
                        dest,
                    );
                    tree_refresh += 1;
                }
                Ok(None) => {
                    if !name.trim().is_empty() {
                        show_toast("Rename failed: invalid or existing name".into());
                    }
                }
                Err(err) => show_toast(format!("Rename failed: {err}")),
            }
        }
        rename_target.set(None);
    };
    let mut palette_query = use_signal(String::new);
    // Selected index within the current candidate list.
    let mut palette_sel = use_signal(|| 0usize);
    // false = command list, true = file-search mode.
    let mut palette_file_mode = use_signal(|| false);

    // Chunk7: in-document find bar (Cmd+F) and full-text search panel.
    let mut find_open = use_signal(|| false);
    let mut find_query = use_signal(String::new);
    let mut search_open = use_signal(|| false);
    let mut search_query = use_signal(String::new);
    let mut search_sel = use_signal(|| 0usize);
    let mut search_hits = use_signal(Vec::<search::Hit>::new);
    let mut search_generation = use_signal(app_state::generation::Generation::default);
    let mut search_task = use_signal(|| None::<Task>);
    let mut search_cancel = use_signal(|| None::<Arc<AtomicBool>>);
    let mut config_save_generation = use_signal(app_state::generation::Generation::default);
    let mut session_save_generation = use_signal(app_state::generation::Generation::default);

    // Effective appearance (light/dark) resolved from the theme choice; drives
    // which stylesheets load and the frame's dark class.
    let appearance = use_memo(move || theme().resolve());

    // Project-switch dropdown (top-left header) open state + filter query.
    let mut proj_menu_open = use_signal(|| false);
    let mut proj_menu_query = use_signal(String::new);

    // Fixed-position overlays can leave WKWebView's document scroll offset a
    // few pixels away from zero after Esc/blur. When no overlay is visible,
    // force the root scroll back so no page background appears under the app.
    use_effect(move || {
        let any_overlay_open =
            palette_open() || search_open() || settings_open() || proj_menu_open();
        if !any_overlay_open {
            spawn(async move {
                let _ = document::eval(js::reset_root_scroll_js()).await;
            });
        }
    });

    // Switch to `new_primary` (with `new_roots` as the full sidebar set),
    // retaining per-project tabs. Parks the current project's tabs, restores the
    // target's (or starts empty + opens `open_pick`). A re-selection of the same
    // primary root keeps the live tabs untouched. Centralised so Zed auto-switch,
    // IPC OpenDir, and the manual dropdown all behave identically.
    let mut switch_project = move |new_primary: PathBuf,
                                   new_roots: Vec<PathBuf>,
                                   expanded_set: HashSet<PathBuf>,
                                   open_pick: Option<PathBuf>| {
        let old = root();
        let same = old.as_ref() == Some(&new_primary);
        // Park current + take target tabs via the pure helper.
        let mut restored = project_tabs
            .write()
            .switch(old.as_ref(), &tabs.read(), &new_primary);
        if same {
            // Same project re-selected: root/roots/expanded are already correct,
            // so skip their signal writes to avoid spurious reactive updates
            // (tree rebuild, document reload, sidebar flicker). Only honour an
            // explicit additional file pick.
            if let Some(f) = open_pick {
                tabs.write().open(f);
            }
            return;
        }
        // Keep a reference to the primary path for B4 (latest-file lookup)
        // before it is consumed by `root.set`.
        let primary_for_latest = new_primary.clone();
        let roots_for_reveal = new_roots.clone();
        root.set(Some(new_primary));
        roots.set(new_roots);
        // When the project has previously-parked (or session-restored) tabs,
        // trust the restored active tab rather than clobbering it with a
        // generic representative markdown. Only seed with a pick when the
        // project has no tab history at all.
        let effective_pick = if restored.paths().is_empty() {
            // B4: when the toggle is on and no explicit pick was supplied,
            // prefer the most-recently-modified markdown over the
            // representative-file heuristic (pick_markdown).
            if open_pick.is_none() && open_latest_on_project_open() {
                file_service::latest_markdown(&primary_for_latest).or(open_pick)
            } else {
                open_pick
            }
        } else {
            None
        };
        if let Some(f) = effective_pick {
            restored.open(f);
        }
        // Reveal whatever tab ends up active (restored session tab, the pick,
        // or the latest-file) in the tree. The caller-provided expansion is
        // computed from its own pick, which may differ from the file actually
        // shown -- so expand to the real active file too, otherwise the open
        // document has no locatable home in the folder tree.
        let mut expanded_set = expanded_set;
        if let Some(active) = restored.active() {
            expanded_set.extend(file_service::ancestor_dirs_multi(&roots_for_reveal, active));
        }
        expanded.set(expanded_set);
        tabs.set(restored);
        // The right pane is not per-project (v1); collapse it on a switch.
        if split() {
            split.set(false);
            active_pane.set(0);
            tabs_r.set(Tabs::default());
        }
    };

    // Apply one IPC/initial message to the live state: open a file in a tab, or
    // switch the project root (opening a representative md and expanding to it).
    let mut apply_msg = move |msg: Msg| match msg {
        Msg::NewWindow => open_main_window(),
        Msg::Open { path } => {
            if path.is_file() && files::is_markdown(&path) {
                if root().is_none() {
                    if let Some(parent) = path.parent() {
                        let p = parent.to_path_buf();
                        root.set(Some(p.clone()));
                        roots.set(vec![p]);
                    }
                }
                open_active(path);
            }
        }
        Msg::OpenMany { paths } => {
            for path in paths {
                if path.is_file() && files::is_markdown(&path) {
                    if root().is_none() {
                        if let Some(parent) = path.parent() {
                            let p = parent.to_path_buf();
                            root.set(Some(p.clone()));
                            roots.set(vec![p]);
                        }
                    }
                    open_active(path);
                }
            }
        }
        Msg::OpenDir { path } => {
            if path.is_dir() {
                let pick = file_service::pick_markdown(&path);
                let exp = pick
                    .as_ref()
                    .map(|f| file_service::ancestor_dirs(&path, f))
                    .unwrap_or_default();
                switch_project(path.clone(), vec![path], exp, pick);
            }
        }
    };

    // Apply the CLI-derived startup intent exactly once. Files open as tabs (the
    // first becomes the project root's basis); a Dir becomes the root. A pure
    // Zed start contributes nothing here.
    use_hook(move || {
        // CLI files/dir take priority. A pure Zed start (Target::Zed, no msgs)
        // first restores the last session so the window comes back as it was;
        // a live Zed project switch may still override it shortly after.
        let msgs: Vec<Msg> = initial_intent
            .as_ref()
            .map(|intent| Msg::from_target(&intent.target))
            .unwrap_or_default();
        if msgs.is_empty() {
            // Honour the General "起動時の表示" setting: only `Restore` brings the
            // last session back. `Docs` leaves it for the Zed watcher to fill;
            // `Blank` stays empty.
            let restore = saved_config.startup == config::StartupBehavior::Restore;
            let sess = &saved_session;
            let restored_roots: Vec<PathBuf> =
                sess.roots.iter().filter(|p| p.is_dir()).cloned().collect();
            if restore && !restored_roots.is_empty() {
                root.set(Some(restored_roots[0].clone()));
                roots.set(restored_roots);
                let t = sess.restore_tabs();
                if let Some(active) = t.active() {
                    expanded.set(file_service::ancestor_dirs_multi(&roots(), active));
                }
                tabs.set(t);
            }
        } else {
            for msg in msgs {
                apply_msg(msg);
            }
        }
    });

    let window_ipc_rx = use_hook(|| Arc::new(Mutex::new(Some(register_window_message_receiver()))));

    // Drain process-routed IPC messages and apply them to this window.
    use_future(move || {
        let window_ipc_rx = Arc::clone(&window_ipc_rx);
        async move {
            let rx = window_ipc_rx.lock().ok().and_then(|mut g| g.take());
            if let Some(mut rx) = rx {
                while let Some(msg) = rx.recv().await {
                    apply_msg(msg);
                }
            }
        }
    });

    // Apply View-menu zoom. macOS consumes Cmd+=/-/0 as menu key equivalents
    // before the webview sees them, so we register them as menu accelerators and
    // handle the events here via Dioxus' muda hook (the same approach arto uses).
    dioxus::desktop::use_muda_event_handler(move |event| match event.id().0.as_str() {
        "zoom_in" => zoom.set(theme::zoom_in(zoom())),
        "zoom_out" => zoom.set(theme::zoom_out(zoom())),
        "zoom_reset" => zoom.set(theme::ZOOM_DEFAULT),
        _ => {}
    });

    // Watch Zed: on a project switch, swap the root, open a representative
    // markdown file in a new active tab, and expand the directories leading to
    // it. Existing tabs are kept. Only the base window tracks Zed; secondary
    // windows (Cmd+N) remain pinned to their current project.
    use_future(move || async move {
        if !is_base_window {
            return; // secondary windows don't follow Zed
        }
        let mut subscription = services::watch_service::zed_projects();
        while let Some(active) = subscription.rx.recv().await {
            // Read the *current* sync mode here (not a thread snapshot): the
            // watch thread always notifies; policy is applied on the async side.
            let decision = sync_mode().decide();
            if !decision.update_root {
                continue; // Off: ignore the switch entirely.
            }
            if let Some(p) = active {
                // Multi-root aware: a Zed workspace may have several roots.
                let new_roots = p.roots();
                let Some(primary) = new_roots.first().cloned() else {
                    continue;
                };
                // Representative markdown is picked from the primary root.
                let pick = file_service::pick_markdown(&primary);
                let exp = pick
                    .as_ref()
                    .map(|f| file_service::ancestor_dirs_multi(&new_roots, f))
                    .unwrap_or_default();
                // SelfPinned updates the sidebar but does not steal the tab.
                let open_pick = if decision.open_markdown { pick } else { None };
                switch_project(primary, new_roots, exp, open_pick);
            }
        }
    });

    // Derived: the active file (drives content, ToC and sidebar highlight).
    let active = use_memo(move || tabs.read().active().cloned());

    // Sidebar active-row sync: clear stale highlights even if an old TreeView
    // row did not re-render, then mark the current active file row.
    use_effect(move || {
        tree_refresh();
        let active_path = active().map(|p| p.to_string_lossy().to_string());
        let script = js::sidebar_active_js(active_path.as_deref());
        spawn(async move {
            let _ = document::eval(&script).await;
        });
    });

    // Build file trees off the UI thread. A generation prevents an older scan
    // from replacing a newer project after rapid root changes.
    use_effect(move || {
        tree_refresh();
        let current_roots = roots();
        let generation = tree_generation.write().advance();
        if current_roots.is_empty() {
            trees.set(Vec::new());
            return;
        }
        trees.set(Vec::new());
        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                current_roots
                    .into_iter()
                    .map(|root| {
                        let nodes = files::build_tree(&root);
                        (root, nodes)
                    })
                    .collect::<Vec<_>>()
            })
            .await;
            if !tree_generation.read().is_current(generation) {
                return;
            }
            match result {
                Ok(next) => trees.set(next),
                Err(err) => show_toast(format!("Project scan failed: {err}")),
            }
        });
    });

    // Flat tree across all roots, for the palette/full-text file lists.
    let tree = use_memo(move || {
        trees()
            .into_iter()
            .flat_map(|(_, nodes)| nodes)
            .collect::<Vec<_>>()
    });

    // Read and render each pane off the UI thread. The snapshot derives rendered
    // HTML, raw HTML, ToC, and find counts from one source read.
    use_effect(move || {
        reload();
        let path = active();
        let current_roots = roots();
        let generation = document_generation.write().advance();
        document.set(file_service::DocumentSnapshot::loading(path.clone()));
        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                file_service::load_document(path, &current_roots)
            })
            .await;
            if !document_generation.read().is_current(generation) {
                return;
            }
            match result {
                Ok(next) if active().as_deref() == next.path() => document.set(next),
                Ok(_) => {}
                Err(err) => show_toast(format!("Document load failed: {err}")),
            }
        });
    });

    let active_r = use_memo(move || tabs_r.read().active().cloned());
    use_effect(move || {
        reload();
        let path = if split() { active_r() } else { None };
        let current_roots = roots();
        let generation = document_generation_r.write().advance();
        document_r.set(file_service::DocumentSnapshot::loading(path.clone()));
        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                file_service::load_document(path, &current_roots)
            })
            .await;
            if !document_generation_r.read().is_current(generation) {
                return;
            }
            match result {
                Ok(next) if active_r().as_deref() == next.path() => document_r.set(next),
                Ok(_) => {}
                Err(err) => show_toast(format!("Document load failed: {err}")),
            }
        });
    });

    let html = use_memo(move || {
        let active_path = active();
        let snapshot = document.read();
        if snapshot.path() != active_path.as_deref() {
            return file_service::loading_html().to_string();
        }
        if raw_view() {
            snapshot.raw_html().to_string()
        } else {
            snapshot.rendered_html().to_string()
        }
    });

    let html_r = use_memo(move || {
        let active_path = if split() { active_r() } else { None };
        let snapshot = document_r.read();
        if snapshot.path() != active_path.as_deref() {
            return file_service::loading_html().to_string();
        }
        if raw_view() {
            snapshot.raw_html().to_string()
        } else {
            snapshot.rendered_html().to_string()
        }
    });
    let focused_file =
        move || app_state::pane::focused_path(active(), active_r(), split(), active_pane());
    // Copy a filesystem path via the native pbcopy command, bypassing the
    // WebView clipboard API which can emit a spurious error even on success.
    let copy_path_native = move |text: String, success_message: Option<String>| {
        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                services::platform::native_clipboard_write(&text)
            })
            .await;
            match result {
                Ok(Ok(())) => {
                    if let Some(msg) = success_message {
                        show_toast(msg);
                    }
                }
                Ok(Err(err)) => show_toast(format!("Copy failed: {err}")),
                Err(err) => show_toast(format!("Copy failed: {err}")),
            }
        });
    };
    let copy_focused_path = move || {
        if let Some(file) = focused_file() {
            copy_path_native(
                services::platform::canonical_clipboard_text(file),
                Some("Copied!".into()),
            );
        }
    };

    // Collapse the split once the right pane has no tabs left (e.g. the user
    // closed its last tab via the tab bar's × or Cmd+W).
    use_effect(move || {
        if split() && tabs_r.read().paths().is_empty() {
            split.set(false);
            active_pane.set(0);
        }
    });

    let toc = use_memo(move || {
        if split() && active_pane() == 1 {
            let active_path = active_r();
            let snapshot = document_r.read();
            if snapshot.path() == active_path.as_deref() {
                snapshot.toc().to_vec()
            } else {
                Vec::new()
            }
        } else {
            let active_path = active();
            let snapshot = document.read();
            if snapshot.path() == active_path.as_deref() {
                snapshot.toc().to_vec()
            } else {
                Vec::new()
            }
        }
    });

    // Watch both panes independently so an edit in the unfocused pane still
    // reloads. Cancelling the Dioxus task drops and joins its subscription.
    use_effect(move || {
        if let Some(task) = file_watch_task.write().take() {
            task.cancel();
        }
        let Some(file) = active() else {
            return;
        };
        let task = spawn(async move {
            let mut subscription = services::watch_service::file_changes(file);
            while subscription.rx.recv().await.is_some() {
                reload += 1;
            }
        });
        file_watch_task.set(Some(task));
    });
    use_effect(move || {
        if let Some(task) = file_watch_task_r.write().take() {
            task.cancel();
        }
        if !split() {
            return;
        }
        let Some(file) = active_r() else {
            return;
        };
        let task = spawn(async move {
            let mut subscription = services::watch_service::file_changes(file);
            while subscription.rx.recv().await.is_some() {
                reload += 1;
            }
        });
        file_watch_task_r.set(Some(task));
    });

    // Sidebar auto-update: watch every project root recursively. Restarts when
    // the root set changes (e.g. a Zed project switch or multi-root workspace).
    use_effect(move || {
        if let Some(task) = tree_watch_task.write().take() {
            task.cancel();
        }
        let rs = roots();
        if rs.is_empty() {
            return;
        }
        let task = spawn(async move {
            let mut subscription = services::watch_service::tree_changes(rs);
            while subscription.rx.recv().await.is_some() {
                tree_refresh += 1;
            }
        });
        tree_watch_task.set(Some(task));
    });

    // Persist user settings whenever theme/sync/zoom/favorites change. The first
    // run after mount also writes (harmless: it just re-saves the loaded values).
    use_effect(move || {
        let cfg = config::Config {
            theme: theme(),
            sync_mode: persisted_sync_mode(),
            zoom: zoom(),
            favorites: favorites(),
            window_width: win_w(),
            window_height: win_h(),
            window_x: win_x(),
            window_y: win_y(),
            startup: startup_behavior(),
            sidebar_visible_default: sidebar_default(),
            external_links_in_browser: external_links_in_browser(),
            code_font: code_font(),
            code_font_size: code_font_size(),
            keybindings: keymap(),
            export_dir: export_dir_sig(),
            feature_katex: feature_katex(),
            feature_html_export: feature_html_export(),
            feature_pdf_export: feature_pdf_export(),
            open_latest_on_project_open: open_latest_on_project_open(),
            line_height: line_height(),
            feature_task_view: feature_task_view(),
            task_view_tasks_subpath: task_view_tasks_subpath(),
            task_view_scan_roots: task_view_scan_roots(),
            task_view_scan_exclude: task_view_scan_exclude(),
            task_view_days: task_view_days(),
        };
        let generation = config_save_generation.write().advance();
        spawn(async move {
            if !config_save_generation.read().is_current(generation) {
                return;
            }
            match config::save_queued(cfg).await {
                Ok(()) => {}
                Err(err) => show_toast(format!("Settings save failed: {err}")),
            }
        });
    });

    // Persist the session (roots, tabs, active, sidebar width, per-project tabs)
    // on any change so the next launch restores it. Saving on every change avoids
    // relying on a desktop quit hook.
    use_effect(move || {
        let sess = session::Session::capture_full(
            roots(),
            &tabs.read(),
            sidebar_width(),
            &project_tabs.read(),
        );
        let generation = session_save_generation.write().advance();
        spawn(async move {
            if !session_save_generation.read().is_current(generation) {
                return;
            }
            match session::save_queued(sess).await {
                Ok(()) => {}
                Err(err) => show_toast(format!("Session save failed: {err}")),
            }
        });
    });

    // Derived: current project name for the top-left switcher button.
    let current_proj = use_memo(move || {
        root()
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "—".to_string())
    });

    // Derived: project-switch candidates. Union of mzed-opened projects
    // (project_tabs keys), the current sidebar roots, and Zed's recent
    // workspaces — de-duplicated, current roots first so they stay near the top.
    let proj_candidates = use_memo(move || {
        let mut out: Vec<PathBuf> = Vec::new();
        let mut push = |p: PathBuf| {
            if !out.contains(&p) {
                out.push(p);
            }
        };
        for r in roots() {
            push(r);
        }
        for r in project_tabs.read().roots() {
            push(r.clone());
        }
        if let Some(db) = zed::default_zed_db_path() {
            for r in zed::recent_workspaces(&db) {
                push(r);
            }
        }
        out
    });

    // Re-run highlight/mermaid/katex and (re)attach UI bridges whenever the
    // rendered HTML changes. The eval keeps a channel open: internal `.mdo-link`
    // clicks and external links post a message back, handled by the loop below.
    use_effect(move || {
        let _ = html();
        // Also re-run when the right pane's HTML changes (split view) and when
        // the appearance / KaTeX flag flips so it re-themes / re-renders live.
        let _ = html_r();
        let dark = appearance() == theme::Appearance::Dark;
        let katex = feature_katex();
        let allowed_roots = file_service::allowed_roots_for_active_files(
            &roots(),
            active().as_ref(),
            active_r().as_ref(),
        );
        spawn(async move {
            let mut eval = document::eval(&js::post_render_js(dark, katex));
            // Receive {kind, path|url} messages from the WebView.
            while let Ok(msg) = eval.recv::<serde_json::Value>().await {
                let kind = msg.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                match kind {
                    "post_render_complete" => {
                        let elapsed_ms = msg
                            .get("elapsed_ms")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        let panes = msg
                            .get("panes")
                            .and_then(|v| v.as_u64())
                            .unwrap_or_default()
                            .to_string();
                        let dark = msg
                            .get("dark")
                            .and_then(|v| v.as_bool())
                            .unwrap_or_default()
                            .to_string();
                        let katex = msg
                            .get("katex")
                            .and_then(|v| v.as_bool())
                            .unwrap_or_default()
                            .to_string();
                        perf::log_elapsed_ms(
                            "webview.post_render",
                            elapsed_ms,
                            &[("panes", panes), ("dark", dark), ("katex", katex)],
                        );
                        if !FIRST_POST_RENDER_LOGGED.swap(true, Ordering::AcqRel) {
                            perf::log_since_process_start(
                                "app.first_post_render",
                                &[
                                    (
                                        "panes",
                                        msg.get("panes")
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or_default()
                                            .to_string(),
                                    ),
                                    (
                                        "dark",
                                        msg.get("dark")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or_default()
                                            .to_string(),
                                    ),
                                    (
                                        "katex",
                                        msg.get("katex")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or_default()
                                            .to_string(),
                                    ),
                                ],
                            );
                        }
                    }
                    "open" => {
                        if let Some(p) = msg.get("path").and_then(|v| v.as_str()) {
                            let path = PathBuf::from(p);
                            if path.is_file()
                                && file_service::path_inside_roots(&path, &allowed_roots)
                            {
                                open_active(path);
                            }
                        }
                    }
                    "external" => {
                        if external_links_in_browser() {
                            if let Some(u) = msg.get("url").and_then(|v| v.as_str()) {
                                if let Err(err) = services::platform::open_target(u) {
                                    show_toast(format!("Open failed: {err}"));
                                }
                            }
                        }
                    }
                    "open_mermaid" => {
                        if let Some(src) = msg.get("src").and_then(|v| v.as_str()) {
                            if !src.trim().is_empty() {
                                open_mermaid_window(src.to_string(), dark);
                            }
                        }
                    }
                    _ => {}
                }
            }
        });
    });

    // Global keyboard bridge: install one persistent webview keydown listener
    // and dispatch the messages it posts back. Re-runs when the keymap changes
    // (re-injecting just swaps `window.__mdoKeymap`).
    use_effect(move || {
        let script = js::keydown_bridge_js(&keymap.read());
        spawn(async move {
            let mut eval = document::eval(&script);
            while let Ok(msg) = eval.recv::<serde_json::Value>().await {
                let Ok(command) = AppCommand::from_value(&msg) else {
                    continue;
                };
                match command {
                    AppCommand::PaletteToggle => {
                        let now = !palette_open();
                        palette_open.set(now);
                        if now {
                            // Re-scan the tree from disk so files created since
                            // launch (that the watcher may have missed) show up.
                            tree_refresh += 1;
                            palette_query.set(String::new());
                            palette_sel.set(0);
                            palette_file_mode.set(false);
                        }
                    }
                    AppCommand::OpenProjectMenu => {
                        let now = !proj_menu_open();
                        proj_menu_open.set(now);
                        if now {
                            proj_menu_query.set(String::new());
                        }
                    }
                    AppCommand::NewWindow => open_main_window(),
                    AppCommand::ZoomIn => zoom.set(theme::zoom_in(zoom())),
                    AppCommand::ZoomOut => zoom.set(theme::zoom_out(zoom())),
                    AppCommand::ZoomReset => zoom.set(theme::ZOOM_DEFAULT),
                    AppCommand::Settings => settings_open.set(true),
                    AppCommand::ToggleSidebar => {
                        let v = sidebar_visible();
                        sidebar_visible.set(!v);
                    }
                    AppCommand::FindToggle => {
                        let now = !find_open();
                        find_open.set(now);
                        if now {
                            find_query.set(String::new());
                        }
                    }
                    AppCommand::QuickOpen => {
                        tree_refresh += 1; // fresh disk scan for newly added files
                        palette_open.set(true);
                        palette_file_mode.set(true);
                        palette_query.set(String::new());
                        palette_sel.set(0);
                    }
                    AppCommand::FullTextSearch => {
                        tree_refresh += 1; // fresh disk scan for newly added files
                        search_open.set(true);
                        search_query.set(String::new());
                        search_sel.set(0);
                    }
                    AppCommand::CopyPath => {
                        copy_focused_path();
                    }
                    AppCommand::ToggleFav => {
                        if let Some(p) = focused_file() {
                            toggle_fav(p);
                        }
                    }
                    AppCommand::ToggleSplit => {
                        if split() {
                            // Collapse: drop the right pane, refocus the left.
                            split.set(false);
                            active_pane.set(0);
                            tabs_r.set(Tabs::default());
                        } else {
                            // Open a right pane seeded with the left pane's
                            // current file (VSCode-style duplicate), focus it.
                            let mut t = Tabs::default();
                            if let Some(f) = active() {
                                t.open(f);
                            }
                            tabs_r.set(t);
                            split.set(true);
                            active_pane.set(1);
                        }
                    }
                    AppCommand::FocusPane { index } => {
                        if index == 1 {
                            if !split() {
                                // Cmd+2 with no split yet: create it first.
                                let mut t = Tabs::default();
                                if let Some(f) = active() {
                                    t.open(f);
                                }
                                tabs_r.set(t);
                                split.set(true);
                            }
                            active_pane.set(1);
                        } else {
                            active_pane.set(0);
                        }
                    }
                    AppCommand::CloseTab => act_tabs().write().close_active(),
                    AppCommand::NextTab => act_tabs().write().activate_next(),
                    AppCommand::PrevTab => act_tabs().write().activate_prev(),
                    AppCommand::RenameActive => {
                        // Rename the focused pane's active file (Enter shortcut).
                        if let Some(p) = focused_file() {
                            let stem = p
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default();
                            rename_buf.set(stem);
                            rename_target.set(Some(p));
                        }
                    }
                    AppCommand::OpenTaskView => {
                        if feature_task_view() {
                            task_view_open.set(!task_view_open());
                        }
                    }
                    AppCommand::ToggleSyncPin => {
                        let new_mode = match sync_mode() {
                            theme::SyncMode::Auto => theme::SyncMode::SelfPinned,
                            _ => theme::SyncMode::Auto,
                        };
                        sync_mode.set(new_mode);
                        persisted_sync_mode.set(new_mode);
                        let label = match new_mode {
                            theme::SyncMode::Auto => "Sync: Auto",
                            theme::SyncMode::SelfPinned => "Sync: Self",
                            theme::SyncMode::Off => "Sync: Off",
                        };
                        show_toast(label.into());
                    }
                    AppCommand::Escape => {
                        // Close the topmost open overlay, if any. Order mirrors
                        // visual stacking (modals over the inline find bar).
                        if settings_open() {
                            settings_open.set(false);
                        } else if palette_open() {
                            palette_open.set(false);
                        } else if task_view_open() {
                            task_view_open.set(false);
                        } else if search_open() {
                            search_open.set(false);
                        } else if find_open() {
                            find_open.set(false);
                        }
                    }
                }
            }
        });
    });

    // In-document find: re-highlight whenever the query, the find bar's open
    // state, or the rendered HTML changes. Closing clears the highlight (empty
    // query). Also re-reads `html()` so highlights survive a content re-render.
    use_effect(move || {
        let _ = html();
        let q = if find_open() {
            find_query()
        } else {
            String::new()
        };
        spawn(async move {
            let _ = document::eval(&js::find_highlight_js(&q))
                .recv::<()>()
                .await;
        });
    });

    // Step the current find match.
    let find_step = move |dir: i32| {
        let script = js::find_step_js(dir);
        spawn(async move {
            let _ = document::eval(&script).recv::<()>().await;
        });
    };

    // Count of current matches, shown in the find bar.
    let find_count = use_memo(move || {
        let _ = html();
        if !find_open() {
            return 0usize;
        }
        let q = find_query();
        if split() && active_pane() == 1 {
            let active_path = active_r();
            let snapshot = document_r.read();
            if snapshot.path() == active_path.as_deref() {
                snapshot.find_count(&q)
            } else {
                0
            }
        } else {
            let active_path = active();
            let snapshot = document.read();
            if snapshot.path() == active_path.as_deref() {
                snapshot.find_count(&q)
            } else {
                0
            }
        }
    });

    // Debounce full-text search, run it off the UI thread, and cooperatively
    // cancel an older scan when the query or tree changes.
    use_effect(move || {
        if let Some(cancel) = search_cancel.write().take() {
            cancel.store(true, Ordering::Release);
        }
        if let Some(task) = search_task.write().take() {
            task.cancel();
        }

        let query = search_query();
        let paths = files::flatten_md(&tree());
        let generation = search_generation.write().advance();
        if !search_open() || query.trim().is_empty() {
            search_hits.set(Vec::new());
            return;
        }

        let cancel = Arc::new(AtomicBool::new(false));
        search_cancel.set(Some(Arc::clone(&cancel)));
        let task = spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            if cancel.load(Ordering::Acquire) {
                return;
            }
            let worker_cancel = Arc::clone(&cancel);
            let result = tokio::task::spawn_blocking(move || {
                search::search_paths_with_policy(
                    &paths,
                    &query,
                    search::SearchPolicy::default(),
                    || worker_cancel.load(Ordering::Acquire),
                )
            })
            .await;
            if !search_generation.read().is_current(generation) {
                return;
            }
            match result {
                Ok(report) if !report.cancelled => search_hits.set(report.hits),
                Ok(_) => {}
                Err(err) => show_toast(format!("Search failed: {err}")),
            }
        });
        search_task.set(Some(task));
    });

    // Dispatch a palette action against the Chunk4 state signals.
    let run_action = move |action: palette::Action| {
        use palette::Action::*;
        let mut set_sync = move |mode: theme::SyncMode| {
            sync_mode.set(mode);
            persisted_sync_mode.set(mode);
        };
        match action {
            SetThemeLight => theme.set(theme::Theme::Light),
            SetThemeDark => theme.set(theme::Theme::Dark),
            SetThemeSystem => theme.set(theme::Theme::System),
            SetSyncAuto => set_sync(theme::SyncMode::Auto),
            SetSyncSelf => set_sync(theme::SyncMode::SelfPinned),
            SetSyncOff => set_sync(theme::SyncMode::Off),
            ToggleZedSync => zed_sync_on.set(!zed_sync_on()),
            ToggleSyncPin => {
                let new_mode = match sync_mode() {
                    theme::SyncMode::Auto => theme::SyncMode::SelfPinned,
                    _ => theme::SyncMode::Auto, // SelfPinned or Off -> Auto
                };
                set_sync(new_mode);
                let label = match new_mode {
                    theme::SyncMode::Auto => "Sync: Auto (following Zed)",
                    theme::SyncMode::SelfPinned => "Sync: Self (pinned)",
                    theme::SyncMode::Off => "Sync: Off",
                };
                show_toast(label.into());
            }
            ZoomIn => zoom.set(theme::zoom_in(zoom())),
            ZoomOut => zoom.set(theme::zoom_out(zoom())),
            ZoomReset => zoom.set(theme::ZOOM_DEFAULT),
            FileSearch => {
                // Switch into file-search mode without closing the palette.
                palette_file_mode.set(true);
                palette_query.set(String::new());
                palette_sel.set(0);
                return;
            }
            FullTextSearch => {
                // Close the palette and open the full-text search panel.
                palette_open.set(false);
                search_query.set(String::new());
                search_sel.set(0);
                search_open.set(true);
                return;
            }
            CopyFilePath => {
                copy_focused_path();
            }
            ExportHtml => {
                if feature_html_export() {
                    let export_pane = app_state::pane::focused_pane_index(split(), active_pane());
                    if let Some(file) = focused_file() {
                        let dir = export_dir(&export_dir_sig());
                        spawn(async move {
                            // Capture the rendered body (with SVG diagrams) from
                            // the live view, then wrap + write a white-page doc.
                            let script = js::export_capture_js(export_pane);
                            let body = document::eval(&script)
                                .recv::<String>()
                                .await
                                .unwrap_or_default();
                            if !body.is_empty() {
                                let assets = export::Assets {
                                    github_css: EXPORT_GITHUB_CSS,
                                    highlight_css: EXPORT_HLJS_CSS,
                                    katex_css: EXPORT_KATEX_CSS,
                                };
                                match file_service::write_export_html(&file, &body, &dir, &assets) {
                                    Ok(out) => {
                                        let name = out
                                            .file_name()
                                            .map(|s| s.to_string_lossy().to_string())
                                            .unwrap_or_default();
                                        show_toast(format!("HTML generated! ({name})"));
                                    }
                                    Err(err) => {
                                        show_toast(format!("HTML export failed: {err}"));
                                    }
                                }
                            } else {
                                show_toast("HTML export failed: empty rendered body".into());
                            }
                        });
                    }
                }
            }
            ExportPdf => {
                if feature_pdf_export() {
                    // Fire the WebView's print dialog; the OS offers "Save as PDF".
                    // `@media print` in mdo.css hides app chrome so only the body prints.
                    spawn(async move {
                        match document::eval(services::platform::print_result_js())
                            .recv::<serde_json::Value>()
                            .await
                        {
                            Ok(value) => {
                                if let Some(error) =
                                    js::webview_action_error(&value, "PDF export failed")
                                {
                                    show_toast(error);
                                }
                            }
                            Err(err) => show_toast(format!("PDF export failed: {err}")),
                        }
                    });
                }
            }
        }
        palette_open.set(false);
    };

    let dark = appearance() == theme::Appearance::Dark;

    rsx! {
        // Theme CSS is emitted as inline <style> (not <link>) so switching
        // light<->dark swaps the element's text instead of leaving a stale
        // link behind (Dioxus does not remove unmounted document::Stylesheet
        // links), which would otherwise keep both themes applied at once.
        style {
            dangerous_inner_html: if dark { EXPORT_GITHUB_DARK_CSS } else { EXPORT_GITHUB_CSS },
        }
        style {
            dangerous_inner_html: if dark { EXPORT_HLJS_DARK_CSS } else { EXPORT_HLJS_CSS },
        }
        {
            // Code-block font (Theme settings). Empty family -> built-in mono.
            let cf = code_font();
            let fam = if cf.trim().is_empty() {
                "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace".to_string()
            } else {
                cf.replace(['{', '}', '<', '>', ';'], "")
            };
            let fs = code_font_size().clamp(8, 32);
            let css = format!(
                ".markdown-body pre, .markdown-body code, .markdown-body pre code {{ font-family: {fam} !important; font-size: {fs}px !important; }}"
            );
            rsx! { style { dangerous_inner_html: "{css}" } }
        }
        {
            // Body line-height (Appearance settings). Clamp to readable range.
            let lh = line_height().clamp(1.2, 2.4);
            let css = format!(
                ".markdown-body.markdown-body {{ line-height: {lh} !important; }}"
            );
            rsx! { style { dangerous_inner_html: "{css}" } }
        }
        document::Stylesheet { href: MDO_CSS }
        document::Stylesheet { href: KATEX_CSS }
        document::Script { src: HLJS_JS }
        document::Script { src: MERMAID_JS }
        document::Script { src: KATEX_JS }
        document::Script { src: KATEX_AUTO }
        {
            // Frame colours: dark vs light variants for app chrome (sidebar,
            // header, tab bar, ToC) since github-markdown-css only styles .markdown-body.
            let panel_bg = if dark { "#161b22" } else { "#f6f8fa" };
            let panel_border = if dark { "#30363d" } else { "#d0d7de" };
            let body_bg = if dark { "#0d1117" } else { "#ffffff" };
            // Zoom is handled globally via the webview's native page zoom.
            rsx! {
                div {
                    style: "position: fixed; inset: 0; display: flex; flex-direction: column; width: 100vw; height: 100vh; margin: 0; overflow: hidden; background: {body_bg};",
                    // Top bar: a Zed-style project switcher button on the left that
                    // opens a dropdown of known/recent projects. Replaces the old
                    // "📁 proj · file" header line.
                    div {
                        style: "position: relative; display: flex; align-items: center; padding: 6px 12px; background: #24292f; color: #fff; font: 13px -apple-system, sans-serif; flex: 0 0 auto;",
                        button {
                            class: "mdo-proj-btn",
                            style: "display: inline-flex; align-items: center; gap: 6px; padding: 4px 8px; background: transparent; color: #fff; border: none; border-radius: 6px; cursor: pointer; font: 600 13px -apple-system, sans-serif;",
                            onclick: move |_| {
                                let now = !proj_menu_open();
                                proj_menu_open.set(now);
                                if now { proj_menu_query.set(String::new()); }
                            },
                            span { "📁" }
                            span { "{current_proj}" }
                            span { style: "opacity: 0.7; font-size: 10px;", "▾" }
                        }
                        // (ProjectMenu is mounted at the overlay layer near the end of
                        // this rsx, outside the app frame div — see the comment there.)
                    }
                    div {
                        style: "display: flex; flex: 1 1 auto; min-height: 0;",
                        if sidebar_visible() {
                            {
                                let sw = sidebar_width();
                                let multi = trees().len() > 1;
                                rsx! {
                                div {
                                    style: "width: {sw}px; flex: 0 0 auto; overflow: auto; border-right: 1px solid {panel_border}; background: {panel_bg}; padding: 6px 0;",
                                    // Quick Access favorites (arto-style): one ordered
                                    // list of files + project dirs, told apart by icon.
                                    if !favorites().is_empty() {
                                        {
                                            let fav_fg = if dark { "#c9d1d9" } else { "#1f2328" };
                                            let fav_muted = if dark { "#8b949e" } else { "#8c959f" };
                                            rsx! {
                                                div {
                                                    style: "padding: 6px 10px 4px; font: 600 11px -apple-system, sans-serif; color: #8b949e; text-transform: uppercase; letter-spacing: 0.4px; display: flex; align-items: center; gap: 6px;",
                                                    svg { width: "12", height: "12", view_box: "0 0 24 24", fill: "#e3b341",
                                                        path { d: "M12 2l3 7h7l-5.5 4.5L18 21l-6-4-6 4 1.5-7.5L2 9h7z" } }
                                                    "お気に入り"
                                                }
                                                for fav in favorites() {
                                                    {
                                                        let is_dir = fav.is_dir();
                                                        let name = fav.file_name()
                                                            .map(|s| s.to_string_lossy().to_string())
                                                            .unwrap_or_else(|| fav.display().to_string());
                                                        let open_path = fav.clone();
                                                        let rm_path = fav.clone();
                                                        rsx! {
                                                            div {
                                                                key: "fav-{fav.display()}",
                                                                class: "mdo-fav-row mdo-tree-row",
                                                                style: "display: flex; align-items: center; gap: 6px; padding: 5px 8px 5px 14px; cursor: pointer; user-select: none; font: 13px -apple-system, sans-serif; line-height: 1.4; color: {fav_fg}; border-radius: 4px;",
                                                                onclick: move |_| {
                                                                    if open_path.is_dir() {
                                                                        let pick = file_service::pick_markdown(&open_path);
                                                                        let exp = pick.as_ref()
                                                                            .map(|f| file_service::ancestor_dirs(&open_path, f))
                                                                            .unwrap_or_default();
                                                                        switch_project(open_path.clone(), vec![open_path.clone()], exp, pick);
                                                                    } else {
                                                                        open_active(open_path.clone());
                                                                    }
                                                                },
                                                                if is_dir { {folder_closed_icon(fav_muted)} } else { {file_icon(fav_muted)} }
                                                                span { style: "overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1 1 auto;", "{name}" }
                                                                button {
                                                                    class: "mdo-fav-x",
                                                                    style: "background: transparent; border: none; color: {fav_muted}; cursor: pointer; flex: 0 0 auto; padding: 0 2px; font-size: 12px;",
                                                                    title: "お気に入りから外す",
                                                                    onclick: move |e| { e.stop_propagation(); toggle_fav(rm_path.clone()); },
                                                                    "✕"
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                div { style: "height: 1px; background: {panel_border}; margin: 6px 8px;" }
                                            }
                                        }
                                    }
                                    for (r, nodes) in trees() {
                                        // Show a section header per root only when multi-root.
                                        if multi {
                                            {
                                                let name = r.file_name()
                                                    .map(|s| s.to_string_lossy().to_string())
                                                    .unwrap_or_else(|| r.display().to_string());
                                                rsx! {
                                                    div {
                                                        style: "padding: 6px 10px 4px; font: 600 11px -apple-system, sans-serif; color: #8b949e; text-transform: uppercase; letter-spacing: 0.4px;",
                                                        "{name}"
                                                    }
                                                }
                                            }
                                        }
                                        for node in nodes {
                                            TreeView { key: "{node.path.display()}", node, depth: 0, expanded, tabs, on_open: move |p| open_active(p), on_context: move |c| ctx_menu.set(Some(c)), rename_target, rename_buf, on_rename_commit: commit_rename, favorites, on_toggle_fav: move |p| toggle_fav(p), on_copy_path: move |p| copy_path_native(services::platform::canonical_clipboard_text(p), Some("Copied!".into())), dark }
                                        }
                                    }
                                }
                                // Draggable divider: starts a document-level drag
                                // that streams the cursor X back to Rust, which
                                // clamps and updates `sidebar_width`.
                                div {
                                    style: "flex: 0 0 auto; width: 5px; cursor: col-resize; background: transparent; align-self: stretch;",
                                    class: "mdo-sidebar-divider",
                                    onmousedown: move |e| {
                                        e.prevent_default();
                                        spawn(async move {
                                            let mut eval =
                                                document::eval(js::sidebar_resize_js());
                                            while let Ok(msg) = eval.recv::<serde_json::Value>().await {
                                                if let Some(x) = msg.get("x").and_then(|v| v.as_f64()) {
                                                    let w = (x.round() as i64).clamp(150, 600) as u32;
                                                    sidebar_width.set(w);
                                                }
                                            }
                                        });
                                    },
                                }
                                }
                            }
                        }
                        div {
                            style: "flex: 1 1 auto; display: flex; flex-direction: column; min-width: 0; position: relative;",
                            // Content toolbar (mo-style): ToC toggle, copy raw
                            // markdown, raw-view toggle. Sits below the find bar
                            // (lower z-index) so the two don't overlap.
                            ContentToolbar {
                                theme,
                                toc_open,
                                raw_view,
                                has_toc: !toc().is_empty(),
                                find_open: find_open(),
                                dark,
                                on_copy: move |_| {
                                    let md = if split() && active_pane() == 1 {
                                        let active_path = active_r();
                                        let snapshot = document_r.read();
                                        if snapshot.path() == active_path.as_deref() {
                                            snapshot.source().to_string()
                                        } else {
                                            String::new()
                                        }
                                    } else {
                                        let active_path = active();
                                        let snapshot = document.read();
                                        if snapshot.path() == active_path.as_deref() {
                                            snapshot.source().to_string()
                                        } else {
                                            String::new()
                                        }
                                    };
                                    if !md.is_empty() {
                                        // Copy, then flash a checkmark on the button,
                                        // restoring its clipboard icon after ~1.2s.
                                        let js = services::platform::clipboard_write_with_feedback_js(
                                            &md,
                                            "mdo-copy-md-btn",
                                        );
                                        spawn(async move {
                                            match document::eval(&js)
                                                .recv::<serde_json::Value>()
                                                .await
                                            {
                                                Ok(value) => {
                                                    if let Some(error) =
                                                        js::webview_action_error(&value, "Copy failed")
                                                    {
                                                        show_toast(error);
                                                    }
                                                }
                                                Err(err) => {
                                                    show_toast(format!("Copy failed: {err}"));
                                                }
                                            }
                                        });
                                    }
                                },
                            }
                            if find_open() {
                                FindBar {
                                    query: find_query,
                                    open: find_open,
                                    count: find_count(),
                                    dark,
                                    on_step: find_step,
                                }
                            }
                            // Panes row: one (or two when split) editor columns,
                            // each with its own tab bar and rendered body.
                            div {
                                style: "flex: 1 1 auto; display: flex; min-width: 0; min-height: 0;",
                                {
                                    let focus0 = !split() || active_pane() == 0;
                                    let bd0 = if split() && focus0 { "#1f6feb" } else { "transparent" };
                                    rsx! {
                                        div {
                                            style: "flex: 1 1 0; display: flex; flex-direction: column; min-width: 0; min-height: 0; border-top: 2px solid {bd0};",
                                            onmousedown: move |_| active_pane.set(0),
                                            TabBar { tabs, root: root(), dark }
                                            div {
                                                style: "flex: 1 1 auto; width: 100%; overflow: auto; background: {body_bg};",
                                                // File drag & drop: native absolute paths.
                                                // Dropped `.md` opens in the focused pane; a
                                                // dropped directory becomes the project root.
                                                ondragover: move |e| e.prevent_default(),
                                                ondrop: move |e: DragEvent| {
                                                    e.prevent_default();
                                                    for fd in e.files() {
                                                        let path = fd.path();
                                                        if path.is_dir() {
                                                            apply_msg(Msg::OpenDir { path });
                                                        } else if path.extension().map(|x| x == "md").unwrap_or(false) {
                                                            apply_msg(Msg::Open { path });
                                                        }
                                                    }
                                                },
                                                div { class: "markdown-body", "data-mdo-pane": "0", dangerous_inner_html: "{html}" }
                                            }
                                        }
                                    }
                                }
                                if split() {
                                    div { style: "width: 1px; flex: 0 0 auto; background: {panel_border};" }
                                    {
                                        let focus1 = active_pane() == 1;
                                        let bd1 = if focus1 { "#1f6feb" } else { "transparent" };
                                        rsx! {
                                            div {
                                                style: "flex: 1 1 0; display: flex; flex-direction: column; min-width: 0; min-height: 0; border-top: 2px solid {bd1};",
                                                onmousedown: move |_| active_pane.set(1),
                                                TabBar { tabs: tabs_r, root: root(), dark }
                                                div {
                                                    style: "flex: 1 1 auto; width: 100%; overflow: auto; background: {body_bg};",
                                                    div { class: "markdown-body", "data-mdo-pane": "1", dangerous_inner_html: "{html_r}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if toc_open() && !toc().is_empty() {
                            TocPanel { entries: toc(), dark }
                        }
                    }
                }
                if palette_open() {
                    Palette {
                        query: palette_query,
                        sel: palette_sel,
                        file_mode: palette_file_mode,
                        open: palette_open,
                        on_open: move |p| open_and_reveal(p),
                        files: files::flatten_md(&tree()),
                        html_export_on: feature_html_export(),
                        pdf_export_on: feature_pdf_export(),
                        dark,
                        on_action: run_action,
                    }
                }
                if search_open() {
                    SearchPanel {
                        query: search_query,
                        sel: search_sel,
                        open: search_open,
                        on_open: move |p| open_and_reveal(p),
                        hits: search_hits(),
                        roots: roots(),
                        dark,
                    }
                }
                if settings_open() {
                    Settings {
                        open: settings_open,
                        tab: settings_tab,
                        theme,
                        sync_mode,
                        persisted_sync_mode,
                        zoom,
                        win_w,
                        win_h,
                        startup_behavior,
                        sidebar_default,
                        external_links_in_browser,
                        code_font,
                        code_font_size,
                        line_height,
                        keymap,
                        export_dir_sig,
                        feature_katex,
                        feature_html_export,
                        feature_pdf_export,
                        open_latest_on_project_open,
                        feature_task_view,
                        task_view_tasks_subpath,
                        task_view_scan_roots,
                        task_view_scan_exclude,
                        task_view_days,
                        dark,
                    }
                }
                if task_view_open() && feature_task_view() {
                    TaskView {
                        roots,
                        scan_roots: task_view_scan_roots,
                        scan_exclude: task_view_scan_exclude,
                        subpath: task_view_tasks_subpath,
                        default_days: task_view_days,
                        proj_name: current_proj(),
                        dark,
                        favorites,
                        on_toggle_fav: move |p| toggle_fav(p),
                        on_copy_path: move |p: PathBuf| copy_path_native(
                            services::platform::canonical_clipboard_text(p),
                            Some("Copied!".into()),
                        ),
                        on_toast: move |msg| show_toast(msg),
                        on_open_project_menu: move |_| {
                            proj_menu_open.set(true);
                            proj_menu_query.set(String::new());
                        },
                    }
                }
                // Project switcher dropdown. Mounted at the overlay layer (a
                // sibling of the app frame div, after the Task View overlay):
                // the frame div is position:fixed with z-index auto, so anything
                // nested inside it — whatever its own z-index — paints below the
                // Task View overlay. Out here, DOM order + z 1000/1001 win.
                if proj_menu_open() {
                    ProjectMenu {
                        open: proj_menu_open,
                        query: proj_menu_query,
                        candidates: proj_candidates(),
                        current: root(),
                        dark,
                        on_pick: move |path: PathBuf| {
                            proj_menu_open.set(false);
                            if root().as_ref() == Some(&path) {
                                return;
                            }
                            let pick = file_service::pick_markdown(&path);
                            let exp = pick
                                .as_ref()
                                .map(|f| file_service::ancestor_dirs(&path, f))
                                .unwrap_or_default();
                            switch_project(path.clone(), vec![path], exp, pick);
                        },
                        on_open_folder: move |_| {
                            proj_menu_open.set(false);
                            // Async dialog: the sync rfd picker deadlocks the tao
                            // event loop when spun on the main thread.
                            spawn(async move {
                                if let Some(handle) = rfd::AsyncFileDialog::new().pick_folder().await {
                                    let path = handle.path().to_path_buf();
                                    let pick = file_service::pick_markdown(&path);
                                    let exp = pick
                                        .as_ref()
                                        .map(|f| file_service::ancestor_dirs(&path, f))
                                        .unwrap_or_default();
                                    switch_project(path.clone(), vec![path], exp, pick);
                                }
                            });
                        },
                    }
                }
                // Sidebar right-click context menu.
                if let Some(c) = ctx_menu() {
                    {
                        let menu_bg = if dark { "#1c2128" } else { "#ffffff" };
                        let menu_border = if dark { "#30363d" } else { "#d0d7de" };
                        let item_fg = if dark { "#e6edf3" } else { "#1f2328" };
                        let path = c.path.clone();
                        let rel = search::relative_display(&path, root().as_deref());
                        let abs = file_service::canonical_display(&path);
                        // Edit only the stem (the `.md` extension is preserved on
                        // commit), so the user renames the file name, not its type.
                        let cur_name = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let item = "display: block; width: 100%; text-align: left; padding: 6px 12px; border: none; background: transparent; color: inherit; cursor: pointer; border-radius: 5px; font: 13px -apple-system, sans-serif;";
                        let sep = format!("height: 1px; margin: 4px 6px; background: {menu_border};");
                        let is_fav = favorites().iter().any(|p| p == &path);
                        let (p_tab, p_reveal, p_app, p_copy, p_rel, p_ren, p_del, p_fav) = (
                            path.clone(), path.clone(), path.clone(), abs.clone(), rel.clone(),
                            path.clone(), path.clone(), path.clone(),
                        );
                        rsx! {
                            div {
                                style: "position: fixed; inset: 0; z-index: 1000;",
                                onclick: move |_| ctx_menu.set(None),
                                oncontextmenu: move |e| { e.prevent_default(); ctx_menu.set(None); },
                            }
                            div {
                                style: "position: fixed; left: {c.x}px; top: {c.y}px; z-index: 1001; min-width: 210px; background: {menu_bg}; border: 1px solid {menu_border}; border-radius: 8px; padding: 4px; box-shadow: 0 8px 24px rgba(0,0,0,0.3); color: {item_fg};",
                                if !c.is_dir {
                                    button { class: "mdo-ctx-item", style: "{item}",
                                        onclick: move |_| { open_active(p_tab.clone()); ctx_menu.set(None); },
                                        "新規タブで開く" }
                                    div { style: "{sep}" }
                                }
                                button { class: "mdo-ctx-item", style: "{item}",
                                    onclick: move |_| { toggle_fav(p_fav.clone()); ctx_menu.set(None); },
                                    if is_fav { "お気に入りから外す" } else { "お気に入りに追加" } }
                                div { style: "{sep}" }
                                button { class: "mdo-ctx-item", style: "{item}",
                                    onclick: move |_| {
                                        if let Err(err) = services::platform::reveal_in_finder(&p_reveal) {
                                            show_toast(format!("Finder failed: {err}"));
                                        }
                                        ctx_menu.set(None);
                                    },
                                    "Finder で表示" }
                                button { class: "mdo-ctx-item", style: "{item}",
                                    onclick: move |_| {
                                        if let Err(err) = services::platform::open_target(&p_app) {
                                            show_toast(format!("Open failed: {err}"));
                                        }
                                        ctx_menu.set(None);
                                    },
                                    "デフォルトアプリで開く" }
                                div { style: "{sep}" }
                                button { class: "mdo-ctx-item", style: "{item}",
                                    onclick: move |_| {
                                        copy_path_native(p_copy.clone(), Some("Copied!".into()));
                                        ctx_menu.set(None);
                                    },
                                    "パスをコピー" }
                                button { class: "mdo-ctx-item", style: "{item}",
                                    onclick: move |_| {
                                        copy_path_native(p_rel.clone(), Some("Copied!".into()));
                                        ctx_menu.set(None);
                                    },
                                    "相対パスをコピー" }
                                div { style: "{sep}" }
                                button { class: "mdo-ctx-item", style: "{item}",
                                    onclick: move |_| {
                                        rename_buf.set(cur_name.clone());
                                        rename_target.set(Some(p_ren.clone()));
                                        ctx_menu.set(None);
                                    },
                                    "名前を変更" }
                                button { class: "mdo-ctx-item", style: "{item} color: #f85149;",
                                    onclick: move |_| {
                                        let path = p_del.clone();
                                        spawn(async move {
                                            match services::platform::move_to_trash(&path) {
                                                Ok(()) => show_toast("Moved to Trash".into()),
                                                Err(err) => show_toast(format!("Move to Trash failed: {err}")),
                                            }
                                        });
                                        ctx_menu.set(None);
                                    },
                                    "ゴミ箱へ移動" }
                            }
                        }
                    }
                }
                // Transient top-right toast.
                if let Some(msg) = toast() {
                    div {
                        class: "mdo-toast",
                        style: "position: fixed; top: 16px; right: 16px; z-index: 2000; pointer-events: none; background: #238636; color: #fff; padding: 8px 14px; border-radius: 8px; box-shadow: 0 6px 20px rgba(0,0,0,0.35); font: 600 13px -apple-system, sans-serif; display: flex; align-items: center; gap: 8px;",
                        svg { width: "14", height: "14", view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "3", stroke_linecap: "round", stroke_linejoin: "round",
                            path { d: "M20 6L9 17l-5-5" } }
                        "{msg}"
                    }
                }
            }
        }
    }
}
