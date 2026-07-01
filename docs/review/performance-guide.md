# mzed パフォーマンスガイド

## 目的

mzed は軽量 Markdown viewer として作る。
性能は後から足す最適化ではなく、設計の制約として扱う。

## 基本方針

- 遅い UI はバグ。
- UI thread は表示と入力応答に使う。
- ファイル I/O、Zed DB query、Markdown parse、search、export は UI thread から外す。
- 初回表示、プロジェクト切替、タブ切替、検索の体感速度を優先する。
- 速さのために security を弱めない。

## 目標

初期値。実測後に更新する。

| 対象 | 目標 |
|---|---|
| cold start | 500ms 台を目指す |
| project switch | cache hit では即時表示 |
| active file reload | 変更ファイルだけ再読込 |
| command palette | 入力ごとに詰まらない |
| search | UI を止めず、結果を段階表示できる構造にする |

## 設計ルール

### UI thread で重い処理をしない

NG:

```rust
let html = use_memo(move || {
    std::fs::read_to_string(active_file()).map(render_markdown)
});
```

ファイル I/O と Markdown parse が描画の同期処理になっている。

OK:

```rust
let html = use_resource(move || async move {
    markdown_service.render(active_file()).await
});
```

I/O と parse を非同期境界に逃がす。

### キャッシュは責務ごとに持つ

キャッシュ候補:

- file tree: project root + mtime snapshot
- rendered markdown: path + mtime + renderer options
- Zed recent workspaces: db mtime
- search index: project root + file mtimes

禁止:

- UI component の中に大きな cache map を持つ。
- 無効化条件が説明できない cache を入れる。

### 差分更新を優先する

- ファイル変更では active file だけを再レンダリングする。
- tree 変更では sidebar tree だけを再構築する。
- project switch では古い tree を消してから待つのではなく、cache があれば先に出す。
- HTML 全体の後処理を毎回走らせる前に、対象を限定できるか確認する。

### clone と allocation を測る

- `PathBuf` / `String` の clone は、スレッド移動、保存、UI 表示のためなら許可する。
- 大きな Markdown 本文、HTML、検索結果の clone は避ける。
- ループ内の `format!`、`to_string()`、`collect::<Vec<_>>()` は理由を確認する。

NG:

```rust
let all = files.clone();
let hits = all.into_iter().map(|p| p.display().to_string()).collect::<Vec<_>>();
```

OK:

```rust
let hits = files
    .iter()
    .map(|p| p.display().to_string())
    .collect::<Vec<_>>();
```

### watcher を増やしすぎない

- root watcher と active file watcher の役割を分ける。
- root / active file が変わったら古い watcher が止まることを確認する。
- 同じ root に複数 watcher を張らない。
- notify event が欠ける前提で、必要な箇所だけ poll fallback を使う。

## Markdown rendering

- parser、toc、post-process、sanitize の順序を固定する。
- Mermaid / KaTeX は初回 paint 後に後処理する。
- 大きな Markdown では仮想スクロールまたは section 単位 rendering を検討する。
- syntax highlight は重い。初回表示を遅らせるなら lazy highlight を検討する。

## Search

- 小規模 project は逐次 scan でよい。
- 大規模 project では index 化を検討する。
- 入力ごとに全ファイルを同期 read しない。
- search result は上限を持つ。
- snippet 生成は結果候補に絞る。

## 測定

性能改善では、変更前後の少なくとも1つを記録する。
具体的な測定手順と初回 baseline は [performance/measurement-methods.md](performance/measurement-methods.md) と [performance/prototype-baseline-2026-06-28.md](performance/prototype-baseline-2026-06-28.md) に置く。

候補:

- cold start 時間
- project switch 時間
- active file render 時間
- file tree build 時間
- search 時間
- memory usage
- watcher 数

簡易ログ例:

```rust
let start = std::time::Instant::now();
let html = render_markdown(&md);
tracing::debug!(elapsed_ms = start.elapsed().as_millis(), "render markdown");
```

## Review checklist

- UI thread で I/O していないか。
- parse / search / export が background へ逃げているか。
- cache key と invalidation が説明できるか。
- watcher が重複しないか。
- 大きな clone が増えていないか。
- performance のために sanitizer や URL validation を外していないか。

## 参考にする思想

- mo: CLI ファースト、軽量 single binary、file watch + push。
- arto: 読むことに特化した Markdown viewer、Tauri + WebView、オフライン表示。
- Zed: UI thread を止めない、I/O と parse を background に逃がす、古い snapshot を表示しながら裏で更新する。
