//! macOS Services: Finder → right-click files → Quick Actions / Services →
//! **"Send with Kopico"**.
//!
//! `data/macos/Info.plist` declares the service (`NSServices`: message `sendFiles`, file
//! types `public.item`); this module provides the object macOS calls. It reads the
//! selected files from the pasteboard and hands them to `GApplication::open`, i.e. the same
//! path as "Open With → Kopico Desktop" and `kopico-desktop <files>` on Linux.
//!
//! A real Share-menu entry would need a Share Extension (separate sandboxed `.appex`,
//! built with Xcode); a Service gives the same right-click entry without one.

use std::cell::OnceCell;

use gtk4::gio;
use gtk4::prelude::*;
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSPasteboard, NSPasteboardTypeFileURL, NSUpdateDynamicServices};
use objc2_foundation::{NSString, NSURL};

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no ivars, no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "KopicoServicesProvider"]
    struct ServicesProvider;

    impl ServicesProvider {
        /// `NSMessage = sendFiles` in Info.plist → `sendFiles:userData:error:`.
        #[unsafe(method(sendFiles:userData:error:))]
        fn send_files(
            &self,
            pasteboard: &NSPasteboard,
            _user_data: Option<&NSString>,
            _error: *mut *mut NSString,
        ) {
            let files = file_paths(pasteboard);
            println!("[services] Send with Kopico: {} item(s)", files.len());
            if files.is_empty() {
                return;
            }
            if let Some(app) = gio::Application::default() {
                let files: Vec<gio::File> = files.iter().map(gio::File::for_path).collect();
                app.open(&files, "");
            }
        }
    }
);

impl ServicesProvider {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: plain NSObject initialisation
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

thread_local! {
    /// NSApplication doesn't retain its services provider — keep it alive here.
    static PROVIDER: OnceCell<Retained<ServicesProvider>> = const { OnceCell::new() };
}

/// Register the provider. Call once at startup on the main thread (after GTK init).
pub fn register() {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("[services] not on the main thread — Send with Kopico unavailable");
        return;
    };
    PROVIDER.with(|cell| {
        let provider = cell.get_or_init(|| ServicesProvider::new(mtm));
        // SAFETY: the provider implements the method named in Info.plist and lives for
        // the rest of the process (thread-local on the main thread)
        unsafe { NSApplication::sharedApplication(mtm).setServicesProvider(Some(provider)) };
    });
    NSUpdateDynamicServices();
}

/// File paths on the pasteboard (one `public.file-url` per item).
fn file_paths(pasteboard: &NSPasteboard) -> Vec<std::path::PathBuf> {
    let Some(items) = pasteboard.pasteboardItems() else { return Vec::new() };
    // SAFETY: reading an immutable AppKit constant
    let file_url_type = unsafe { NSPasteboardTypeFileURL };
    items
        .iter()
        .filter_map(|item| item.stringForType(file_url_type))
        .filter_map(|url| NSURL::URLWithString(&url))
        .filter_map(|url| url.path())
        .map(|path| path.to_string().into())
        .collect()
}
