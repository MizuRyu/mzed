---
name: mzed-change-docs
description: >
  mzed のコード変更後にドキュメント3点セットを同期する（docs/specs の該当仕様、
  manual-test-cases.md の TC、mzed-setup スキルの config/キーバインド表）。
  機能追加・挙動変更・config 追加・キーバインド追加を実装した直後、および
  実装エージェントへの指示書に docs 同期を含めるときに使う。
---

# mzed 変更後ドキュメント同期

実装変更をマージ可能にする前に、以下を漏れなく同期する。仕様の正は `docs/specs/`
（索引は `docs/README.md`）。すべて日本語で書く。`docs/memo/` は gitignore
（公開されない）なので同期対象ではない。

## 同期チェックリスト

### 1. docs/specs/ の該当仕様（常に）

変更内容がどの spec に属するか判断して更新。冒頭の「最終確認日」も更新する。

| spec | 担当領域 |
|---|---|
| 03-features | 機能一覧（v1 + 将来検討、P-xx/C-xx 等の ID） |
| 04-project-structure | ディレクトリ構成・モジュール・session/config スキーマ |
| 05-zed-integration | Zed 連動・sync モード・マルチウィンドウ挙動 |
| 06-rendering-pipeline | Markdown→HTML、wikilink、画像、生 HTML サブセット、KaTeX |
| 07-ipc-and-concurrency | IPC・ソケット・スレッド・状態管理 |
| 08-export | HTML/PDF エクスポート |
| 10-settings-and-context-menu | 設定画面・config 表・キーバインド表・右クリック |
| 11-task-view | Task View（走査方針・行操作・設定） |

新しい領域なら `NN-<名前>.md` を新設し、`docs/README.md` の索引に行を追加する。

### 2. manual-test-cases.md の TC（挙動が変わったら）

`docs/specs/manual-test-cases.md` に追記。書式:

```markdown
### <PREFIX>-NN <タイトル>

| 項目 | 内容 |
|---|---|
| **前提** | ... |
| **手順** | ... |
| **期待結果** | ... |
```

- TC-ID は永続識別子（変更・再利用しない）。既存カテゴリ: SEC / WIKI / IMG / HTML /
  EXP / ZED / WIN / SES / SYN / FILE / DIST / TV。新領域は新プレフィックスを起こす
- 自動テスト化済みの項目は「自動化済み: <テストファイル>」と注記して手動対象から外す
- バグ修正は「再発したら気づける TC」を必ず1つ追加する

### 3. mzed-setup スキル（config / キーバインドを変えたら）

`.agents/skills/mzed-setup/SKILL.md` を更新:

- config フィールド追加 → 「config.json フィールド一覧」表に行を追加
  （フィールド名 / 型 / デフォルト / 意味）
- キーバインド追加 → 「デフォルトキーバインド」表に行を追加
- インストール・CLI・トラブルシュートの挙動が変わったら該当節も

## 検証

同期漏れの機械チェック（新しい config フィールド名・キーバインドアクション名で）:

```sh
rg -l "<新フィールド名>" src/config.rs docs/specs/10-settings-and-context-menu.md .agents/skills/mzed-setup/SKILL.md
# → 3ファイルすべてに現れること
```

README.md（ユーザー向け）はキーバインド表と機能一覧を持つ。ユーザーが日常で使う
変更（新ショートカット等）なら README も更新する。
