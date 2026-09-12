# 10 - 設定画面・右クリックメニュー・キーバインド閲覧

対象: 設定画面の General セクション / 右クリックメニュー（サイドバーのファイル行）/ メモ / キーバインド閲覧 / オーバーレイ一覧（プロジェクトメニュー・コマンドパレット）の選択操作。
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
| `tab_insert` | enum | `start` | 新しく開いたタブの位置。`start`(左端) / `end`(右端)。既に開いているタブを開き直したときは位置を動かさずアクティブにするだけ。セッション復元は保存順をそのまま並べる（設定に依らない） |
| `open_latest_on_project_open` | bool | false | プロジェクト切替時に最終更新 Markdown を自動で開く（復元タブが無い場合のみ） |
| `frontmatter_default_open` | bool | false | frontmatter の「Metadata」折りたたみを開いた状態で表示。設定 General でトグル、表示中ドキュメントに即反映（`mzed serve` はサーバ起動時の値） |
| `project_aliases` | `[{path, alias}]` | `[]` | プロジェクトフォルダに付ける論理名（別名） |
| `project_menu_hidden` | `[string]` | `[]` | プロジェクト切替（Cmd+O）から隠すパス。候補行の ✕ で追加、設定 General の「非表示のプロジェクト」で復元。Zed の履歴自体は編集できないためローカルの重ね掛け |
| `sync_skip_worktrees` | bool | `true` | Zed が git worktree（`.git` がファイル）を開いても追従しない（→ [05](05-zed-integration.md)）。プロジェクト連動タブでトグル。Orca 由来の切替には効かない |
| `sync_source` | enum | `auto` | 追従元 `auto`(Zed & Orca) / `zed` / `orca`（→ [05](05-zed-integration.md)）。プロジェクト連動タブで選択 |

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
| メモを追加 | Cmd+Shift+M |
| ズーム | Cmd+= / Cmd+- / Cmd+0 |

## オーバーレイ一覧の選択（プロジェクトメニュー / コマンドパレット）

Cmd+O のプロジェクトメニューと Cmd+Shift+P / Cmd+P のパレットは、行の選択に関して同じ規則で動く。

- ホバーとキーボードは**単一の選択インデックス**を共有する。ハイライトは常に「いま選ばれている1行」だけに付く（プロジェクトメニュー末尾の「Open Folder…」行も選択対象）。
- Enter は選択中の行を実行する。行のクリックは即コミット。
- 選択行のスクロール追従はキーボード操作のときだけ走る。ホバーでの選択変更ではリストを動かさない（動かすとカーソルの下に別の行が入り、選択が連鎖する）。
- キー操作の直後 250ms はホバーを無視する。矢印キーのスクロールで静止したカーソルの下に別の行が入り、その `mouseenter` が選択を戻してしまうため。開いてからまだ一度もキーを押していない間はガードを掛けない（開いた位置に既にカーソルがある行をすぐ選べる）。

### 候補の並び順（`fuzzy::rank_tiered`）

クエリの一致は 4 段階の tier で判定し、tier 順 → 同 tier 内はスコア順 → 同スコアは recency（新しい順）で並べる。tier は上から: 1) 完全一致（大文字小文字無視。ファイルは拡張子あり/なし両方で判定） 2) 前方一致 3) 連続部分一致 4) 既存のファジースコア。1〜3 のスコアは 0 固定（tier 内で差が付かないので recency がそのままタイブレークになる）、4 だけ実際のファジースコアで差が付く。空クエリは全件を recency 降順で返す。

- **Cmd+P（ファイル検索）**: 一致対象はプロジェクトルートからの相対パス（複数 root では root 名を先頭に付ける）。recency はファイルの mtime。表示は「ファイル名（濃い）　相対ディレクトリ（薄い）」の1行。未読ファイルは tier 順より上（先頭）に来て緑丸が付く（→ [03](03-features.md) N-06）。
- **Cmd+O（プロジェクトメニュー）**: 現在のプロジェクトを常に先頭固定し、残りを tier 順 → 同 tier は「mzed で最後に開いた時刻」の降順で並べる。検索キーは表示名（`project_aliases` の alias があれば alias、無ければフォルダ名）とフルパスの両方（パスの一部でも当たる）。

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

## 本文の右クリック

**mzed は介入しない**。本文を選択していてもいなくても、WebView 標準のメニュー（コピー / 調べる）がそのまま出る。
メモの入口は選択の終端に浮くアイコン（→ 下記「メモ」）。

## メモ（注釈をエージェントへ戻す）

読みながら「ここを直して」を残し、エージェントがそれを読んで直すための一方向の受け渡し。
ビューアは読み取り専用のまま、メモは対象ファイルではなくグローバルの保存先に書く。

### 入口

| 入口 | 操作 |
|---|---|
| アイコン | 本文を選択して指を離すと、選択の終端に丸いアイコン（吹き出し 1 つ、tooltip「メモ」）が浮く。クリックでポップオーバー |
| キーバインド | 本文を選択して Cmd+Shift+M（`add_note`） |
| コマンドパレット | `Add Note` / `Open Notes Folder`（保存先を Finder で開く） |

選択が無い状態では保存せず、toast「本文を選択してからメモを追加してください」を出す。

**全体を選択した状態は対象外**。選択テキストがそのペイン本文（`data-mdo-pane` 配下の `textContent`、
空白を除いた文字数）の 90% 以上ならアイコンを出さず、Cmd+Shift+M / パレットには toast
「全体を選択した状態ではメモを付けられません」を返す。全体を直す指示はメモではなく CLI で直接直す作業だから。

### アイコンとポップオーバー

アイコンはペイン本文のスクロールコンテナ内に絶対配置する（`.mdo-note-layer` / `.mdo-note-icon`）。
本文の DOM は書き換えないので、コピーや検索ハイライトや再描画に影響しない。スクロールすると本文と一緒に動く。
選択が解ける（本文をクリックする）とアイコンも消える。

**アイコンは選択より長生きしない**。選択が変わった時点で必ず捨て、指を離した（mouseup / Shift キーの keyup）ときに
描き直す。だからドラッグ中やキーボードで選択を広げている途中には出ない。
さらに**クリック時点で選択を読み直して**送るので、アイコンが古い範囲を送ることはない。
受け取った Rust 側もキーバインドと同じ経路で `too_broad` と pane を検証する（同じ toast、同じ拒否）。

ポップオーバーは入力欄 1 行だけ（幅 320px、placeholder「メモ」）。引用のプレビューは出さない（選択がそのまま見えているため）。
Enter で保存、Esc で閉じる。空文字は保存しない。**ポップオーバーの外側の mousedown**（本文・サイドバー・ツールバーを問わず）でも
保存せずに閉じる。× ボタンは持たない。検索バーとは同時に出さない（両方向で排他: メモを開くと Cmd+F の欄を閉じ、Cmd+F を押すとポップオーバーを閉じる）。

位置は選択終端。WebView 側が `--mdo-note-x` / `--mdo-note-y`（viewport 座標）を publish し、ポップオーバーは
`position: fixed` でそれを読む。scroll / resize / ペイン本文の `ResizeObserver` で、アイコンとハイライトの
座標を引き直す（Range は生きているので rects を取り直せる。行の折り返し数が変わったときだけ作り直す）。
画面外にはみ出す場合は viewport 内にクランプする。

ポップオーバーの入力欄にフォーカスが移ると document の選択は消えるため、**引用範囲には薄い背景色を重ねる**
（`.mdo-note-mark`、`::selection` と同じ色）。Range の client rects を元にした透明オーバーレイで、`<mark>` で包んだりはしない。

**表示中は引用を凍結する**。ポップオーバーを開いた時点の Range を保持し、以後 document の選択が何をしようと
ハイライトも保存対象も動かない。本文で新しく選択し始めたら（＝外側クリック）ポップオーバーを閉じて Icon モードに戻る、という単一の規則にする。
保存すると toast「メモを保存しました」を出し、アイコン・ポップオーバー・覚えている選択をまとめて捨てる。

### 選択の取得と紐づけ

選択の取得は WebView 側で行う。パレットやポップオーバーは開いた時点でフォーカスを奪い document の選択を壊すため、
**ペイン本文で最後に確定した選択を WebView 側で覚えておく**。忘れる条件は2つ:

- 本文をクリックして選択を解いたとき（オーバーレイの入力欄へフォーカスが移っただけなら忘れない）
- 引用元のテキストノードが document から外れたとき（ライブリロード・タブ切替・テーマ切替による再描画、分割の解除）。覚えている引用が画面上の文章と食い違うのを防ぐ。再描画の検知はペイン本文への `MutationObserver`（childList）で、**引用元と同じペインが変わったときだけ**捨てる（分割の反対側が再描画しても手を出さない）

紐づけ先のファイルは**選択を取得した時点**で確定する。ポップオーバーを開いた後にタブを切り替えても、保存されるのは選択したときのファイル（そのタブを閉じていても同じ）。

受け取り側の検証:

- `pane` は 0 / 1 のみ。1 は分割表示中だけ受理し、それ以外は「選択無し」扱い（toast）
- 選択が2つのペインにまたがる場合（`startContainer` と `endContainer` のペインが違う）は選択無し扱い
- `quote` 4,000 文字 / `heading` 300 文字 / `note` 4,000 文字で切る（`services::notes` のコンストラクタで強制）。切った場合は toast「メモを保存しました（長すぎる部分は切りました）」

### 保存先と形式

`~/.config/mzed/notes/` に 1 メモ 1 JSON。ファイル名は `<UTC yyyyMMddTHHmmssZ>-<8桁hex>.json`
（`ls` が時系列に並ぶ）。hex は file / 時刻 / 引用 / 本文に加えて**プロセス内の連番と PID** を混ぜる。
同じ秒に同じ内容を 2 回保存しても別ファイルになり、既存のメモを上書きしない。

書き込みは `.json.tmp` に書いてから rename（読み取り側が途中状態を読まないため）。
名前が既に存在する場合は書かずに別の hex で再試行する。既存を置換する `persistence::atomic_write` はここでは使わない。

```json
{
  "version": 1,
  "created_at": "2026-09-12T01:22:05Z",
  "project_root": "/Users/me/dev/repos/foo",
  "file": "/Users/me/dev/repos/foo/docs/plan.md",
  "rel_path": "docs/plan.md",
  "heading": "## 実装方針",
  "quote": "選択した本文",
  "note": "ここは Orca 由来のイベントも対象にして"
}
```

- `project_root` はアクティブプロジェクトの root。root 外のファイル（ドラッグ&ドロップ等）はその親ディレクトリを root にする
- `heading` は選択開始位置より上の最も近い見出し（`#` を level 分付ける）。無ければ `null`
- 行番号は持たない。エージェントは `quote` で grep して位置を特定する
- 1 ファイル 1 書き込みなので、mzed の書き込みとエージェントの読み取りが競合しない。処理済みメモは `done/` へ移動するだけで済み、状態フィールドの更新競合が無い
- 書き込みは services 層（`services::notes`）。UI component から fs を触らない

### 対象外

- `mzed serve`（読み取り専用配信）は非対応。serve の shell に選択取得の JS を入れない
- メモの編集 UI は持たない（JSON を直接編集すればよい）。diff 生成・LLM への直接送信も持たない

### エージェント側

`.agents/skills/mzed-notes/SKILL.md`（保存先の読み方、`jq` での絞り込み、処理済みの `done/` 移動）。

## 段階

1. 設定画面 General（`Use Current` 含む）+ config 永続化 + ホットリロード
2. 右クリックメニュー（Reveal / Open / Copy Path×2 / Rename / Delete）
3. Keybindings 閲覧テーブル
4. Theme セクションの設定画面集約

## 保留

- **お気に入り / クイックアクセス**: file 単位・project 単位のピン、サイドバー `★ Favorites` セクション、Cmd+D トグル、`config.json` の `favorites`。仕様を別途固めてから着手。
- 新規ウィンドウで開く（マルチウィンドウ機構が前提）。
- キーバインド編集（FT-12）。
