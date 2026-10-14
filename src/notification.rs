//! System notifications (Linux/BSD only — this module is not compiled on macOS).
//!
//! Uses `gio::Notification`, the GNOME-native API; clicking a notification runs
//! `app.show-window`.

use adw::Application;
use adw::prelude::*;
use gtk4::gio;

/// Notification id; sending again replaces the previous one instead of stacking.
const ENABLED_ID: &str = "enabled";

/// Tell the user Kopico was turned on (from the window or the tray).
pub fn send_enabled(app: &Application) {
    let notification = gio::Notification::new("Kopico Enabled");
    notification.set_body(Some("Kopico is running in the background."));
    notification.set_default_action(&format!("app.{}", crate::tray::ACTION_SHOW_WINDOW));
    app.send_notification(Some(ENABLED_ID), &notification);
}
