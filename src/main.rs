use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use adw::{
    AboutDialog, ActionRow, Application, ApplicationWindow, ComboRow, EntryRow, PasswordEntryRow,
    SwitchRow, Toast, ToastOverlay,
};
use adw::gtk::Label;
use gtk4::{gio, glib};

mod clipboard;
mod croc_names;
mod notification;
mod receiver;
mod sender;
mod tray;
mod transfers;
#[cfg(target_os = "macos")]
mod macos_bundle;
#[cfg(target_os = "macos")]
mod macos_services;

use receiver::RelayMode;
use transfers::Transfers;
use tray::{
    ACTION_ENABLED, ACTION_QUIT, ACTION_SHOW_WINDOW, ACTION_TOGGLE_ENABLED, ACTION_TRAY, Tray,
};

const APP_ID: &str = "in.tl2a.airEcho";
const PROJECT_URL: &str = "https://airecho.tl2a.in";
const ISSUE_URL: &str = "https://github.com/tl2a/airecho-desktop/issues";

/// Default receive code.  Both sender and receiver must agree on this value.
/// Users can override it in Advanced → Receive Code.
const DEFAULT_RECEIVE_CODE: &str = "my-code-phase";
/// croc rejects shorter codes.
const MIN_CODE_LEN: usize = 6;

// Receiver settings — stateful string actions (`app.<name>`), see setup_app()
const ACTION_RELAY_MODE: &str = "relay-mode";
const ACTION_RELAY_ADDRESS: &str = "relay-address";
const ACTION_RELAY_PASSWORD: &str = "relay-password";
const ACTION_RECEIVE_CODE: &str = "receive-code";
/// Optional Local mode manual override: `--ip` when automatic discovery cannot find the sender.
const ACTION_SENDER_IP: &str = "sender-ip";
/// Bool, default on: send text whenever the user copies some (Advanced → Send).
const ACTION_SEND_CLIPBOARD: &str = "send-clipboard";
/// Opens a file chooser and sends the chosen files (Advanced → Send Files…).
const ACTION_SEND_FILES: &str = "send-files";

/// `app.relay-mode` values, in the same order as the Relay Mode combo row.
const RELAY_MODES: [&str; 3] = ["local", "own-relay", "public-relay"];

fn main() -> gtk4::glib::ExitCode {
    #[cfg(target_os = "linux")]
    configure_gnome_wayland_backend();

    #[cfg(target_os = "macos")]
    macos_bundle::setup_environment();

    let bytes = include_bytes!(env!("GRESOURCE_FILE"));
    let resource = gio::Resource::from_data(&gtk4::glib::Bytes::from_static(bytes))
        .expect("Failed to load gresource data");
    gio::resources_register(&resource);

    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    let transfers = Transfers::new(&app);
    let clipboard_watch_status = Rc::new(Cell::new(ClipboardWatchStatus::NotStarted));
    {
        let transfers = transfers.clone();
        app.connect_startup(move |app| setup_app(app, &transfers));
    }
    {
        let transfers = transfers.clone();
        let clipboard_watch_status = clipboard_watch_status.clone();
        app.connect_activate(move |app| {
            show_window(app);
            let Some(window) = app
                .windows()
                .into_iter()
                .find_map(|window| window.downcast::<ApplicationWindow>().ok())
            else {
                eprintln!("[clipboard] no application window — clipboard watching not started");
                return;
            };

            if window.is_active() {
                start_clipboard_watch(&clipboard_watch_status, &transfers);
            } else if clipboard_watch_status.get() == ClipboardWatchStatus::NotStarted {
                clipboard_watch_status.set(ClipboardWatchStatus::WaitingForFocus);
                let clipboard_watch_status = clipboard_watch_status.clone();
                let transfers = transfers.clone();
                window.connect_is_active_notify(move |window| {
                    if window.is_active() {
                        start_clipboard_watch(&clipboard_watch_status, &transfers);
                    }
                });
            }
        });
    }
    // Files handed to AirEcho — Finder "Send with AirEcho" / "Open With", Linux file-manager
    // actions, or `airecho <files>` — are sent, not opened.
    app.connect_open(move |_, files, _hint| {
        transfers.send_files(files.iter().filter_map(|f| f.path()).collect());
    });
    app.run()
}

#[cfg(target_os = "linux")]
fn configure_gnome_wayland_backend() {
    if should_prefer_x11(
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        std::env::var_os("GDK_BACKEND").is_some(),
    ) {
        eprintln!("[platform] GNOME Wayland detected; preferring XWayland with Wayland fallback");
        // SAFETY: this runs at process startup before GTK initialization or any threads.
        unsafe { std::env::set_var("GDK_BACKEND", "x11,wayland") };
    }
}

#[cfg(target_os = "linux")]
fn should_prefer_x11(
    current_desktop: Option<&str>,
    has_wayland_display: bool,
    has_backend_override: bool,
) -> bool {
    !has_backend_override
        && has_wayland_display
        && current_desktop.is_some_and(|desktops| {
            desktops
                .split(':')
                .any(|desktop| desktop.eq_ignore_ascii_case("GNOME"))
        })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClipboardWatchStatus {
    NotStarted,
    WaitingForFocus,
    Started,
}

fn start_clipboard_watch(status: &Cell<ClipboardWatchStatus>, transfers: &Transfers) {
    if status.replace(ClipboardWatchStatus::Started) == ClipboardWatchStatus::Started {
        return;
    }
    // GDK may expose the existing Wayland clipboard only after the window becomes active.
    let transfers = transfers.clone();
    clipboard::watch(Box::new(move |text| transfers.on_text_copied(text)));
}

#[cfg(all(test, target_os = "linux"))]
mod platform_tests {
    use super::should_prefer_x11;

    #[test]
    fn prefers_x11_only_for_gnome_wayland_without_an_override() {
        assert!(should_prefer_x11(Some("GNOME"), true, false));
        assert!(should_prefer_x11(Some("KDE:GNOME"), true, false));
        assert!(!should_prefer_x11(Some("KDE"), true, false));
        assert!(!should_prefer_x11(Some("GNOME"), false, false));
        assert!(!should_prefer_x11(Some("GNOME"), true, true));
        assert!(!should_prefer_x11(None, true, false));
    }
}

/// Current value of a boolean stateful action.
fn action_bool(group: &impl IsA<gio::ActionGroup>, name: &str) -> bool {
    group
        .action_state(name)
        .and_then(|state| state.get::<bool>())
        .unwrap_or(false)
}

/// Current value of a string stateful action.
fn action_string(group: &impl IsA<gio::ActionGroup>, name: &str) -> String {
    group
        .action_state(name)
        .and_then(|state| state.get::<String>())
        .unwrap_or_default()
}

/// Receiver configuration from the `app.relay-*` / `app.receive-code` action states.
fn receiver_config(app: &Application) -> Result<(RelayMode, String), String> {
    let mode = match action_string(app, ACTION_RELAY_MODE).as_str() {
        "own-relay" => {
            let address = action_string(app, ACTION_RELAY_ADDRESS);
            if address.is_empty() {
                return Err("Enter your relay address to start receiving".into());
            }
            RelayMode::OwnRelay { address, password: action_string(app, ACTION_RELAY_PASSWORD) }
        }
        "public-relay" => RelayMode::PublicRelay,
        _ => {
            let ip = action_string(app, ACTION_SENDER_IP);
            RelayMode::Local { sender_ip: (!ip.is_empty()).then_some(ip) }
        }
    };
    Ok((mode, action_string(app, ACTION_RECEIVE_CODE)))
}

/// "“photo.jpg” saved to ~/Downloads/AirEcho" / "3 items saved to …".
fn received_message(names: &[String], out_dir: &std::path::Path) -> String {
    let dir = receiver::display_path(out_dir);
    match names {
        [] => format!("Files saved to {dir}"),
        [name] => format!("\u{201c}{name}\u{201d} saved to {dir}"),
        _ => format!("{} items saved to {dir}", names.len()),
    }
}

/// Toast on the main window (if it exists — it may be hidden in the tray).
fn show_toast(app: &Application, message: &str) {
    for window in app.windows() {
        if let Some(window) = window.downcast_ref::<ApplicationWindow>() {
            show_toast_on_window(window, message);
        }
    }
}

/// Walk the widget tree to find the first `ToastOverlay` and add a toast to it.
fn show_toast_on_window(window: &ApplicationWindow, message: &str) {
    fn find_overlay(widget: &adw::gtk::Widget) -> Option<ToastOverlay> {
        if let Some(o) = widget.downcast_ref::<ToastOverlay>() {
            return Some(o.clone());
        }
        let mut child = widget.first_child();
        while let Some(c) = child {
            if let Some(found) = find_overlay(&c) {
                return Some(found);
            }
            child = c.next_sibling();
        }
        None
    }

    if let Some(overlay) = find_overlay(window.upcast_ref()) {
        overlay.add_toast(Toast::new(message));
    }
}

// ── App setup ──────────────────────────────────────────────────────────────────

/// Runs once per process on the primary instance: registers actions, tray, receiver.
fn setup_app(app: &Application, transfers: &Transfers) {
    let tray: Rc<RefCell<Option<Tray>>> = Rc::new(RefCell::new(None));
    tray::install_menu_handler();
    notification::init(); // macOS: permission prompt on first run
    #[cfg(target_os = "macos")]
    macos_services::register(); // Finder → Services → "Send with AirEcho"

    // ── app.enabled — power toggle + tray checkbox ────────────────────────────
    let enabled_action = gio::SimpleAction::new_stateful(ACTION_ENABLED, None, &true.to_variant());
    {
        let tray = tray.clone();
        let transfers = transfers.clone();
        let app_notif = app.downgrade();

        enabled_action.connect_change_state(move |action, value| {
            let Some(enabled) = value.and_then(|v| v.get::<bool>()) else { return };
            let was_enabled = action.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
            action.set_state(&enabled.to_variant());

            if let Some(t) = tray.borrow().as_ref() {
                t.set_enabled(enabled);
            }
            println!("AirEcho {}", if enabled { "enabled" } else { "disabled" });

            // Start / stop the croc receive loop (off also cancels sending copied text)
            transfers.set_enabled(enabled);

            // System notification on actual off→on transition only
            if enabled && !was_enabled {
                if let Some(app) = app_notif.upgrade() {
                    notification::send_enabled(&app);
                }
            }
        });
    }
    app.add_action(&enabled_action);

    let toggle_enabled_action = gio::SimpleAction::new(ACTION_TOGGLE_ENABLED, None);
    {
        let app_w = app.downgrade();
        toggle_enabled_action.connect_activate(move |_, _| {
            if let Some(app) = app_w.upgrade() {
                app.change_action_state(
                    ACTION_ENABLED,
                    &(!action_bool(&app, ACTION_ENABLED)).to_variant(),
                );
            }
        });
    }
    app.add_action(&toggle_enabled_action);

    // ── app.tray ──────────────────────────────────────────────────────────────
    let tray_action = gio::SimpleAction::new_stateful(ACTION_TRAY, None, &true.to_variant());
    {
        let tray = tray.clone();
        let app_w = app.downgrade();
        tray_action.connect_change_state(move |action, value| {
            let (Some(show), Some(app)) = (value.and_then(|v| v.get::<bool>()), app_w.upgrade())
            else { return };
            action.set_state(&show.to_variant());
            let mut t = tray.borrow_mut();
            match (show, t.is_some()) {
                (true, false) => *t = Some(Tray::new(&app, action_bool(&app, ACTION_ENABLED))),
                (false, true) => *t = None,
                _ => {}
            }
            println!("System tray {}", if show { "enabled" } else { "disabled" });
        });
    }
    app.add_action(&tray_action);
    *tray.borrow_mut() = Some(Tray::new(app, action_bool(app, ACTION_ENABLED)));

    // ── app.show-window ───────────────────────────────────────────────────────
    let show_action = gio::SimpleAction::new(ACTION_SHOW_WINDOW, None);
    {
        let app_w = app.downgrade();
        show_action.connect_activate(move |_, _| {
            if let Some(app) = app_w.upgrade() { show_window(&app); }
        });
    }
    app.add_action(&show_action);

    // ── app.quit ──────────────────────────────────────────────────────────────
    let quit_action = gio::SimpleAction::new(ACTION_QUIT, None);
    {
        let app_w = app.downgrade();
        quit_action.connect_activate(move |_, _| {
            if let Some(app) = app_w.upgrade() { app.quit(); }
        });
    }
    app.add_action(&quit_action);
    app.set_accels_for_action("app.quit", &["<primary>q"]);

    // ── app.about ────────────────────────────────────────────────────────────
    let about_action = gio::SimpleAction::new("about", None);
    {
        let app_w = app.downgrade();
        about_action.connect_activate(move |_, _| {
            let Some(app) = app_w.upgrade() else { return };
            let Some(parent) = app.active_window() else { return };
            show_about_dialog(&parent);
        });
    }
    app.add_action(&about_action);

    // ── Receiver settings: app.relay-mode / relay-address / relay-password / receive-code ──
    // Stateful string actions: the Advanced page reads them for its initial values and
    // changes them with change_action_state(); each accepted change restarts the loop.
    let settings: [(&str, &str, fn(&str) -> bool); 5] = [
        (ACTION_RELAY_MODE, RELAY_MODES[0], |v| RELAY_MODES.contains(&v)),
        (ACTION_RELAY_ADDRESS, "", |v| v.is_empty() || v.contains(':')),
        (ACTION_RELAY_PASSWORD, "", |_| true),
        (ACTION_RECEIVE_CODE, DEFAULT_RECEIVE_CODE, |v| v.chars().count() >= MIN_CODE_LEN),
        (ACTION_SENDER_IP, "", |v| v.is_empty() || receiver::is_valid_sender_ip(v)),
    ];
    for (name, default, is_valid) in settings {
        let action = gio::SimpleAction::new_stateful(name, None, &default.to_variant());
        let transfers = transfers.clone();
        action.connect_change_state(move |action, value| {
            let Some(value) = value.and_then(|v| v.get::<String>()) else { return };
            let value = value.trim().to_owned();
            if !is_valid(&value) || action.state().and_then(|s| s.get::<String>()) == Some(value.clone()) {
                return;
            }
            action.set_state(&value.to_variant());
            transfers.apply_settings();
        });
        app.add_action(&action);
    }

    // ── app.send-clipboard — send copied text (on by default) ──────────────────
    app.add_action(&gio::SimpleAction::new_stateful(
        ACTION_SEND_CLIPBOARD,
        None,
        &true.to_variant(),
    ));

    // ── app.send-files — pick files in a dialog and send them ─────────────────
    let send_files_action = gio::SimpleAction::new(ACTION_SEND_FILES, None);
    {
        let transfers = transfers.clone();
        let app_w = app.downgrade();
        send_files_action.connect_activate(move |_, _| {
            let Some(app) = app_w.upgrade() else { return };
            let dialog = adw::gtk::FileDialog::builder().title("Send Files").modal(true).build();
            let transfers = transfers.clone();
            dialog.open_multiple(app.active_window().as_ref(), gio::Cancellable::NONE, move |result| {
                let Ok(files) = result else { return }; // dismissed
                let paths = files
                    .iter::<gio::File>()
                    .filter_map(|f| f.ok().and_then(|f| f.path()))
                    .collect();
                transfers.send_files(paths);
            });
        });
    }
    app.add_action(&send_files_action);

    // AirEcho starts enabled → start receiving right away (Local mode by default)
    transfers.start_receiving();

    {
        let transfers = transfers.clone();
        app.connect_shutdown(move |_| {
            tray.borrow_mut().take();
            transfers.shutdown(); // no croc left running
        });
    }
}

// ── Window ─────────────────────────────────────────────────────────────────────

fn show_window(app: &Application) {
    #[cfg(target_os = "macos")]
    macos_bundle::set_dock_icon_visible(true);

    let window = app
        .windows()
        .into_iter()
        .next()
        .unwrap_or_else(|| build_ui(app).upcast());
    window.present();
    #[cfg(target_os = "macos")]
    macos_bundle::activate_application();
}

fn build_ui(app: &Application) -> ApplicationWindow {
    let builder = gtk4::Builder::from_resource("/in/tl2a/airEcho/ui/window.ui");

    let window: ApplicationWindow   = builder.object("window").expect("window");
    let main_menu_button: gtk4::MenuButton =
        builder.object("main_menu_button").expect("main_menu_button");
    let state_title: Label          = builder.object("state_title").expect("state_title");
    let state_description: Label    = builder.object("state_description").expect("state_description");
    let tray_row: SwitchRow         = builder.object("tray_row").expect("tray_row");
    let send_clipboard_row: SwitchRow =
        builder.object("send_clipboard_row").expect("send_clipboard_row");
    let relay_mode_row: ComboRow    = builder.object("relay_mode_row").expect("relay_mode_row");
    let relay_address_row: EntryRow = builder.object("relay_address_row").expect("relay_address_row");
    let relay_password_row: PasswordEntryRow =
        builder.object("relay_password_row").expect("relay_password_row");
    let receive_code_row: EntryRow  = builder.object("receive_code_row").expect("receive_code_row");
    let sender_ip_row: EntryRow     = builder.object("sender_ip_row").expect("sender_ip_row");
    let save_to_row: ActionRow      = builder.object("save_to_row").expect("save_to_row");
    let toast_overlay: ToastOverlay = builder.object("toast_overlay").expect("toast_overlay");

    let menu = gio::Menu::new();
    menu.append(Some("About"), Some("app.about"));
    menu.append(Some("Quit"), Some("app.quit"));
    main_menu_button.set_menu_model(Some(&menu));

    window.set_application(Some(app));
    #[cfg(target_os = "macos")]
    window.connect_is_active_notify(|window| {
        if window.is_active() {
            macos_bundle::set_dock_icon_visible(true);
        } else {
            let window = window.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                if window.is_visible() && !window.is_active() {
                    macos_bundle::set_dock_icon_visible(false);
                }
            });
        }
    });

    // ── Home: status text follows app.enabled ─────────────────────────────────
    let update_state_text = move |enabled: bool| {
        if enabled {
            state_title.set_label("AirEcho Is On");
            state_description.set_label(
                "Receiving files in the background. Turn off anytime from here or the tray.",
            );
        } else {
            state_title.set_label("AirEcho Is Off");
            state_description.set_label("AirEcho is paused. Turn it on to start receiving.");
        }
    };
    update_state_text(action_bool(app, ACTION_ENABLED));
    if let Some(action) = app.lookup_action(ACTION_ENABLED) {
        action.connect_notify_local(Some("state"), move |action, _| {
            update_state_text(action.state().and_then(|s| s.get()).unwrap_or(false));
        });
    }

    // ── Advanced: receive settings ────────────────────────────────────────────
    save_to_row.set_subtitle(&receiver::display_path(&receiver::default_out_dir()));

    // Relay mode — combo index ↔ RELAY_MODES; each mode shows only its own rows
    let update_relay_rows = {
        let relay_address_row = relay_address_row.clone();
        let relay_password_row = relay_password_row.clone();
        let sender_ip_row = sender_ip_row.clone();
        move |row: &ComboRow| {
            let own_relay = row.selected() == 1;
            relay_address_row.set_visible(own_relay);
            relay_password_row.set_visible(own_relay);
            sender_ip_row.set_visible(row.selected() == 0);
            row.set_subtitle(match row.selected() {
                0 => "Devices on this network · checks every 3 s",
                1 => "Your own croc relay · checks every 10 s",
                _ => "croc\u{2019}s public relays · checks every 30 s",
            });
        }
    };
    let current_mode = action_string(app, ACTION_RELAY_MODE);
    let index = RELAY_MODES.iter().position(|m| *m == current_mode).unwrap_or(0);
    relay_mode_row.set_selected(index as u32);
    update_relay_rows(&relay_mode_row);
    {
        let app_w = app.downgrade();
        relay_mode_row.connect_selected_notify(move |row| {
            let Some(app) = app_w.upgrade() else { return };
            update_relay_rows(row);
            let mode = RELAY_MODES.get(row.selected() as usize).unwrap_or(&RELAY_MODES[0]);
            app.change_action_state(ACTION_RELAY_MODE, &mode.to_variant());
        });
    }

    // Text settings: apply button → validated action change; invalid input is marked
    let text_rows: [(&EntryRow, &str, &str); 4] = [
        (&sender_ip_row, ACTION_SENDER_IP, "Use an IP address or host name, e.g. 192.168.1.20 or 192.168.1.20:9009"),
        (&relay_address_row, ACTION_RELAY_ADDRESS, "Use host:port, e.g. myrelay.example.com:9009"),
        (relay_password_row.upcast_ref(), ACTION_RELAY_PASSWORD, ""),
        (&receive_code_row, ACTION_RECEIVE_CODE, "The code needs at least 6 characters"),
    ];
    for (row, action, invalid_message) in text_rows {
        row.set_text(&action_string(app, action));
        row.connect_changed(|row| row.remove_css_class("error"));
        let app_w = app.downgrade();
        let toast = toast_overlay.clone();
        let action = action.to_owned();
        let invalid_message = invalid_message.to_owned();
        row.connect_apply(move |row| {
            let Some(app) = app_w.upgrade() else { return };
            let value = row.text().trim().to_owned();
            app.change_action_state(&action, &value.to_variant());
            if action_string(&app, &action) == value {
                toast.add_toast(Toast::new("Saved"));
            } else {
                row.add_css_class("error");
                toast.add_toast(Toast::new(&invalid_message));
            }
        });
    }

    // ── Advanced: tray switch ─────────────────────────────────────────────────
    // ── Advanced: "Send Copied Text" switch → app.send-clipboard ──────────────
    send_clipboard_row.set_active(action_bool(app, ACTION_SEND_CLIPBOARD));
    {
        let app_w = app.downgrade();
        send_clipboard_row.connect_active_notify(move |row| {
            if let Some(app) = app_w.upgrade() {
                app.change_action_state(ACTION_SEND_CLIPBOARD, &row.is_active().to_variant());
            }
        });
    }

    tray_row.set_active(action_bool(app, ACTION_TRAY));
    {
        let app_w = app.downgrade();
        let toast = toast_overlay.clone();
        tray_row.connect_active_notify(move |row| {
            let Some(app) = app_w.upgrade() else { return };
            let show = row.is_active();
            app.change_action_state(ACTION_TRAY, &show.to_variant());
            if !show {
                toast.add_toast(Toast::new("Closing the window will now quit AirEcho"));
            }
        });
    }

    // ── Close → hide to tray or quit ─────────────────────────────────────────
    window.connect_close_request(|window| {
        let in_tray = window
            .application()
            .is_some_and(|app| action_bool(&app, ACTION_TRAY));
        if in_tray {
            window.set_visible(false);
            #[cfg(target_os = "macos")]
            macos_bundle::set_dock_icon_visible(false);
            println!("Window hidden — AirEcho is still running in the tray");
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });

    window
}

fn show_about_dialog(parent: &impl IsA<gtk4::Widget>) {
    #[cfg(target_os = "macos")]
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::IconTheme::for_display(&display)
            .add_resource_path("/in/tl2a/airEcho/icons/hicolor");
    }

    let dialog = AboutDialog::new();
    dialog.set_application_name("AirEcho");
    dialog.set_application_icon(APP_ID);
    dialog.set_version(env!("CARGO_PKG_VERSION"));
    dialog.set_website(PROJECT_URL);
    dialog.set_issue_url(ISSUE_URL);
    dialog.set_developers(&["Arpan Biswas https://tl2a.in"]);
    dialog.add_acknowledgement_section(
        Some("Special Thanks"),
        &["croc https://github.com/schollz/croc"],
    );
    dialog.add_acknowledgement_section(
        Some("Libraries and tools"),
        &[
            "GTK https://www.gtk.org",
            "libadwaita https://gnome.pages.gitlab.gnome.org/libadwaita/",
        ],
    );
    dialog.set_copyright("© 2025-2026 AirEcho Contributors");
    dialog.set_license_type(gtk4::License::Gpl30);
    dialog.present(Some(parent));
}
