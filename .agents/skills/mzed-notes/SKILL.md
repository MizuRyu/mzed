---
name: mzed-notes
description: >
  mzed のビューアで本文に付けられたメモ（注釈）を読み、指示どおりに Markdown を直す。
  「mzed のメモを見て」「注釈を反映して」「メモを処理して」と頼まれたとき、および
  ドキュメント修正の作業を始める前に未処理のメモがあるか確認するときに使う。
---

# mzed のメモを読んで反映する

mzed のメモは「読んだ人が本文を選択して残した修正指示」。1 メモ 1 JSON ファイルで、
プロジェクトの中ではなくグローバルの保存先に溜まる。エージェントはそれを読んで直し、
処理したファイルを `done/` へ移す。

## 保存先

```
~/.config/mzed/notes/            未処理のメモ（*.json）
~/.config/mzed/notes/done/       処理済み（自分で作る）
```

ファイル名は `<UTC yyyyMMddTHHmmssZ>-<8桁hex>.json` で、`ls` が古い順に並ぶ。

## ファイル形式

```json
{
  "version": 1,
  "created_at": "2026-09-12T01:22:05Z",
  "project_root": "/Users/me/dev/repos/foo",
  "file": "/Users/me/dev/repos/foo/docs/plan.md",
  "rel_path": "docs/plan.md",
  "heading": "## 実装方針",
  "quote": "選択された本文",
  "note": "ここは Orca 由来のイベントも対象にして"
}
```

| フィールド | 意味 |
|---|---|
| `project_root` | メモを付けた時のアクティブプロジェクト root（root 外のファイルはその親ディレクトリ） |
| `file` | 対象ファイルの絶対パス |
| `rel_path` | `project_root` 基準の相対パス |
| `heading` | 選択位置より上の最も近い見出し（`## 見出し` 形式）。無ければ `null` |
| `quote` | 選択された本文そのまま（行番号は持たない。これで grep する） |
| `note` | ユーザーが書いた指示 |

## 手順

### 1. 今のプロジェクトのメモを一覧する

```sh
ls ~/.config/mzed/notes/*.json 2>/dev/null   # 1件も無ければここで終わり
```

絞り込みは `project_root` の**完全一致**が既定。リポジトリのサブディレクトリで作業していても同じ結果になるよう、root は cwd ではなく git から取る:

```sh
ROOT=$(git rev-parse --show-toplevel 2>/dev/null || echo "$PWD")

jq -r --arg root "$ROOT" '
  select(.project_root == $root)
  | "\(input_filename)\n  file: \(.rel_path)\n  head: \(.heading // "-")\n  quote: \(.quote)\n  note: \(.note)\n"
' ~/.config/mzed/notes/*.json
```

メモ側の `project_root` が今の root の**祖先**になっていることがある（root 外のファイルを開いていた、別の親フォルダで開いていた等）。それも拾うなら区切り `/` を必ず付けて比較する:

```sh
jq -r --arg root "$ROOT" '
  select(.project_root as $p | $p == $root or ($root | startswith($p + "/")))
  | input_filename
' ~/.config/mzed/notes/*.json
```

- `.project_root as $p` で先に束縛する。`$root | startswith(.project_root + "/")` と書くと `.` が `$root`（文字列）になり `Cannot index string with string` で落ちる
- 比較は必ず `/` を付ける。付けないと `/repo` のメモが `/repo-other` にマッチする
- 逆向き（`project_root` が root の子）を拾うときも同じ形で `select(.project_root | startswith($root + "/"))`

他プロジェクトのメモは**触らない**（残したまま次の作業へ渡す）。

### 2. 対象箇所を特定する

`quote` を対象ファイルの中で探す。正規表現ではなく固定文字列で引く:

```sh
NOTE=~/.config/mzed/notes/20260912T012205Z-1a2b3c4d.json
TARGET=$(jq -r '.file' "$NOTE")
jq -r '.quote' "$NOTE" | head -1 | rg -F -n -f - "$TARGET"
```

`quote` が複数行のときは 1 行目だけで引く（上のコマンドの `head -1`）。
それでも見つからないときは `heading` で該当セクションへ降りてから、`quote` の特徴的な語で探す。

### 3. `note` に従って直す

- 直すのは `file` が指す Markdown だけ。メモの JSON は書き換えない
- `note` は選択箇所への指示。**引用箇所の外まで広げて直さない**
- 指示が曖昧で複数の解釈があるときは直さずに 5 の「残す」へ回す

### 4. 処理したメモを `done/` へ移す

```sh
move_done() {
  src=$1
  dir=~/.config/mzed/notes/done
  mkdir -p "$dir"
  dest=$dir/$(basename "$src")
  n=1
  while [ -e "$dest" ]; do
    dest=$dir/$(basename "$src" .json)-$n.json
    n=$((n + 1))
  done
  mv -n "$src" "$dest"
}

move_done "$NOTE"
```

`mv -n`（上書きしない）と連番で、`done/` にある過去のメモを潰さない。
移すのは**直し終えた後**。移動だけで完了を表すので、JSON に状態を書き足す必要はない。

### 5. 処理しないメモは残す

次のどれかに当たるメモは、`done/` へ移さずそのまま残し、理由をユーザーへ報告する。

- `file` が既に存在しない、または `quote` が見つからない（本文が変わった後のメモ）
- 指示の解釈が複数あり、どちらで直すか決められない
- 直すと他の記述と矛盾する、スコープ外の変更になる

残したメモは次回も一覧に出る。ユーザーが判断したら改めて処理する。

## 注意

- mzed は**このディレクトリにファイルを作るだけ**、エージェントは**読んで移すだけ**。同じファイルを両方が書かないので排他は不要
- `done/` の中身は消さない（何を根拠に直したかの記録）
- メモは mzed のデスクトップアプリ専用。`mzed serve`（ブラウザ表示）からは作られない
