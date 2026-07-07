use super::*;
/// Category in the two-pane settings modal (left-nav selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    General,
    Appearance,
    Features,
    Sync,
    Hotkeys,
}
/// Settings modal (Cmd+,). An Obsidian-style two-pane overlay: a left category
/// nav (Appearance / Sync / Hotkeys) and a right pane of setting rows. Each row
/// is a bold title + muted description on the left and a control on the right.
/// Controls write straight to the App's signals; persistence happens in the
/// App's config `use_effect`. Esc / backdrop click / the close button dismiss it.
#[component]
#[allow(clippy::useless_format)]
pub(crate) fn Settings(
    mut open: Signal<bool>,
    mut tab: Signal<SettingsTab>,
    mut theme: Signal<theme::Theme>,
    mut sync_mode: Signal<theme::SyncMode>,
    mut persisted_sync_mode: Signal<theme::SyncMode>,
    mut zoom: Signal<f32>,
    mut win_w: Signal<i32>,
    mut win_h: Signal<i32>,
    mut startup_behavior: Signal<config::StartupBehavior>,
    mut sidebar_default: Signal<bool>,
    mut external_links_in_browser: Signal<bool>,
    mut code_font: Signal<String>,
    mut code_font_size: Signal<i32>,
    mut line_height: Signal<f32>,
    mut keymap: Signal<Vec<config::KeyBinding>>,
    mut export_dir_sig: Signal<Option<PathBuf>>,
    mut feature_katex: Signal<bool>,
    mut feature_html_export: Signal<bool>,
    mut feature_pdf_export: Signal<bool>,
    mut open_latest_on_project_open: Signal<bool>,
    mut feature_task_view: Signal<bool>,
    mut task_view_tasks_subpath: Signal<String>,
    mut task_view_scan_roots: Signal<Vec<PathBuf>>,
    mut task_view_days: Signal<u32>,
    dark: bool,
) -> Element {
    let win = dioxus::desktop::use_window();
    // Index of the keybinding row currently capturing a keystroke.
    let mut recording = use_signal(|| None::<usize>);
    let overlay_bg = if dark { "#1e1e1e" } else { "#ffffff" };
    let overlay_border = if dark { "#30363d" } else { "#d0d7de" };
    let nav_bg = if dark { "#161b22" } else { "#f6f8fa" };
    let text_color = if dark { "#e6edf3" } else { "#1f2328" };
    let muted = if dark { "#8b949e" } else { "#57606a" };
    let nav_active_bg = if dark { "#1f6feb33" } else { "#0969da1a" };
    let btn_border = if dark { "#30363d" } else { "#d0d7de" };
    let row_border = if dark { "#21262d" } else { "#eaecef" };

    let cur_tab = tab();
    let cur_theme = theme();
    let cur_sync = sync_mode();
    let zoom_pct = (zoom() * 100.0).round() as i32;

    // Left-nav item style: highlighted when it is the current category.
    let nav_item = move |active: bool| {
        if active {
            format!(
                "padding: 8px 12px; border-radius: 6px; cursor: pointer; font: 600 13px -apple-system, sans-serif; color: {text_color}; background: {nav_active_bg};"
            )
        } else {
            format!(
                "padding: 8px 12px; border-radius: 6px; cursor: pointer; font: 13px -apple-system, sans-serif; color: {muted}; background: transparent;"
            )
        }
    };

    // Dropdown style. `appearance: none` strips the native macOS 3D bezel; the
    // `.mdo-select` class adds a flat caret. Only theme colours stay inline.
    let select_style = format!(
        "-webkit-appearance: none; appearance: none; padding: 6px 30px 6px 10px; border: 1px solid {btn_border}; background-color: {overlay_bg}; color: {text_color}; border-radius: 6px; cursor: pointer; font: 13px -apple-system, sans-serif; min-width: 130px;"
    );
    let step_btn = format!(
        "width: 28px; height: 28px; border: 1px solid {btn_border}; background: transparent; color: {text_color}; border-radius: 6px; cursor: pointer; font: 15px -apple-system, sans-serif;"
    );
    let reset_btn = format!(
        "padding: 5px 12px; border: 1px solid {btn_border}; background: transparent; color: {text_color}; border-radius: 6px; cursor: pointer; font: 13px -apple-system, sans-serif;"
    );

    // A single Obsidian-style setting row: title + description left, control right.
    let row = "display: flex; justify-content: space-between; align-items: center; gap: 24px; padding: 14px 0;";
    let row_title = "font: 600 14px -apple-system, sans-serif;";
    let row_desc =
        format!("font: 12px -apple-system, sans-serif; color: {muted}; margin-top: 2px;");

    rsx! {
        div {
            style: "position: fixed; inset: 0; background: rgba(0,0,0,0.35); display: flex; justify-content: center; align-items: center; z-index: 1000;",
            onclick: move |_| open.set(false),
            div {
                style: "width: 880px; height: 600px; max-width: 94vw; max-height: 90vh; display: flex; background: {overlay_bg}; border: 1px solid {overlay_border}; border-radius: 12px; box-shadow: 0 16px 50px rgba(0,0,0,0.45); overflow: hidden; color: {text_color};",
                onclick: move |e| e.stop_propagation(),

                // Left category nav.
                div {
                    style: "width: 200px; flex: 0 0 auto; background: {nav_bg}; border-right: 1px solid {overlay_border}; padding: 16px 10px; display: flex; flex-direction: column; gap: 2px;",
                    div {
                        style: "font: 600 11px -apple-system, sans-serif; color: {muted}; text-transform: uppercase; letter-spacing: 0.5px; padding: 0 12px 8px;",
                        "Settings"
                    }
                    div { style: nav_item(cur_tab == SettingsTab::General), onclick: move |_| tab.set(SettingsTab::General), "一般 (General)" }
                    div { style: nav_item(cur_tab == SettingsTab::Appearance), onclick: move |_| tab.set(SettingsTab::Appearance), "外観 (Appearance)" }
                    div { style: nav_item(cur_tab == SettingsTab::Features), onclick: move |_| tab.set(SettingsTab::Features), "機能 (Features)" }
                    div { style: nav_item(cur_tab == SettingsTab::Sync), onclick: move |_| tab.set(SettingsTab::Sync), "Zed 連動 (Sync)" }
                    div { style: nav_item(cur_tab == SettingsTab::Hotkeys), onclick: move |_| tab.set(SettingsTab::Hotkeys), "ショートカット (Hotkeys)" }
                }

                // Right content pane.
                div {
                    style: "flex: 1 1 auto; min-width: 0; display: flex; flex-direction: column;",
                    // Header with close button.
                    div {
                        style: "display: flex; justify-content: space-between; align-items: center; padding: 16px 24px; border-bottom: 1px solid {overlay_border};",
                        span {
                            style: "font: 600 17px -apple-system, sans-serif;",
                            {match cur_tab {
                                SettingsTab::General => "一般",
                                SettingsTab::Appearance => "外観",
                                SettingsTab::Features => "機能",
                                SettingsTab::Sync => "Zed 連動",
                                SettingsTab::Hotkeys => "ショートカット",
                            }}
                        }
                        button {
                            style: "border: none; background: transparent; color: {muted}; cursor: pointer; font-size: 22px; line-height: 1; padding: 2px 6px; border-radius: 4px;",
                            onclick: move |_| open.set(false),
                            "×"
                        }
                    }
                    div {
                        style: "flex: 1 1 auto; overflow: auto; padding: 8px 24px 24px;",
                        match cur_tab {
                            SettingsTab::General => {
                                let num_input = "width: 84px; box-sizing: border-box; -webkit-appearance: none; appearance: none; padding: 6px 8px; border: 1px solid ".to_string() + btn_border + "; border-radius: 6px; background: transparent; color: " + text_color + "; font: 13px -apple-system, sans-serif; outline: none;";
                                rsx! {
                                    // Default window size + Use Current.
                                    div {
                                        style: "{row} border-bottom: 1px solid {row_border};",
                                        div {
                                            div { style: row_title, "デフォルトウィンドウサイズ" }
                                            div { style: "{row_desc}", "次回起動時の幅 × 高さ（px）" }
                                        }
                                        div {
                                            style: "display: flex; gap: 8px; align-items: center;",
                                            input {
                                                r#type: "number", style: "{num_input}", value: "{win_w}",
                                                oninput: move |e| { if let Ok(v) = e.value().parse::<i32>() { win_w.set(v.clamp(360, 6000)); } },
                                            }
                                            span { style: "color: {muted};", "×" }
                                            input {
                                                r#type: "number", style: "{num_input}", value: "{win_h}",
                                                oninput: move |e| { if let Ok(v) = e.value().parse::<i32>() { win_h.set(v.clamp(240, 4000)); } },
                                            }
                                            button {
                                                style: "{reset_btn}",
                                                onclick: move |_| {
                                                    let sz = win.inner_size();
                                                    let sf = win.scale_factor().max(0.1);
                                                    win_w.set(((sz.width as f64 / sf).round() as i32).max(360));
                                                    win_h.set(((sz.height as f64 / sf).round() as i32).max(240));
                                                },
                                                "現在のサイズを使う"
                                            }
                                        }
                                    }
                                    // Startup behavior.
                                    div {
                                        style: "{row} border-bottom: 1px solid {row_border};",
                                        div {
                                            div { style: row_title, "起動時の表示" }
                                            div { style: "{row_desc}", "ファイル/フォルダ指定がない場合" }
                                        }
                                        select {
                                            class: "mdo-select", style: "{select_style}",
                                            onchange: move |e| {
                                                startup_behavior.set(match e.value().as_str() {
                                                    "docs" => config::StartupBehavior::Docs,
                                                    "blank" => config::StartupBehavior::Blank,
                                                    _ => config::StartupBehavior::Restore,
                                                });
                                            },
                                            option { value: "restore", selected: startup_behavior() == config::StartupBehavior::Restore, "前回のセッションを復元" }
                                            option { value: "docs", selected: startup_behavior() == config::StartupBehavior::Docs, "Zed 連動の docs を開く" }
                                            option { value: "blank", selected: startup_behavior() == config::StartupBehavior::Blank, "空で開く" }
                                        }
                                    }
                                    // Sidebar default.
                                    div {
                                        style: "{row} border-bottom: 1px solid {row_border};",
                                        div {
                                            div { style: row_title, "サイドバーを起動時に表示" }
                                            div { style: "{row_desc}", "オフなら畳んだ状態で起動" }
                                        }
                                        input {
                                            r#type: "checkbox", checked: sidebar_default(),
                                            style: "width: 16px; height: 16px; cursor: pointer;",
                                            onchange: move |e| sidebar_default.set(e.value() == "true"),
                                        }
                                    }
                                    // External links.
                                    div {
                                        style: "{row} border-bottom: 1px solid {row_border};",
                                        div {
                                            div { style: row_title, "外部リンクをブラウザで開く" }
                                            div { style: "{row_desc}", "http/https リンクを既定ブラウザで開く" }
                                        }
                                        input {
                                            r#type: "checkbox", checked: external_links_in_browser(),
                                            style: "width: 16px; height: 16px; cursor: pointer;",
                                            onchange: move |e| external_links_in_browser.set(e.value() == "true"),
                                        }
                                    }
                                    // Auto-open latest file on project open.
                                    div {
                                        style: "{row} border-bottom: 1px solid {row_border};",
                                        div {
                                            div { style: row_title, "プロジェクトを開いたとき最新ファイルを自動で開く" }
                                            div { style: "{row_desc}", "復元タブがない場合に、最終更新日時が最も新しい Markdown を自動で開く" }
                                        }
                                        input {
                                            r#type: "checkbox", checked: open_latest_on_project_open(),
                                            style: "width: 16px; height: 16px; cursor: pointer;",
                                            onchange: move |e| open_latest_on_project_open.set(e.value() == "true"),
                                        }
                                    }
                                    // Export folder.
                                    {
                                        let cur_dir = export_dir(&export_dir_sig());
                                        let dir_label = cur_dir.display().to_string();
                                        rsx! {
                                            div {
                                                style: "{row}",
                                                div {
                                                    div { style: row_title, "エクスポート先フォルダ" }
                                                    div { style: "{row_desc}", "HTML/PDF の出力先（既定: Downloads）" }
                                                }
                                                div {
                                                    style: "display: flex; gap: 8px; align-items: center; max-width: 320px;",
                                                    span {
                                                        style: "font: 12px ui-monospace, monospace; color: {muted}; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; direction: rtl;",
                                                        title: "{dir_label}",
                                                        "{dir_label}"
                                                    }
                                                    button {
                                                        style: "{reset_btn}",
                                                        onclick: move |_| {
                                                            // Async dialog: the sync rfd picker deadlocks the
                                                            // tao event loop when spun on the main thread.
                                                            spawn(async move {
                                                                if let Some(h) = rfd::AsyncFileDialog::new().pick_folder().await {
                                                                    export_dir_sig.set(Some(h.path().to_path_buf()));
                                                                }
                                                            });
                                                        },
                                                        "選択"
                                                    }
                                                    button {
                                                        style: "{reset_btn}",
                                                        title: "Downloads に戻す",
                                                        onclick: move |_| export_dir_sig.set(None),
                                                        "↺"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                            SettingsTab::Features => {
                                rsx! {
                                    div {
                                        style: "{row_desc} padding-bottom: 10px;",
                                        "拡張機能的な処理のオンオフ。重い機能を切ると軽くなります。"
                                    }
                                    div {
                                        style: "{row} border-bottom: 1px solid {row_border};",
                                        div {
                                            div { style: row_title, "KaTeX 数式" }
                                            div { style: "{row_desc}", "$...$ / $$...$$ の数式描画" }
                                        }
                                        input {
                                            r#type: "checkbox", checked: feature_katex(),
                                            style: "width: 16px; height: 16px; cursor: pointer;",
                                            onchange: move |e| feature_katex.set(e.value() == "true"),
                                        }
                                    }
                                    div {
                                        style: "{row} border-bottom: 1px solid {row_border};",
                                        div {
                                            div { style: row_title, "HTML エクスポート" }
                                            div { style: "{row_desc}", "コマンドパレットから HTML 出力" }
                                        }
                                        input {
                                            r#type: "checkbox", checked: feature_html_export(),
                                            style: "width: 16px; height: 16px; cursor: pointer;",
                                            onchange: move |e| feature_html_export.set(e.value() == "true"),
                                        }
                                    }
                                    div {
                                        style: "{row} border-bottom: 1px solid {row_border};",
                                        div {
                                            div { style: row_title, "PDF エクスポート" }
                                            div { style: "{row_desc}", "コマンドパレットから PDF 出力（印刷ダイアログ）" }
                                        }
                                        input {
                                            r#type: "checkbox", checked: feature_pdf_export(),
                                            style: "width: 16px; height: 16px; cursor: pointer;",
                                            onchange: move |e| feature_pdf_export.set(e.value() == "true"),
                                        }
                                    }
                                    // ── Task View ────────────────────────────────────────
                                    div {
                                        style: "font: 600 12px -apple-system, sans-serif; color: {muted}; \
                                                text-transform: uppercase; letter-spacing: 0.4px; \
                                                padding: 14px 0 4px;",
                                        "Task View (Cmd+Shift+D)"
                                    }
                                    div {
                                        style: "{row} border-bottom: 1px solid {row_border};",
                                        div {
                                            div { style: row_title, "Task View を有効にする" }
                                            div { style: "{row_desc}", "Cmd+Shift+D でタスク一覧モードを開く（個人向け実験機能）" }
                                        }
                                        input {
                                            r#type: "checkbox", checked: feature_task_view(),
                                            style: "width: 16px; height: 16px; cursor: pointer;",
                                            onchange: move |e| feature_task_view.set(e.value() == "true"),
                                        }
                                    }
                                    div {
                                        style: "{row} border-bottom: 1px solid {row_border};",
                                        div {
                                            div { style: row_title, "タスクフォルダのサブパス" }
                                            div { style: "{row_desc}", "プロジェクトルートからの相対パス（既定: docs/memo/tasks）" }
                                        }
                                        input {
                                            r#type: "text",
                                            value: "{task_view_tasks_subpath}",
                                            placeholder: "docs/memo/tasks",
                                            style: "width: 200px; box-sizing: border-box; -webkit-appearance: none; \
                                                    padding: 6px 10px; border: 1px solid {btn_border}; \
                                                    border-radius: 6px; background: transparent; \
                                                    color: {text_color}; font: 13px -apple-system, sans-serif; outline: none;",
                                            oninput: move |e| task_view_tasks_subpath.set(e.value()),
                                        }
                                    }
                                    div {
                                        style: "{row} border-bottom: 1px solid {row_border};",
                                        div {
                                            div { style: row_title, "All Projects スキャンルート" }
                                            div { style: "{row_desc}", "プロジェクトを探す親ディレクトリ（空なら現プロジェクトのみ）" }
                                        }
                                        div {
                                            style: "display: flex; flex-direction: column; gap: 6px; max-width: 300px;",
                                            for (i, root_path) in task_view_scan_roots().iter().enumerate() {
                                                {
                                                    let display = root_path.display().to_string();
                                                    let remove_idx = i;
                                                    rsx! {
                                                        div {
                                                            key: "{display}",
                                                            style: "display: flex; align-items: center; gap: 6px;",
                                                            span {
                                                                style: "font: 11px ui-monospace, monospace; color: {muted}; \
                                                                        overflow: hidden; text-overflow: ellipsis; white-space: nowrap; \
                                                                        flex: 1 1 auto; direction: rtl;",
                                                                title: "{display}",
                                                                "{display}"
                                                            }
                                                            button {
                                                                style: "background: transparent; border: none; color: {muted}; \
                                                                        cursor: pointer; font-size: 14px; flex: 0 0 auto; padding: 0 4px;",
                                                                title: "削除",
                                                                onclick: move |_| {
                                                                    let mut roots = task_view_scan_roots.write();
                                                                    if remove_idx < roots.len() {
                                                                        roots.remove(remove_idx);
                                                                    }
                                                                },
                                                                "✕"
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            button {
                                                style: "{reset_btn}",
                                                onclick: move |_| {
                                                    spawn(async move {
                                                        if let Some(h) = rfd::AsyncFileDialog::new().pick_folder().await {
                                                            task_view_scan_roots.write().push(h.path().to_path_buf());
                                                        }
                                                    });
                                                },
                                                "+ ディレクトリを追加"
                                            }
                                        }
                                    }
                                    div {
                                        style: "{row}",
                                        div {
                                            div { style: row_title, "All Projects 表示日数" }
                                            div { style: "{row_desc}", "直近 N 日に作成したタスクを表示（既定: 7）" }
                                        }
                                        div {
                                            style: "display: flex; gap: 8px; align-items: center;",
                                            button {
                                                style: "{step_btn}",
                                                onclick: move |_| task_view_days.set(task_view_days().saturating_sub(1).max(1)),
                                                "−"
                                            }
                                            span {
                                                style: "min-width: 44px; text-align: center; \
                                                        font: 14px -apple-system, sans-serif; \
                                                        font-variant-numeric: tabular-nums;",
                                                "{task_view_days()} 日"
                                            }
                                            button {
                                                style: "{step_btn}",
                                                onclick: move |_| task_view_days.set(task_view_days().saturating_add(1).min(365)),
                                                "+"
                                            }
                                            button {
                                                style: "{reset_btn}",
                                                onclick: move |_| task_view_days.set(7),
                                                "リセット"
                                            }
                                        }
                                    }
                                }
                            },
                            SettingsTab::Appearance => rsx! {
                                // Base theme.
                                div {
                                    style: "{row} border-bottom: 1px solid {row_border};",
                                    div {
                                        div { style: row_title, "ベーステーマ" }
                                        div { style: "{row_desc}", "配色を選択" }
                                    }
                                    select {
                                        class: "mdo-select",
                                        style: "{select_style}",
                                        onchange: move |e| {
                                            match e.value().as_str() {
                                                "light" => theme.set(theme::Theme::Light),
                                                "dark" => theme.set(theme::Theme::Dark),
                                                _ => theme.set(theme::Theme::System),
                                            }
                                        },
                                        option { value: "light", selected: cur_theme == theme::Theme::Light, "Light" }
                                        option { value: "dark", selected: cur_theme == theme::Theme::Dark, "Dark" }
                                        option { value: "system", selected: cur_theme == theme::Theme::System, "System" }
                                    }
                                }
                                // Font size.
                                div {
                                    style: "{row}",
                                    div {
                                        div { style: row_title, "文字サイズ" }
                                        div { style: "{row_desc}", "本文の拡大率" }
                                    }
                                    div {
                                        style: "display: flex; gap: 8px; align-items: center;",
                                        button { style: "{step_btn}", onclick: move |_| zoom.set(theme::zoom_out(zoom())), "−" }
                                        span {
                                            style: "min-width: 52px; text-align: center; font: 14px -apple-system, sans-serif; font-variant-numeric: tabular-nums;",
                                            "{zoom_pct}%"
                                        }
                                        button { style: "{step_btn}", onclick: move |_| zoom.set(theme::zoom_in(zoom())), "+" }
                                        button { style: "{reset_btn}", onclick: move |_| zoom.set(theme::ZOOM_DEFAULT), "リセット" }
                                    }
                                }
                                // Code-block font family.
                                div {
                                    style: "{row} border-top: 1px solid {row_border};",
                                    div {
                                        div { style: row_title, "コードフォント" }
                                        div { style: "{row_desc}", "コードブロックの等幅フォント（空で既定）" }
                                    }
                                    input {
                                        r#type: "text",
                                        value: "{code_font}",
                                        placeholder: "例: JetBrains Mono",
                                        style: "width: 200px; box-sizing: border-box; -webkit-appearance: none; padding: 6px 10px; border: 1px solid {btn_border}; border-radius: 6px; background: transparent; color: {text_color}; font: 13px -apple-system, sans-serif; outline: none;",
                                        oninput: move |e| code_font.set(e.value()),
                                    }
                                }
                                // Code-block font size.
                                div {
                                    style: "{row}",
                                    div {
                                        div { style: row_title, "コードフォントサイズ" }
                                        div { style: "{row_desc}", "コードブロックの文字サイズ（px）" }
                                    }
                                    div {
                                        style: "display: flex; gap: 8px; align-items: center;",
                                        button { style: "{step_btn}", onclick: move |_| code_font_size.set((code_font_size() - 1).clamp(8, 32)), "−" }
                                        span {
                                            style: "min-width: 44px; text-align: center; font: 14px -apple-system, sans-serif; font-variant-numeric: tabular-nums;",
                                            "{code_font_size}px"
                                        }
                                        button { style: "{step_btn}", onclick: move |_| code_font_size.set((code_font_size() + 1).clamp(8, 32)), "+" }
                                        button { style: "{reset_btn}", onclick: move |_| code_font_size.set(14), "リセット" }
                                    }
                                }
                                // Body line-height.
                                div {
                                    style: "{row} border-top: 1px solid {row_border};",
                                    div {
                                        div { style: row_title, "行間" }
                                        div { style: "{row_desc}", "本文の行間（1.2〜2.4、既定 1.7）" }
                                    }
                                    div {
                                        style: "display: flex; gap: 8px; align-items: center;",
                                        button {
                                            style: "{step_btn}",
                                            onclick: move |_| {
                                                let v = ((line_height() - 0.1) * 10.0).round() / 10.0;
                                                line_height.set(v.clamp(1.2, 2.4));
                                            },
                                            "−"
                                        }
                                        span {
                                            style: "min-width: 44px; text-align: center; font: 14px -apple-system, sans-serif; font-variant-numeric: tabular-nums;",
                                            { format!("{:.1}", line_height()) }
                                        }
                                        button {
                                            style: "{step_btn}",
                                            onclick: move |_| {
                                                let v = ((line_height() + 0.1) * 10.0).round() / 10.0;
                                                line_height.set(v.clamp(1.2, 2.4));
                                            },
                                            "+"
                                        }
                                        button { style: "{reset_btn}", onclick: move |_| line_height.set(1.7), "リセット" }
                                    }
                                }
                            },
                            SettingsTab::Sync => rsx! {
                                div {
                                    style: "{row}",
                                    div {
                                        div { style: row_title, "連動モード" }
                                        div { style: "{row_desc}", "Zed のプロジェクト切替への追従" }
                                    }
                                    select {
                                        class: "mdo-select",
                                        style: "{select_style}",
                                        onchange: move |e| {
                                            let next = match e.value().as_str() {
                                                "self" => theme::SyncMode::SelfPinned,
                                                "off" => theme::SyncMode::Off,
                                                _ => theme::SyncMode::Auto,
                                            };
                                            sync_mode.set(next);
                                            persisted_sync_mode.set(next);
                                        },
                                        option { value: "auto", selected: cur_sync == theme::SyncMode::Auto, "Auto" }
                                        option { value: "self", selected: cur_sync == theme::SyncMode::SelfPinned, "Self" }
                                        option { value: "off", selected: cur_sync == theme::SyncMode::Off, "Off" }
                                    }
                                }
                            },
                            SettingsTab::Hotkeys => {
                                let kbd = format!("font: 12px ui-monospace, SFMono-Regular, monospace; color: {text_color}; border: 1px solid {btn_border}; border-radius: 5px; padding: 4px 10px; min-width: 64px; text-align: center; background: transparent; cursor: pointer; white-space: nowrap;");
                                let recording_style = format!("font: 12px ui-monospace, monospace; color: #fff; border: 1px solid #1f6feb; border-radius: 5px; padding: 4px 10px; min-width: 64px; text-align: center; background: #1f6feb; outline: none; width: 90px; box-sizing: border-box;");
                                let reset_one = "background: transparent; border: none; color: ".to_string() + muted + "; cursor: pointer; font-size: 14px; padding: 2px 4px;";
                                rsx! {
                                    div {
                                        style: "{row_desc} padding-bottom: 10px;",
                                        "右の割当をクリック → 新しいキーを押すと変更。Esc でキャンセル、↺ で既定に戻す。ズーム/タブ移動/Enter は固定です。"
                                    }
                                    for (i, b) in keymap().iter().enumerate() {
                                        {
                                            let action = b.action.clone();
                                            let label = action_label(&action);
                                            let chord = chord_label(b);
                                            let is_rec = recording() == Some(i);
                                            rsx! {
                                                div {
                                                    key: "{action}",
                                                    style: "display: flex; justify-content: space-between; align-items: center; gap: 24px; padding: 9px 0; border-bottom: 1px solid {row_border};",
                                                    span { style: "font: 14px -apple-system, sans-serif;", "{label}" }
                                                    div {
                                                        style: "display: flex; align-items: center; gap: 6px;",
                                                        if is_rec {
                                                            input {
                                                                style: "{recording_style}",
                                                                value: "キーを押す…",
                                                                autofocus: true,
                                                                onmounted: move |e| { spawn(async move { let _ = e.set_focus(true).await; }); },
                                                                onkeydown: move |e| {
                                                                    e.prevent_default();
                                                                    e.stop_propagation(); // don't let the global bridge fire
                                                                    let k = e.key();
                                                                    if matches!(k, Key::Meta | Key::Shift | Key::Alt | Key::Control) { return; }
                                                                    if k == Key::Escape { recording.set(None); return; }
                                                                    let m = e.modifiers();
                                                                    let code = format!("{:?}", e.code());
                                                                    let mut km = keymap.write();
                                                                    if let Some(slot) = km.get_mut(i) {
                                                                        slot.code = code;
                                                                        slot.meta = m.contains(Modifiers::META) || m.contains(Modifiers::CONTROL);
                                                                        slot.shift = m.contains(Modifiers::SHIFT);
                                                                        slot.alt = m.contains(Modifiers::ALT);
                                                                    }
                                                                    drop(km);
                                                                    recording.set(None);
                                                                },
                                                            }
                                                        } else {
                                                            button {
                                                                style: "{kbd}",
                                                                onclick: move |_| recording.set(Some(i)),
                                                                "{chord}"
                                                            }
                                                            button {
                                                                style: "{reset_one}",
                                                                title: "既定に戻す",
                                                                onclick: move |_| {
                                                                    if let Some(def) = config::default_keybindings().into_iter().find(|d| d.action == action) {
                                                                        if let Some(slot) = keymap.write().get_mut(i) { *slot = def; }
                                                                    }
                                                                },
                                                                "↺"
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}

/// Japanese label for a rebindable action id.
fn action_label(action: &str) -> &'static str {
    match action {
        "open_project_menu" => "プロジェクトを開く",
        "new_window" => "新規ウィンドウ",
        "quick_open" => "ファイルを開く",
        "palette_toggle" => "コマンドパレット",
        "full_text_search" => "全文検索",
        "find_toggle" => "ページ内検索",
        "toggle_sidebar" => "サイドバー開閉",
        "toggle_split" => "左右分割",
        "toggle_fav" => "お気に入り切替",
        "open_task_view" => "Task View",
        "copy_path" => "パスをコピー",
        "close_tab" => "タブを閉じる",
        "settings" => "設定",
        "toggle_sync_pin" => "Sync モード切替",
        other => Box::leak(other.to_string().into_boxed_str()),
    }
}

/// Human-readable key for a DOM `KeyboardEvent.code`.
fn code_label(code: &str) -> String {
    if let Some(c) = code.strip_prefix("Key") {
        return c.to_string();
    }
    if let Some(d) = code.strip_prefix("Digit") {
        return d.to_string();
    }
    match code {
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        "Backslash" => "\\",
        "Minus" => "-",
        "Equal" => "=",
        "Semicolon" => ";",
        "Quote" => "'",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backquote" => "`",
        "Space" => "Space",
        other => other,
    }
    .to_string()
}

/// macOS-style chord label, e.g. "⌘⇧P".
fn chord_label(b: &config::KeyBinding) -> String {
    let mut s = String::new();
    if b.meta {
        s.push('⌘');
    }
    if b.alt {
        s.push('⌥');
    }
    if b.shift {
        s.push('⇧');
    }
    s.push_str(&code_label(&b.code));
    s
}
