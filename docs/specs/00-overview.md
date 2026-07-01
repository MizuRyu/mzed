# 00 - 概要 / 索引

## mzed とは

Zed エディタ連動の Markdown ビューア。Zed でプロジェクトを切り替えると、対応する docs が瞬時に切り替わる。arto の読みやすさ、mo の CLI とグループ管理、Zed のパフォーマンス思想を取り込む。

- コマンド名: `mzed`
- 技術: **Dioxus 0.7（desktop / webview = tao+wry）+ Rust**。Mermaid/KaTeX のみ JS
- 対象: macOS（個人ツール。配布は GitHub Release で未署名 — 自分＋数人規模、Apple 署名はしない）

> 実装状況（2026-07-02）: 実装済み機能の一覧は [03-features.md](03-features.md) を正とする。本 specs は設計の確定事項を記録する位置づけ。

## 設計の3本柱

1. Zed 連動 — プロジェクト切替を検知し docs を完全入れ替え（→ [05](05-zed-integration.md)）
2. パフォーマンス — メインスレッドを止めない。Zed の思想を Dioxus で実践（→ [07](07-ipc-and-concurrency.md)）
3. GitHub 忠実なレンダリング — pulldown-cmark + lol_html + highlight.js / Mermaid / KaTeX（→ [06](06-rendering-pipeline.md)）

## 仕様書索引

| # | ファイル | 内容 |
|---|---|---|
| 00 | overview | 概要・索引（本書） |
| 01 | [architecture](01-architecture.md) | システム構成、Zed 連動、データフロー |
| 02 | [tech-stack](02-tech-stack.md) | 技術選定と理由（Dioxus 確定版） |
| 03 | [features](03-features.md) | 機能一覧（v1 + 将来検討） |
| 04 | [project-structure](04-project-structure.md) | ディレクトリ・モジュール構成 |
| 05 | [zed-integration](05-zed-integration.md) | Zed 連動の技術詳細 |
| 06 | [rendering-pipeline](06-rendering-pipeline.md) | MD パース〜表示 |
| 07 | [ipc-and-concurrency](07-ipc-and-concurrency.md) | IPC・スレッド・状態管理 |
| 08 | [export](08-export.md) | HTML / PDF エクスポート |
| 10 | [settings-and-context-menu](10-settings-and-context-menu.md) | 設定画面・右クリック・キーバインド |

## 決定事項

- 名称・フォルダ名とも **mzed** に確定
- **Dioxus 採用**（Tauri / SolidJS / React は不採用）。GPUI も不採用（MD ビューアには WebView で十分）
- MD パースは **pulldown-cmark**（comrak でなく）、ハイライトは **highlight.js（JS）**、HTML 後処理は **lol_html**、単一インスタンスは **interprocess**（いずれも arto 準拠）
- マルチルート Zed ワークスペースは v1 でフラット集約（実装済み）
- 配布は GitHub Release（未署名）。Apple 署名・notarization・Homebrew tap はしない

## 既知の未解決

- mermaid インラインのガント/ジャーニー/マインドマップ崩れ（別ウィンドウ表示では正しく出る）
- PDF は `window.print()` 方式。ワンクリック保存や WKWebView createPDF は将来検討。
