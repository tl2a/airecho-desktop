# AirEcho

AirEcho is a small background app that **sends and receives files and text with
[croc](https://github.com/schollz/croc)** — without you having to type a `croc` command
every time. Leave it running in the system tray: copy text on one machine and paste it on
the other; right-click files and choose *Send with AirEcho*. Received files land in
`~/Downloads/AirEcho`, received text goes straight to your clipboard.

It is a GNOME-style app (GTK 4 + libadwaita, written in Rust) that runs on **Linux** and
**macOS**.

## What it does

- **Receives automatically.** While AirEcho is on, it keeps asking croc for incoming
  transfers using a code you agree on with the sender.
- **Files** are saved to `~/Downloads/AirEcho`. A file that already exists is never
  overwritten — the new one is saved as `name (1).ext`.
- **Text** (`croc send --text …`) is copied to the clipboard.
- **Sends what you copy.** Copy text anywhere and AirEcho sends it (switch off in Advanced →
  *Send Copied Text*). Copies that password managers mark as private are never sent.
- **Sends files** from the right-click menu: Finder → *Send with AirEcho* (Services /
  Quick Actions) or *Open With → AirEcho*; Nautilus, Dolphin, Nemo, and Thunar
  can use *Send with AirEcho* after `./dev-install.sh`; or Advanced → *Send Files…*.
- **Lives in the tray / menu bar.** Closing the window hides it; the tray icon turns
  green while AirEcho is on. Tray menu: *Enable*, *Settings*, *Quit*.
- **Notifies you** when files or text arrive, and when AirEcho is switched on.
- **Three ways to find the sender:**

  | Mode | Use when | Checks every |
  |---|---|---|
  | **Local Network** (default) | Sender is on the same network. Optionally give the sender's IP — needed where automatic discovery doesn't work (e.g. on macOS) | 3 s |
  | **Own Relay** | You run your own `croc relay` | 10 s |
  | **Public Relay** | Sender is anywhere on the internet | 30 s |

  Why polling and why these intervals: see [docs/receiver.md](docs/receiver.md).

## Sending to AirEcho

Another AirEcho with the same code just works. From plain croc:

On the other machine, use the same code AirEcho listens for (Advanced → Receive Code):

```sh
CROC_SECRET=<your-code> croc send photo.jpg            # a file (or several, or a folder)
CROC_SECRET=<your-code> croc send --text "hello"       # text → lands on the clipboard
```

croc requires the code in the `CROC_SECRET` environment variable on Linux/macOS. If you
set a **Sender IP** in AirEcho, don't use `--no-local` on the sender — AirEcho connects to
the small relay croc starts on the sender (port 9009).

## Sending from AirEcho

AirEcho sends with the same code and mode it receives with, e.g. in Local mode:

```sh
CROC_SECRET=<your-code> croc --local send --text "<what you copied>"
```

Receiving pauses while sending (with one shared code AirEcho would otherwise receive its
own transfer) and resumes afterwards. Text nobody picks up within 30 s is dropped; newer
copied text replaces older text that is still waiting. Details:
[docs/sending.md](docs/sending.md).

## Download

Ready-made builds are on the [Releases](https://github.com/tl2a/kopico-desktop/releases)
page: a `.dmg` for macOS 15+ (Apple Silicon and Intel) and an `.AppImage` for Linux
(ARM and x86_64; needs glibc 2.43+, e.g. Ubuntu 26.04 / Fedora 44). Install
[croc](https://github.com/schollz/croc) as well — AirEcho doesn't include it. The macOS app
isn't notarized: allow it once in *System Settings → Privacy & Security*. Details and how
releases are made: [docs/releasing.md](docs/releasing.md).

## Requirements (building)

| | Linux | macOS |
|---|---|---|
| Rust (2024 edition) | ✓ | ✓ |
| GTK ≥ 4.22, libadwaita ≥ 1.9 | distro packages (GNOME 50 era) | MacPorts |
| `blueprint-compiler`, `glib-compile-resources`, `pkg-config` | ✓ | MacPorts |
| [croc](https://github.com/schollz/croc) on `PATH` (or in `/usr/local/bin`, `/opt/homebrew/bin`, `/opt/local/bin`, `~/.local/bin`, `~/go/bin`) | ✓ | ✓ |
| Adwaita icon theme | usually installed | `sudo port install adwaita-icon-theme` |
| Tray icon on GNOME Shell | *AppIndicator and KStatusNotifierItem Support* extension | — |
| `wl-clipboard` (optional, Wayland) | lets AirEcho set the clipboard while unfocused | — |
| `nautilus-python` (optional) | *Send with AirEcho* in GNOME Files | — |

The GTK/libadwaita minimums come from the crate feature flags (`gnome_50`, `v1_9`), which
are checked at build time. Check yours with
`pkg-config --modversion gtk4 libadwaita-1`.

> **macOS:** install the GTK stack from **one** package manager. Mixing Homebrew and
> MacPorts libraries makes the app crash at startup — see
> [docs/development.md](docs/development.md#macos-one-glib-only).

## Build and run

### Linux

```sh
# Fedora:         sudo dnf install gtk4-devel libadwaita-devel blueprint-compiler glib2-devel
# Debian/Ubuntu:  sudo apt install libgtk-4-dev libadwaita-1-dev blueprint-compiler libglib2.0-dev-bin
./dev-install.sh      # once: .desktop file (notifications) + "Send with AirEcho" menus
cargo run
```

### macOS

```sh
sudo port install gtk4 libadwaita blueprint-compiler adwaita-icon-theme pkgconfig
cargo build
./dev-install.sh      # once: creates and registers "~/Applications/AirEcho.app"
cargo run             # runs AirEcho from that app bundle (notifications, Finder menu)
```

On the first start macOS asks to allow notifications; AirEcho then appears in
System Settings → Notifications.

### Meson (install / packaging)

```sh
meson setup builddir
ninja -C builddir
ninja -C builddir install
```

`cargo build` alone is enough for development — `build.rs` compiles the Blueprint UI and
resources itself.

### Tests

```sh
cargo test                                    # unit tests
cargo test -- --ignored --test-threads=1      # end-to-end tests with real croc processes
```

Debug the receiver with `AIRECHO_DEBUG=1 cargo run` (the legacy `KOPICO_DEBUG=1` is
also accepted). This logs every croc run, but never the code, password, or received text.

More: [docs/development.md](docs/development.md).

## Libraries

| Crate | Why |
|---|---|
| `gtk4` 0.11 + `libadwaita` 0.9 | UI toolkit and GNOME widgets. Versions move in lockstep (libadwaita 0.9 is built on gtk4 0.11) |
| `tray-icon` 0.26 (`ksni` backend) | Tray / menu bar icon. Its default Linux backend loads GTK 3, which can't run next to GTK 4 |
| `async-channel` | Delivers events from the background croc thread to the UI without polling |
| `dirs-next` | Finds the Downloads and home folders |
| `notify-rust` (macOS) | Notifications through Apple's `UNUserNotificationCenter` |
| `objc2`, `objc2-app-kit`, `objc2-foundation` (macOS) | Clipboard (`NSPasteboard`: write received text, watch copies) and the Finder *Send with AirEcho* Service |

Linux notifications use GTK's own `gio::Notification`; no extra crate.

## Platform constraints and decisions

The short version — each one is explained in
[docs/platform-decisions.md](docs/platform-decisions.md):

| Constraint | Decision |
|---|---|
| croc can't wait for a sender; each run finds one or exits | Run croc in a loop; intervals per mode, 60 s back-off when a relay rate-limits |
| Public croc relays allow ~30 joins/min | Public mode checks every 30 s; **Local** is the default |
| croc's LAN discovery fails on some machines (seen on macOS) | Optional **Sender IP** — croc connects to the sender directly |
| `tray-icon`'s default Linux backend uses GTK 3 | Use the `ksni` (D-Bus) backend |
| GLib has no macOS notification backend | Linux: `gio::Notification`; macOS: `notify-rust` |
| macOS notifications require a signed app bundle | `cargo run` runs AirEcho from an ad-hoc-signed `AirEcho.app` |
| GNOME only shows notifications for apps with a `.desktop` file | `dev-install.sh` installs one; Meson installs it |
| GTK's clipboard copies nothing on macOS | Write to `NSPasteboard` directly |
| Wayland may refuse clipboard writes from an unfocused app | Use `wl-copy` when available, GTK otherwise |
| Two GLib copies in one process crash GTK | One package manager for the GTK stack on macOS |
| Sending and receiving use one croc code | Pause receiving while sending; never interrupt an incoming transfer |
| Two AirEcho instances would bounce copied text back and forth | Don't send text that was just received or sent |
| macOS has no clipboard-change event | Check the pasteboard every 500 ms |
| GNOME/Wayland clipboard notifications are limited for native Wayland clients | Prefer XWayland for AirEcho; if XWayland is unavailable, copies are noticed only while its window is focused |
| A Finder *Share* entry needs a separate Share Extension | Finder *Services* item + *Open With*, both handled in-app |

## Known limitations

- **Settings are not saved yet** — receive code, relay mode, sender IP etc. reset when the
  app restarts.
- **The default receive code is in the source code.** Anyone who knows it and can reach
  you (or your relay) can send you files. Change it in Advanced, or in `src/main.rs`.
- Clicking a notification on macOS does nothing yet (on Linux it opens the window).
- Copied text is passed to croc as a command-line argument, so other users on the same
  machine could see it in the process list while it is being sent.
- On GNOME (Wayland), AirEcho prefers XWayland for background clipboard change events. If
  XWayland is unavailable, GTK falls back to Wayland, where copies are only noticed while
  AirEcho's window is focused. An explicit `GDK_BACKEND` setting is respected.
- The Linux file-manager *Send with AirEcho* actions are not tested on every file manager.
- macOS release builds would need a real Apple Developer signature instead of the ad-hoc
  development one.
- `meson.build` still asks for GTK ≥ 4.14 / libadwaita ≥ 1.6 although the code needs
  4.22 / 1.9.

## Project layout

```
src/main.rs           app setup, actions (= app state), window, clipboard
src/transfers.rs      coordinates receiving and sending (one croc code)
src/receiver.rs       croc receive loop (background thread)
src/sender.rs         croc send (text, files)
src/croc_names.rs     send files under names croc accepts (e.g. macOS screenshots)
src/clipboard.rs      write received text, watch copied text
src/macos_services.rs Finder "Send with AirEcho" (macOS Services)
src/macos_bundle.rs   release .app: points GTK at its bundled data
src/tray.rs           tray icon and menu
src/notification.rs   system notifications (Linux: gio, macOS: notify-rust)
data/ui/window.blp    UI (Blueprint)
data/style.css        app CSS
data/macos/           macOS bundle metadata (Info.plist: Services, Open With; AppIcon.icns)
data/icons/           app icon (SVG)
data/linux/           File-manager integrations ("Send with AirEcho")
scripts/macos-app.sh  builds, signs and runs AirEcho.app (macOS `cargo run` runner)
scripts/build-*.sh    release packaging: .dmg (macOS), .AppImage (Linux)
.github/workflows/    release workflow (builds on every push to main)
dev-install.sh        registers the dev build for notifications (Linux and macOS)
docs/                 deeper documentation
```

## Documentation

- [docs/development.md](docs/development.md) — setup, running, debugging, testing, troubleshooting
- [docs/architecture.md](docs/architecture.md) — how the app is put together
- [docs/receiver.md](docs/receiver.md) — the croc receive loop and croc's behaviour it relies on
- [docs/sending.md](docs/sending.md) — sending copied text and files, and how it fits with receiving
- [docs/platform-decisions.md](docs/platform-decisions.md) — platform constraints and the decisions taken
- [docs/releasing.md](docs/releasing.md) — release builds (.dmg, .AppImage) and the GitHub workflow
- [docs/gtk4-libadwaita-patterns.md](docs/gtk4-libadwaita-patterns.md) — GTK 4 / libadwaita coding patterns used here

## License

GPL-3.0-or-later (as declared in `meson.build`).
