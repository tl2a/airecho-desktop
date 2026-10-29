# Contributing to AirEcho

Thanks for your interest in improving AirEcho. Contributions to bug fixes,
documentation, testing, and new features are welcome.

## Before you start

- For a substantial change, open an issue first to discuss the proposed approach.
- Keep changes focused, and avoid bundling unrelated cleanup with a feature or fix.
- Check the existing [development guide](docs/development.md) and
  [architecture overview](docs/architecture.md) before changing behavior.

## Development setup

AirEcho is a Rust 2024 application built with GTK 4, libadwaita, Blueprint, and
Meson. See the [README](README.md#requirements-building) for platform-specific
package installation commands.

You will need Rust, GTK 4.22 or newer, libadwaita 1.9 or newer,
`blueprint-compiler`, `glib-compile-resources`, and `pkg-config`. Install croc
as well to test real transfers.

On macOS, install the GTK stack from only one package manager. Mixing Homebrew
and MacPorts libraries can load multiple GLib copies and cause startup failures.
The [development guide](docs/development.md#macos-one-glib-only) explains how
to check the linked libraries.

Build and run the app with:

```sh
cargo build
cargo run
```

On macOS, `cargo run` uses the repository's runner to launch AirEcho from its
application bundle. For Linux desktop integration during development, run
`./dev-install.sh` once; see the development guide for details.

## Making changes

- Follow the existing Rust style and patterns. Keep GTK widget work on the GTK
  main thread; use the established channels and background-thread patterns for
  blocking work.
- Make UI layout changes in `data/ui/*.blp`, not generated `.ui` files. The
  build regenerates those files and the GResource bundle.
- Keep widget IDs in Blueprint in sync with any `builder.object("…")` lookups
  in Rust.
- Wrap new user-visible Blueprint strings in `_()` for translation.
- Keep Linux and macOS behavior consistent where possible. If a change affects
  a platform-specific constraint or project decision, update the relevant
  documentation, including `docs/platform-decisions.md` when appropriate.
- Treat transfer codes, relay passwords, clipboard contents, and received text
  as sensitive. Do not add them to logs, test output, screenshots, or commits.
- Do not edit generated files such as `Cargo.lock` by hand. Let Cargo update it
  when dependency changes require it.

For changes to croc receiving or sending, read
[Receiving Files](docs/receiver.md) and [Sending](docs/sending.md) first. The
code depends on croc's command-line behavior and output.

## Validate your changes

Run the checks relevant to your change before opening a pull request:

```sh
cargo fmt --check
blueprint-compiler compile --output /dev/null data/ui/window.blp
cargo build
cargo test
git diff --check
```

If you change multiple Blueprint files, compile each changed file. For
end-to-end transfer tests, croc must be installed and the tests need available
local ports:

```sh
cargo test -- --ignored --test-threads=1
```

These tests start real croc processes; see the
[development guide's test section](docs/development.md#tests) for prerequisites
and coverage. Local multicast discovery cannot be meaningfully tested on one
machine; use two devices on the same network.

## Pull requests

When opening a pull request:

- Explain the problem and the behavior of the change.
- Include steps to reproduce the issue or verify the change where useful.
- List the checks you ran and any checks you could not run.
- Include screenshots for visible UI changes, and state which platform(s) you
  tested.
- Update relevant documentation when behavior, setup, or platform requirements
  change.
- Keep the pull request focused and ensure it contains no credentials or other
  private data.
