# Kopico Desktop — AI Agent Context

## Project Overview

**kopico-desktop** is a GNOME desktop application written in **Rust** using:
- [`gtk4`](https://crates.io/crates/gtk4) — GTK4 Rust bindings (`adw::gtk::*` re-exports GTK widgets)
- [`libadwaita`](https://crates.io/crates/libadwaita) crate alias `adw` — Adwaita widgets for GNOME HIG compliance
- **Blueprint** markup (`.blp` files) — compiled to GtkBuilder XML (`.ui`) then bundled as a GResource
- **Meson** — primary build system (orchestrates Blueprint → `.ui` → `.gresource` → `cargo`)
- **Cargo** — also works standalone via `build.rs` (no Meson required for dev)

App ID: `in.tl2a.kopico-desktop`

---

## Build Commands

```bash
# Dev (cargo-only, no Meson needed)
cargo build
cargo run

# Production / install (Meson)
meson setup builddir
ninja -C builddir
ninja -C builddir install

# Check Blueprint syntax without building
blueprint-compiler compile --output /dev/null data/ui/window.blp
```

> `build.rs` automatically runs `blueprint-compiler` + `glib-compile-resources` as part of
> every `cargo build`. The compiled `.gresource` is embedded into the binary via
> `include_bytes!(env!("GRESOURCE_FILE"))`.

---

## Project Structure

```
kopico-desktop/
├── build.rs                                    # Drives blp→ui→gresource for cargo builds
├── meson.build                                 # Meson build (Blueprint + gresource + cargo)
├── Cargo.toml
├── src/
│   └── main.rs                                 # Application entry point + signal handlers
└── data/
    ├── in.tl2a.kopico-desktop.gresource.xml   # GResource manifest
    └── ui/
        └── window.blp                          # ✏️  Edit UI here (Blueprint markup)
```

### UI Pipeline

```
data/ui/*.blp
  → [blueprint-compiler compile]    → $OUT_DIR/data/ui/*.ui   (GtkBuilder XML)
  → [glib-compile-resources]        → $OUT_DIR/in.tl2a.kopico-desktop.gresource
  → [include_bytes! in main.rs]     → embedded in binary
```

---

## Architecture

### Imports

```rust
use adw::prelude::*;                         // Must be in scope for property methods
use adw::{Application, ApplicationWindow, …}; // Adwaita widgets live in adw::
use adw::gtk::{Button, Label, ListBox, …};   // GTK widgets re-exported via adw::gtk
use gtk4::gio;                               // For gio::Resource / resources_register
```

`adw` is the crate alias for `libadwaita`. Never import from both `gtk4::` and `adw::gtk::`
for the same type — pick one path consistently.

### Resource Registration

```rust
fn main() -> gtk4::glib::ExitCode {
    let bytes = include_bytes!(env!("GRESOURCE_FILE"));
    let resource = gio::Resource::from_data(&gtk4::glib::Bytes::from_static(bytes))
        .expect("Failed to load gresource");
    gio::resources_register(&resource);
    // …
}
```

### Loading Widgets from UI Resource

```rust
let builder = gtk4::Builder::from_resource("/in/tl2a/kopico-desktop/ui/window.ui");
let window: adw::ApplicationWindow = builder.object("window").expect("window");
let button: adw::gtk::Button      = builder.object("button").expect("button");
```

Widget IDs in `.blp` files map 1-to-1 to `builder.object("id")` calls.
Always annotate the type — Rust cannot infer it from `object()`.

---

## Blueprint UI Files (`data/ui/*.blp`)

### Conventions

- One `.blp` file per top-level window/dialog.
- All user-visible strings must be wrapped with `_()` for i18n:
  `label: _("Click me!");`
- Widget IDs use `snake_case` and match the Rust variable names exactly.
- CSS classes use the `styles [...]` block:
  ```blueprint
  styles ["pill", "suggested-action"]
  ```
- Adwaita breakpoints are declared inline in Blueprint (no Rust `Breakpoint` code needed):
  ```blueprint
  Adw.Breakpoint {
    condition ("max-width: 600sp")
    setters { split_view.collapsed: true; }
  }
  ```

### Adding a New UI File

1. Create `data/ui/my_dialog.blp`.
2. Add `<file preprocess="xml-stripblanks">ui/my_dialog.ui</file>` to
   `data/in.tl2a.kopico-desktop.gresource.xml`.
3. Add `'data/ui/my_dialog.blp'` to the `blueprints` list in `meson.build`.
4. `build.rs` auto-discovers all `*.blp` in `data/ui/` — no changes needed there.
5. Load with `gtk4::Builder::from_resource("/in/tl2a/kopico-desktop/ui/my_dialog.ui")`.

---

## GTK4 + Libadwaita Patterns

### Property Bindings (bidirectional sync)

```rust
split_view
    .bind_property("show-sidebar", &sidebar_toggle, "active")
    .bidirectional()
    .sync_create()   // immediately syncs current value
    .build();
```

### Signal Handlers (clone pattern)

```rust
{
    let cloned = widget.clone();
    button.connect_clicked(move |_| {
        cloned.do_something();
    });
}
```
Use a scoped block `{ }` to limit the clone to the closure registration.

### Toasts

```rust
toast_overlay.add_toast(adw::Toast::new("Message text"));
// or with custom duration / action:
let toast = adw::Toast::builder()
    .title("Copied!")
    .timeout(2)
    .build();
toast_overlay.add_toast(toast);
```

### OverlaySplitView (responsive sidebar)

```blueprint
Adw.OverlaySplitView split_view {
  show-sidebar: true;

  Adw.Breakpoint {
    condition ("max-width: 600sp")
    setters { split_view.collapsed: true; }
  }

  sidebar: Adw.ToolbarView { … };
  content: Adw.ToolbarView { … };
}
```
In Rust, bind `show-sidebar` ↔ a `ToggleButton.active` for the hamburger button.

### NavigationView (multi-page navigation)

```blueprint
Adw.NavigationView nav {
  Adw.NavigationPage {
    tag: "main";
    title: _("Main");
    child: Adw.ToolbarView { … };
  }
}
```
Push/pop in Rust: `nav.push_by_tag("detail")` / `nav.pop()`.

---

## Adwaita HIG Checklist

- Use `Adw.ApplicationWindow` (not `Gtk.ApplicationWindow`) — it supports breakpoints.
- Use `Adw.ToolbarView` + `Adw.HeaderBar` instead of `Gtk.HeaderBar` directly.
- Use `Adw.StatusPage` for empty states.
- Use `Adw.PreferencesPage` / `Adw.PreferencesGroup` / `Adw.ActionRow` for settings.
- Never hardcode pixel sizes — use `sp` units in breakpoints.
- All destructive actions: CSS class `"destructive-action"`.
- Primary actions: CSS class `"suggested-action"`, optionally add `"pill"`.

---

## Crate Versions (Cargo.toml)

| Crate | Version | Notes |
|---|---|---|
| `gtk4` | `0.11` | `features = ["gnome_50"]` |
| `libadwaita` (alias `adw`) | `0.9` | `features = ["v1_9"]` |
| `gio` | `0.20` | For `Resource`, `resources_register` |

Bump feature flags together when targeting a newer GNOME release.
`gnome_50` enables APIs added up to GNOME 50 / GTK 4.18.

---

## Common Pitfalls

| Mistake | Fix |
|---|---|
| Forgetting `use adw::prelude::*` | Widget property setters won't compile without it |
| Using `gtk4::ApplicationWindow` instead of `adw::ApplicationWindow` | Adwaita breakpoints need `AdwApplicationWindow` |
| Missing `fs = import('fs')` in `meson.build` | `fs.stem()` fails at configure time |
| Widget ID mismatch between `.blp` and `builder.object("id")` | Panics at runtime — keep IDs in sync |
| Editing the generated `.ui` file | It's regenerated from `.blp` every build — edit `.blp` only |

