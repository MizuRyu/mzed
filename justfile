[private]
default:
  @just --list

fmt:
  cargo fmt

check:
  cargo fmt --check
  cargo clippy -- -D warnings

test:
  cargo nextest run

verify: fmt check test

run:
  cargo run --bin mzed

watch:
  cargo run --bin zed_watch

version := `sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1`
bundle_dir := "target/dx/mzed/bundle/macos/macos"

# Build the macOS .app (and .dmg) bundle into target/dx/mzed/bundle/macos/macos/.
# Re-sign ad-hoc after dx bundle: dx leaves a broken resource seal that makes
# Gatekeeper reject the app on other machines even after removing quarantine.
bundle:
  dx bundle --platform desktop --release --bin mzed --package-types macos
  codesign --force --deep -s - {{bundle_dir}}/mzed.app
  codesign -v {{bundle_dir}}/mzed.app
  hdiutil create -volname mzed -srcfolder {{bundle_dir}}/mzed.app -ov -format UDZO {{bundle_dir}}/mzed_{{version}}_aarch64.dmg

# Install the freshly bundled mzed.app into /Applications and strip the quarantine
# attribute (the build is unsigned, so Gatekeeper would otherwise block it).
# Also creates ~/.local/bin/mzed symlink so the CLI is on PATH without sudo.
install: bundle
  rm -rf /Applications/mzed.app
  cp -R target/dx/mzed/bundle/macos/macos/mzed.app /Applications/mzed.app
  xattr -dr com.apple.quarantine /Applications/mzed.app
  mkdir -p ~/.local/bin
  ln -sf /Applications/mzed.app/Contents/MacOS/mzed ~/.local/bin/mzed
  @echo "Installed /Applications/mzed.app"
  @echo "CLI symlink: ~/.local/bin/mzed → /Applications/mzed.app/Contents/MacOS/mzed"
  @echo "Make sure ~/.local/bin is in PATH (add to ~/.zshrc: export PATH=\"\$HOME/.local/bin:\$PATH\")"

# Tag, push and publish a GitHub Release with the dmg attached.
# Expects Cargo.toml's version to be bumped and committed beforehand.
release:
  git diff --quiet && git diff --cached --quiet || (echo "error: working tree not clean" && exit 1)
  just verify
  git tag v{{version}}
  git push origin main v{{version}}
  just bundle
  gh release create v{{version}} {{bundle_dir}}/mzed_{{version}}_aarch64.dmg --title "mzed v{{version}}" --generate-notes

# Remove /Applications/mzed.app and the ~/.local/bin/mzed CLI symlink.
uninstall:
  rm -rf /Applications/mzed.app
  rm -f ~/.local/bin/mzed
  @echo "Uninstalled mzed"
