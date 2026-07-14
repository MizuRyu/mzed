---
name: mzed-setup
description: >
  mzed（Zed連動Markdownビューア）のインストール・設定変更・キーバインド変更・
  トラブルシュートを頼まれたときに使う。ビルド・デプロイ手順、config.json の
  全フィールド、キーバインドの形式、セッションリセット方法を網羅している。
---

# mzed セットアップ・運用スキル

## プロジェクト概要

- **スタック**: Rust + Dioxus 0.7, macOS デスクトップアプリ
- **役割**: Zed エディタの focused project を追従して Markdown をプレビューする
- **リポジトリ**: `src/` 配下が全実装。設定の根幹は `src/config.rs`、テーマ・SyncMode は `src/theme.rs`

---

## インストール・更新手順

```bash
# 1. ビルド & バンドル（.app + .dmg を生成）
just bundle
# 出力先: target/dx/mzed/bundle/macos/macos/mzed.app

# 2. /Applications にインストール（quarantine も解除）
just install
# 内部動作:
#   rm -rf /Applications/mzed.app
#   cp -R target/dx/mzed/bundle/macos/macos/mzed.app /Applications/mzed.app
#   xattr -dr com.apple.quarantine /Applications/mzed.app
```

`just install` は `just bundle` を前提条件として呼ぶので、通常は `just install` だけ叩けばよい。

### CLI シンボリックリンク

`just install` が `~/.local/bin/mzed` へのシンボリックリンクも作成する。`~/.local/bin` に PATH が通っていない場合はシェル設定に追加する:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

`just uninstall` で .app とシンボリックリンクの両方が削除される。

---

## CLI 引数仕様

`src/cli.rs` が `clap` で定義。

```
mzed [PATH...] [--sync <auto|self|off>]
```

| 引数 | 説明 |
|------|------|
| `PATH`（複数可） | 開くファイルまたはディレクトリ。省略時は Zed 連動モードで起動 |
| `--sync auto` | Zed の focused project を完全追従（デフォルト） |
| `--sync self` | sidebar root は追従するがアクティブタブは奪わない |
| `--sync off` | Zed を無視して独立動作 |

**パス解決ルール**:
- 引数なし → Zed 連動（Target::Zed）
- ディレクトリ 1 つ → そのディレクトリをプロジェクトルートに（Target::Dir）
- ファイル複数（ディレクトリ混在時はファイルのみ抽出）→ タブで開く（Target::Files）

---

## 設定ファイル

### パス

```
~/.config/mzed/config.json
~/.config/mzed/state.json   ← セッション（後述）
```

`src/config.rs` の `config_dir()` 関数が決定する。`$HOME` が取れる場合は常に `~/.config/mzed/`（`dirs::config_dir()` は使わない）。macOS の `~/Library/Application Support` ではないので注意。

### config.json フィールド一覧

フォーマットは JSON。欠損フィールドはデフォルト値で補完されるので、変更したい項目だけ書けばよい。

| フィールド | 型 | デフォルト | 意味 |
|-----------|-----|-----------|------|
| `theme` | `"light"` \| `"dark"` \| `"system"` | `"system"` | 表示テーマ |
| `sync_mode` | `"auto"` \| `"self"` \| `"off"` | `"auto"` | Zed 連動ポリシー |
| `zoom` | float | `1.0` | Markdown 本文の表示倍率（0.5 〜 2.0、0.1 刻み） |
| `startup` | `"restore"` \| `"docs"` \| `"blank"` | `"restore"` | 起動時の動作（前回セッション復元 / Zed の docs 表示 / 空） |
| `favorites` | `["/path", ...]` | `[]` | Quick Access ブックマーク（ファイル・ディレクトリ） |
| `window_width` | int | `1100` | ウィンドウ幅（論理 px、手動リサイズで自動追従） |
| `window_height` | int | `760` | ウィンドウ高さ（論理 px、手動リサイズで自動追従） |
| `window_x` | int \| null | `null` | ウィンドウ X 座標（物理 px）。null = OS 既定位置 |
| `window_y` | int \| null | `null` | ウィンドウ Y 座標（物理 px）。null = OS 既定位置。復元時はオフスクリーン検証あり（外部ディスプレイ切断後も安全） |
| `sidebar_visible_default` | bool | `true` | 起動時にサイドバーを表示するか |
| `external_links_in_browser` | bool | `true` | 外部リンクをブラウザで開くか |
| `code_font` | string | `""` | コードブロックのフォントファミリー（空 = 組み込みモノスペース） |
| `code_font_size` | int | `14` | コードブロックのフォントサイズ（px） |
| `keybindings` | `[{...}]` | 後述 | キーバインド上書き設定 |
| `export_dir` | string \| null | `null` | エクスポート先ディレクトリ（null = OS の Downloads） |
| `feature_katex` | bool | `true` | KaTeX 数式レンダリング |
| `feature_html_export` | bool | `true` | HTML エクスポート機能 |
| `feature_pdf_export` | bool | `true` | PDF エクスポート機能 |
| `open_latest_on_project_open` | bool | `false` | プロジェクト切替時に最終更新 Markdown を自動で開く（復元タブが無い場合のみ） |
| `line_height` | float | `1.7` | 本文行間（1.2〜2.4）。設定 Appearance の「行間」から変更可 |
| `feature_task_view` | bool | `true` | Task View モード（Cmd+Shift+D）の有効/無効。設定 Features タブでトグル |
| `task_view_tasks_subpath` | string | `"docs/memo/tasks"` | プロジェクトルートからタスクフォルダまでの相対パス |
| `task_view_scan_roots` | `["/path", ...]` | `[]` | All Projects スキャン対象の親ディレクトリ群。空なら現プロジェクトのみ表示 |
| `task_view_days` | int | `7` | All Projects で表示する直近日数（`created` が今日から N 日以内） |
| `task_view_scan_exclude` | `["name", ...]` | `[]` | スキャン時に入らないディレクトリ名（ビルトイン枝刈りに追加） |
| `task_view_group_by_status` | bool | `true` | タスクをステータス見出しでグループ化する |
| `task_view_group_order` | `"project_first"` \| `"status_first"` | `"project_first"` | 外側の見出しをプロジェクト／ステータスのどちらにするか |
| `task_view_status_order` | `["todo", ...]` | `["todo","in_progress","review","done"]` | ステータス見出しの並び順。配列に無いものは末尾（「その他」） |
| `task_view_date_order` | `"desc"` \| `"asc"` | `"desc"` | グループ内タスクの `created` 並び順（新しい順／古い順） |

### sync_mode の詳細（`src/theme.rs`）

| 値 | Zed project switch 時の挙動 |
|----|---------------------------|
| `"auto"` | sidebar root を切り替え + 代表 Markdown をタブで開く |
| `"self"` | sidebar root だけ更新、アクティブタブは変えない |
| `"off"` | Zed を完全に無視 |

---

## キーバインド設定

### 形式（`src/config.rs` の `KeyBinding` 構造体）

`keybindings` はデフォルトに対する**上書きリスト**。存在するアクション名と一致したエントリのみ適用され、未知のアクションは無視される。表示順はデフォルト順で固定。

```json
{
  "keybindings": [
    { "action": "find_toggle", "code": "KeyF", "meta": true, "shift": false, "alt": false }
  ]
}
```

- `action`: アクション名（下表参照）
- `code`: DOM `KeyboardEvent.code` 値（物理キー、レイアウト非依存）
- `meta`: Cmd（macOS）または Ctrl（他 OS）
- `shift`: Shift キー
- `alt`: Option/Alt キー

### デフォルトキーバインド（`src/config.rs` の `default_keybindings()`）

| アクション | デフォルトキー | 説明 |
|-----------|--------------|------|
| `open_project_menu` | Cmd+O | プロジェクト選択メニュー |
| `new_window` | Cmd+N | 新しいウィンドウ |
| `quick_open` | Cmd+P | クイックオープン |
| `palette_toggle` | Cmd+Shift+P | コマンドパレット |
| `full_text_search` | Cmd+Shift+F | 全文検索 |
| `find_toggle` | Cmd+F | ページ内検索 |
| `toggle_sidebar` | Cmd+B | サイドバー表示切替 |
| `toggle_split` | Cmd+\\ | 分割ペイン切替 |
| `toggle_fav` | Cmd+D | お気に入り登録/解除 |
| `open_task_view` | Cmd+Shift+D | Task View モードをトグル（`feature_task_view` が ON のとき有効） |
| `task_view_refresh` | Cmd+R | Task View のタスク一覧を再スキャン（Task View 表示中のみ） |
| `task_view_toggle_scope` | Ctrl+Tab | Task View の This Project ⇄ All Projects をトグル（閉じているときは従来どおり次のタブへ） |
| `copy_path` | Cmd+Shift+C | ファイルパスをコピー |
| `close_tab` | Cmd+W | タブを閉じる |
| `settings` | Cmd+, | 設定画面を開く |
| `toggle_sync_pin` | Cmd+Shift+L | Zed 連動モードを auto ⇄ self でトグル |

### 固定ショートカット（変更不可）

- `Cmd+1` / `Cmd+2`: 左/右ペインにフォーカス
- `Ctrl+Tab` / `Ctrl+Shift+Tab`: 次/前のタブ（Ctrl+Tab は Task View 表示中は `task_view_toggle_scope` が優先）
- `Enter`（テキスト入力外）: 選択ファイルのリネーム
- `Esc`: オーバーレイを閉じる
- `Cmd+=` / `Cmd+-` / `Cmd+0`: ズームイン / ズームアウト / リセット

> キーバインド実装の詳細は `src/js/keyboard.rs`（JS ブリッジ）と `src/ui/settings.rs`（Hotkeys UI）を参照。変更中の可能性があるため、最新のアクション一覧は `src/config.rs` の `default_keybindings()` で確認すること。

### GUI での変更方法

`Cmd+,` で設定画面を開き、左ナビの **Hotkeys** を選択。各行をクリックするとキャプチャモードに入り、押したキーが即座に登録される。変更は自動保存（`~/.config/mzed/config.json` に書き込み）。

---

## 設定変更の反映タイミング

| 変更方法 | 反映タイミング |
|---------|-------------|
| GUI 設定画面（Cmd+,） | 即時（Dioxus signal 経由で即時反映、config.json にも非同期で保存） |
| `config.json` を直接編集 | **再起動が必要**（起動時にのみ読み込む） |

---

## セッションファイル

`~/.config/mzed/state.json` にウィンドウの一時状態を保存。

| フィールド | 内容 |
|-----------|------|
| `roots` | サイドバーのプロジェクトルート一覧 |
| `tabs` | 開いているタブのパス（順序保持） |
| `active` | アクティブタブのパス |
| `sidebar_width` | サイドバー幅（px、デフォルト 280） |

---

## リセット方法

| リセット対象 | 操作 |
|------------|------|
| 設定を初期化 | `rm ~/.config/mzed/config.json` して再起動 |
| セッション（タブ・ルート）を初期化 | `rm ~/.config/mzed/state.json` して再起動 |
| 全リセット | `rm -rf ~/.config/mzed/` して再起動 |

---

## よくあるトラブル

### Gatekeeper でブロックされる

```
"mzed.app" is damaged and can't be opened.
```

ビルドは署名なし。`just install` が `xattr -dr com.apple.quarantine` を自動実行するが、手動でコピーした場合は：

```bash
xattr -dr com.apple.quarantine /Applications/mzed.app
```

### `mzed` コマンドが見つからない

`just install` 済みか確認。symlink はあるのに引けない場合は `~/.local/bin` の PATH 設定を確認（上記「CLI シンボリックリンク」参照）。

### Zed のプロジェクトを追従しない

1. `config.json` の `sync_mode` が `"off"` または `"self"` になっていないか確認
2. CLI 起動時に `--sync off` を渡していないか確認
3. `just watch`（`cargo run --bin zed_watch`）で Zed watch プロセスが動いているか確認
4. 詳細は `src/zed.rs` と `src/watcher.rs` を参照

### 設定を書き換えたのに反映されない

直接ファイル編集の場合は再起動が必要（起動時のみ読み込み）。GUI 経由なら即時反映。

### ビルドエラー（dx コマンドが見つからない）

```bash
cargo install dioxus-cli
```

---

## 開発コマンド早見表

| コマンド | 内容 |
|---------|------|
| `just run` | デバッグ起動（`cargo run --bin mzed`） |
| `just watch` | Zed watch プロセスを起動（`cargo run --bin zed_watch`） |
| `just test` | テスト実行（`cargo nextest run`） |
| `just check` | フォーマット + Clippy |
| `just bundle` | リリースビルド + .app/.dmg 生成 |
| `just install` | bundle 後に /Applications へインストール |
