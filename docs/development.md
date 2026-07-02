# 開発ガイド

## セットアップ

devShell（cargo / dx / node / sqlite 等）を nix flake で提供している:

```sh
direnv allow      # .envrc 経由（推奨）
nix develop       # 直接入る場合
```

## 日常コマンド

```sh
just run          # mzed ウィンドウ起動（開発実行）
just watch        # Zed 監視ログのみ（standalone bin: zed_watch）
just verify       # fmt --check + clippy -D warnings + test（完了前に必ず通す）
```

pre-commit フック（gitleaks + cargo fmt --check）を導入している:

```sh
pre-commit install
```

## ビルド & インストール

```sh
just bundle       # target/dx/mzed/bundle/macos/macos/ に mzed.app と .dmg を生成
just install      # /Applications にコピー + quarantine 解除 + ~/.local/bin/mzed symlink
just uninstall    # .app と symlink を削除
```

- `just install` は未署名ビルドのため `xattr -dr com.apple.quarantine` を自動実行する
- `~/.local/bin` に PATH が通っていなければ `export PATH="$HOME/.local/bin:$PATH"` を追加
- nix の package output は用意していない。`dx bundle` がアセット解決でネットワークを要し nix sandbox で通らないため、`.app` 生成はローカルの `just bundle` に寄せている

## リリース

タグ・Release・dmg は常にワンセット。`scripts/install.sh` は「最新 Release の dmg」を取得するため、タグだけ打っても各端末には配布されない。

### バージョンの目安（semver）

| 種別 | 例 | いつ切るか |
|---|---|---|
| patch（v1.0.x） | バグ修正、見た目のリグレッション修正、エラーメッセージ改善 | ユーザーに見える修正が1件でも溜まり、他端末に配りたくなったら。溜め込まず気軽に切る |
| minor（v1.x.0） | 新機能、キーバインド追加、後方互換な挙動追加 | 機能がひとまとまり動くようになったら |
| major（vX.0.0） | config/セッションの互換が壊れる変更、UI 大改編 | 移行手順を README に書ける状態になってから |

- main は常にグリーン（`just verify` 通過）を保ち、リリースは main の任意時点から切る
- リファクタ・docs・CI だけの変更はリリース不要。次の patch/minor に相乗りさせる

### 手順

```sh
# Cargo.toml の version を上げてコミットした後:
just release
```

`just release` は クリーンツリー確認 → verify → タグ付与 → push → bundle → dmg 付き Release 作成（ノートは commit から自動生成）まで行う。

Apple 署名 / notarization はしない（配布規模が小さいため意図的に未署名）。

## テスト

- ユニット + 統合テスト: `cargo test`（単一インスタンス IPC は `tests/instance_integration.rs`）
- 手動回帰テスト: [specs/manual-test-cases.md](specs/manual-test-cases.md)（TC-ID 付き、リリース前に流す）
- レンダリング確認用フィクスチャ: `tests/fixtures/showcase.md`

## 技術スタック

Dioxus 0.7（desktop / tao + wry）、pulldown-cmark、highlight.js、mermaid、KaTeX、Zed の SQLite 監視（notify + rusqlite）。

設計ドキュメントの索引は [README.md](README.md)、コーディング規約は [review/coding-standards.md](review/coding-standards.md) を参照。
