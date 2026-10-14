//! System tray icon (menu bar on macOS, StatusNotifierItem on Linux).
//!
//! Every tray menu item's id is the name of an `app.*` action, so a click just runs
//! `app.activate_action(id)` on the GTK main thread. The actions themselves are
//! registered in `main.rs`, which keeps all application logic in one place.

use adw::Application;
use adw::prelude::*;
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
            .with_tooltip("Kopico Desktop")
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

    /// Mirror the `app.enabled` state: "Enable" checkmark + green / normal icon.
    pub fn set_enabled(&self, enabled: bool) {
        self.enable_item.set_checked(enabled);
        self.set_icon(enabled);
    }

    fn set_icon(&self, enabled: bool) {
        let result = if enabled {
            // Drawn as-is (never a template), so the green survives on macOS too
            self.icon.set_icon(Some(tray_icon_image(GREEN)))
        } else {
            // macOS: template image, recoloured by the menu bar (light/dark aware)
            #[cfg(target_os = "macos")]
            let result = self.icon.set_icon_templated(Some(tray_icon_image(NORMAL)));
            #[cfg(not(target_os = "macos"))]
            let result = self.icon.set_icon(Some(tray_icon_image(NORMAL)));
            result
        };
        if let Err(err) = result {
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

/// Enabled: GNOME palette green.
const GREEN: [u8; 3] = [0x2e, 0xc2, 0x7e];
/// Disabled: black template on macOS (menu bar recolours it), Adwaita blue elsewhere.
const NORMAL: [u8; 3] = if cfg!(target_os = "macos") { [0, 0, 0] } else { [0x35, 0x84, 0xe4] };

/// A 32×32 ring with a centre dot in the given colour.
fn tray_icon_image(colour: [u8; 3]) -> Icon {
    const SIZE: u32 = 32;

    let centre = (SIZE as f32 - 1.0) / 2.0;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let d = ((x as f32 - centre).powi(2) + (y as f32 - centre).powi(2)).sqrt();
            let on = (11.0..=15.0).contains(&d) || d <= 5.0;
            rgba.extend_from_slice(&colour);
            rgba.push(if on { 0xff } else { 0x00 });
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("valid tray icon")
}
