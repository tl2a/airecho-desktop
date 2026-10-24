#!/usr/bin/env bash
# build-appimage.sh — build dist/AirEcho_Desktop-<version>-<arch>.AppImage for this
# machine's architecture (x86_64 or aarch64).
#
# Needs GTK >= 4.22 and libadwaita >= 1.9 (the crates' gnome_50 / v1_9 features), i.e. a
# GNOME 50 distribution. CI runs it in an ubuntu:26.04 container. Locally, with Docker:
#
#   docker run --rm -v "$PWD":/src -w /src ubuntu:26.04 scripts/build-appimage.sh --install-deps
#
# The AppImage bundles GTK, libadwaita and their data (GSettings schemas, pixbuf loaders,
# Adwaita icons). It does NOT bundle croc — users install croc themselves. It runs on
# distributions whose glibc is at least the build system's (Ubuntu 26.04: 2.43).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

APP_ID="in.tl2a.airEcho"
ARCH="$(uname -m)"
VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)}"
OUT_DIR="${OUT_DIR:-$REPO_ROOT/dist}"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
WORK_DIR="$TARGET_DIR/appimage"
APPDIR="$WORK_DIR/AppDir"
OUTPUT_NAME="AirEcho_Desktop-$VERSION-$ARCH.AppImage"

if [ "${1:-}" = "--install-deps" ]; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends \
        build-essential pkg-config curl ca-certificates file git \
        libgtk-4-dev libadwaita-1-dev blueprint-compiler libglib2.0-bin \
        libgdk-pixbuf2.0-bin librsvg2-common adwaita-icon-theme hicolor-icon-theme \
        ibus-gtk4 patchelf
    if ! command -v cargo >/dev/null; then
        curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    fi
fi
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

echo "==> Building airecho $VERSION ($ARCH)"
cargo build --release --locked

echo "==> Assembling AppDir"
rm -rf "$APPDIR"
install -Dm755 "$TARGET_DIR/release/airecho" "$APPDIR/usr/bin/airecho"
install -Dm644 "data/$APP_ID.desktop" "$APPDIR/usr/share/applications/$APP_ID.desktop"
install -Dm644 "data/icons/$APP_ID.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"
# The UI's symbolic icons come from the Adwaita theme, which not every desktop has (KDE)
cp -a /usr/share/icons/Adwaita "$APPDIR/usr/share/icons/"
cp /usr/share/icons/hicolor/index.theme "$APPDIR/usr/share/icons/hicolor/"

echo "==> Fetching linuxdeploy"
mkdir -p "$WORK_DIR/tools"
LINUXDEPLOY="$WORK_DIR/tools/linuxdeploy-$ARCH.AppImage"
GTK_PLUGIN="$WORK_DIR/tools/linuxdeploy-plugin-gtk.sh"
RUNTIME_FILE="$WORK_DIR/tools/runtime-$ARCH"
[ -x "$LINUXDEPLOY" ] || {
    curl -fsSL -o "$LINUXDEPLOY" \
        "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$ARCH.AppImage"
    chmod +x "$LINUXDEPLOY"
}
[ -x "$GTK_PLUGIN" ] || {
    curl -fsSL -o "$GTK_PLUGIN" \
        "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh"
    chmod +x "$GTK_PLUGIN"
}
export PATH="$WORK_DIR/tools:$PATH"   # linuxdeploy finds plugins on PATH
export APPIMAGE_EXTRACT_AND_RUN=1     # containers have no FUSE
export DEPLOY_GTK_VERSION=4

echo "==> Bundling libraries (linuxdeploy + GTK plugin)"
"$LINUXDEPLOY" --appdir "$APPDIR" --plugin gtk
# The plugin copies GTK's modules (IBus input method) but not the libraries they need
# (libibus), and the modules can't find bundled libraries without an rpath
"$LINUXDEPLOY" --appdir "$APPDIR" --deploy-deps-only "$APPDIR/usr/lib/gtk-4.0"
# shellcheck disable=SC2016 # $ORIGIN is for the dynamic linker, not the shell
find "$APPDIR/usr/lib/gtk-4.0" -name '*.so' -exec patchelf --set-rpath '$ORIGIN/../../..' {} \;

# The GTK plugin's runtime hook forces X11 ("crash with Wayland" — a GTK 3 issue) and
# GTK_THEME=Adwaita:light, which breaks libadwaita's dark mode and accent colours.
HOOK="$APPDIR/apprun-hooks/linuxdeploy-plugin-gtk.sh"
sed -i -e '/^export GDK_BACKEND=/d' -e '/^export GTK_THEME=/d' "$HOOK"
if grep -qE '^export (GDK_BACKEND|GTK_THEME)=' "$HOOK"; then
    echo "error: could not remove GDK_BACKEND/GTK_THEME from $HOOK" >&2
    exit 1
fi

echo "==> Creating $OUTPUT_NAME"
echo "==> Fetching AppImage runtime for $ARCH"
curl -fSL --retry 5 --retry-all-errors --retry-delay 2 --retry-max-time 120 \
    -o "$RUNTIME_FILE" \
    "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-$ARCH"
(cd "$WORK_DIR" && \
    LDAI_OUTPUT="$OUTPUT_NAME" \
    LDAI_RUNTIME_FILE="$RUNTIME_FILE" \
    "$LINUXDEPLOY" --appdir "$APPDIR" --output appimage)
mkdir -p "$OUT_DIR"
mv "$WORK_DIR/$OUTPUT_NAME" "$OUT_DIR/"
echo "Built: $OUT_DIR/$OUTPUT_NAME"
