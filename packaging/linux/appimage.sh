#!/usr/bin/env bash
#
# Build a self-contained, double-clickable AppImage.
#
#   ./packaging/linux/appimage.sh
#
# Needs linuxdeploy on PATH (or it will fetch it into target/dist). Build this
# on the oldest glibc you intend to support — an AppImage bundles the app's own
# libraries but still links against the host's libc.

set -euo pipefail

cd "$(dirname "$0")/../.."

BIN_NAME="discord-desktop-fixer"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
ARCH="$(uname -m)"
DIST="target/dist"
APPDIR="$DIST/AppDir"

say() { printf '\033[1;35m==>\033[0m %s\n' "$1"; }

say "Building release binary ($VERSION)"
cargo build --release

say "Staging AppDir"
rm -rf "$APPDIR"
install -Dm755 "target/release/$BIN_NAME" "$APPDIR/usr/bin/$BIN_NAME"
install -Dm644 "packaging/linux/$BIN_NAME.desktop" \
  "$APPDIR/usr/share/applications/$BIN_NAME.desktop"
# Must be exactly 256×256 — linuxdeploy rejects a mis-sized file in this dir,
# so the 1024px master won't do.
install -Dm644 assets/icon-256.png \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps/$BIN_NAME.png"

TOOL="$(command -v linuxdeploy || true)"
if [[ -z "$TOOL" ]]; then
  say "Fetching linuxdeploy"
  TOOL="$DIST/linuxdeploy-$ARCH.AppImage"
  curl -fsSL -o "$TOOL" \
    "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$ARCH.AppImage"
  chmod +x "$TOOL"
fi

say "Building AppImage"
OUTPUT="$DIST/DiscordDesktopFixer-$VERSION-$ARCH.AppImage" \
VERSION="$VERSION" \
  "$TOOL" --appdir "$APPDIR" --output appimage \
  --desktop-file "$APPDIR/usr/share/applications/$BIN_NAME.desktop" \
  --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/$BIN_NAME.png"

say "Done: $DIST/DiscordDesktopFixer-$VERSION-$ARCH.AppImage"
