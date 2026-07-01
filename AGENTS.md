# AGENTS.md

mzed — Zed 連動 Markdown ビューア（Rust + Dioxus 0.7、macOS desktop）。

## 仕様とガイドライン

- 現行仕様は `docs/specs/` が正。索引は `docs/README.md`
- コーディング規約は `docs/review/coding-standards.md` を入口に読む
  - Rust 一般: `docs/review/rust-coding-guide.md`
  - mzed 固有設計（レンダリング責務、Zed 連動、security）: `docs/review/mzed-engineering-guide.md`
  - 性能方針: `docs/review/performance-guide.md`
- 手動回帰テスト: `docs/specs/manual-test-cases.md`（TC-ID 付き）
- インストール・設定・キーバインドの操作手順: `.claude/skills/mzed-setup/SKILL.md`

## コマンド

```sh
just run       # 開発実行
just verify    # fmt --check + clippy -D warnings + test（完了前に必ず通す）
just bundle    # .app / .dmg 生成
just install   # /Applications へ配置 + ~/.local/bin/mzed symlink
```

nix + direnv 環境。シェルによっては `direnv exec . just verify` で実行する。

## 原則

- 速度は機能要件。UI を止める実装はバグとして扱う
- 整形、構造変更、挙動変更を同じ作業に混ぜない
- Markdown → HTML 経路は「raw HTML 全エスケープ → 再パース」の設計を崩さない（XSS 防御の要）
- IPC / パス検証（roots 包含、markdown 拡張子チェック）を迂回する変更をしない
- Dioxus 継続で固定。Tauri/Solid 移行前提の設計を混ぜない
