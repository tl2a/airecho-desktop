#!/usr/bin/env bash
# make-icons.sh — regenerate status icons and, on macOS, data/macos/AppIcon.icns.
#
# Run after changing the status SVGs (needs rsvg-convert from librsvg). On macOS,
# also regenerates AppIcon.icns from the app icon SVG (needs iconutil). The generated
# assets are committed, so release builds don't need these tools.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ICONS_DIR="$REPO_ROOT/data/icons"

rsvg-convert -w 16 -h 16 \
    "$ICONS_DIR/in.tl2a.airEcho-status-icon-active.svg" \
    -o "$ICONS_DIR/in.tl2a.airEcho-status-icon-active-16.png"
rsvg-convert -w 16 -h 16 \
    "$ICONS_DIR/in.tl2a.airEcho-status-icon-inactive.svg" \
    -o "$ICONS_DIR/in.tl2a.airEcho-status-icon-inactive-16.png"
echo "Wrote Linux status icon PNGs"

if [[ "$(uname -s)" == "Darwin" ]]; then
    SVG="$ICONS_DIR/in.tl2a.airEcho.svg"
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
    rsvg-convert -w 256 -h 256 "$MACOS_SVG" -o "$REPO_ROOT/data/macos/about-icon.png"
    iconutil -c icns "$ICONSET" -o "$REPO_ROOT/data/macos/AppIcon.icns"
    echo "Wrote data/macos/AppIcon.icns"
    echo "Wrote data/macos/about-icon.png"
fi
