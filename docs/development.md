# Development

Setting up, running, debugging and testing AirEcho on Linux and macOS.

- [Toolchain and system libraries](#toolchain-and-system-libraries)
- [Linux](#linux)
- [macOS](#macos)
- [How a build works](#how-a-build-works)
- [Debugging](#debugging)
- [Tests](#tests)
- [Troubleshooting](#troubleshooting)

Release packaging (`.dmg`, `.AppImage`) and the release workflow: [releasing.md](releasing.md).

## Toolchain and system libraries

| Tool / library | Minimum | Why |
|---|---|---|
| Rust | edition 2024 | |
| GTK | 4.22 | `gtk4` crate feature `gnome_50` (GTK 4.22, GLib/GIO 2.88) |
| libadwaita | 1.9 | `libadwaita` crate feature `v1_9` |
| `blueprint-compiler` | | compiles `data/ui/*.blp` to GtkBuilder XML |
| `glib-compile-resources` | | bundles UI + CSS into a GResource |
| `pkg-config` | | lets the `*-sys` crates find GTK |
| `croc` | tested with 11.5.4 | does the actual transfers — see [receiver.md](receiver.md) |

The feature flags are **minimum system library versions** and are checked at build time.
Never set them higher than what is installed:

```sh
pkg-config --modversion gtk4 libadwaita-1 glib-2.0
```

On an older distribution you can lower them (e.g. `gnome_48` / `v1_7`) as long as the code
doesn't use newer APIs — the compiler tells you when it does.

## Linux

```sh
# Fedora
sudo dnf install gtk4-devel libadwaita-devel blueprint-compiler glib2-devel
# Debian / Ubuntu
sudo apt install libgtk-4-dev libadwaita-1-dev blueprint-compiler libglib2.0-dev-bin

./dev-install.sh   # once, and again if the repo moves
cargo run
```

`dev-install.sh` copies `data/in.tl2a.airEcho.desktop` to
`~/.local/share/applications/` with `Exec=` pointing at `target/debug/airecho`.
GNOME Shell only shows notifications for apps it has a `.desktop` file for. It also
installs the *Send with AirEcho* right-click entries: a Nautilus extension in
`~/.local/share/nautilus-python/extensions/` (needs `nautilus-python` /
`python3-nautilus`; restart Files with `nautilus -q`), a Dolphin service menu in
`~/.local/share/kio/servicemenus/`, a Nemo action in `~/.local/share/nemo/actions/`,
and a Thunar custom action merged into `~/.config/Thunar/uca.xml`. Restart each file
manager after installation; KDE's service-menu cache is refreshed when available.
The Thunar installer preserves the other custom actions in `uca.xml`.

Optional:

- **Tray icon on GNOME Shell:** install the *AppIndicator and KStatusNotifierItem Support*
  extension. KDE and most other desktops show tray icons without it.
- **`wl-clipboard`** (Wayland): AirEcho uses `wl-copy` to put received text on the
  clipboard, which can work while AirEcho's window is hidden. Without it, AirEcho falls back
  to GTK's clipboard, which some compositors ignore for unfocused apps.

## macOS

### Install the GTK stack (MacPorts)

```sh
sudo port install gtk4 libadwaita blueprint-compiler adwaita-icon-theme pkgconfig
```

`adwaita-icon-theme` provides the symbolic icons (home, settings, power …); without it they
show as missing.

### macOS: one GLib only

Install the GTK stack from **one** package manager. If the build picks up some libraries
from Homebrew and others from MacPorts, the binary loads two copies of GLib and crashes at
startup:

```
GLib-GObject-CRITICAL **: … signal 'startup' is invalid for instance '…' of type '(null)'
thread 'main' panicked at … glib … assertion failed: handle > 0
```

Typical cause: a `PKG_CONFIG_PATH` in your shell profile that puts Homebrew's
`/usr/local/lib/pkgconfig` first. Remove it — MacPorts' `pkg-config` already finds
everything in `/opt/local`. Check a build with:

```sh
otool -L target/debug/airecho   # every GTK/GLib line should be /opt/local/...
```

### Run

```sh
cargo build
./dev-install.sh   # once: creates "~/Applications/AirEcho.app"
cargo run
```

On macOS, `cargo run` doesn't start `target/debug/airecho` directly. The runner in
`.cargo/config.toml` calls `scripts/macos-app.sh`, which on every run:

1. copies the fresh binary into `~/Applications/AirEcho.app/Contents/MacOS/`
   (with `data/macos/Info.plist`),
2. ad-hoc signs the bundle (`codesign --sign -`),
3. registers it with LaunchServices (`lsregister`),
4. starts AirEcho from inside the bundle — output still goes to your terminal.

This is what makes notifications work: macOS only allows them for a **signed app bundle**
(see [platform-decisions.md](platform-decisions.md#macos-notifications)). On the first
start macOS asks to allow notifications; afterwards **AirEcho** is listed in
System Settings → Notifications. Permission is tied to the bundle id, so it survives
rebuilds.

Running `target/debug/airecho` directly still works — AirEcho just logs that it is
not in a bundle and skips notifications. Test binaries (`cargo test`) pass through the
runner unchanged.

## How a build works

```
data/ui/*.blp ──blueprint-compiler──▶ $OUT_DIR/*.ui ─┐
data/style.css ───────────────────────────────────────────────┼─glib-compile-resources─▶ .gresource
data/in.tl2a.airEcho.gresource.xml (manifest) ─────────┘                              │
                                                              include_bytes! in main.rs ◀────┘
```

`build.rs` runs both tools on every `cargo build`, so Meson isn't needed for development.
Edit `.blp` files, never the generated `.ui`. `AdwApplication` loads `style.css` from the
resource automatically.

Meson (`meson setup builddir && ninja -C builddir`) is meant for installing/packaging; it
also installs the `.desktop` file.

## Debugging

```sh
AIRECHO_DEBUG=1 cargo run
```

logs every croc run:

```
[croc-receive] run: /usr/local/bin/croc --yes --ignore-stdin --rename --out … --local (code: 13 chars)
[croc-receive]   → NoSender (Ok(ExitStatus(unix_wait_status(256))))
[croc-receive]     stderr: connecting... / looking for sender...
[croc-receive]     stdout: could not connect to : found no relay addresses
```

The code, relay password and received text are never printed. What the common lines mean
is in [receiver.md](receiver.md#reading-the-debug-log).

## Tests

```sh
cargo test                                  # parser unit tests, fast, no network
cargo test -- --ignored --test-threads=1    # end-to-end with real croc (receive + send)
```

The end-to-end tests start real `croc relay` / `croc send` processes on local ports and
check: receiving a file through your own relay, receiving via `--ip`, receiving text
byte-for-byte, the wait limit and stop latency, rate-limit detection, and AirEcho's own
sending of text and files plus cancelling a waiting send. They need croc
installed and free local ports 9009–9013 (croc send's own relay) and 9410–9434 (test
relays).

croc's **LAN discovery** (`--local` without an IP) can't be tested on one machine — croc
doesn't see its own discovery broadcast. Test it with two devices on the same network.

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| Crash at startup with `type '(null)'` and `handle > 0` | Two GLib copies (macOS): see [one GLib only](#macos-one-glib-only) |
| `failed to select a version for gtk4-sys … links to the native library gtk-4` | `gtk4` and `libadwaita` crate versions don't match — bump them together (`cargo info libadwaita@<ver>` lists its `gtk` version) |
| Icons missing | Install the Adwaita icon theme |
| Nothing received in Local mode, log shows `found no relay addresses` | Neither AirEcho's multicast probe nor croc's own discovery found the sender — set **Sender IP** in Advanced |
| Nothing received at all | Codes differ: the sender must use the exact Receive Code (`CROC_SECRET=…`) |
| No notifications on Linux | Run `./dev-install.sh`; on GNOME the `.desktop` file must be installed |
| No notifications on macOS / not in Settings → Notifications | Start with `cargo run` (or the `.app`), not `target/debug/…`; allow notifications when asked |
| No tray icon on GNOME | Install the AppIndicator extension |
| `croc is not installed or not on PATH` toast | Install croc, or put it in one of the folders listed in the README |
| Copied text isn't sent | AirEcho on? Advanced → *Send Copied Text* on? Password-manager copies are skipped by design. GNOME/Wayland: AirEcho prefers XWayland; without XWayland the native Wayland watcher only works while focused |
| "the receiver rejected the transfer" (croc: `peer error: refusing files`) | croc sends this for **any** error on the receiving side — its own output says which. Usual causes: a file name croc won't create (AirEcho renames those, see [sending.md](sending.md#file-names-croc-refuses)), or a receiver without `--yes` and no terminal (it declines). Text is never affected |
| "Not delivered: nobody picked up …" | The other side isn't receiving with the same code/mode — or (Local mode) can't reach this machine; give it this machine's IP as *Sender IP* |
| No *Send with AirEcho* in Finder | Start AirEcho once with `cargo run` (registers the service), then check System Settings → Keyboard → Keyboard Shortcuts → Services → Files and Folders |
| No *Send with AirEcho* in Nautilus | Install `nautilus-python` (Fedora) or `python3-nautilus` (Debian/Ubuntu), run `./dev-install.sh`, then restart Nautilus with `nautilus -q` |
| No *Send with AirEcho* in Nemo | Run `./dev-install.sh`, then restart Nemo with `nemo -q` |
| No *Send with AirEcho* in Thunar | Run `./dev-install.sh`, then restart Thunar with `thunar -q`; check `~/.config/Thunar/uca.xml` for the AirEcho action |
| No *Send with AirEcho* in Dolphin | Run `./dev-install.sh`; if it still does not appear, restart Dolphin or log out and back in |
| Copied text isn't detected while AirEcho is in the background on GNOME Wayland | AirEcho prefers XWayland when no `GDK_BACKEND` override is set. Ensure XWayland is available, fully quit and relaunch AirEcho, and check that the session provides `DISPLAY` |
