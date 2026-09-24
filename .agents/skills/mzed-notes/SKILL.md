---
name: mzed-notes
description: >
  mzed のビューアで本文に付けられたメモ（注釈）を読み、指示どおりに Markdown を直す。
  「mzed のメモを見て」「注釈を反映して」「メモを処理して」と頼まれたとき、および
  ドキュメント修正の作業を始める前に未処理のメモがあるか確認するときに使う。
  人間が付けたメモは全件反映し、結果を記録して done/ へ移す。
---

# mzed のメモを読んで反映する

mzed のメモは「読んだ人が本文を選択して残した修正指示」。1 メモ 1 JSON ファイルで、
プロジェクトの中ではなくグローバルの保存先に溜まる。エージェントはそれを全件反映し、
結果を書き足して `done/` へ移す。

## 保存先

```
~/.config/mzed/notes/            受信箱: 未処理のメモ（*.json）
~/.config/mzed/notes/done/       処理済み（無ければ作る）
```

ファイル名は `<UTC yyyyMMddTHHmmssZ>-<8桁hex>.json` で、`ls` が古い順に並ぶ。
mzed は受信箱にファイルを作るだけで既存のメモを書き換えない。エージェントがメモを
書き換えるのは `done/` へ移すときだけなので、排他は不要。

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
| `quote` | 選択された本文そのまま（行番号は持たない。これで検索する） |
| `note` | ユーザーが書いた指示 |

処理済みのメモには次の 3 つが加わる。

| フィールド | 値 |
|---|---|
| `status` | `applied`（ファイルを直した）または `skipped`（反映できなかった） |
| `resolved_at` | UTC 時刻（`date -u +%Y-%m-%dT%H:%M:%SZ`） |
| `resolution` | 何を直したか、または反映しなかった理由を 1 行 |

## 手順

### 1. 今のプロジェクトのメモを一覧する

```sh
ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd -P)
ls ~/.config/mzed/notes/*.json 2>/dev/null || exit 0   # 1 件も無ければ終わり

jq -r --arg root "$ROOT" '
  select(.project_root as $p | $p == $root or ($root | startswith($p + "/")) or ($p | startswith($root + "/")))
  | "\(input_filename)\n  file: \(.rel_path)\n  head: \(.heading // "-")\n  quote: \(.quote)\n  note: \(.note)\n"
' ~/.config/mzed/notes/*.json
```

- `.project_root as $p` で先に束縛する。`$root | startswith(...)` の中では `.` が `$root`（文字列）になる
- 比較は必ず `/` を付ける。付けないと `/repo` のメモが `/repo-other` にマッチする
- 他プロジェクトのメモは受信箱に残して触らない。プロジェクトを問わず全件と言われたら `select` を外す

古い順（ファイル名順）に処理する。後のメモが同じ箇所の前のメモを補うことがある。

### 2. 対象箇所を特定する

```sh
NOTE=~/.config/mzed/notes/20260912T012205Z-1a2b3c4d.json
TARGET=$(jq -r '.file' "$NOTE")
jq -r '.quote' "$NOTE" | head -1 | rg -F -n -f - "$TARGET"
```

固定文字列で、複数行の `quote` は 1 行目だけで引く。見つからないときは `heading` の節へ
降りてから `quote` の特徴的な語で探す。メモの後に本文が編集されていることがあるので、
位置ではなく意味で合わせる。

### 3. すべてのメモを反映する

人間が付けたメモは全件反映する。

- 直すのは `file` が指す Markdown だけ。引用箇所と指示が直接求める範囲の外まで広げない
- 解釈が複数あるときは、周囲の文脈に最も合う解釈で直し、どの解釈にしたかを `resolution` に書く
- 指示が質問の形なら、チャットで答えるだけでなく引用箇所の本文に答えを反映する

反映しない（`skipped`）のは、`file` が消えている、または `quote` もその見出しの節も
見つからない場合だけ。

### 4. 結果を記録して `done/` へ移す

```sh
resolve_note() {  # resolve_note <note> <applied|skipped> <resolution>
  src=$1; dir=~/.config/mzed/notes/done
  mkdir -p "$dir"
  dest=$dir/$(basename "$src"); n=1
  while [ -e "$dest" ]; do dest=$dir/$(basename "$src" .json)-$n.json; n=$((n + 1)); done
  jq --arg s "$2" --arg r "$3" --arg t "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
     '. + {status: $s, resolved_at: $t, resolution: $r}' "$src" > "$dest.tmp" \
    && mv -n "$dest.tmp" "$dest" && rm "$src"
}

resolve_note "$NOTE" applied "再試行の段落を番号付きリストに書き換えた"
```

移すのは直した内容を保存した後。`done/` は何を根拠に直したかの記録なので消さない。

### 5. 報告する

メモごとに「ファイル / 何を直したか（または反映しなかった理由）」を並べる。
他プロジェクトのメモを受信箱に残した場合はその件数も書く。

履歴の確認:

```sh
jq -r '"\(.resolved_at) \(.status) \(.rel_path): \(.resolution)"' ~/.config/mzed/notes/done/*.json
```

## 注意

- メモは mzed のデスクトップアプリ専用。`mzed serve`（ブラウザ表示）からは作られない
