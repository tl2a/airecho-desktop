use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use adw::{Application, ApplicationWindow, ComboRow, EntryRow, SwitchRow, Toast, ToastOverlay};
use adw::gtk::Label;
use gtk4::{gio, glib};

#[cfg(not(target_os = "macos"))]
mod notification;
mod receiver;
mod tray;

use receiver::{ReceiverHandle, RelayMode};
use tray::{ACTION_ENABLED, ACTION_QUIT, ACTION_SHOW_WINDOW, ACTION_TRAY, Tray};

const APP_ID: &str = "in.tl2a.kopico-desktop";

/// Default receive code.  Both sender and receiver must agree on this value.
/// Users can override it in Advanced → Receive Code.
const DEFAULT_RECEIVE_CODE: &str = "kopico-receive";

fn main() -> gtk4::glib::ExitCode {
    let bytes = include_bytes!(env!("GRESOURCE_FILE"));
    let resource = gio::Resource::from_data(&gtk4::glib::Bytes::from_static(bytes))
        .expect("Failed to load gresource data");
    gio::resources_register(&resource);

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_startup(setup_app);
    app.connect_activate(show_window);
    app.run()
}

/// Current value of a boolean stateful action.
fn action_bool(group: &impl IsA<gio::ActionGroup>, name: &str) -> bool {
    group
        .action_state(name)
        .and_then(|state| state.get::<bool>())
        .unwrap_or(false)
}

// ── Receiver state ─────────────────────────────────────────────────────────────

struct ReceiverState {
    handle: Option<ReceiverHandle>,
    mode: RelayMode,
    code: String,
}

impl ReceiverState {
    fn new() -> Self {
        Self {
            handle: None,
            mode: RelayMode::Local,
            code: DEFAULT_RECEIVE_CODE.to_owned(),
        }
    }

    fn start(&mut self, app: &Application) {
        if self.handle.is_some() {
            return;
        }
        let out_dir = receiver::default_out_dir();
        let mode = self.mode;
        let code = self.code.clone();
        let app_weak = app.downgrade();

        let handle = ReceiverHandle::start(mode, out_dir, code, move |filename| {
            // Called on the GTK main thread via glib::idle_add_once (in receiver.rs)
            let Some(app) = app_weak.upgrade() else { return };
            println!("[receiver] received: {filename}");

            #[cfg(not(target_os = "macos"))]
            notification::send_file_received(&app, &filename);

            // Show an in-app toast on every open window
            for win in app.windows() {
                if let Some(win) = win.downcast_ref::<ApplicationWindow>() {
                    show_toast_on_window(win, &format!("Received: {filename}"));
                }
            }
        });

        self.handle = Some(handle);
        println!(
            "[receiver] started ({:?} mode, code='{}')",
            self.mode, self.code
        );
    }

    fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.stop();
            println!("[receiver] stopped");
        }
    }

    fn restart(&mut self, app: &Application) {
        self.stop();
        self.start(app);
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
fn setup_app(app: &Application) {
    let tray: Rc<RefCell<Option<Tray>>> = Rc::new(RefCell::new(None));
    let receiver: Rc<RefCell<ReceiverState>> = Rc::new(RefCell::new(ReceiverState::new()));
    tray::install_menu_handler();

    // ── app.enabled — power toggle + tray checkbox ────────────────────────────
    let enabled_action = gio::SimpleAction::new_stateful(ACTION_ENABLED, None, &true.to_variant());
    {
        let tray = tray.clone();
        let receiver = receiver.clone();
        #[cfg(not(target_os = "macos"))]
        let app_notif = app.downgrade();
        let app_recv = app.downgrade();

        enabled_action.connect_change_state(move |action, value| {
            let Some(enabled) = value.and_then(|v| v.get::<bool>()) else { return };
            #[cfg(not(target_os = "macos"))]
            let was_enabled = action.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
            action.set_state(&enabled.to_variant());

            if let Some(t) = tray.borrow().as_ref() {
                t.set_enabled(enabled);
            }
            println!("Kopico {}", if enabled { "enabled" } else { "disabled" });

            // Start / stop the croc receive loop
            if let Some(app) = app_recv.upgrade() {
                if enabled {
                    receiver.borrow_mut().start(&app);
                } else {
                    receiver.borrow_mut().stop();
                }
            }

            // System notification on actual off→on transition only
            #[cfg(not(target_os = "macos"))]
            if enabled && !was_enabled {
                if let Some(app) = app_notif.upgrade() {
                    notification::send_enabled(&app);
                }
            }
        });
    }
    app.add_action(&enabled_action);

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

    // ── app.set-relay-mode ("local" | "relay") ────────────────────────────────
    // Sent by the relay-mode ComboRow in build_ui.
    let set_mode_action = gio::SimpleAction::new(
        "set-relay-mode",
        Some(glib::VariantTy::STRING),
    );
    {
        let receiver = receiver.clone();
        let app_w = app.downgrade();
        set_mode_action.connect_activate(move |_, param| {
            let Some(mode_str) = param.and_then(|p| p.get::<String>()) else { return };
            let mode = if mode_str == "local" { RelayMode::Local } else { RelayMode::PublicRelay };
            let Some(app) = app_w.upgrade() else { return };
            {
                let mut r = receiver.borrow_mut();
                r.mode = mode;
                if r.handle.is_some() {
                    r.restart(&app);
                }
            }
        });
    }
    app.add_action(&set_mode_action);

    // ── app.set-receive-code (<string>) ───────────────────────────────────────
    // Sent by the EntryRow apply button in build_ui.
    let set_code_action = gio::SimpleAction::new(
        "set-receive-code",
        Some(glib::VariantTy::STRING),
    );
    {
        let receiver = receiver.clone();
        let app_w = app.downgrade();
        set_code_action.connect_activate(move |_, param| {
            let Some(code) = param.and_then(|p| p.get::<String>()) else { return };
            let Some(app) = app_w.upgrade() else { return };
            {
                let mut r = receiver.borrow_mut();
                r.code = code;
                if r.handle.is_some() {
                    r.restart(&app);
                }
            }
        });
    }
    app.add_action(&set_code_action);

    // Start the receive loop immediately (Kopico starts enabled)
    receiver.borrow_mut().start(app);

    app.connect_shutdown(move |_| {
        tray.borrow_mut().take();
        receiver.borrow_mut().stop();
    });
}

// ── Window ─────────────────────────────────────────────────────────────────────

fn show_window(app: &Application) {
    let window = app
        .windows()
        .into_iter()
        .next()
        .unwrap_or_else(|| build_ui(app).upcast());
    window.present();
}

fn build_ui(app: &Application) -> ApplicationWindow {
    let builder = gtk4::Builder::from_resource("/in/tl2a/kopico-desktop/ui/window.ui");

    let window: ApplicationWindow   = builder.object("window").expect("window");
    let state_title: Label          = builder.object("state_title").expect("state_title");
    let state_description: Label    = builder.object("state_description").expect("state_description");
    let tray_row: SwitchRow         = builder.object("tray_row").expect("tray_row");
    let relay_mode_row: ComboRow    = builder.object("relay_mode_row").expect("relay_mode_row");
    let receive_code_row: EntryRow  = builder.object("receive_code_row").expect("receive_code_row");
    let toast_overlay: ToastOverlay = builder.object("toast_overlay").expect("toast_overlay");

    window.set_application(Some(app));

    // ── Home: status text follows app.enabled ─────────────────────────────────
    let update_state_text = move |enabled: bool| {
        if enabled {
            state_title.set_label("Kopico Is On");
            state_description.set_label(
                "Receiving files in the background. Turn off anytime from here or the tray.",
            );
        } else {
            state_title.set_label("Kopico Is Off");
            state_description.set_label("Kopico is paused. Turn it on to start receiving.");
        }
    };
    update_state_text(action_bool(app, ACTION_ENABLED));
    if let Some(action) = app.lookup_action(ACTION_ENABLED) {
        action.connect_notify_local(Some("state"), move |action, _| {
            update_state_text(action.state().and_then(|s| s.get()).unwrap_or(false));
        });
    }

    // ── Advanced: relay mode ──────────────────────────────────────────────────
    // Index 0 = Local, 1 = Public Relay (matches the StringList in window.blp)
    {
        let app_w = app.downgrade();
        let toast = toast_overlay.clone();
        relay_mode_row.connect_selected_notify(move |row| {
            let Some(app) = app_w.upgrade() else { return };
            let (mode_str, label) = if row.selected() == 0 {
                ("local", "Local (LAN)")
            } else {
                ("relay", "Public Relay")
            };
            app.activate_action("set-relay-mode", Some(&mode_str.to_variant()));
            toast.add_toast(Toast::new(&format!("Relay mode: {label}")));
        });
    }

    // ── Advanced: receive code ────────────────────────────────────────────────
    receive_code_row.set_text(DEFAULT_RECEIVE_CODE);
    {
        let app_w = app.downgrade();
        let toast = toast_overlay.clone();
        receive_code_row.connect_apply(move |row| {
            let Some(app) = app_w.upgrade() else { return };
            let code = row.text().to_string();
            if code.trim().is_empty() { return; }
            app.activate_action("set-receive-code", Some(&code.to_variant()));
            toast.add_toast(Toast::new("Receive code updated — restart sender too"));
        });
    }

    // ── Advanced: tray switch ─────────────────────────────────────────────────
    tray_row.set_active(action_bool(app, ACTION_TRAY));
    {
        let app_w = app.downgrade();
        let toast = toast_overlay.clone();
        tray_row.connect_active_notify(move |row| {
            let Some(app) = app_w.upgrade() else { return };
            let show = row.is_active();
            app.change_action_state(ACTION_TRAY, &show.to_variant());
            if !show {
                toast.add_toast(Toast::new("Closing the window will now quit Kopico"));
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
            println!("Window hidden — Kopico is still running in the tray");
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });

    window
}
