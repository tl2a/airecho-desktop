#!/usr/bin/env bash
# make-icons.sh — regenerate data/macos/AppIcon.icns from the SVG app icon.
#
# Run on macOS after changing data/icons/in.tl2a.kopico-desktop.svg (needs rsvg-convert
# from librsvg: `port install librsvg` / `brew install librsvg`). The .icns is committed,
# so release builds don't need these tools.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SVG="$REPO_ROOT/data/icons/in.tl2a.kopico-desktop.svg"
ICONSET="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$ICONSET"

for size in 16 32 128 256 512; do
    rsvg-convert -w "$size" -h "$size" "$SVG" -o "$ICONSET/icon_${size}x${size}.png"
    rsvg-convert -w $((size * 2)) -h $((size * 2)) "$SVG" -o "$ICONSET/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$REPO_ROOT/data/macos/AppIcon.icns"
rm -rf "$(dirname "$ICONSET")"
echo "Wrote data/macos/AppIcon.icns"
