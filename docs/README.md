# docs/ インデックス

最終確認日: 2026-07-02

「どのファイルが有効な仕様か」をここで一元管理する。

---

## 開発者向け

| ファイル | 内容 |
|---|---|
| [development.md](development.md) | セットアップ・ビルド・テスト・リリース手順 |

## 現行仕様（参照すべきドキュメント）

これらは現在の実装と対応しており、設計判断の根拠として参照できる。

### docs/specs/

| ファイル | 内容 | 最終確認 |
|---|---|---|
| [00-overview.md](specs/00-overview.md) | 概要・索引・決定事項 | 2026-07-02 |
| [01-architecture.md](specs/01-architecture.md) | システム構成・データフロー | 2026-07-02 |
| [02-tech-stack.md](specs/02-tech-stack.md) | 技術選定と理由 | 2026-07-02 |
| [03-features.md](specs/03-features.md) | 機能一覧（v1 + 将来検討） | 2026-07-02 |
| [04-project-structure.md](specs/04-project-structure.md) | ディレクトリ・モジュール・設定スキーマ | 2026-07-02 |
| [05-zed-integration.md](specs/05-zed-integration.md) | Zed 連動の技術詳細・マルチウィンドウ挙動 | 2026-07-02 |
| [06-rendering-pipeline.md](specs/06-rendering-pipeline.md) | MD パース〜表示パイプライン | 2026-07-02 |
| [07-ipc-and-concurrency.md](specs/07-ipc-and-concurrency.md) | IPC・スレッド・状態管理・ソケット権限 | 2026-07-02 |
| [08-export.md](specs/08-export.md) | HTML / PDF エクスポート・連番衝突回避 | 2026-07-02 |
| [10-settings-and-context-menu.md](specs/10-settings-and-context-menu.md) | 設定画面・右クリック・キーバインド | 2026-07-02 |
| [manual-test-cases.md](specs/manual-test-cases.md) | 恒久手動テストケース集（TC-ID 付き） | 2026-07-02 |

### docs/review/（現行ガイドライン）

| ファイル | 内容 |
|---|---|
| [coding-standards.md](review/coding-standards.md) | コーディング規約 |
| [mzed-engineering-guide.md](review/mzed-engineering-guide.md) | mzed 固有エンジニアリングガイド |
| [rust-coding-guide.md](review/rust-coding-guide.md) | Rust コーディングガイド |
| [performance-guide.md](review/performance-guide.md) | パフォーマンス計測・改善ガイド |
| [product-direction.md](review/product-direction.md) | プロダクト方針 |

---

> 設計過程・実装計画・レビューログ・調査メモ・ベースライン計測はローカルの `docs/memo/archive/` に移動済み（gitignore 対象）。
