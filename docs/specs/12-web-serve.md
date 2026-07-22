# 12 - Web serve モード（`mzed serve`）

画面共有で md を見せるための、フォルダ単位のブラウザビューア。desktop と同じレンダリングパイプラインを localhost の HTTP サーバ越しに使う。既存のブラウザビューア CLI `mo` の置き換えを意図する（mzed 側に一本化し、ビューアの二重メンテをやめる）。

## CLI

```sh
mzed serve [DIR] [-p/--port PORT] [--no-open]
```

- `DIR` 省略時はカレントディレクトリ。canonicalize してルートとする
- 既定ポート **6280**（`mo` の 6275 と衝突しない値）。使用中なら明快なエラーで終了（`--port` を案内）
- 起動時に既定ブラウザを自動で開く（`--no-open` で抑止）
- **フォアグラウンド実行**。Ctrl+C で停止。常駐・多重登録・状態ファイルは持たない。別フォルダは別ターミナル＋別ポートで
- GUI・単一インスタンス IPC を一切通らない headless 経路（`app::run` の冒頭で分岐）

## セキュリティ

- **127.0.0.1 固定 bind**。LAN 公開オプションは意図的に設けない（用途は「自分の画面を共有」であり、他者への配信ではない）
- ドキュメント要求（`/api/doc` / `/api/stat` の `path`）は percent-decode → canonicalize → **served root 包含チェック** → `is_markdown` 検証。desktop の roots 包含と同じ規律
- 本文 HTML は desktop と同一の `file_service::load_document`（全エスケープ→再パース、URL allowlist、data URL 画像）をそのまま使う
- アセットは**コンパイル時に埋め込んだ `assets/` のコピー**（`include_dir`）から配信。アセットパスがファイルシステムに触れる経路はない

## エンドポイント

| パス | 内容 |
|---|---|
| `GET /` | シェルページ（HTML 一枚。CSS/JS インライン） |
| `GET /api/tree` | サイドバーツリー JSON（`files::build_tree`、絶対パス） |
| `GET /api/doc?path=` | レンダリング済み HTML + フラット ToC + mtime の JSON |
| `GET /api/stat?path=` | mtime のみ（live-reload ポーリング用） |
| `GET /assets/*` | 埋め込みアセット（github-markdown css、highlight、mermaid、KaTeX + フォント） |

## ブラウザシェル

- 2ペイン + ToC: サイドバーツリー（chevron 開閉・md 件数・**ファイル名フィルタ**）/ 本文（`.markdown-body`、最大幅 900px）/ 右に **ToC パネル**（level インデント、クリックでスクロール。幅 900px 未満では非表示）
- 本文ロード後に desktop と同じ post-render: highlight.js、mermaid（`securityLevel: 'strict'`、テーマ連動）、KaTeX auto-render（`$$..$$` / `\(..\)` / `\[..\]`、単一 `$` 無効）
- 内部リンク（`a.mdo-link`、`data-path` はサーバ側で包含検証済み）はクリックで同ページ内遷移。URL ハッシュに現在パスを保持しリロード復元
- **テーマ**: `prefers-color-scheme` 追従 + ヘッダのトグルボタン。切替時は現在ドキュメントを再ロードして mermaid を再描画
- **live-reload**: 表示中ドキュメントの `/api/stat` を 700ms 間隔でポーリングし、mtime 変化で再取得（スクロール位置保持）。ツリーは 3s 間隔で `/api/tree` を取得して差分時のみ再描画
- 初期表示: URL ハッシュがあればそのファイル、なければルートの README、なければ最浅のファイル

## 実装

- `src/serve.rs`（サーバ・ルーティング・検証）+ `src/serve/shell.rs`（シェル HTML 一枚）
- 依存: `tiny_http`（同期・シングルスレッドで十分。localhost の単一閲覧者想定）、`include_dir`、`percent-encoding`
- アセット埋め込みでバイナリは +約4MB（mermaid.min.js が大半）
