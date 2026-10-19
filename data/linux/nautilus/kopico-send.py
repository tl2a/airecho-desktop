"""Nautilus (GNOME Files): right-click files → "Send with Kopico".

Needs nautilus-python (Fedora: nautilus-python, Debian/Ubuntu: python3-nautilus).
dev-install.sh copies this to ~/.local/share/nautilus-python/extensions/ with
@KOPICO_BIN@ replaced; restart Nautilus afterwards (nautilus -q).

Runs `kopico-desktop <files>`: GApplication hands the files to the running Kopico over
D-Bus (or starts it), which sends them with croc.
"""

import subprocess

import gi

try:
    gi.require_version("Nautilus", "4.0")  # Nautilus 43+
except ValueError:
    gi.require_version("Nautilus", "3.0")
from gi.repository import GObject, Nautilus

KOPICO = "@KOPICO_BIN@"


class KopicoSendMenu(GObject.GObject, Nautilus.MenuProvider):
    def get_file_items(self, *args):
        files = args[-1]  # (files) on Nautilus 43+, (window, files) before
        paths = [path for f in files if (path := f.get_location().get_path())]
        if not paths:  # nothing local (e.g. only network locations)
            return []
        item = Nautilus.MenuItem(
            name="KopicoSendMenu::send",
            label="Send with Kopico",
            tip="Send the selection with croc",
        )
        item.connect("activate", lambda *_: subprocess.Popen([KOPICO, *paths]))
        return [item]
