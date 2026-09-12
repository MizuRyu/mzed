<p align="center">
  <img src="assets/icon-1024.png" width="128" alt="mzed icon">
</p>

# mzed

[![release](https://img.shields.io/github/v/release/MizuRyu/mzed)](https://github.com/MizuRyu/mzed/releases)
[![license](https://img.shields.io/github/license/MizuRyu/mzed)](LICENSE)

Zed（エディタ）と Orca（worktree 管理ツール）に連動する Markdown ビューア。フォーカス中のプロジェクトを検知し、その docs を自動で表示する。

English version: [README.en.md](README.en.md)

## 特徴

- **Zed / Orca 連動** — Zed / Orca のプロジェクト切替に追従して表示中の docs を丸ごと入れ替える。追従元は `sync_source`（auto / zed / orca、既定 auto）または `--sync-source` で選べる。`Cmd+Shift+L` で追従の固定/解除
- **リッチなレンダリング** — GitHub スタイル、シンタックスハイライト、Mermaid、KaTeX、GitHub Alerts、frontmatter、画像 lightbox
- **ライブリロード** — ファイル保存を検知して即再描画
- **快適なナビゲーション** — サイドバー、マルチタブ、左右分割、目次、コマンドパレット、ファジー検索、全文検索
- **Task View（`Cmd+Shift+D`）** — `docs/memo/tasks/` のタスクフォルダを status 別に一覧し、task.md を即読みする専用ビュー。複数プロジェクト横断にも対応
- **メモ（`Cmd+Shift+M`）** — 読みながら本文を選択して「ここを直して」を残す。`~/.config/mzed/notes/` に 1 メモ 1 JSON で溜まり、AI エージェントがスキル `mzed-notes` で読んで直す
- **エクスポート** — self-contained な HTML / PDF
- **CLI** — `mzed file.md` で単一インスタンスに転送。ドラッグ&ドロップ、セッション復元も対応

## 動作環境

macOS（Apple Silicon）。x86_64 Mac および他 OS は対象外。

## インストール

1行でインストール・更新（最新 Release の取得、quarantine 解除、CLI symlink まで自動）:

```sh
curl -fsSL https://raw.githubusercontent.com/MizuRyu/mzed/main/scripts/install.sh | bash
```

手動の場合は [Releases](https://github.com/MizuRyu/mzed/releases) から `.dmg` をダウンロードし、`mzed.app` を `/Applications` に置いてから quarantine を解除する（未署名のため初回のみ必要）:

```sh
xattr -dr com.apple.quarantine /Applications/mzed.app
ln -sf /Applications/mzed.app/Contents/MacOS/mzed ~/.local/bin/mzed  # CLI を使う場合
```

ソースからビルドする場合は [docs/development.md](docs/development.md) を参照。

## 使い方

```sh
mzed               # Zed / Orca 連動モードで起動
mzed file.md       # ファイルをタブで開く
mzed ./docs        # ディレクトリをルートにして開く
mzed --sync self   # 連動モード指定
mzed serve ./docs  # フォルダをブラウザで表示（127.0.0.1 のみ、live-reload。画面共有向け）
```

連動モードは3つ。どこまで追うか（`sync_mode`）を決める:

| モード | 動作 |
| --- | --- |
| `auto` | プロジェクト + docs を自動で切り替える（デフォルト） |
| `self` | プロジェクトだけ切り替え、docs は自動で開かない |
| `off` | 連動なし（手動操作のみ） |

### 主なキーバインド

| キー | 動作 |
| --- | --- |
| `Cmd+Shift+P` | コマンドパレット |
| `Cmd+Shift+L` | 追従の固定/解除（auto⇄self） |
| `Cmd+P` | ファイルのファジー検索 |
| `Cmd+F` | ドキュメント内検索 |
| `Cmd+O` | プロジェクト切替 |
| `Cmd+Shift+D` | Task View（タスク一覧）のトグル |
| `Cmd+Shift+M` | 選択した本文にメモを追加 |
| `Cmd+\` | 左右分割 |
| `Cmd+= / Cmd+-` / `Cmd+0` | ズームイン / アウト / リセット |

キーバインドは設定画面から変更できる。

## 開発

セットアップ・ビルド・テストは [docs/development.md](docs/development.md)、設計ドキュメントは [docs/README.md](docs/README.md) を参照。

## 謝辞

設計と UX は次のプロジェクトから影響を受けている。

- [Zed](https://github.com/zed-industries/zed) — 連動先のエディタ。キーバインドとコマンドパレットの操作感も参考にした
- [mo](https://github.com/k1LoW/mo) — Markdown をブラウザで見せるビューア。`mzed serve` はこの用途を mzed 側に寄せたもの
- [Arto](https://github.com/arto-app/Arto) — ファイルをドロップしてすぐ表示する体験

## License

[MIT](LICENSE)

再配布しているサードパーティのアセット（github-markdown-css、highlight.js、Mermaid、KaTeX）のライセンス原文は [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md) に収録している。
