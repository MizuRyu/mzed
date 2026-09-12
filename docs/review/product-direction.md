# mzed 製品版方針

## 決定

製品版は Dioxus 0.7 desktop と Rust を継続する。
`prototype/` から repository root へ昇格した現行 crate を製品版として育て、Tauri/SolidJS への移行は行わない。

決定日: 2026-06-28

## 理由

- 現行機能が Dioxus 上で動作しており、UI 基盤の移行より責務分離と品質改善を優先できる。
- Rust 内で Markdown、Zed 連携、watcher、状態遷移を完結できる。
- WebView 向け JS を境界化すれば、Dioxus 固有コードを app shell と UI に限定できる。
- 製品価値である起動、切替、表示の軽さを、現行実装との比較で継続測定できる。

## v1 の範囲

- Zed / Orca のアクティブプロジェクト追従
- CLI、single instance、drag and drop
- 複数プロジェクト、タブ、split pane
- Markdown、frontmatter、alerts、Mermaid、KaTeX、syntax highlight
- sidebar、ToC、検索、command palette、設定
- HTML/PDF export
- Finder、外部アプリ、clipboard、Trash 連携
- config と session の復元

Markdown 編集機能は含めない。

## 技術方針

- Markdown parser は当面 `pulldown-cmark` を継続する。
- Markdown 由来 HTML は security policy を通してから WebView へ渡す。
- UI component は filesystem、DB、watcher、外部プロセスを直接扱わない。
- watcher と background thread は停止条件を持つ service が所有する。
- JS 文字列と注入値は `js` モジュールに閉じ込め、値は JSON encode する。
- 追従元（Zed / Orca）の選択は pure function にしてテストする。
- 性能変更は [performance-guide.md](performance-guide.md) の指標で前後比較する。

## 配布方針

最初の製品版は macOS を対象とする。
署名、notarization、配布チャネルは release phase で決める。未決定の間は、開発用ビルドを正式配布物として扱わない。

## 非方針

v1 では Tauri/SolidJS への移行を計画しない。
性能や配布の課題は、まず Dioxus root crate 内の責務分離、計測、macOS 配布設計で解決する。

## 再検討記録

### 2026-09-09: ox-content への parser 差し替え — 見送り

`ox_content_parser` / `ox_content_renderer` を pulldown-cmark の代替として spike した（記録: ローカル `docs/memo/tasks/260909-01-v2大型update計画/p4-result.md`）。

- 性能: 通常の 1MB 散文で差なし。ox が速く見えたのは pulldown-cmark 0.13 の脚注処理が O(n²) の場合のみ。mzed の実コストは自前の二重パースが主因で、parser を変えずに直せる。
- 防御: ox の `disallow_raw_html` は GFM tagfilter（9 タグ）で全エスケープではない。属性のエスケープ形式も違い、`raw_html::reconstruct_allowed` が壊れる。protocol-relative URL を通す。
- 互換: 純 Rust 経路に frontmatter が無い。alerts クラス名、mermaid の `pre class`、table alignment の DOM が異なる。
- 部分取り込み: ハイライト（tree-sitter）と mermaid の crate は crates.io 未公開。sanitize の許可リストは mzed より広い。

再検討条件: ox-content が frontmatter を純 Rust 経路で提供し、raw HTML を全エスケープするモードを持ち、ハイライト crate を公開したとき。
