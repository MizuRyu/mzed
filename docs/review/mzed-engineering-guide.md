# mzed エンジニアリングガイド

## 目的

mzed 固有の設計方針をまとめる。
Rust 一般の作法は [rust-coding-guide.md](rust-coding-guide.md) に置く。

## プロダクト方針

mzed は Zed 連動の軽量 Markdown viewer。
中心価値は次の順に置く。

1. Zed のプロジェクト切替に素早く追従する。
2. Markdown を GitHub に近い見た目で正しく読める。
3. UI が軽く、入力と切替を邪魔しない。
4. CLI からすぐ開ける。

編集機能は主目的にしない。

## 現在の前提

- 製品版は Dioxus 0.7 desktop と Rust を継続する。
- `prototype/` は repository root へ昇格済み。以後は root crate を製品版として育てる。
- 方針の根拠と再検討条件は [product-direction.md](product-direction.md) に置く。

## 責務境界

### App shell

担当:

- 起動
- CLI intent の適用
- window / menu
- top-level component の組み立て

禁止:

- Markdown parse
- Zed DB query
- ファイル tree scan
- export 本体
- JS 文字列の大きな組み立て

### UI components

担当:

- 表示
- ユーザーイベントの発火
- 小さなローカル表示状態

禁止:

- `std::fs::*` の直接呼び出し
- watcher の直接起動
- DB query
- 外部プロセス起動
- 設定ファイル保存

### App state

担当:

- tabs
- active project
- split pane
- settings
- session restore
- palette/search state

方針:

- 状態遷移は UI component から切り出す。
- 文字列 action ではなく enum で表す。
- 保存可能な状態と一時 UI 状態を分ける。

### Services

担当:

- filesystem
- Zed DB
- watcher
- export
- clipboard / Finder / trash
- single instance IPC

方針:

- UI からは service API を呼ぶ。
- service は `Result` を返す。
- macOS 固有処理は `platform` 境界に閉じ込める。

### Markdown

担当:

- parse
- frontmatter
- alerts
- toc
- relative link
- image handling
- sanitization policy

方針:

- Markdown 本文は信頼しない。
- HTML post-process は入力と出力をテストする。
- Mermaid / KaTeX は WebView 側仕上げでも、信頼境界を明示する。

## UI 規約

- アプリは viewer として静かにする。装飾より可読性を優先する。
- サイドバー、タブ、検索、ToC は切替速度を優先する。
- command palette は「速く開いて速く絞れる」ことを優先する。
- 設定画面は状態変更の入口であり、保存処理本体を持たない。
- 右クリックメニューや Finder 操作は platform service へ委譲する。

## Zed 連動

- Zed DB が無い状態は正常系。
- DB schema 差分や lock はアプリ全体を落とさない。
- multi-root workspace は最初から考慮する。
- プロジェクト切替時は、キャッシュがあれば即表示し、裏で再スキャンする。
- `auto` / `self` / `off` の挙動は pure function としてテストする。

## Markdown security

- 生 HTML は許可しない。ユーザー由来の `Event::Html` / `Event::InlineHtml` は text として表示する。
- `javascript:` URL を許可しない。
- `data:` / `file:` / protocol-relative URL を Markdown link / image の信頼入力にしない。
- HTML event 属性を許可しない。allowlist sanitizer を追加する場合も、event 属性は既定拒否にする。
- Mermaid は `securityLevel: 'strict'` を既定にする。
- 相対画像と内部 `.md` link は canonical path が project root 配下に収まる場合だけ解決する。
- 外部 URL と内部 Markdown link を分ける。
- 外部 URL は OS ブラウザへ逃がす。
- `document::eval` に Markdown 本文、ファイルパス、検索語を直接埋め込まない。
- JS に値を渡すときは `serde_json` で JSON 文字列にする。

### OK

```rust
let query_json = serde_json::to_string(&query).unwrap_or_else(|_| "\"\"".to_string());
let js = find_highlight_js(&query_json);
```

値を JS literal として安全に渡している。

### NG

```rust
let js = format!("highlight('{}')", query);
```

検索語に quote や script 断片が含まれると壊れる。

## Config / session

- config はユーザーが意図して変更する設定。
- session はアプリが自動保存する復元状態。
- config の破壊的変更は migration 方針を書く。
- session restore では存在しないファイルを落とす。
- 保存頻度が高い処理は debounce を検討する。

## Release

- 未署名配布か署名配布かを README に明記する。
- Gatekeeper 回避手順は、個人利用向けの暫定手順として扱う。
- Homebrew 配布を v1 に含めるなら、bundle、license、version、artifact 名を固定する。

## 判断待ちにするもの

次はレビューだけで決めない。

- 生 HTML allowlist を製品版で追加するか。
- 未署名配布を v1 として許容するか。
- UI 操作感が変わる大きな再設計。
