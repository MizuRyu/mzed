<p align="center">
  <img src="assets/icon-1024.png" width="128" alt="mzed icon">
</p>

# mzed

[![release](https://img.shields.io/github/v/release/MizuRyu/mzed)](https://github.com/MizuRyu/mzed/releases)
[![license](https://img.shields.io/github/license/MizuRyu/mzed)](LICENSE)

Zed 連動の Markdown ビューア。Zed でフォーカスしているプロジェクトを検知し、その docs を自動で表示する。

English version: [README.en.md](README.en.md)

## 特徴

- **Zed 連動** — Zed のプロジェクト切替に追従して表示中の docs を丸ごと入れ替える。`Cmd+Shift+L` で追従の固定/解除
- **リッチなレンダリング** — GitHub スタイル、シンタックスハイライト、Mermaid、KaTeX、GitHub Alerts、frontmatter、画像 lightbox
- **ライブリロード** — ファイル保存を検知して即再描画
- **快適なナビゲーション** — サイドバー、マルチタブ、左右分割、目次、コマンドパレット、ファジー検索、全文検索
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
mzed               # Zed 連動モードで起動
mzed file.md       # ファイルをタブで開く
mzed ./docs        # ディレクトリをルートにして開く
mzed --sync self   # 連動モード指定
```

連動モードは3つ:

| モード | 動作 |
| --- | --- |
| `auto` | Zed のフォーカス中プロジェクトに追従（デフォルト） |
| `self` | 今のプロジェクトに固定 |
| `off` | 連動なし |

### 主なキーバインド

| キー | 動作 |
| --- | --- |
| `Cmd+Shift+P` | コマンドパレット |
| `Cmd+Shift+L` | Zed 追従の固定/解除（auto⇄self） |
| `Cmd+P` | ファイルのファジー検索 |
| `Cmd+F` | ドキュメント内検索 |
| `Cmd+O` | プロジェクト切替 |
| `Cmd+\` | 左右分割 |
| `Cmd+= / Cmd+-` / `Cmd+0` | ズームイン / アウト / リセット |

キーバインドは設定画面から変更できる。

## 開発

セットアップ・ビルド・テストは [docs/development.md](docs/development.md)、設計ドキュメントは [docs/README.md](docs/README.md) を参照。

## License

[MIT](LICENSE)
