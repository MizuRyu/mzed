<p align="center">
  <img src="assets/icon-1024.png" width="128" alt="mzed icon">
</p>

# mzed

[![release](https://img.shields.io/github/v/release/MizuRyu/mzed)](https://github.com/MizuRyu/mzed/releases)
[![license](https://img.shields.io/github/license/MizuRyu/mzed)](LICENSE)

A Markdown viewer that follows Zed. It detects the project currently focused in Zed and automatically displays its docs.

日本語版: [README.md](README.md)

## Features

- **Zed integration** — swaps the displayed docs as you switch projects in Zed; pin/unpin with `Cmd+Shift+L`
- **Rich rendering** — GitHub styling, syntax highlighting, Mermaid, KaTeX, GitHub Alerts, frontmatter, image lightbox
- **Live reload** — re-renders on file save
- **Comfortable navigation** — sidebar, multi-tab, split pane, table of contents, command palette, fuzzy finder, full-text search
- **Export** — self-contained HTML / PDF
- **CLI** — `mzed file.md` forwards to the single running instance; drag & drop and session restore included

## Requirements

macOS (Apple Silicon). x86_64 Macs and other operating systems are not supported.

## Installation

One-liner install/update (fetches the latest release, strips quarantine, creates the CLI symlink):

```sh
curl -fsSL https://raw.githubusercontent.com/MizuRyu/mzed/main/scripts/install.sh | bash
```

Or manually: download the `.dmg` from [Releases](https://github.com/MizuRyu/mzed/releases), move `mzed.app` to `/Applications`, then remove the quarantine attribute once (required because the app is unsigned):

```sh
xattr -dr com.apple.quarantine /Applications/mzed.app
ln -sf /Applications/mzed.app/Contents/MacOS/mzed ~/.local/bin/mzed  # for CLI use
```

To build from source, see [docs/development.md](docs/development.md).

## Usage

```sh
mzed               # start in Zed-linked mode
mzed file.md       # open a file in a tab
mzed ./docs        # open a directory as the root
mzed --sync self   # choose the sync mode
```

Three sync modes:

| Mode | Behavior |
| --- | --- |
| `auto` | Follow the project focused in Zed (default) |
| `self` | Pin to the current project |
| `off` | No integration |

### Key bindings

| Key | Action |
| --- | --- |
| `Cmd+Shift+P` | Command palette |
| `Cmd+Shift+L` | Pin/unpin Zed tracking (auto⇄self) |
| `Cmd+P` | Fuzzy file finder |
| `Cmd+F` | In-document search |
| `Cmd+O` | Switch project |
| `Cmd+\` | Split pane |
| `Cmd+= / Cmd+-` / `Cmd+0` | Zoom in / out / reset |

Key bindings are editable in the settings screen.

## Development

See [docs/development.md](docs/development.md) for setup, build, and tests, and [docs/README.md](docs/README.md) for design documents (Japanese).

## License

[MIT](LICENSE)
