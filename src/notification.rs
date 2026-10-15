//! System notifications (Linux/BSD only — this module is not compiled on macOS).
//!
//! Uses `gio::Notification`, the GNOME-native API; clicking a notification runs
//! `app.show-window`.

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

/// Tell the user a text message arrived and is on the clipboard. The text itself is
/// not shown — notifications can appear on the lock screen.
pub fn send_text_received(app: &Application, message: &str) {
    let notification = gio::Notification::new("Text Received");
    notification.set_body(Some(message));
    notification.set_default_action(&format!("app.{}", crate::tray::ACTION_SHOW_WINDOW));
    app.send_notification(Some(FILE_ID), &notification);
}

/// Tell the user files arrived via croc. `message` is e.g. "“a.jpg” saved to ~/Downloads/Kopico".
pub fn send_files_received(app: &Application, message: &str) {
    let notification = gio::Notification::new("Files Received");
    notification.set_body(Some(message));
    notification.set_default_action(&format!("app.{}", crate::tray::ACTION_SHOW_WINDOW));
    // Same id for every transfer → a new one replaces the last instead of stacking
    app.send_notification(Some(FILE_ID), &notification);
}
