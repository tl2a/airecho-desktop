//! System notifications (Linux/BSD only — this module is not compiled on macOS).\n//!\n//! Uses `gio::Notification`, the GNOME-native API; clicking a notification runs\n//! `app.show-window`.

use adw::Application;
use adw::prelude::*;
use gtk4::gio;

/// Notification ids — re-using an id replaces the previous notification.
const ENABLED_ID: &str = "enabled";
const FILE_ID: &str = "file-received";

/// Tell the user Kopico was turned on (from the window or the tray).
pub fn send_enabled(app: &Application) {
    let notification = gio::Notification::new("Kopico Enabled");
    notification.set_body(Some("Kopico is running in the background."));
    notification.set_default_action(&format!("app.{}", crate::tray::ACTION_SHOW_WINDOW));
    app.send_notification(Some(ENABLED_ID), &notification);
}

/// Tell the user a file arrived via croc.
pub fn send_file_received(app: &Application, filename: &str) {
    let notification = gio::Notification::new("File Received");
    notification.set_body(Some(&format!("\u{201c}{filename}\u{201d} saved to ~/Downloads/Kopico")));
    notification.set_default_action(&format!("app.{}", crate::tray::ACTION_SHOW_WINDOW));
    // Each file gets the same id → consecutive files replace each other instead
    // of stacking (keeps the notification centre tidy).
    app.send_notification(Some(FILE_ID), &notification);
}
