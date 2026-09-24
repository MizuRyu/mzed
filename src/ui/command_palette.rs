use std::time::Instant;

use super::*;

/// Command palette overlay. Owns its candidate list derivation from the current
/// query + mode, and handles all navigation keys via the input's `onkeydown`
/// (Esc closes, ↑/↓ move selection, Enter runs). Commands dispatch through
/// `on_action`; file-search opens the chosen file in a new tab.
#[component]
pub(crate) fn Palette(
    mut query: Signal<String>,
    mut sel: Signal<usize>,
    mut file_mode: Signal<bool>,
    mut open: Signal<bool>,
    on_open: EventHandler<PathBuf>,
    files: Vec<files::PaletteFile>,
    html_export_on: bool,
    pdf_export_on: bool,
    dark: bool,
    on_action: EventHandler<palette::Action>,
) -> Element {
    let q = query();
    // Candidate rows: either commands or matched file paths. Feature-flagged
    // export commands are hidden when disabled.
    // Recomputed on every render; the palette re-opens per use, so the share
    // label always reflects whether the server is currently running.
    let share_url = crate::serve::app_share_url();
    let cmd_rows: Vec<palette::Command> = palette::filter_commands(&q, share_url.as_deref())
        .into_iter()
        .filter(|c| match c.action {
            palette::Action::ExportHtml => html_export_on,
            palette::Action::ExportPdf => pdf_export_on,
            _ => true,
        })
        .collect();
    // Ranking reads only the query and the candidate set, so moving the
    // selection (arrow keys, hover) redraws without ranking every file again.
    let ranked = use_memo(use_reactive((&files,), move |(files,)| {
        if file_mode() {
            rank_files(&query(), &files)
        } else {
            FileRows::default()
        }
    }));
    let ranked = ranked.read();
    let file_rows = &ranked.rows;
    let hidden = ranked.hidden;

    let len = if file_mode() {
        file_rows.len()
    } else {
        cmd_rows.len()
    };
    let cur = if len == 0 { 0 } else { sel().min(len - 1) };

    // Hover and the keyboard drive the same `sel`; these two keep them apart.
    let mut sel_src = use_signal(|| SelChange::Keyboard);
    let mut last_key_at = use_signal(|| None::<Instant>);

    // Keep the selected row in view: on every selection change, scroll the row
    // with the matching data-mdo-row into the candidate list's visible range.
    use_effect(move || {
        let i = sel();
        if sel_src() == SelChange::Hover {
            return;
        }
        spawn(async move {
            let script = js::overlay_row_scroll_js(js::OverlayRowKind::Command, i);
            let _ = document::eval(&script).recv::<()>().await;
        });
    });

    let overlay_bg = if dark { "#161b22" } else { "#ffffff" };
    let overlay_border = if dark { "#30363d" } else { "#d0d7de" };
    let text_color = if dark { "#e6edf3" } else { "#1f2328" };
    let muted = if dark { "#8b949e" } else { "#57606a" };
    let sel_bg = if dark { "#1f6feb" } else { "#0969da" };

    // Commit the current selection.
    let file_for_enter = file_rows.get(cur).map(|f| f.path.clone());
    let cmd_for_enter = cmd_rows.get(cur).map(|c| c.action);
    let mut commit = move || {
        if file_mode() {
            if let Some(path) = file_for_enter.clone() {
                on_open.call(path);
                open.set(false);
            }
        } else if let Some(action) = cmd_for_enter {
            on_action.call(action);
        }
    };

    rsx! {
        // Click-away backdrop.
        div {
            style: "position: fixed; inset: 0; background: rgba(0,0,0,0.25); display: flex; justify-content: center; align-items: flex-start; z-index: 1000;",
            onclick: move |_| open.set(false),
            div {
                style: "margin-top: 12vh; width: 560px; max-width: 90vw; background: {overlay_bg}; border: 1px solid {overlay_border}; border-radius: 10px; box-shadow: 0 12px 40px rgba(0,0,0,0.35); overflow: hidden; color: {text_color};",
                // Stop backdrop click from closing when interacting with the box.
                onclick: move |e| e.stop_propagation(),
                input {
                    autofocus: true,
                    // autofocus is unreliable in the webview; force focus on mount
                    // so arrow/Enter/Esc reach the input immediately.
                    onmounted: move |e| {
                        spawn(async move {
                            let _ = e.set_focus(true).await;
                        });
                    },
                    value: "{q}",
                    placeholder: if file_mode() { "Search files…" } else { "Type a command…" },
                    style: "width: 100%; box-sizing: border-box; padding: 14px 16px; font: 15px -apple-system, sans-serif; border: none; border-bottom: 1px solid {overlay_border}; background: transparent; color: {text_color}; outline: none;",
                    oninput: move |e| {
                        last_key_at.set(Some(Instant::now()));
                        query.set(e.value());
                        sel_src.set(SelChange::Keyboard);
                        sel.set(0);
                    },
                    onkeydown: move |e| {
                        last_key_at.set(Some(Instant::now()));
                        match e.key() {
                            Key::Escape => {
                                e.prevent_default();
                                // Esc backs out of file mode first, else closes.
                                if file_mode() {
                                    file_mode.set(false);
                                    query.set(String::new());
                                    sel_src.set(SelChange::Keyboard);
                                    sel.set(0);
                                } else {
                                    open.set(false);
                                }
                            }
                            Key::ArrowDown => {
                                e.prevent_default();
                                if len > 0 {
                                    sel_src.set(SelChange::Keyboard);
                                    sel.set((cur + 1) % len);
                                }
                            }
                            Key::ArrowUp => {
                                e.prevent_default();
                                if len > 0 {
                                    sel_src.set(SelChange::Keyboard);
                                    sel.set((cur + len - 1) % len);
                                }
                            }
                            Key::Enter => {
                                e.prevent_default();
                                commit();
                            }
                            _ => {}
                        }
                    },
                }
                div {
                    "data-mdo-scroll": "palette",
                    style: "max-height: 50vh; overflow: auto; padding: 6px;",
                    if file_mode() {
                        for (i, f) in file_rows.iter().enumerate() {
                            {
                                let name = f.name.clone();
                                let dir = f.dir().to_string();
                                let bg = if i == cur { sel_bg } else { "transparent" };
                                let fg = if i == cur { "#ffffff" } else { text_color };
                                let dir_fg = if i == cur { "#ffffffcc" } else { muted };
                                let unread = f.unread;
                                let pick = f.path.clone();
                                rsx! {
                                    div {
                                        "data-mdo-row": "{i}",
                                        style: "display: flex; align-items: baseline; gap: 8px; padding: 8px 12px; border-radius: 6px; cursor: pointer; background: {bg}; color: {fg}; font: 14px -apple-system, sans-serif;",
                                        onmouseenter: move |_| {
                                            if hover_takes_selection(i, cur, (*last_key_at.peek()).map(|t| t.elapsed())) {
                                                sel_src.set(SelChange::Hover);
                                                sel.set(i);
                                            }
                                        },
                                        onclick: move |_| {
                                            on_open.call(pick.clone());
                                            open.set(false);
                                        },
                                        if unread {
                                            {unread_dot(dark)}
                                        }
                                        span { style: "flex: 0 0 auto;", "{name}" }
                                        span {
                                            style: "min-width: 0; flex: 1 1 auto; font-size: 12px; color: {dir_fg}; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                            "{dir}"
                                        }
                                    }
                                }
                            }
                        }
                        if hidden > 0 {
                            div {
                                style: "padding: 8px 12px; color: {muted}; font: 12px -apple-system, sans-serif;",
                                "他 {hidden} 件。絞り込んでください"
                            }
                        }
                    } else {
                        for (i, c) in cmd_rows.iter().enumerate() {
                            {
                                let bg = if i == cur { sel_bg } else { "transparent" };
                                let fg = if i == cur { "#ffffff" } else { text_color };
                                let action = c.action;
                                rsx! {
                                    div {
                                        "data-mdo-row": "{i}",
                                        style: "padding: 8px 12px; border-radius: 6px; cursor: pointer; background: {bg}; color: {fg}; font: 14px -apple-system, sans-serif;",
                                        onmouseenter: move |_| {
                                            if hover_takes_selection(i, cur, (*last_key_at.peek()).map(|t| t.elapsed())) {
                                                sel_src.set(SelChange::Hover);
                                                sel.set(i);
                                            }
                                        },
                                        onclick: move |_| on_action.call(action),
                                        "{c.label}"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// File-search rows actually drawn. why: a DOM row per match stalls every
/// keystroke on a large project, and past a couple hundred rows the query is too
/// loose to scan by eye; arrow keys stay inside these rows too. An exact name
/// match is always drawn: typing the whole name must reach the file even when
/// unread partial matches fill the list ahead of it.
const FILE_ROW_LIMIT: usize = 200;

/// The drawn rows and how many matches were left out.
#[derive(Clone, Default, PartialEq)]
struct FileRows {
    rows: Vec<files::PaletteFile>,
    hidden: usize,
}

fn rank_files(query: &str, files: &[files::PaletteFile]) -> FileRows {
    let mut ranked = fuzzy::rank_tiered(query, files, |f| f.keys(), |f| f.mtime);
    // Unread outranks every tier; the sort is stable, so the tiered order
    // survives inside the read group.
    ranked.sort_by_key(|f| !f.unread);
    sort_unread_by_recency(&mut ranked);

    // Same test as `fuzzy`'s exact tier: some key equals the query, ignoring case.
    let q = query.trim().to_lowercase();
    let exact =
        |f: &files::PaletteFile| !q.is_empty() && f.keys().iter().any(|k| k.to_lowercase() == q);
    let exact_count = ranked.iter().filter(|f| exact(f)).count();
    let mut exact_room = exact_count.min(FILE_ROW_LIMIT);
    let mut other_room = FILE_ROW_LIMIT - exact_room;
    let rows: Vec<files::PaletteFile> = ranked
        .iter()
        .filter(|f| {
            let room = if exact(f) {
                &mut exact_room
            } else {
                &mut other_room
            };
            let keep = *room > 0;
            *room = room.saturating_sub(1);
            keep
        })
        .map(|f| (*f).clone())
        .collect();
    FileRows {
        hidden: ranked.len() - rows.len(),
        rows,
    }
}

/// Re-sort the unread prefix of an already `!f.unread`-partitioned list by
/// mtime descending, leaving the read suffix (still in match-tier order) and
/// tie order (stable) untouched.
fn sort_unread_by_recency(ranked: &mut [&files::PaletteFile]) {
    let unread_len = ranked.iter().take_while(|f| f.unread).count();
    ranked[..unread_len].sort_by_key(|f| std::cmp::Reverse(f.mtime));
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    fn file(name: &str, mtime: u64, unread: bool) -> files::PaletteFile {
        files::PaletteFile {
            path: PathBuf::from(name),
            rel: name.to_string(),
            name: name.to_string(),
            mtime,
            unread,
        }
    }

    #[test]
    fn 未読グループだけmtime降順に並び既読は元の順を保つ() {
        let a = file("a.md", 100, true);
        let b = file("b.md", 300, true);
        let c = file("c.md", 200, true);
        let d = file("d.md", 999, false);
        let e = file("e.md", 1, false);
        // 既読グループ(d, e)は一致順(呼び出し前の順)のまま、mtimeでは並ばない。
        let mut ranked = vec![&a, &b, &c, &d, &e];
        sort_unread_by_recency(&mut ranked);
        let names: Vec<&str> = ranked.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["b.md", "c.md", "a.md", "d.md", "e.md"]);
    }

    #[test]
    fn 上限を超えた候補は描画せず件数だけ返す() {
        let many: Vec<_> = (0..FILE_ROW_LIMIT as u64 + 50)
            .map(|i| file(&format!("f{i}.md"), i, false))
            .collect();
        let ranked = rank_files("", &many);
        assert_eq!(ranked.rows.len(), FILE_ROW_LIMIT);
        assert_eq!(ranked.hidden, 50);
        // 空クエリは新しい順なので、残すのは mtime の大きい側。
        assert_eq!(ranked.rows[0].name, format!("f{}.md", FILE_ROW_LIMIT + 49));
    }

    #[test]
    fn 上限の外に落ちる完全一致も描画に残す() {
        // 未読の部分一致 200 件が既読の完全一致より上に並ぶ。
        let mut all: Vec<_> = (0..FILE_ROW_LIMIT as u64)
            .map(|i| file(&format!("plan-{i}.md"), i, true))
            .collect();
        all.push(file("plan.md", 0, false));
        let ranked = rank_files("plan", &all);
        assert_eq!(ranked.rows.len(), FILE_ROW_LIMIT);
        assert_eq!(ranked.hidden, 1);
        // 押し出されるのは末尾の部分一致で、完全一致は最後の行（↑ で選べる位置）に残る。
        assert_eq!(ranked.rows.last().unwrap().name, "plan.md");
        assert!(ranked.rows.iter().all(|f| f.name != "plan-0.md"));
    }

    #[test]
    fn 上限以内なら全件描画し残りは0() {
        let few = vec![file("a.md", 1, false), file("b.md", 2, true)];
        let ranked = rank_files("", &few);
        let names: Vec<&str> = ranked.rows.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["b.md", "a.md"]);
        assert_eq!(ranked.hidden, 0);
    }

    #[test]
    fn 未読が無ければ何もしない() {
        let d = file("d.md", 999, false);
        let e = file("e.md", 1, false);
        let mut ranked = vec![&d, &e];
        sort_unread_by_recency(&mut ranked);
        let names: Vec<&str> = ranked.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["d.md", "e.md"]);
    }
}
