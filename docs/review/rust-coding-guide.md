# Rust コーディングガイド

## 目的

Rust を使う上での一般規約。
mzed の仕様ではなく、どの Rust コードにも適用する作法をここに置く。

## 所有権と借用

### 原則

- 呼び出し側に所有権を残せるなら `&T` / `&Path` / `&str` を受け取る。
- 関数内で保持する、スレッドへ渡す、構造体に保存する場合だけ所有する型を受け取る。
- `clone()` は意図を説明できる場所でだけ使う。
- `String` / `PathBuf` を引数で要求する前に、借用で足りるか確認する。

### OK

```rust
pub fn relative_display(path: &Path, root: Option<&Path>) -> String {
    path.strip_prefix(root.unwrap_or_else(|| Path::new("")))
        .unwrap_or(path)
        .display()
        .to_string()
}
```

借用で読み取り、表示文字列だけを返している。

### NG

```rust
pub fn relative_display(path: PathBuf, root: Option<PathBuf>) -> String {
    let root = root.unwrap_or_default();
    path.strip_prefix(root).unwrap_or(&path).display().to_string()
}
```

読み取りだけなのに所有権を奪っている。

## `clone()` の扱い

### OK

```rust
let path_for_thread = path.clone();
std::thread::spawn(move || {
    watch_file(&path_for_thread, on_change)
});
```

別スレッドへ所有権を渡すための clone。

### NG

```rust
for path in paths.clone() {
    render(path.clone());
}
```

借用で足りる可能性が高い。ループ全体の clone と要素 clone が重なっている。

## 命名

Rust の標準的な命名に寄せる。

| 対象 | 形式 | 例 |
|---|---|---|
| module | `snake_case` | `project_tabs` |
| function | `snake_case` | `render_file` |
| local variable | `snake_case` | `active_file` |
| type / enum / trait | `UpperCamelCase` | `ActiveProject` |
| enum variant | `UpperCamelCase` | `SelfPinned` |
| const | `SCREAMING_SNAKE_CASE` | `ZOOM_DEFAULT` |

避ける命名:

- `data`, `item`, `thing`, `tmp` のように意味が薄い名前。
- `manager` の乱用。何を管理するかが名前から分からない。
- `handle_*` の乱用。UI event 以外では具体的な動詞を使う。

### OK

```rust
pub fn query_active_project(db_path: &Path) -> Result<Option<ActiveProject>>
```

何を問い合わせるか、戻り値が何かが分かる。

### NG

```rust
pub fn get_data(path: PathBuf) -> Result<Option<Data>>
```

対象も意味も曖昧。

## モジュールと公開範囲

- 新規では `mod.rs` を使わない。
- `pub` はモジュール境界で必要なものだけに付ける。
- crate 内だけで使うなら `pub(crate)` を検討する。
- テストのために公開範囲を広げない。純粋関数を小さく切り出してテストする。

### OK

```rust
pub(crate) fn split_frontmatter(input: &str) -> (Option<String>, String) {
    // ...
}
```

crate 内の協調に必要な範囲だけ公開している。

### NG

```rust
pub fn internal_parse_step(input: &str) -> String {
    // tests need this
}
```

テスト都合で public API を増やしている。

## エラー処理

- 本体コードで `unwrap()` / `expect()` を使わない。テストでは許可する。
- 外部状態に依存する処理は `Result` を返す。
- UI で失敗を表示する必要がある処理は、エラーを文字列に潰す前に型で持つ。
- 無視してよい失敗だけ `let _ = ...` を使う。製品版ではログまたは UI 通知に流す。

### OK

```rust
pub fn read_markdown(path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("read markdown: {}", path.display()))
}
```

原因と対象パスが残る。

### NG

```rust
pub fn read_markdown(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}
```

ファイルが空なのか、読み込みに失敗したのか分からない。

## `Option` と `Result`

- 「無い」が正常なら `Option`。
- 失敗理由が必要なら `Result`。
- `Option<Result<T>>` や `Result<Option<T>>` は意味を説明できる場合だけ使う。

### OK

```rust
pub fn default_zed_db_path() -> Option<PathBuf>
```

Zed DB が存在しないことは正常系。

### OK

```rust
pub fn query_active_project(db_path: &Path) -> Result<Option<ActiveProject>>
```

DB が無い/壊れていることと、active project が無いことを分けている。

## コレクション

- 小さな順序付きリストは `Vec` でよい。
- 重複排除や membership が主目的なら `HashSet` を使う。
- 順序と重複排除を両方守る場合は、なぜ `Vec::contains` で足りるかを確認する。
- 大きな project tree や検索 index では、線形探索を前提にしない。

## 非同期とスレッド

- UI thread でファイル I/O、DB query、Markdown parse、export を行わない。
- `std::thread::spawn` は停止条件と所有権の境界を明確にする。
- async code で lock を `.await` 越しに保持しない。
- channel は切断を終了条件として扱う。

### OK

```rust
while rx.recv().await.is_some() {
    reload += 1;
}
```

送信側が落ちたらループも終わる。

### NG

```rust
loop {
    if changed() {
        reload += 1;
    }
}
```

停止条件も待機もない。

## テスト

- pure function は単体テストする。
- ファイルシステムは `tempfile` で閉じる。
- SQLite は一時 DB と最小 schema で検証する。
- UI component の中にある状態遷移は、普通の関数に切り出してからテストする。
- テスト名は日本語でもよい。失敗時に意図が分かる名前にする。

## lint と format

必須:

```sh
direnv exec . cargo fmt --check
direnv exec . cargo clippy -- -D warnings
direnv exec . cargo test
```

整形だけの差分は、挙動変更と分ける。

## 参考資料

- Ownership: https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html
- Error handling: https://doc.rust-lang.org/book/ch09-00-error-handling.html
- API Guidelines: https://rust-lang.github.io/api-guidelines/
- Clippy usage: https://doc.rust-lang.org/clippy/usage.html
- Cargo project layout: https://doc.rust-lang.org/cargo/guide/project-layout.html
