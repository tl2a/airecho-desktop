"""Nautilus (GNOME Files): right-click files → "Send with AirEcho".

Needs nautilus-python (Fedora: nautilus-python, Debian/Ubuntu: python3-nautilus).
dev-install.sh copies this to ~/.local/share/nautilus-python/extensions/ with
@AIRECHO_BIN@ replaced; restart Nautilus afterwards (nautilus -q).

Runs `airecho <files>`: GApplication hands the files to the running AirEcho over
D-Bus (or starts it), which sends them with croc.
"""

import subprocess

import gi

from gi.repository import GObject, Nautilus

KOPICO = "@AIRECHO_BIN@"


class AirEchoSendMenu(GObject.GObject, Nautilus.MenuProvider):
    def get_file_items(self, *args):
        files = args[-1]  # (files) on Nautilus 43+, (window, files) before
        paths = [path for f in files if (path := f.get_location().get_path())]
        if not paths:  # nothing local (e.g. only network locations)
            return []
        item = Nautilus.MenuItem(
            name="AirEchoSendMenu::send",
            label="Send with AirEcho",
            tip="Send the selection with croc",
        )
        item.connect("activate", lambda *_: subprocess.Popen([KOPICO, *paths]))
        return [item]
