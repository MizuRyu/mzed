#!/usr/bin/env bash
# Install or update mzed from the latest GitHub Release.
#   curl -fsSL https://raw.githubusercontent.com/MizuRyu/mzed/main/scripts/install.sh | bash
set -euo pipefail

REPO="MizuRyu/mzed"
APP="/Applications/mzed.app"
BIN_DIR="$HOME/.local/bin"

echo "==> Resolving latest release..."
# Follow the /releases/latest redirect instead of the API (no rate limit).
tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")
tag=${tag##*/}                                  # e.g. v1.0.0
version=${tag#v}
dmg_url="https://github.com/$REPO/releases/download/$tag/mzed_${version}_aarch64.dmg"
[ "$tag" != "latest" ] || { echo "error: could not resolve the latest release tag" >&2; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"; [ -n "${mount_point:-}" ] && hdiutil detach "$mount_point" -quiet || true' EXIT

echo "==> Downloading $(basename "$dmg_url")..."
curl -fsSL -o "$tmp/mzed.dmg" "$dmg_url"

echo "==> Mounting dmg..."
mount_point=$(hdiutil attach "$tmp/mzed.dmg" -nobrowse -readonly |
  grep -o '/Volumes/.*' | head -1)
[ -d "$mount_point/mzed.app" ] || { echo "error: mzed.app not found in dmg" >&2; exit 1; }

echo "==> Installing to $APP..."
rm -rf "$APP"
cp -R "$mount_point/mzed.app" "$APP"
hdiutil detach "$mount_point" -quiet
mount_point=""

echo "==> Removing quarantine attribute..."
xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true

echo "==> Creating CLI symlink..."
mkdir -p "$BIN_DIR"
ln -sf "$APP/Contents/MacOS/mzed" "$BIN_DIR/mzed"

echo "==> Done: $("$BIN_DIR/mzed" --version)"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "note: add ~/.local/bin to PATH: export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
esac
