# mzed コーディング標準

## 目的

この文書はレビュー基準の入口。
Rust 一般の書き方、mzed 固有の設計方針、性能方針を混ぜないため、詳細は分けて管理する。

## 読む順序

1. [rust-coding-guide.md](rust-coding-guide.md)
2. [mzed-engineering-guide.md](mzed-engineering-guide.md)
3. [performance-guide.md](performance-guide.md)

## 分担

| 文書 | 扱うこと | 扱わないこと |
|---|---|---|
| Rust coding guide | 所有権、借用、命名、エラー、テスト、モジュール、lint | mzed の機能仕様 |
| mzed engineering guide | Markdown viewer としての責務、UI、Zed 連動、security | Rust 一般作法の細目 |
| Performance guide | 軽量・高速を守るための設計、測定、禁止事項 | 機能要求の優先順位 |

## 共通原則

- 速度は機能要件。UI を止める実装はバグとして扱う。
- Rust の一般規約と mzed 固有ルールを分けて判断する。
- 好みのリファクタを finding にしない。保守性、検証性、安全性、性能に効くものだけを扱う。
- 整形、構造変更、挙動変更を同じ作業に混ぜない。
- 製品版方針は Dioxus 継続で固定する。Tauri/Solid 移行前提の設計を混ぜない。

## 受け入れゲート

各改善単位は、少なくとも次を確認してから完了扱いにする。

```sh
direnv exec . cargo fmt --check
direnv exec . cargo clippy -- -D warnings
direnv exec . cargo test
```

`just` が使える環境では次でもよい。

```sh
direnv exec . just verify
```

## 参考資料

- Rust Book: https://doc.rust-lang.org/book/
- Rust API Guidelines: https://rust-lang.github.io/api-guidelines/
- Cargo Book: https://doc.rust-lang.org/cargo/
- Clippy: https://doc.rust-lang.org/clippy/
- rustfmt: https://rust-lang.github.io/rustfmt/
- Dioxus docs: https://dioxuslabs.com/learn/0.7/
- OWASP XSS Prevention Cheat Sheet: https://cheatsheetseries.owasp.org/cheatsheets/Cross_Site_Scripting_Prevention_Cheat_Sheet.html
