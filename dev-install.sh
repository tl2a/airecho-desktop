#!/usr/bin/env bash
# dev-install.sh — registers the dev build with the desktop so that `cargo run` can show
# system notifications under AirEcho's own name.
#
#   Linux: installs the .desktop file to ~/.local/share/applications/ (gio::Notification
#          needs it) and the "Send with AirEcho" right-click entries for Nautilus
#          (nautilus-python), Dolphin, Nemo, and Thunar.
#   macOS: creates "~/Applications/AirEcho.app" (signed bundle with the debug
#          binary) — UNUserNotificationCenter only works for a signed app bundle.
#          `cargo run` refreshes it automatically (scripts/macos-app.sh).
#
# Run once after cloning. Re-run if the repo moves.
# Safe to run repeatedly (just overwrites).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
BINARY="$REPO_ROOT/target/debug/airecho"
APP_ID="in.tl2a.airEcho"

install_linux() {
    local desktop_src="$REPO_ROOT/data/$APP_ID.desktop"
    local dest_dir="$HOME/.local/share/applications"
    local dest="$dest_dir/$APP_ID.desktop"

    mkdir -p "$dest_dir"

    # Remove the previous application identity and its right-click integrations.
    rm -f "$dest_dir/in.tl2a.kopico-desktop.desktop"
    rm -f "$HOME/.local/share/icons/hicolor/scalable/apps/in.tl2a.kopico-desktop.svg"
    rm -f "$HOME/.local/share/nautilus-python/extensions/kopico-send.py"
    rm -f "$HOME/.local/share/kio/servicemenus/kopico-send.desktop"
    rm -f "$HOME/.local/share/nemo/actions/kopico-send.nemo_action"

    # Copy the template and patch Exec= to the actual debug binary path
    sed "s|^Exec=.*|Exec=$BINARY %F|" "$desktop_src" > "$dest"

    echo "Installed: $dest"
    echo "  Exec= → $BINARY %F"

    local icon_dir="$HOME/.local/share/icons/hicolor/scalable/apps"
    mkdir -p "$icon_dir"
    cp "$REPO_ROOT/data/icons/$APP_ID.svg" "$icon_dir/$APP_ID.svg"
    echo "Installed: $icon_dir/$APP_ID.svg"

    # Right-click → "Send with AirEcho" in Nautilus (needs nautilus-python)
    local nautilus_dir="$HOME/.local/share/nautilus-python/extensions"
    mkdir -p "$nautilus_dir"
    sed "s|@AIRECHO_BIN@|$BINARY|" "$REPO_ROOT/data/linux/nautilus/airecho-send.py" \
        > "$nautilus_dir/airecho-send.py"
    echo "Installed: $nautilus_dir/airecho-send.py"
    if command -v rpm &>/dev/null; then
        if rpm -q nautilus-python &>/dev/null; then
            nautilus_python_installed=true
        else
            nautilus_python_installed=false
        fi
    elif command -v dpkg-query &>/dev/null; then
        if dpkg-query -W -f='${Status}' python3-nautilus 2>/dev/null | grep -q 'install ok installed'; then
            nautilus_python_installed=true
        else
            nautilus_python_installed=false
        fi
    else
        nautilus_python_installed=false
    fi
    if [ "$nautilus_python_installed" != true ]; then
        echo "  Nautilus requires nautilus-python (Fedora) or python3-nautilus (Debian/Ubuntu)."
    fi

    # Dolphin service menu
    local kio_dir="$HOME/.local/share/kio/servicemenus"
    mkdir -p "$kio_dir"
    sed "s|@AIRECHO_BIN@|$BINARY|" "$REPO_ROOT/data/linux/kio/airecho-send.desktop" \
        > "$kio_dir/airecho-send.desktop"
    chmod +x "$kio_dir/airecho-send.desktop" # KDE only loads executable service menus
    echo "Installed: $kio_dir/airecho-send.desktop"

    # Nemo action
    local nemo_dir="$HOME/.local/share/nemo/actions"
    mkdir -p "$nemo_dir"
    sed "s|@AIRECHO_BIN@|$BINARY|" "$REPO_ROOT/data/linux/nemo/airecho-send.nemo_action" \
        > "$nemo_dir/airecho-send.nemo_action"
    echo "Installed: $nemo_dir/airecho-send.nemo_action"

    # Thunar custom action. Merge into the user's existing UCA config rather than
    # replacing their other custom actions.
    local thunar_config="$HOME/.config/Thunar/uca.xml"
    mkdir -p "$(dirname "$thunar_config")"
    AIRECHO_BIN="$BINARY" python3 - "$thunar_config" <<'PY'
import os
import shlex
import sys
import xml.etree.ElementTree as ET

config = sys.argv[1]
parser = ET.XMLParser(target=ET.TreeBuilder(insert_comments=True))
if os.path.exists(config):
    tree = ET.parse(config, parser=parser)
    root = tree.getroot()
    if root.tag != "actions":
        raise SystemExit(f"Unexpected Thunar custom-action root in {config}: {root.tag}")
else:
    root = ET.Element("actions")
    tree = ET.ElementTree(root)

action = ET.Element("action")
for tag, text in (
    ("icon", "document-send"),
    ("name", "Send with AirEcho"),
    ("unique-id", "airecho-send"),
    ("command", f"{shlex.quote(os.environ['AIRECHO_BIN'])} %F"),
    ("description", "Send selected files with AirEcho"),
):
    ET.SubElement(action, tag).text = text
range_element = ET.SubElement(action, "range")
ET.SubElement(range_element, "file")
ET.SubElement(range_element, "directory")
ET.SubElement(action, "patterns").text = "*"
ET.SubElement(action, "startup-notify")

for index, existing in enumerate(root.findall("action")):
    if existing.findtext("unique-id") in {"airecho-send", "kopico-desktop-send"}:
        root.remove(existing)
        root.insert(index, action)
        break
else:
    root.append(action)

ET.indent(tree, space=" ")
tree.write(config, encoding="UTF-8", xml_declaration=True)
PY
    echo "Installed: $thunar_config"

    echo "Restart Nautilus (nautilus -q), Nemo (nemo -q), and Thunar to load new actions."
    if command -v kbuildsycoca6 &>/dev/null; then
        kbuildsycoca6 --noincremental
    elif command -v kbuildsycoca5 &>/dev/null; then
        kbuildsycoca5 --noincremental
    fi

    # Refresh the GNOME app database so GAppInfo picks up the new entry immediately
    if command -v update-desktop-database &>/dev/null; then
        update-desktop-database "$dest_dir"
        echo "Desktop database updated."
    fi
}

install_macos() {
    # Same bundle `cargo run` uses (see .cargo/config.toml): copied + ad-hoc signed
    # executable, registered with LaunchServices
    "$REPO_ROOT/scripts/macos-app.sh" install
    echo ""
    echo "\`cargo run\` now starts AirEcho from this bundle. On the first start macOS asks"
    echo "to allow notifications; afterwards AirEcho is listed in"
    echo "System Settings → Notifications."
}

case "$(uname -s)" in
    Darwin) install_macos ;;
    *)      install_linux ;;
esac

if [ ! -x "$BINARY" ]; then
    echo ""
    echo "Note: $BINARY doesn't exist yet — run \`cargo build\` first."
fi

echo ""
echo "Done. System notifications will work once the app is running."
echo "(Re-run this script if you move the repo.)"
