#!/usr/bin/env bash
# make-icons.sh — regenerate data/macos/AppIcon.icns with a macOS-specific crop.
#
# Run on macOS after changing data/icons/in.tl2a.airEcho.svg (needs rsvg-convert
# from librsvg: `port install librsvg` / `brew install librsvg`). The .icns is committed,
# so release builds don't need these tools.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SVG="$REPO_ROOT/data/icons/in.tl2a.airEcho.svg"
BUILD_DIR="$(mktemp -d)"
trap 'rm -rf "$BUILD_DIR"' EXIT
MACOS_SVG="$BUILD_DIR/AppIcon.svg"
ICONSET="$BUILD_DIR/AppIcon.iconset"
mkdir -p "$ICONSET"

sed 's/viewBox="0 0 128 128"/viewBox="8 8 112 112"/' "$SVG" > "$MACOS_SVG"
if ! grep -q 'viewBox="8 8 112 112"' "$MACOS_SVG"; then
    echo "Could not create the macOS icon crop from $SVG" >&2
    exit 1
fi

for size in 16 32 128 256 512; do
    rsvg-convert -w "$size" -h "$size" "$MACOS_SVG" -o "$ICONSET/icon_${size}x${size}.png"
    rsvg-convert -w $((size * 2)) -h $((size * 2)) "$MACOS_SVG" -o "$ICONSET/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$REPO_ROOT/data/macos/AppIcon.icns"
echo "Wrote data/macos/AppIcon.icns"
