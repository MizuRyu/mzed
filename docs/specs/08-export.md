# 08 - エクスポート

コマンドパレットから現在のファイルを HTML / PDF に書き出す。VSCode の Markdown PDF 拡張のような体験。

## 方針

WebView 上のレンダリング結果（GitHub CSS + Mermaid SVG + KaTeX 済み）を、見た目そのまま書き出す。Mermaid と KaTeX は描画後の DOM に SVG/MathML として存在するので、それを取り込めば忠実性を保てる。

## MD → HTML

self-contained な単一 HTML を出す。外部依存なしで、どこでも開ける状態にする。

```mermaid
flowchart LR
    DOM[描画済み DOM] --> Serial[DOM シリアライズ]
    Serial --> Inline[CSS インライン化]
    Inline --> Embed[画像を data URI 埋め込み]
    Embed --> Save[単一 .html 保存]
```

- 出力先に同名ファイルが存在する場合は `title (1).html`、`title (2).html`... と連番を付与して衝突を回避する（最大 100 件で打ち切りエラー）
- ライブビューの描画済み `.markdown-body` の innerHTML をキャプチャ（操作 UI のコピーボタン・mermaid ツールバーは除去）
- **白背景の markdown-pdf 風ページ**で包む（github-markdown **light** + 余白・中央寄せ max-width 880px）。アプリのテーマに依らず常に白
- Mermaid は **light テーマで再描画した SVG** を埋め込む（退避ソース `data-mdo-src` から再レンダリング）。ダークで作業中でも白ページで読みやすい
- KaTeX・シンタックスハイライトは描画済みで入る。画像は data URL 済み
- **出力先は設定の「エクスポート先フォルダ」（既定 OS の Downloads）に 1 クリックで直接書き出し**。ダイアログは出さない。完了で「HTML generated!」トースト

実装メモ: 旧版はアプリ用 `mdo.css`（`overflow:hidden`）をインラインしていたため単体 HTML がスクロール不能だった。専用の軽量ページ CSS（`export.rs` の `PAGE_CSS`）に差し替えて解消。

## MD → PDF

二段構え。

### 簡易版（先行実装）

`window.print()` を呼び、OS の印刷ダイアログから「PDF として保存」。print 用 CSS（`@media print`）で余白やページ区切りを整える。バックエンド実装ゼロで出せる。

### 忠実版（後追い）

chromiumoxide でヘッドレス Chromium を起動し、エクスポート用 HTML を読み込んで PDF 化する。

```mermaid
flowchart LR
    HTML[self-contained HTML] --> Headless[chromiumoxide]
    Headless --> Render[Chromium 描画]
    Render --> PDF[printToPDF]
    PDF --> Save[.pdf 保存]
```

- Mermaid SVG・KaTeX も Chromium が忠実に描画
- ページサイズ・余白・ヘッダフッタを指定可能
- 代償: Chromium バイナリ依存（バンドル増）。フィーチャーフラグで分離し、必要時のみ有効化

判断（2026-06-28 更新）: 簡易版（print）のみ実装済み。**真のワンクリック PDF 生成は保留**。本命は **macOS ネイティブ `WKWebView.createPDF`（objc2 FFI、依存追加ゼロ）** — エクスポート用 HTML をオフスクリーン WebView に読み込み → createPDF → Downloads 直書き、という流れ。chromiumoxide は Chromium バイナリ（〜150MB）同梱で重く、軽量志向と矛盾するため非採用寄り。

## 将来のエクスポート（FT）

| 形式 | 用途 | 候補技術 |
|---|---|---|
| DOCX | テンプレート指定で仕様書発行 | docx-rs / pandoc |
| XLSX | テーブル → Excel | rust_xlsxwriter |

これらは「フォーマット指定で文書生成」という別系統の機能。ビューア本体とは分けて設計する。

## コマンドパレットからの導線

| コマンド | 動作 |
|---|---|
| `Export: HTML` | 現在のファイルを self-contained HTML で**エクスポート先フォルダに直接保存**（既定 Downloads）→ トースト |
| `Export: PDF` | 簡易版（print ダイアログ →「PDF として保存」） |

- 出力先は設定 General の「エクスポート先フォルダ」（既定 Downloads。選択・リセット可）で管理。毎回ダイアログは出さない。
- **フィーチャーフラグ**: 設定 Features で `HTML エクスポート` / `PDF エクスポート` をオンオフ。オフにするとコマンドパレットからも消える（KaTeX 描画も同様にフラグ化）。
