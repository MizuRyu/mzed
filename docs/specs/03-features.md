# 03 - 機能一覧

## v1 スコープ

### コア

| ID | 機能 | 説明 | 優先度 |
|---|---|---|---|
| C-01 | Zed プロジェクト連動 | Zed のプロジェクト切り替え検知 → docs 自動切替 | ★★★ |
| C-02 | 連動モード | auto / self / off の切替。固定トグル Cmd+Shift+L（auto⇄self）あり | ★★★ |
| C-03 | 瞬時プロジェクト切替 | インメモリキャッシュで Zed 級の速度 | ★★★ |
| C-04 | シングルインスタンス | IPC で既存プロセスにルーティング | ★★★ |

### レンダリング

| ID | 機能 | 優先度 |
|---|---|---|
| R-01 | GitHub Flavored Markdown | ★★★ |
| R-02 | GitHub Style CSS | ★★★ |
| R-03 | シンタックスハイライト | ★★★ |
| R-04 | テーブル (GFM) | ★★★ |
| R-05 | Mermaid ダイアグラム | ★★☆ |
| R-06 | KaTeX 数式 | ★★☆ |
| R-07 | YAML Frontmatter 折りたたみ | ★★☆ |
| R-08 | GitHub Alerts (NOTE/WARNING 等) | ★★☆ |
| R-09 | タスクリスト | ★★☆ |
| R-10 | 脚注 | ★★☆ |
| R-11 | 画像表示 + クリック拡大 | ★★☆ |
| R-12 | コードブロックコピーボタン | ★★☆ |
| R-13 | ドキュメント間リンク遷移 | ★★☆ |

### ナビゲーション

| ID | 機能 | 優先度 |
|---|---|---|
| N-01 | サイドバー (Obsidian ライク、フォルダ横にファイル件数表示) | ★★★ |
| N-02 | 目次 (ToC) パネル | ★★★ |
| N-03 | マルチタブ | ★★★ |
| N-04 | ファイル内検索 (Cmd+F) | ★★★ |
| N-05 | 全文検索 | ★★☆ |

### ファイル管理

| ID | 機能 | 優先度 |
|---|---|---|
| F-01 | CLI ファイル指定 (`mzed file.md`) | ★★★ |
| F-02 | CLI ディレクトリ指定 (`mzed docs/`) | ★★★ |
| F-03 | ドラッグ & ドロップ | ★★☆ |
| F-04 | ライブリロード | ★★★ |
| F-05 | セッション永続化（ルート・タブ・プロジェクト別最終ページを保存し次回復元） | ★★☆ |
| F-06 | 右クリックメニュー (Reveal/Open/Copy Path/Rename/Delete) → [10](10-settings-and-context-menu.md) | ★★☆ |
| F-07 | プロジェクトオープン時に最終更新ファイルを自動で開く（config トグル、デフォルト OFF） | ★★☆ |

### UI

| ID | 機能 | 優先度 |
|---|---|---|
| U-01 | コマンドパレット (Cmd+Shift+P) | ★★★ |
| U-02 | ダーク / ライトモード (システム連動 + 手動) | ★★★ |
| U-03 | ズーム (Cmd+/-) | ★★☆ |
| U-04 | 設定画面 General (window サイズ/Use Current/zoom 既定/起動挙動) → [10](10-settings-and-context-menu.md) | ★★☆ |
| U-05 | 設定画面 Theme (mode/コードフォント) → [10](10-settings-and-context-menu.md) | ★★☆ |
| U-06 | キーバインド閲覧・編集 → [10](10-settings-and-context-menu.md) | ★★☆ |
| U-07 | お気に入り / クイックアクセス | ★★☆ |
| U-08 | Raw Markdown 表示 | ★★☆ |

### コマンドパレット操作

| ID | 操作 | 優先度 |
|---|---|---|
| P-01 | テーマ切替 (light / dark) | ★★★ |
| P-02 | 連動モード切替 (auto / self / off) | ★★★ |
| P-03 | プロジェクト切替 | ★★★ |
| P-04 | ファイル検索 (ファジー) | ★★★ |
| P-05 | MD → HTML エクスポート | ★★☆ |
| P-06 | MD → PDF エクスポート | ★★☆ |
| P-07 | Toggle Sync Pin（auto⇄self、Cmd+Shift+L） | ★★★ |

### CLI

| ID | コマンド | 優先度 |
|---|---|---|
| L-01 | `mzed` — 起動、Zed アクティブプロジェクトを自動検知 | ★★★ |
| L-02 | `mzed <file>` — 単体ファイル | ★★★ |
| L-03 | `mzed <dir>` — ディレクトリ内の md | ★★★ |
| L-04 | `mzed --sync <mode>` — 連動モード指定 | ★★☆ |
| L-05 | `mzed --version` | ★★★ |
| L-06 | `mzed --help` | ★★★ |

### 配布

| ID | 方法 | 優先度 |
|---|---|---|
| D-01 | Homebrew tap (`brew install MizuRyu/tap/mzed`) | ★★☆ |
| D-02 | GitHub Releases (.app / .dmg) | ★★☆ |

---

## 将来検討

| ID | 機能 |
|---|---|
| FT-01 | MD → DOCX エクスポート (テンプレート指定で仕様書発行) |
| FT-02 | MD → XLSX エクスポート (テーブル → Excel) |
| FT-03 | コンテンツ幅切替 (wide / narrow) |
| FT-04 | ピン留め検索 (色分けハイライト) |
| FT-05 | ディレクトリ履歴 (戻る / 進む) |
| FT-06 | Glob パターン監視 |
| FT-07 | stdin パイプ入力 |
| FT-08 | Vim / Emacs キーバインドプリセット |
| FT-09 | テーブル → CSV コピー |
| FT-10 | Nix パッケージ |
| FT-11 | Linux / Windows 対応 |
| FT-12 | VSCode 連携 |
| FT-13 | タブのピン留め |
| FT-14 | フラット表示モード |
| FT-15 | サイドバー遅延読み込み |
| FT-16 | GUI cold start / first render instrumentation |
