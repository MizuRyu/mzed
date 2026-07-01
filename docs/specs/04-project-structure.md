# 04 - プロジェクト構成

## ディレクトリ

```
mzed/
├── docs/
│   ├── research/                # 調査資料
│   └── specs/                   # 設計仕様書
│
├── src/                         # Dioxus root crate
│   ├── main.rs                  # 起動のみ
│   ├── app.rs                   # Dioxus app shell / wiring
│   ├── cli.rs
│   ├── config.rs
│   ├── instance.rs
│   ├── zed.rs
│   ├── watcher.rs
│   ├── app_state/
│   ├── js/
│   ├── markdown/
│   ├── services/
│   └── ui/
│       ├── command_palette.rs
│       ├── find_bar.rs
│       ├── navigation.rs
│       ├── project_menu.rs
│       ├── search_panel.rs
│       ├── settings.rs
│       ├── sidebar.rs
│       ├── toolbar.rs
│       └── window.rs
│
├── assets/                      # WebView assets (CSS / Mermaid / KaTeX)
├── tests/                       # 統合テスト（cargo nextest）
│   ├── fixtures/                # Markdown テストフィクスチャ
│   └── instance_integration.rs  # 二重起動・stale socket・ソケット権限等
│
├── Dioxus.toml
├── Justfile
├── README.md
└── .gitignore
```

## Rust モジュール依存

```mermaid
graph TD
    main_rs[main.rs] --> app_rs[app.rs]
    app_rs --> cli
    app_rs --> instance
    app_rs --> app_state
    app_rs --> services
    app_rs --> ui
    app_rs --> js
    app_rs --> markdown
    app_rs --> zed
    app_rs --> watcher

    services --> file_service
    services --> watch_service
    services --> instance_service
    services --> platform

    watch_service --> watcher
    watch_service --> zed
    instance_service --> instance

    ui --> command_palette
    ui --> find_bar
    ui --> navigation
    ui --> project_menu
    ui --> search_panel
    ui --> settings
    ui --> sidebar
    ui --> toolbar
    ui --> window

    markdown --> render
    markdown --> post_process
```

## 設定ファイル

パス: `~/.config/mzed/`

```jsonc
// config.json — ユーザー設定
{
  "theme": "system",             // light | dark | system
  "sync_mode": "auto",           // auto | self | off
  "zoom": 1.0,
  "favorites": [],
  "window_width": 1100,
  "window_height": 760,
  "startup": "restore",
  "keybindings": []
}
```

```jsonc
// session.json — 自動管理
{
  "roots": ["/path/to/project"],
  "tabs": ["/path/to/project/README.md"],
  "active": "/path/to/project/README.md",
  "sidebar_width": 280,
  "project_tabs": {
    // プロジェクト別の最終タブ状態（キー: プロジェクトルートパス）
    "/path/to/project": {
      "tabs": ["/path/to/project/README.md"],
      "active": "/path/to/project/README.md"
    },
    "/path/to/other-project": {
      "tabs": ["/path/to/other-project/docs/index.md"],
      "active": "/path/to/other-project/docs/index.md"
    }
  }
}
```
