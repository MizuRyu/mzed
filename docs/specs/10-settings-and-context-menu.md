# 10 - 設定画面・右クリックメニュー・キーバインド閲覧

対象: 設定画面の General セクション / サイドバーのファイル右クリックメニュー / キーバインド閲覧。
お気に入り（クイックアクセス）は本仕様では**保留**（→ 末尾「保留」参照）。

## 設定画面

Cmd+, で開く単一の設定画面。Obsidian 風に左ナビ + 右ペイン。
ナビ項目は `General` / `Theme` / `Keybindings` の3つ。

保存先は `config.json`（arto 同様、変更は broadcast でホットリロード）。

### General

| 設定 | 型 | 既定 | 説明 |
|---|---|---|---|
| `window.width` | int(px) | 1100 | 新規ウィンドウの初期幅（手動リサイズで追従） |
| `window.height` | int(px) | 760 | 新規ウィンドウの初期高さ（手動リサイズで追従） |
| `window.x` | int(physical px) \| null | null | 前回終了時のウィンドウ X 座標（物理ピクセル）。null = OS 既定位置 |
| `window.y` | int(physical px) \| null | null | 前回終了時のウィンドウ Y 座標（物理ピクセル）。null = OS 既定位置 |
| `window.remember_position` | bool | true | 前回ウィンドウ位置を記憶（オフスクリーン検証あり） |
| `zoom.default` | float | 1.0 | 起動時ズーム倍率（0.8〜2.0、0.1 刻み） |
| `startup.behavior` | enum | `restore` | `restore`(前回セッション) / `docs`(Zed 連動 docs) / `blank` |
| `sync.default_mode` | enum | `auto` | Zed 連動の初期モード `auto` / `self` / `off` |
| `sidebar.visible_default` | bool | true | 起動時のサイドバー表示 |
| `external_links.open_in_browser` | bool | true | http/https リンクを既定ブラウザで開く |
| `open_latest_on_project_open` | bool | false | プロジェクト切替時に最終更新 Markdown を自動で開く（復元タブが無い場合のみ） |
| `project_aliases` | `[{path, alias}]` | `[]` | プロジェクトフォルダに付ける論理名（別名） |

#### プロジェクトの別名（`project_aliases`）

プロジェクト切替（Cmd+O）の候補は Zed の recent workspaces と mzed 自身の履歴から作るため、**ディスク上のフォルダ名でしか探せない**。任意のフォルダに論理名を付けて、その名前でも引けるようにする。

- 設定 General の「プロジェクトの別名」でフォルダを選び、名前を入力する（追加時はフォルダ名を初期値に入れる）。
- 切替メニューの検索はパスと別名の**両方**にマッチする。行にはフォルダ名の隣に別名のバッジを出す。
- **別名を付けたフォルダは常に候補に出す**（Zed の履歴にも mzed の履歴にも無くてよい）。実在しないパスは候補から落とす。

UI 補足:
- `window.width` / `window.height` は数値入力の横に **`Use Current`** ボタン。押下で現ウィンドウの実寸を取得して両フィールドに反映。
- `zoom.default` はスライダ or ステップ選択。現在のウィンドウには即時適用せず、次回起動 or 新規ウィンドウから有効（明示ラベル）。
- `startup.behavior` が `docs` のとき Zed 非起動なら blank にフォールバック。

実装メモ:
- `Use Current` はウィンドウサイズ取得（dioxus-desktop の window API）→ signal 経由でフィールド更新。
- 既存の `zoom` signal（`use_muda_event_handler` 管理）とは別。`zoom.default` はあくまで初期値。

### Theme

既存の U-02 を設定画面に集約。

| 設定 | 型 | 既定 | 説明 |
|---|---|---|---|
| `theme.mode` | enum | `system` | `system` / `light` / `dark` |
| `theme.code_font` | string | (既定等幅) | コードブロックのフォント family |
| `theme.code_font_size` | int(px) | 14 | コードブロックのフォントサイズ |
| `theme.line_height` | float | 1.7 | 本文行間（1.2〜2.4）。設定 Appearance から変更可 |

本文フォントは GitHub 既定を維持（将来検討 FT-03 の本文版は別途）。

### Keybindings（閲覧・編集）

設定画面からキーバインドを**閲覧および編集**できる。割当行をクリックして新しいキーを押すと即座に反映される。Esc でキャンセル、↺ で個別リセット。編集結果は `config.json` の `keybindings` に永続化される。ズーム / タブ移動 / Enter / ペインフォーカスは固定（変更不可）。

- カラム: `操作` / `キー` / `スコープ`（global / viewer / sidebar）。
- 検索ボックスで絞り込み（任意）。

現状の主なデフォルトバインド（抜粋）:

| 操作 | キー |
|---|---|
| コマンドパレット | Cmd+Shift+P |
| ファイル検索 | Cmd+P |
| 全文検索 | Cmd+Shift+F |
| ファイル内検索 | Cmd+F |
| サイドバー開閉 | Cmd+B |
| 設定を開く | Cmd+, |
| プロジェクトメニュー | Cmd+O |
| タブを閉じる | Cmd+W |
| タブ切替 | Ctrl+Tab |
| タブ番号移動 | Cmd+1〜9 |
| 左右分割 | Cmd+\ |
| Toggle Sync Pin（auto⇄self） | Cmd+Shift+L |
| Task View トグル | Cmd+Shift+D |
| Task View 再スキャン | Cmd+R |
| ズーム | Cmd+= / Cmd+- / Cmd+0 |

## 右クリックメニュー（サイドバーのファイル行）

ビューア専用に絞った文脈メニュー。ファイル新規作成や Cut/Paste 等の編集系は持たない。

| 項目 | 動作 | 実装 |
|---|---|---|
| 新規タブで開く | 別タブで開く | 既存の tabs.open |
| 新規ウィンドウで開く | 別ウィンドウで開く | arto マルチウィンドウ機構（後フェーズ） |
| ─ | | |
| Reveal in Finder | Finder で選択表示 | `open -R <path>` |
| Open in Default App | 既定アプリ（エディタ等）で開く | `open <path>` / `open` crate |
| ─ | | |
| Copy Path | 絶対パスをコピー（ファイル・フォルダ共通） | pbcopy（native） |
| Copy Relative Path | プロジェクトルート基準の相対パス（ファイル・フォルダ共通） | pbcopy（native） |
| ─ | | |
| Rename | インライン編集（F2） | 行を input 化 → fs::rename |
| Delete | ゴミ箱へ移動（完全削除しない） | macOS Trash（NSFileManager / trash crate） |

挙動メモ:
- メニュー表示は行の `oncontextmenu`。表示中は外側クリック / Escape で閉じる。
- Rename / Delete は fs 変更 → notify 監視が拾ってツリー更新（追加のリフレッシュ不要）。
- Delete は確認なしでゴミ箱（復元可能なため）。完全削除は提供しない。
- 「新規ウィンドウで開く」はマルチウィンドウ未実装の間は非活性 or 非表示。

## 段階

1. 設定画面 General（`Use Current` 含む）+ config 永続化 + ホットリロード
2. 右クリックメニュー（Reveal / Open / Copy Path×2 / Rename / Delete）
3. Keybindings 閲覧テーブル
4. Theme セクションの設定画面集約

## 保留

- **お気に入り / クイックアクセス**: file 単位・project 単位のピン、サイドバー `★ Favorites` セクション、Cmd+D トグル、`config.json` の `favorites`。仕様を別途固めてから着手。
- 新規ウィンドウで開く（マルチウィンドウ機構が前提）。
- キーバインド編集（FT-12）。
