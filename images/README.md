# README の画面例

- `main.png`: 架空のプロジェクト `aurora` を、隔離した HOME で起動した mzed で開き、ウィンドウを撮ったもの。ピンは架空のレビュー md。
- `mzed-demo-ja.mp4` / `.gif` / `.png`: 日本語版の20秒紹介動画・README 用 GIF・再生前の静止画。
- `mzed-demo-en.mp4` / `.gif` / `.png`: 英語版。画面は同じで、説明文だけ英語。

個人の設定・メモ・ピン・実在のプロジェクトは使わない。

紹介動画は別リポジトリ `product-movie` の `examples/mzed/movie.json` から生成する。`PM` にそのチェックアウト先を入れて実行する。必要な環境はツールの README にある。

```sh
uv run --no-project python $PM/build.py --config $PM/examples/mzed/movie.json --output images
uv run --no-project python $PM/build.py --config $PM/examples/mzed/movie.json --output images --check
```
