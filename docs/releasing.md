# Releasing

Release binaries are built by [`.github/workflows/release.yml`](../.github/workflows/release.yml):

| Platform | File | Built on | GTK from |
|---|---|---|---|
| macOS, Apple Silicon | `Kopico_Desktop-<version>-arm64.dmg` | `macos-15` | Homebrew (bottles) |
| macOS, Intel | `Kopico_Desktop-<version>-x86_64.dmg` | `macos-15-intel` | MacPorts, `gtk4 +quartz` |
| Linux, ARM | `Kopico_Desktop-<version>-aarch64.AppImage` | `ubuntu-24.04-arm`, `ubuntu:26.04` container | Ubuntu 26.04 |
| Linux, x86_64 | `Kopico_Desktop-<version>-x86_64.AppImage` | `ubuntu-24.04`, `ubuntu:26.04` container | Ubuntu 26.04 |

## How to release

1. Bump `version` in `Cargo.toml` (and `meson.build`) in your pull request.
2. Merge it into `main`.

Every push to `main` builds all four. The release `v<version>` is created only if it
doesn't exist yet. The binaries go in with a `SHA256SUMS` file, the notes from
[`.github/release-notes.md`](../.github/release-notes.md) and GitHub's generated
changelog. Merging without a version bump still builds everything (to catch packaging
breakage), and the files are available as artifacts on the workflow run. It can also be
started by hand (*Actions → Release → Run workflow*).

## What's in a release

**croc is not bundled.** Users install it themselves; Kopico finds it on `PATH` and in the
usual install folders (`receiver::find_croc`). The release notes say so.

### macOS `.dmg` — `scripts/build-macos-dmg.sh`

- `Kopico Desktop.app` with GTK, libadwaita and every library they need in
  `Contents/Frameworks` (copied and relinked by `dylibbundler`), plus GTK's data in
  `Contents/Resources`: GSettings schemas, the Adwaita icon theme (the UI's symbolic icons),
  and the gdk-pixbuf SVG loader.
- **Libraries are referenced as `@rpath/<name>`**, with the executable's rpath set to
  `Contents/Frameworks`. The longer `@executable_path/../Frameworks/<name>` doesn't fit in
  some libraries' headers (MacPorts' librsvg has no padding). The script fails if any
  reference to `/opt/homebrew`, `/opt/local` or `/usr/local` remains.
- **`src/macos_bundle.rs`** runs first thing in `main`. When `Contents/Resources/share`
  exists, it points GTK at the bundle (`XDG_DATA_DIRS`, `GSETTINGS_SCHEMA_DIR`,
  `GTK_DATA_PREFIX`, …). It also writes gdk-pixbuf's `loaders.cache` with the app's real
  path to `~/Library/Caches/in.tl2a.kopico-desktop/`, because that file needs absolute
  paths. Dev builds (`cargo run`) have no bundled data and are unaffected.
- **Ad-hoc signed, not notarized** (that needs a paid Apple Developer account). Downloaded
  copies are quarantined: users confirm the first launch in *System Settings → Privacy &
  Security → Open Anyway*. Notifications work (ad-hoc signing is enough).
- **Minimum macOS** = the build machine's major version (`LSMinimumSystemVersion` is set
  from `sw_vers`): Homebrew bottles and MacPorts builds target the OS they're built on. On
  the `macos-15` runners that's macOS 15.
- The **first launch takes ~25 s** while macOS checks the ~60 newly signed libraries
  (measured on macOS 26, Intel); after that it starts in ~2 s.
- **Why two package managers:** Homebrew only has Quartz GTK bottles for Apple Silicon
  (Intel macOS is unsupported there). MacPorts' binary packages are `+x11`, useless for a
  native app, so on Intel `gtk4 +quartz` is built from source. That build takes a while,
  so `/opt/local` is cached (`macports-<runner>-gtk4-libadwaita-vN`; bump `vN` to
  rebuild).

### Linux `.AppImage` — `scripts/build-appimage.sh`

- Built in an `ubuntu:26.04` container, the first LTS with GNOME 50 (GTK 4.22,
  libadwaita 1.9). The AppImage needs **glibc ≥ 2.43** on the user's system (Ubuntu
  26.04, Fedora 44 or newer).
- `linuxdeploy` + `linuxdeploy-plugin-gtk` bundle GTK, libadwaita, GSettings schemas,
  pixbuf loaders and the IBus input-method module (with its libraries and an rpath). The
  Adwaita icon theme is copied too, because not every desktop has it. Libraries every
  desktop has (glibc, libGL, X11/Wayland, fontconfig, harfbuzz, fribidi, …) are left to
  the system, per AppImage's exclude list.
- The GTK plugin's runtime hook forces `GDK_BACKEND=x11` and `GTK_THEME=Adwaita:light`
  (workarounds for GTK 3). Both are removed: the first loses Wayland, the second breaks
  libadwaita's dark mode and accent colours.
- The *Send with Kopico* file-manager entries aren't installed by an AppImage.

## Building locally

```sh
# macOS (Homebrew or MacPorts GTK stack + dylibbundler, see the script header)
scripts/build-macos-dmg.sh                    # → dist/Kopico_Desktop-<version>-<arch>.dmg

# Linux AppImage, in Docker (any host OS; the image's architecture is the AppImage's)
docker run --rm -v "$PWD":/src -w /src -e CARGO_TARGET_DIR=/src/target/linux \
    ubuntu:26.04 scripts/build-appimage.sh --install-deps
```

`CARGO_TARGET_DIR` keeps the container's build apart from your own `target/`.

### Verified locally

- **x86_64 AppImage:** built in Docker and started in a clean `ubuntu:26.04` container
  without GTK or libadwaita (only the desktop baseline from the exclude list, under Xvfb).
  The window rendered with icons and styling.
- **x86_64 `.dmg`** (MacPorts): every library loaded from the bundle (`DYLD_PRINT_LIBRARIES`),
  no GTK warnings.
- **Not run before CI:** the Homebrew (arm64) build and the aarch64 AppImage.
