#!/usr/bin/env bash
# dev-install.sh — installs the .desktop file to ~/.local/share/applications/
# so that `cargo run` can show system notifications via gio::Notification.
#
# Run once after cloning. Re-run after `cargo build` or if the repo moves.
# Safe to run repeatedly (just overwrites).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
BINARY="$REPO_ROOT/target/debug/kopico-desktop"
DESKTOP_SRC="$REPO_ROOT/data/in.tl2a.kopico-desktop.desktop"
DEST_DIR="$HOME/.local/share/applications"
DEST="$DEST_DIR/in.tl2a.kopico-desktop.desktop"

mkdir -p "$DEST_DIR"

# Copy the template and patch Exec= to the actual debug binary path
sed "s|Exec=.*|Exec=$BINARY|" "$DESKTOP_SRC" > "$DEST"

echo "Installed: $DEST"
echo "  Exec= → $BINARY"

# Refresh the GNOME app database so GAppInfo picks up the new entry immediately
if command -v update-desktop-database &>/dev/null; then
    update-desktop-database "$DEST_DIR"
    echo "Desktop database updated."
fi

echo ""
echo "Done. System notifications will work once the app is running."
echo "(Re-run this script if you move the repo or after a clean build.)"
