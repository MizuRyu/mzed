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

1. `Cargo.toml` の version を更新
2. `git tag vX.Y.Z && git push origin vX.Y.Z`
3. `just bundle` で `.dmg` を生成
4. `gh release create vX.Y.Z target/dx/mzed/bundle/macos/macos/*.dmg --title "mzed vX.Y.Z"`

Apple 署名 / notarization はしない（配布規模が小さいため意図的に未署名）。

## テスト

- ユニット + 統合テスト: `cargo test`（単一インスタンス IPC は `tests/instance_integration.rs`）
- 手動回帰テスト: [specs/manual-test-cases.md](specs/manual-test-cases.md)（TC-ID 付き、リリース前に流す）
- レンダリング確認用フィクスチャ: `tests/fixtures/showcase.md`

## 技術スタック

Dioxus 0.7（desktop / tao + wry）、pulldown-cmark、highlight.js、mermaid、KaTeX、Zed の SQLite 監視（notify + rusqlite）。

設計ドキュメントの索引は [README.md](README.md)、コーディング規約は [review/coding-standards.md](review/coding-standards.md) を参照。
