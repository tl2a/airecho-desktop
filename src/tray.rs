//! System tray icon (menu bar on macOS, StatusNotifierItem on Linux).
//!
//! Every tray menu item's id is the name of an `app.*` action, so a click just runs
//! `app.activate_action(id)` on the GTK main thread. The actions themselves are
//! registered in `main.rs`, which keeps all application logic in one place.

use adw::prelude::*;
use adw::Application;
use gtk4::{gio, glib};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Action names — the tray menu item ids double as these.
pub const ACTION_ENABLED: &str = "enabled";
pub const ACTION_SHOW_WINDOW: &str = "show-window";
pub const ACTION_QUIT: &str = "quit";
/// Not in the tray menu: toggles the tray itself (Advanced → Show in System Tray).
pub const ACTION_TRAY: &str = "tray";

/// A live tray icon. Dropping it removes the icon and lets the app quit again once
/// its last window closes.
pub struct Tray {
    icon: TrayIcon,
    _hold: gio::ApplicationHoldGuard,
    enable_item: CheckMenuItem,
}

impl Tray {
    /// Must be called on the main thread with GTK initialised (startup or later).
    pub fn new(app: &Application, enabled: bool) -> Self {
        let enable_item = CheckMenuItem::with_id(ACTION_ENABLED, "Enable", true, enabled, None);
        let settings_item = MenuItem::with_id(ACTION_SHOW_WINDOW, "Settings", true, None);
        let quit_item = MenuItem::with_id(ACTION_QUIT, "Quit", true, None);

        let menu = Menu::new();
        menu.append_items(&[
            &enable_item,
            &settings_item,
            &PredefinedMenuItem::separator(),
            &quit_item,
        ])
        .expect("failed to build tray menu");

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("AirEcho")
            .build()
            .expect("failed to create tray icon");

        let tray = Self {
            icon,
            // Keep the process alive while every window is hidden in the tray
            _hold: app.hold(),
            enable_item,
        };
        tray.set_icon(enabled);
        tray
    }

    /// Mirror the `app.enabled` state in the menu checkmark and status icon.
    pub fn set_enabled(&self, enabled: bool) {
        self.enable_item.set_checked(enabled);
        self.set_icon(enabled);
    }

    fn set_icon(&self, enabled: bool) {
        if let Err(err) = self.icon.set_icon(Some(tray_icon_image(enabled))) {
            eprintln!("Failed to update tray icon: {err}");
        }
    }
}

/// Route tray menu clicks to `app.*` actions. Call once, at startup.
///
/// The handler may run on a non-GTK thread (the ksni backend on Linux has its own
/// worker thread), so only the Send-able item id crosses over and the action is
/// dispatched from an idle callback on the main loop.
pub fn install_menu_handler() {
    MenuEvent::set_event_handler(Some(|event: MenuEvent| {
        let action = event.id.0;
        glib::idle_add_once(move || {
            if let Some(app) = gio::Application::default() {
                app.activate_action(&action, None);
            }
        });
    }));
}

fn tray_icon_image(enabled: bool) -> Icon {
    use gtk4::gdk::prelude::*;

    #[cfg(target_os = "linux")]
    let name = if enabled {
        "in.tl2a.airEcho-status-icon-active-16.png"
    } else {
        "in.tl2a.airEcho-status-icon-inactive-16.png"
    };
    #[cfg(not(target_os = "linux"))]
    let name = if enabled {
        "in.tl2a.airEcho-status-icon-active.svg"
    } else {
        "in.tl2a.airEcho-status-icon-inactive.svg"
    };
    let path = format!("/in/tl2a/airEcho/icons/{name}");
    let bytes = gio::resources_lookup_data(&path, gio::ResourceLookupFlags::NONE)
        .unwrap_or_else(|err| panic!("failed to load AirEcho tray icon {name}: {err}"));
    let texture = gtk4::gdk::Texture::from_bytes(&bytes)
        .unwrap_or_else(|err| panic!("failed to decode AirEcho tray icon {name}: {err}"));
    let width = texture.width() as u32;
    let height = texture.height() as u32;
    let stride = width as usize * 4;
    let mut bgra = vec![0; stride * height as usize];
    texture.download(&mut bgra, stride);
    let mut rgba = Vec::with_capacity(bgra.len());
    for pixel in bgra.chunks_exact(4) {
        let alpha = pixel[3];
        if alpha == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            let unpremultiply =
                |channel: u8| ((u16::from(channel) * 255 / u16::from(alpha)).min(255)) as u8;
            rgba.extend_from_slice(&[
                unpremultiply(pixel[2]),
                unpremultiply(pixel[1]),
                unpremultiply(pixel[0]),
                alpha,
            ]);
        }
    }
    Icon::from_rgba(rgba, width, height).expect("valid AirEcho tray icon")
}
