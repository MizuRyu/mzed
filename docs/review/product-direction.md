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

- Zed のアクティブプロジェクト追従
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
- 性能変更は [performance-guide.md](performance-guide.md) の指標で前後比較する。

## 配布方針

最初の製品版は macOS を対象とする。
署名、notarization、配布チャネルは release phase で決める。未決定の間は、開発用ビルドを正式配布物として扱わない。

## 非方針

v1 では Tauri/SolidJS への移行を計画しない。
性能や配布の課題は、まず Dioxus root crate 内の責務分離、計測、macOS 配布設計で解決する。
