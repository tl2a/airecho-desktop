//! Running from the release `.app` (built by `scripts/build-macos-dmg.sh`): GTK's libraries
//! are bundled in `Contents/Frameworks` and its data — GSettings schemas, Adwaita icons,
//! the SVG image loader — in `Contents/Resources`. GTK and GLib look for data in their
//! build prefix (`/opt/homebrew`, `/opt/local`), which users don't have, so point them at
//! the bundle before GTK starts.
//!
//! Dev builds (`cargo run`'s bundle from `scripts/macos-app.sh`) have no bundled data and
//! keep using the system libraries; this does nothing for them.

use std::{fs, path::Path};

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

/// Show the Dock icon only while the AirEcho window is active.
pub fn set_dock_icon_visible(visible: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("[macos] cannot update Dock visibility off the main thread");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let policy = if visible {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    if app.activationPolicy() == policy {
        return;
    }
    if !app.setActivationPolicy(policy) {
        eprintln!(
            "[macos] failed to change activation policy from {:?} to {:?}",
            app.activationPolicy(),
            policy
        );
    }
}

/// Activate the app after its window has been presented from the tray.
pub fn activate_application() {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("[macos] cannot activate the app off the main thread");
        return;
    };
    NSApplication::sharedApplication(mtm).activate();
}

/// Call first thing in `main`, before any other thread exists.
pub fn setup_environment() {
    let Some(exe) = std::env::current_exe().ok() else { return };
    let Some(contents) = exe.parent().and_then(Path::parent) else { return };
    let resources = contents.join("Resources");
    let share = resources.join("share");
    if !share.join("glib-2.0/schemas").is_dir() {
        return; // dev build
    }

    // SAFETY: called at the start of main, before any other thread exists
    unsafe {
        std::env::set_var("XDG_DATA_DIRS", &share);
        std::env::set_var("GSETTINGS_SCHEMA_DIR", share.join("glib-2.0/schemas"));
        std::env::set_var("GTK_DATA_PREFIX", &resources);
        std::env::set_var("GTK_EXE_PREFIX", &resources);
        // No GIO modules are bundled — don't load the build machine's (if it has them)
        std::env::set_var("GIO_MODULE_DIR", contents.join("Frameworks/gio-modules"));
    }
    if let Some(cache) = write_pixbuf_loaders_cache(contents) {
        // SAFETY: as above
        unsafe { std::env::set_var("GDK_PIXBUF_MODULE_FILE", cache) };
    }
}

/// gdk-pixbuf's `loaders.cache` needs absolute paths, and the app can be anywhere: the
/// bundle carries a template with `@LOADERS_DIR@`, written out with the real path.
fn write_pixbuf_loaders_cache(contents: &Path) -> Option<std::path::PathBuf> {
    let template = fs::read_to_string(contents.join("Resources/loaders.cache.in")).ok()?;
    let loaders_dir = contents.join("Frameworks");
    let cache = dirs_next::cache_dir()?.join(crate::APP_ID).join("loaders.cache");
    fs::create_dir_all(cache.parent()?).ok()?;
    fs::write(&cache, template.replace("@LOADERS_DIR@", &loaders_dir.to_string_lossy())).ok()?;
    Some(cache)
}
