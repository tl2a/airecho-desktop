#!/usr/bin/env bash
# build-macos-dmg.sh — build dist/Kopico_Desktop-<version>-<arch>.dmg (arch = arm64 or
# x86_64, this Mac's) containing a self-contained "Kopico Desktop.app".
#
# Needs GTK >= 4.22 and libadwaita >= 1.9 built for Quartz, plus blueprint-compiler,
# adwaita-icon-theme, librsvg and dylibbundler, from Homebrew (Apple Silicon) or
# MacPorts (`+quartz`; Homebrew has no Intel bottles for GTK). Found via pkg-config.
#
# The app bundles GTK, libadwaita and their data (src/macos_bundle.rs points GTK at them
# at startup). It does NOT bundle croc — users install croc themselves. It is ad-hoc
# signed, not notarized: macOS asks users to confirm the first launch.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

APP_ID="in.tl2a.kopico-desktop"
ARCH="$(uname -m)"
VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)}"
OUT_DIR="${OUT_DIR:-$REPO_ROOT/dist}"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
WORK_DIR="$TARGET_DIR/macos-dist"
APP="$WORK_DIR/Kopico Desktop.app"
DMG_NAME="Kopico_Desktop-$VERSION-$ARCH.dmg"
DYLIBBUNDLER="${DYLIBBUNDLER:-dylibbundler}"

# The package manager's root (/opt/homebrew, /opt/local): where data from all packages is
# merged. pkg-config's gtk4 prefix isn't it on Homebrew (that's gtk4's own Cellar folder).
ROOT="$(cd "$(dirname "$(command -v glib-compile-schemas)")/.." && pwd)"
PREFIX="$(pkg-config --variable=prefix gtk4)"
PIXBUF_QUERY="$ROOT/bin/gdk-pixbuf-query-loaders"
echo "==> GTK $(pkg-config --modversion gtk4), libadwaita $(pkg-config --modversion libadwaita-1) from $ROOT"

echo "==> Building kopico-desktop $VERSION ($ARCH)"
# Header padding: the executable's library paths are rewritten below
RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-headerpad_max_install_names" \
    cargo build --release --locked

echo "==> Assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources/share"
cp "$TARGET_DIR/release/kopico-desktop" "$APP/Contents/MacOS/kopico-desktop"
cp data/macos/Info.plist "$APP/Contents/Info.plist"
cp data/macos/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
PLIST="$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$PLIST"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $VERSION" "$PLIST"
# The bundled libraries only run on the macOS version they were built for (or newer)
/usr/libexec/PlistBuddy -c "Set :LSMinimumSystemVersion $(sw_vers -productVersion | cut -d. -f1).0" "$PLIST"

# SVG image loader (full-colour SVG icons). gdk-pixbuf needs absolute paths in its
# cache: write a template that macos_bundle.rs fills in at startup.
# (librsvg names it libpixbufloader-svg.so or libpixbufloader_svg.so)
LOADERS_DIR="$ROOT/lib/gdk-pixbuf-2.0/2.10.0/loaders"
svg_loader="$(find -L "$LOADERS_DIR" -name 'libpixbufloader*svg*.so' 2>/dev/null | head -n1)"
if [ -n "$svg_loader" ]; then
    cp -L "$svg_loader" "$APP/Contents/Frameworks/"
    "$PIXBUF_QUERY" "$svg_loader" | sed "s|$LOADERS_DIR|@LOADERS_DIR@|g" \
        > "$APP/Contents/Resources/loaders.cache.in"
    grep -q '@LOADERS_DIR@' "$APP/Contents/Resources/loaders.cache.in"
else
    echo "warning: no gdk-pixbuf SVG loader in $LOADERS_DIR (install librsvg)" >&2
fi

echo "==> Bundling libraries (dylibbundler)"
fix_args=(-x "$APP/Contents/MacOS/kopico-desktop")
for loader in "$APP"/Contents/Frameworks/libpixbufloader*; do
    [ -e "$loader" ] && fix_args+=(-x "$loader")
done
# Libraries are referenced as @rpath/<name>: shorter than the original paths, so it always
# fits (some libraries, e.g. MacPorts' librsvg, have no header padding for longer ones)
"$DYLIBBUNDLER" -of -b "${fix_args[@]}" \
    -d "$APP/Contents/Frameworks" -p @rpath/ \
    -s "$ROOT/lib" -s "$PREFIX/lib" </dev/null
# @rpath = Contents/Frameworks, for the executable and for the dlopen'ed image loader
# (which lives there itself). dylibbundler rewrites existing rpaths to a literal "@rpath/".
set_rpath() { # <file> <rpath>
    local old
    for old in $(otool -l "$1" | awk '/cmd LC_RPATH/ { getline; getline; print $2 }'); do
        install_name_tool -delete_rpath "$old" "$1"
    done
    install_name_tool -add_rpath "$2" "$1"
}
set_rpath "$APP/Contents/MacOS/kopico-desktop" @executable_path/../Frameworks
for loader in "$APP"/Contents/Frameworks/libpixbufloader*; do
    [ -e "$loader" ] && set_rpath "$loader" @loader_path
done

# Nothing may still point into the build machine's package manager
if otool -L "$APP"/Contents/MacOS/* "$APP"/Contents/Frameworks/* \
    | grep -E "^\s+($ROOT|$PREFIX|/opt/homebrew|/opt/local|/usr/local)/"; then
    echo "error: unbundled libraries (above)" >&2
    exit 1
fi

echo "==> Copying GTK data"
SHARE="$APP/Contents/Resources/share"
mkdir -p "$SHARE/glib-2.0/schemas" "$SHARE/icons/hicolor"
cp -L "$ROOT"/share/glib-2.0/schemas/org.gtk.gtk4.*.gschema.xml "$SHARE/glib-2.0/schemas/"
glib-compile-schemas "$SHARE/glib-2.0/schemas"
# The UI's symbolic icons come from the Adwaita theme (-L: Homebrew links it)
cp -RL "$ROOT/share/icons/Adwaita" "$SHARE/icons/"
cp -L "$ROOT/share/icons/hicolor/index.theme" "$SHARE/icons/hicolor/"

echo "==> Signing (ad-hoc)"
find "$APP/Contents/Frameworks" -type f -exec codesign --force --sign - {} \;
codesign --force --sign - --identifier "$APP_ID" "$APP"
codesign --verify --strict --verbose=1 "$APP"

echo "==> Creating $DMG_NAME"
STAGE="$WORK_DIR/dmg"
rm -rf "$STAGE"
mkdir -p "$STAGE" "$OUT_DIR"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
rm -f "$OUT_DIR/$DMG_NAME"
hdiutil create -volname "Kopico Desktop" -srcfolder "$STAGE" -fs HFS+ -format UDZO \
    -ov "$OUT_DIR/$DMG_NAME"
echo "Built: $OUT_DIR/$DMG_NAME"
