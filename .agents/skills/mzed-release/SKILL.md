---
name: mzed-release
description: >
  mzed の新バージョンをリリースする（バージョン bump → 手書きリリースノート →
  タグ・push → dmg ビルド → GitHub Release → ローカル更新）。「vX.Y.Z 切って」
  「リリースして」「新バージョン出して」と頼まれたときに使う。バージョンの
  目安（patch/minor/major）の判断にも使う。
---

# mzed リリース手順

タグ・Release・dmg は常にワンセット。`scripts/install.sh` は「最新 Release の dmg」を
取得するため、タグだけ打っても main に push しただけでも各端末には配布されない。

## 事前条件

1. 作業ツリーがクリーン（未コミットはユーザーに確認して先にコミット）
2. ユーザーの動作確認が済んでいる（大きい変更は `docs/specs/manual-test-cases.md` の該当 TC を案内）
3. バージョン番号の判断: `docs/development.md` の semver 目安を参照
   （patch=バグ修正・見た目、minor=新機能、major=互換破壊）

## 手順

```sh
# 1. バージョン bump（Cargo.toml の version）→ Cargo.lock 更新 → コミット
#    Edit で version を書き換え、cargo check で lock を更新
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to X.Y.Z" # 英語 conventional commits + Co-Authored-By

# 2. リリースノートを手書きして /tmp に置く（後述の規約）
#    --generate-notes は使わない。inline --notes は bash フックが
#    /Applications 等の絶対パスに誤反応するため必ず --notes-file を使う

# 3. 古いバンドルを消してからタグ・push
rm -r target/dx/mzed/bundle   # 古いハッシュ付きアセットの混入防止（必須）
git tag vX.Y.Z
git push origin main vX.Y.Z   # pre-push ゲートが走る（gitleaks + 個人情報 + verify）

# 4. ビルドして Release 作成
just bundle                    # ad-hoc 再署名 + dmg 生成込み
gh release create vX.Y.Z target/dx/mzed/bundle/macos/macos/mzed_X.Y.Z_aarch64.dmg \
  --title "mzed vX.Y.Z" --notes-file /tmp/mzed-vX.Y.Z-notes.md

# 5. 検証 + ローカル更新
gh release view --json tagName,assets -q '.tagName + " / " + (.assets[0].name)'  # latest 確認
just install                   # ローカルも新版に（起動中なら Cmd+Q → 再起動を案内）
```

## リリースノートの規約

日本語・手書き（commit の羅列にしない）。構成:

- 一行サマリ
- `## 新機能`（あれば）: **機能名** — ユーザー視点の説明
- `## 改善・修正`（あれば）: 症状ベースで書く（「〜する問題を修正」）
- `## インストール / 更新`: curl 一行
  `curl -fsSL https://raw.githubusercontent.com/MizuRyu/mzed/main/scripts/install.sh | bash`
- 末尾に「macOS / Apple Silicon 向け。」

## 落とし穴

- **push がブロックされたら**: pre-push ゲート（`scripts/check-push.sh`）の失敗理由を
  読む。flaky テストなら再実行、本物の検出なら直してから push。verify だけ飛ばす
  非常口は `MZED_SKIP_VERIFY=1 git push`（秘密情報検査は飛ばない）
- **`just release` は使わない**: `--generate-notes`（自動生成）のため。手書きノート方針
- **bundle 前の `rm -r target/dx/mzed/bundle`**: 忘れると過去ビルドのハッシュ付き
  アセットが .app に混入する
- コミットメッセージは英語 conventional commits、末尾に
  `Co-Authored-By: Claude <モデル名> <noreply@anthropic.com>`
