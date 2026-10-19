//! Coordinates receiving and sending — they share one croc code, so they can't overlap.
//!
//! - **Receiving** runs while Kopico is enabled ([`ReceiverHandle`]).
//! - **Sending** (copied text, files from Finder/Nautilus or the app) first stops the
//!   receiver — with the same code it would receive Kopico's own transfer — then runs one
//!   `croc send` at a time ([`SendHandle`]) and restarts the receiver when the queue is empty.
//!   If a transfer is *coming in* at that moment, the send waits until it is done.
//! - **Queue:** newer copied text replaces older text that nobody has picked up yet; files
//!   are queued in order.
//! - **No echo:** text that was just received or sent is not sent again. Without this, two
//!   Kopicos would bounce a copy back and forth (receive → clipboard → "copy" → send …).

use std::{cell::RefCell, collections::VecDeque, path::PathBuf, rc::Rc, time::Duration};

use adw::Application;
use adw::prelude::*;
use gtk4::glib;

use crate::receiver::{self, ReceiverEvent, ReceiverHandle};
use crate::sender::{self, SendHandle, SendJob, SendOutcome};
use crate::{clipboard, notification};

/// How often to re-check while waiting for an incoming transfer to finish.
const BUSY_RETRY: Duration = Duration::from_millis(500);

/// Shared handle; clones refer to the same state.
#[derive(Clone)]
pub struct Transfers(Rc<RefCell<Inner>>);

struct Inner {
    app: glib::WeakRef<Application>,
    receiver: Option<ReceiverHandle>,
    sending: Option<(SendJob, SendHandle)>,
    queue: VecDeque<SendJob>,
    /// Last text received or sent — a clipboard change to exactly this is not a new copy.
    last_synced_text: Option<String>,
    /// A retry is scheduled while an incoming transfer finishes.
    retry_scheduled: bool,
}

impl Transfers {
    pub fn new(app: &Application) -> Self {
        Self(Rc::new(RefCell::new(Inner {
            app: app.downgrade(),
            receiver: None,
            sending: None,
            queue: VecDeque::new(),
            last_synced_text: None,
            retry_scheduled: false,
        })))
    }

    fn app(&self) -> Option<Application> {
        self.0.borrow().app.upgrade()
    }

    // ── Receiving ──────────────────────────────────────────────────────────────

    /// Start the receive loop if Kopico is enabled and nothing is being sent.
    pub fn start_receiving(&self) {
        let Some(app) = self.app() else { return };
        {
            let inner = self.0.borrow();
            if inner.receiver.is_some()
                || inner.sending.is_some()
                || !inner.queue.is_empty()
                || !crate::action_bool(&app, crate::ACTION_ENABLED)
            {
                return;
            }
        }
        let (mode, code) = match crate::receiver_config(&app) {
            Ok(config) => config,
            Err(message) => {
                println!("[receiver] not started: {message}");
                crate::show_toast(&app, &message);
                return;
            }
        };
        let out_dir = receiver::default_out_dir();
        println!("[receiver] started ({mode:?}, saving to {})", out_dir.display());

        let this = self.clone();
        let handle = ReceiverHandle::start(mode, out_dir.clone(), code, move |event| {
            // Runs on the GTK main thread
            let Some(app) = this.app() else { return };
            match event {
                ReceiverEvent::Received(names) => {
                    let message = crate::received_message(&names, &out_dir);
                    notification::send_files_received(&app, &message);
                    crate::show_toast(&app, &message);
                }
                ReceiverEvent::Text(text) => {
                    // Remember it first: the clipboard watcher will see this write
                    this.0.borrow_mut().last_synced_text = Some(text.clone());
                    clipboard::copy_to_clipboard(&text);
                    let message = "Received text copied to clipboard";
                    notification::send_text_received(&app, message);
                    crate::show_toast(&app, message);
                }
                ReceiverEvent::Failed(message) => crate::show_toast(&app, &message),
            }
        });
        self.0.borrow_mut().receiver = Some(handle);
    }

    pub fn stop_receiving(&self) {
        let handle = self.0.borrow_mut().receiver.take();
        if let Some(handle) = handle {
            handle.stop();
            println!("[receiver] stopped");
        }
    }

    /// App quitting: stop receiving and kill any running send, so no croc is left behind.
    pub fn shutdown(&self) {
        self.stop_receiving();
        let mut inner = self.0.borrow_mut();
        inner.queue.clear();
        if let Some((_, handle)) = inner.sending.take() {
            handle.kill_now();
        }
    }

    /// Kopico switched on or off. Off also cancels pending sends of copied text.
    pub fn set_enabled(&self, enabled: bool) {
        if enabled {
            self.start_receiving();
        } else {
            self.stop_receiving();
            let mut inner = self.0.borrow_mut();
            inner.queue.retain(|job| matches!(job, SendJob::Files(_)));
            if let Some((SendJob::Text(_), handle)) = &inner.sending {
                handle.cancel();
            }
        }
    }

    /// Receive settings changed: restart the receiver with them (sends pick them up
    /// automatically, the next `croc send` reads the settings again).
    pub fn apply_settings(&self) {
        if self.0.borrow().receiver.is_some() {
            self.stop_receiving();
            self.start_receiving();
        } else {
            self.start_receiving();
        }
    }

    // ── Sending ────────────────────────────────────────────────────────────────

    /// The clipboard watcher saw a copy. Sent if Kopico is on, "Send Copied Text" is on,
    /// and it isn't text Kopico itself just received or sent.
    pub fn on_text_copied(&self, text: String) {
        let Some(app) = self.app() else { return };
        if !crate::action_bool(&app, crate::ACTION_ENABLED)
            || !crate::action_bool(&app, crate::ACTION_SEND_CLIPBOARD)
            || text.trim().is_empty()
        {
            return;
        }
        if self.0.borrow().last_synced_text.as_deref() == Some(text.as_str()) {
            return; // our own write, or an echo of what we just received/sent
        }
        if text.len() > sender::MAX_TEXT_BYTES {
            crate::show_toast(&app, "Copied text is too large to send (max 64 KB)");
            return;
        }
        {
            let mut inner = self.0.borrow_mut();
            inner.last_synced_text = Some(text.clone());
            // Newest copy wins: drop queued text, cancel text nobody has picked up yet
            inner.queue.retain(|job| matches!(job, SendJob::Files(_)));
            if let Some((SendJob::Text(_), handle)) = &inner.sending {
                if !handle.is_connected() {
                    handle.cancel();
                }
            }
            inner.queue.push_back(SendJob::Text(text));
        }
        println!("[sender] copied text queued");
        self.pump();
    }

    /// Send files/folders (Finder "Send with Kopico" / "Open With", Nautilus, Send Files…).
    /// Works even while Kopico is switched off — it's an explicit request.
    pub fn send_files(&self, files: Vec<PathBuf>) {
        let Some(app) = self.app() else { return };
        let files: Vec<PathBuf> = files.into_iter().filter(|f| f.exists()).collect();
        if files.is_empty() {
            return;
        }
        let job = SendJob::Files(files);
        let message = format!("Sending {} — waiting for the receiver", job.describe());
        println!("[sender] {message}");
        notification::send_sending(&app, &message);
        crate::show_toast(&app, &message);
        self.0.borrow_mut().queue.push_back(job);
        self.pump();
    }

    /// Start the next send if possible.
    fn pump(&self) {
        let Some(app) = self.app() else { return };
        {
            let inner = self.0.borrow();
            if inner.sending.is_some() {
                return; // its completion calls pump() again
            }
            if inner.queue.is_empty() {
                drop(inner);
                self.start_receiving();
                return;
            }
            // Don't cut off a transfer that is coming in — try again shortly
            if inner.receiver.as_ref().is_some_and(ReceiverHandle::is_busy) {
                if !inner.retry_scheduled {
                    drop(inner);
                    self.0.borrow_mut().retry_scheduled = true;
                    let this = self.clone();
                    glib::timeout_add_local_once(BUSY_RETRY, move || {
                        this.0.borrow_mut().retry_scheduled = false;
                        this.pump();
                    });
                }
                return;
            }
        }

        self.stop_receiving(); // same code: we'd receive our own send
        let Some(job) = self.0.borrow_mut().queue.pop_front() else { return };
        let (mode, code) = match crate::receiver_config(&app) {
            Ok(config) => config,
            Err(message) => {
                crate::show_toast(&app, &message);
                self.pump();
                return;
            }
        };

        println!("[sender] sending {}", job.describe());
        let this = self.clone();
        let done_job = job.clone();
        let handle = SendHandle::start(job.clone(), mode, code, move |outcome| {
            this.0.borrow_mut().sending = None;
            this.report(&done_job, &outcome);
            this.pump();
        });
        self.0.borrow_mut().sending = Some((job, handle));
    }

    fn report(&self, job: &SendJob, outcome: &SendOutcome) {
        let Some(app) = self.app() else { return };
        let what = job.describe();
        println!("[sender] {what}: {outcome:?}");
        let is_files = matches!(job, SendJob::Files(_));
        let message = match outcome {
            SendOutcome::Delivered { left_out: 0 } => format!("Sent {what}"),
            SendOutcome::Delivered { left_out } => format!(
                "Sent {what} \u{2014} left out {left_out} item(s) croc can\u{2019}t send \
                 (.git/.ssh/.gnupg folders, links pointing outside)"
            ),
            SendOutcome::NoReceiver => {
                format!("Not delivered: nobody picked up {what}. Is Kopico running on the other device?")
            }
            SendOutcome::Cancelled if is_files => format!("Cancelled sending {what}"),
            SendOutcome::Cancelled => return, // replaced by newer copied text
            SendOutcome::Failed(reason) => format!("Couldn\u{2019}t send {what}: {reason}"),
        };
        // Copied text: a toast only — a notification on every copy would be noise
        if is_files {
            notification::send_sent(&app, &message);
        }
        crate::show_toast(&app, &message);
    }
}
