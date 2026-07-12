# 11. Task View モード

タスク専用ビューア。`task-creator` スキルが `docs/memo/tasks/` に作るタスクフォルダを、ツリーを辿らず一覧・status 別に把握し、クリックで `task.md` を即読みするためのモード。個人向け実験機能として feature フラグ管理。

最終確認日: 2026-07-06（モック合意 variant-2 ベース）

## 目的と動機

- 通常はサイドバーのファイルツリーを `docs/memo/tasks/<yymmdd-NN-名前>/task.md` まで辿る必要があり面倒。
- タスクの `status` / `created` は frontmatter で固定スキーマ。これを解析して専用画面を出す。
- さらに「**全プロジェクトの直近 N 日に作ったタスク**」を横断で見たい（複数リポにまたがる作業把握）。

## データモデル（task-creator 準拠 / 固定）

- タスク = `<project>/docs/memo/tasks/<yymmdd-NN-タスク名>/task.md`
- `task.md` frontmatter: `status`(todo|in_progress|review|done), `created`(yymmdd), ほか `outputs` /  `task_ref` / `project` / `worklog_sync` / `referenced_knowledge`
- 本文: `## TODO`, `## 成果物（完了条件）`(任意), `## userの依頼`
- タスクフォルダ内の他のファイルは**全列挙**して子として扱う（1階層、`.` 始まりの隠しファイルとサブディレクトリは除外、名前順）。frontmatter の `outputs` は表示に使わない

## 起動と全体構成

- **キーバインド `Cmd+Shift+D`** で Task View をトグル（開く/閉じる）。`Esc` でも閉じる。既存のキーバインド機構（`src/config.rs` `default_keybindings()` / `src/js/keyboard.rs`）に `open_task_view` として追加、ユーザー変更可。
- **`Cmd+R`（`task_view_refresh`）** でタスク一覧を再スキャン（Task View 表示中のみ有効。ヘッダの ↻ と同じトリガー）。
- **`Ctrl+Tab`（`task_view_toggle_scope`）** で This Project ⇄ All Projects をトグル（リバインド可）。Task View が閉じているときは従来どおり**次のタブへ切替**として動く（既定バインドが固定の Ctrl+Tab タブ切替を覆うため、dispatch 側でフォールバック）。
- 左ペインはドラッグ divider で幅 200〜600px にリサイズ可能（セッション内のみ。永続化は v1 対象外）。
- Task View は通常のサイドバー＋本文領域を置き換える**2ペインモード**:
  - 左ペイン: タスク一覧（下記スコープ切替つきツリー）
  - 右ペイン: 選択タスクの `task.md` を既存 Markdown レンダリングパイプラインで表示
- feature フラグ OFF のときは起動導線ごと無効（キーバインドも無反応）。

## 左ペイン: スコープ切替

上部に **This Project ⇄ All Projects** トグル。

### This Project
- 現在開いているルート（複数ルート時は各ルート）の `docs/memo/tasks/` を対象。
- 全期間表示（日数フィルタなし）。
- ルートノード = プロジェクト（variant-2 表示: **プロジェクト名（太字）＋下に muted のフルパス**）、配下にタスクフォルダ、さらに `task.md`＋成果物ファイルをぶら下げる既存ツリー同等の折りたたみツリー。

### All Projects
- 設定した**スキャンルート**群（下記 config）配下を走査し、`<any>/docs/memo/tasks/<task>/task.md` を収集（Zed 非依存。ファイルシステム走査）。
- **直近 N 日**（`created` が今日から N 日以内）で絞る。上部に日数セレクタ（既定 7 日）。
- プロジェクトごとに variant-2 のルートノードを縦に並べ、各配下に該当タスク。

### ツリー行の見た目（既存 `src/ui/sidebar.rs` 準拠）
- インデント（深さ×14px）、chevron、フォルダ/ファイルアイコン、13px、hover `rgba(127,127,127,0.12)`、アクティブ行 `rgba(9,105,218,0.1)`＋左 `#0969da` 2px。
- **タスクフォルダ行に status 色**（ドット or 左ボーダー、控えめ）: todo=`#8b949e`, in_progress=`#1f6feb`, review=`#d29922`, done=`#3fb950`。
- 並び順: `created` 降順（新しいタスクが上）。

## 右ペイン: タスク詳細

- 選択したタスクフォルダ行 or `task.md` 行クリックで、その `task.md` を既存レンダリングで表示。
- タスクフォルダ内の他ファイルも行として辿れる。Markdown（`.md` / `.markdown`）はクリックで同ペインに表示、それ以外（画像・ログ等）はクリックで OS の既定アプリで開く。
- ヘッダに status バッジ・`created`・フルパス（muted）。

## 設定（config）

`src/config.rs` `Config` に追加（すべて `#[serde(default)]` で後方互換）:

| フィールド | 型 | デフォルト | 意味 |
|---|---|---|---|
| `feature_task_view` | bool | `true` | Task View 機能の有効/無効。Features タブでトグル |
| `task_view_tasks_subpath` | string | `"docs/memo/tasks"` | プロジェクト内のタスクフォルダ相対パス |
| `task_view_scan_roots` | `[string]` | `[]` | All Projects でプロジェクトを探すルートディレクトリ群（例: `~/dev/repos`）。空なら All Projects は現プロジェクトのみ表示＋設定への誘導ヒント |
| `task_view_days` | int | `7` | All Projects で表示する直近日数 |

設定 UI:
- **Features タブ**: `feature_task_view` トグル。
- **General or 専用セクション**: `task_view_tasks_subpath`（テキスト）、`task_view_scan_roots`（ディレクトリ追加/削除リスト）、`task_view_days`（数値、既定7）。

## 走査・解析の実装方針

タスクの場所は `<project>/<task_view_tasks_subpath>`（既定 `docs/memo/tasks`）という既知の相対パス。これを **scan_root 配下の任意の深さから、枝刈り付きディレクトリ walk で発見**する（ユーザーは `~/dev` や `~` のような広い親を指定する）。

- **リポ境界で止める walk（重要）**: `docs/memo/tasks` はリポルート直下にある。よってリポの中身は歩かない。各ディレクトリで:
  1. `<dir>/<subpath>` が `is_dir` なら project = `dir` として採用（タスク列挙へ）。
  2. `<dir>/.git` が存在すれば**リポルート**とみなし、それ以上**子へ潜らない**（1 で直接確認済み）。
  3. それ以外は枝刈りしつつ子ディレクトリへ再帰。**1 で採用した非 repo ディレクトリも再帰を続ける**（足場フォルダ配下のネストしたプロジェクトも発見する）。ただしその場合、採用ディレクトリ自身の subpath 先頭セグメント（例 `docs`）へは入らない — 自分の tasks ツリーの中を歩かないため。
  これで `~/go` や各リポ内部の巨大ツリーに一切入らず、辿るのは「リポより上の浅い足場」だけになる。
- **scan_root 自身も候補**: `<scan_root>/<subpath>` が存在すれば scan_root 自体を project として採用する（プロジェクトルートを直接 scan_roots に指定できる）。scan_root が repo 内部でも、境界チェックは「子に潜るか」の判定なので普通に歩ける。
- **repo 内部のサブプロジェクト**: repo の中にさらにプロジェクトがある場合（例 `<repo>/all/knowledge/*`）、walk は repo 境界で止まるため自動発見されない。その内側のディレクトリを scan_roots に明示追加して対応する。
- **枝刈り（必須）**: 次の名前のディレクトリには入らない — `node_modules` / `target` / `dist` / `build` / `.git` / `Library` / 先頭が `.` の隠しディレクトリ、および **macOS の TCC 保護・クラウドフォルダ**（`Desktop` / `Documents` / `Downloads` / `Pictures` / `Movies` / `Music` / `Public` / `Applications` / `Dropbox` / `Google Drive` / `OneDrive*`）。保護フォルダに触れるとアクセス許可ダイアログが出る（ad-hoc 署名のため再ビルドごとにリセットされる）ので、走査自体が触れないことが重要。そこにリポを置いている場合は該当フォルダを scan_roots に明示追加する。深さ上限を設ける（例: 10）。加えて config の `task_view_scan_exclude` でユーザー定義の除外名を追加できる。
- **タスク列挙**: 発見した `<subpath>` を `readdir` し、各エントリ配下の `task.md` を確認。
- **セッションキャッシュ**: 走査結果はアプリ内にキャッシュし、Task View を開くたび・スコープ/日数を変えるたびに**丸ごと再 walk しない**。キャッシュキーは (scope, scan_roots, subpath, days)。ヘッダに手動「↻ 更新」を置き、明示時のみ再走査。初回だけコストを払う。
- **frontmatter だけ読む**: 各 `task.md` は先頭の frontmatter（最初の `---` 〜 次の `---`）だけ読み、`status`/`created` を抽出。本文は読まない（詳細ペインを開いたときに初めて全体を読む）。既存 `src/markdown/frontmatter.rs` を参考に最小パーサを用意。
- スキャンは非同期（`spawn` + `spawn_blocking`）でバックグラウンド実行。UI は止めない。
- 壊れた frontmatter は status 不明扱いで一覧には出す（クラッシュしない）。
- `created`(yymmdd) を今日のローカル日付と比較して N 日以内を判定。
- 走査結果はモードを開くたび最新化（ファイル監視までは v1 不要）。
- 目安コスト: ディレクトリのみの枝刈り walk＋frontmatter 先頭読み。`~/dev` 規模なら数十 ms〜、`~` 全体でも枝刈り（Library/隠し/依存ディレクトリ除外）＋深さ上限で現実的な範囲。OS がディレクトリキャッシュを持つ 2 回目以降は速い。`spawn_blocking` で UI は止めない。パフォーマンス方針（`docs/review/performance-guide.md`）順守。

## 行操作（絶対パスコピー / Finder / デフォルトで開く / お気に入り）

Task View の各行（プロジェクトルート・タスクフォルダ・ファイル）は以下の操作に対応する。**リネーム・削除はメイン画面のサイドバーで行う（Task View 対象外）**。

### ホバーコピーボタン
- 行にホバーすると行右端にクリップボードアイコン（`.mdo-copy-path`）が現れる。
- クリックで**絶対パス**（`services::platform::canonical_clipboard_text`）をクリップボードに書き込み、「Copied!」トーストを表示する。
- サイドバー（`sidebar.rs`）の `.mdo-copy-path` と同じ CSS クラス・スタイルを踏襲する。

### 行を右クリック → 専用メニュー
Task View 固有の `TaskCtxMenu` overlay（`task_view.rs` 内）。メインの汎用 CtxMenu（リネーム・削除を含む）は使わない。

| メニュー項目 | 対象 |
|---|---|
| 絶対パスをコピー | 全行 |
| Finder で表示 | 全行 |
| デフォルトアプリで開く | ファイル行のみ（ディレクトリ行は Finder と同義のため省略） |
| お気に入りに追加 / 外す | タスクフォルダ・ファイル行のみ（プロジェクトルート行は対象外） |

- メニューは `oncontextmenu` の client 座標に `position: fixed` で表示する。
- 背景クリックで閉じる。スタイルはメイン CtxMenu と同じトークン（背景色 `#1c2128`/`#ffffff`、`border-radius: 8px`、`box-shadow` など）を使用する。

### 絶対パスの保証
- コピー・Finder・お気に入り追加すべてに**絶対パス**（`PathBuf` + `canonical_clipboard_text`）を使用する。
- All Projects モードで別プロジェクトの行を操作しても、プロジェクトルートに依存しない正しいパスが得られる。

## v1 スコープ / 割り切り

- リアルタイム更新は **This Project のみ watcher 連動**（既存の現プロジェクト監視 `tree_refresh` に相乗り。ファイル変更で自動再スキャン）。All Projects は監視しない（他リポへの watcher 大量設置を避ける。↻ か開き直しで更新）。
- タスクの status 編集・並べ替え等の書き込み操作はしない（読み取り専用ビューア）。
- カンバン/ダッシュボード表示は入れない（B案ツリーのみ）。将来 feature 拡張余地として残す。
- `task_view_scan_roots` が空のときの All Projects は現プロジェクトのみ（設定誘導ヒントを出す）。

## テスト

- frontmatter 抽出（status/created、壊れ入力）の純粋関数ユニットテスト。
- 「直近 N 日」判定の純粋関数ユニットテスト（境界: ちょうど N 日前、N+1 日前）。
- 走査結果の集約（プロジェクト別グルーピング、created 降順ソート）のユニットテスト。
- 手動 TC を `docs/specs/manual-test-cases.md` に追加（起動トグル / This⇔All / 日数フィルタ / status 色 / 詳細表示 / feature OFF 無効化 / scan_roots 空時）。
