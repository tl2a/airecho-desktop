//! Sending text and files with croc.
//!
//! One [`SendHandle`] runs one `croc send` on a background thread:
//!
//! ```text
//! CROC_SECRET=<code> croc --yes --ignore-stdin --disable-clipboard [mode] send --text "<text>"
//! CROC_SECRET=<code> croc --yes --ignore-stdin --disable-clipboard [mode] send <file>…
//! ```
//!
//! `[mode]` follows the receive mode: `--local`, `--relay <address>` (own relay), or
//! nothing (public relays). The receive loop must be stopped first — with the same code it
//! would otherwise receive AirEcho's own transfer (see `transfers.rs`).
//!
//! # What croc prints (verified against croc 11.5.4)
//!
//! stderr, separated by `\r` and `\n`:
//!
//! ```text
//! On the other computer, run:  croc <code>  …   ← ready, waiting for a receiver
//! Sending 'text' (12 B) | Sending 2 files (18 B)
//! Sending (->192.168.0.110)                       ← a receiver connected
//! a.txt 100% |████| (11/11 B, 270 B/s) 1/2        ← progress (files)
//! ```
//!
//! A sender waits for a receiver indefinitely, so AirEcho gives up after a mode-dependent
//! wait — but only *before* `Sending (->…)`: a transfer in progress is never killed.
//! Exit status 0 means delivered. `--disable-clipboard` matters: without it croc copies
//! the code to the clipboard, which the clipboard watcher would then try to send.
//! stderr contains the code, so it is never logged verbatim.
//!
//! `croc send --text` writes the text to `croc-stdin-<n>` in its **working directory** and
//! only deletes it after a delivered send — a killed send (cancelled, timed out, quit)
//! leaves the copied text behind. So croc runs in a private directory ([`work_dir`]) that
//! is emptied of `croc-stdin-*` before and after every send.
//!
//! Files whose names croc's receiver refuses (e.g. macOS screenshots, which contain U+202F)
//! are sent under safe names — see [`crate::croc_names`].

use std::{
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use gtk4::glib;

use crate::croc_names;
use crate::receiver::{self, RelayMode};

/// How long to wait for a receiver before giving up. The other side polls every 3 s
/// (local) / 10 s (own relay) / 30 s (public relay), so these leave room for a few polls.
const WAIT_LOCAL: Duration = Duration::from_secs(30);
const WAIT_OWN_RELAY: Duration = Duration::from_secs(30);
const WAIT_PUBLIC_RELAY: Duration = Duration::from_secs(75);

/// Text is passed to croc as a command-line argument; keep it well inside OS limits.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;

const TICK: Duration = Duration::from_millis(200);

/// What to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendJob {
    Text(String),
    Files(Vec<PathBuf>),
}

impl SendJob {
    /// Short description for toasts and logs — never contains the text itself.
    pub fn describe(&self) -> String {
        match self {
            Self::Text(_) => "copied text".into(),
            Self::Files(files) => match files.as_slice() {
                [one] => format!(
                    "\u{201c}{}\u{201d}",
                    one.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
                ),
                many => format!("{} items", many.len()),
            },
        }
    }
}

/// How a send ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    /// The receiver got everything — except `left_out` items croc can't send at all
    /// (`.git`/`.ssh`/`.gnupg` folders, symlinks pointing outside; see `croc_names`).
    Delivered { left_out: usize },
    /// Nobody picked it up within the wait limit.
    NoReceiver,
    /// [`SendHandle::cancel`] was called (e.g. replaced by newer copied text).
    Cancelled,
    /// croc failed (not installed, transfer error, rate limited, …).
    Failed(String),
}

/// A running `croc send`. Dropping it does not stop it; call [`SendHandle::cancel`].
pub struct SendHandle {
    cancel: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    /// The running croc, so [`SendHandle::kill_now`] can kill it from the GTK thread.
    child: Arc<Mutex<Option<Child>>>,
}

impl SendHandle {
    /// Start sending. `on_done` runs once, on the **GTK main thread**.
    pub fn start(
        job: SendJob,
        mode: RelayMode,
        code: String,
        on_done: impl FnOnce(SendOutcome) + 'static,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let connected = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(None));
        let (tx, rx) = async_channel::bounded::<SendOutcome>(1);

        {
            let (cancel, connected, child) = (cancel.clone(), connected.clone(), child.clone());
            let spawned = std::thread::Builder::new().name("croc-send".into()).spawn(move || {
                let outcome = run_send(&job, &mode, &code, &cancel, &connected, &child);
                let _ = tx.send_blocking(outcome);
            });
            if let Err(err) = spawned {
                eprintln!("[croc-send] failed to start send thread: {err}");
            }
        }

        glib::spawn_future_local(async move {
            let outcome = rx
                .recv()
                .await
                .unwrap_or_else(|_| SendOutcome::Failed("send thread ended unexpectedly".into()));
            on_done(outcome);
        });

        Self { cancel, connected, child }
    }

    /// Kill croc right now (app quitting) instead of waiting for the send thread to
    /// notice [`SendHandle::cancel`] — the process may exit before it does, which would
    /// leave croc running in the background.
    pub fn kill_now(&self) {
        self.cancel();
        // If the send thread holds the lock it is already killing/reaping croc
        if let Ok(mut child) = self.child.try_lock() {
            if let Some(child) = child.as_mut() {
                let _ = child.kill();
            }
        }
    }

    /// Stop the send: croc is killed within ~200 ms, and `on_done(Cancelled)` follows
    /// once it has exited (a killed `croc send` takes ~2 s to exit). Has no effect once
    /// delivered.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Whether a receiver has connected (the transfer is under way).
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

fn max_wait(mode: &RelayMode) -> Duration {
    match mode {
        RelayMode::Local { .. } => WAIT_LOCAL,
        RelayMode::OwnRelay { .. } => WAIT_OWN_RELAY,
        RelayMode::PublicRelay => WAIT_PUBLIC_RELAY,
    }
}

/// The `croc` command line (without the program) for `job` in `mode`.
fn croc_args(job: &SendJob, mode: &RelayMode) -> Vec<String> {
    let mut args: Vec<String> =
        ["--yes", "--ignore-stdin", "--disable-clipboard"].map(String::from).into();
    match mode {
        // The sender IP is a receive-side setting; sending only needs --local
        RelayMode::Local { .. } => args.push("--local".into()),
        RelayMode::OwnRelay { address, .. } => args.extend(["--relay".into(), address.clone()]),
        RelayMode::PublicRelay => {}
    }
    args.push("send".into());
    match job {
        SendJob::Text(text) => args.extend(["--text".into(), text.clone()]),
        SendJob::Files(files) => {
            args.extend(files.iter().map(|f| f.to_string_lossy().into_owned()))
        }
    }
    args
}

fn run_send(
    job: &SendJob,
    mode: &RelayMode,
    code: &str,
    cancel: &AtomicBool,
    connected: &AtomicBool,
    shared_child: &Mutex<Option<Child>>,
) -> SendOutcome {
    let Some(croc) = receiver::find_croc() else {
        return SendOutcome::Failed("croc is not installed or not on PATH".into());
    };

    let work_dir = match work_dir() {
        Ok(dir) => dir,
        Err(err) => return SendOutcome::Failed(format!("no private working folder: {err}")),
    };
    remove_text_files(&work_dir); // leftovers from a crash

    // Names croc's receiver would refuse → a staged copy with safe names. Kept alive until
    // croc has exited; dropping it removes the staging folder.
    let prepared = match job {
        SendJob::Text(_) => None,
        SendJob::Files(files) => match croc_names::prepare(files, &work_dir) {
            Ok(prepared) if prepared.paths.is_empty() => {
                return SendOutcome::Failed(
                    "croc can't send .git, .ssh or .gnupg folders, or links pointing outside".into(),
                );
            }
            Ok(prepared) => Some(prepared),
            Err(err) => return SendOutcome::Failed(format!("couldn\u{2019}t prepare the files: {err}")),
        },
    };
    let left_out = prepared.as_ref().map_or(0, |p| p.left_out);
    let send_job = match &prepared {
        Some(prepared) => {
            if prepared.renamed > 0 || prepared.left_out > 0 {
                println!(
                    "[croc-send] {} name(s) made croc-safe, {} item(s) left out",
                    prepared.renamed, prepared.left_out
                );
            }
            SendJob::Files(prepared.paths.clone())
        }
        None => job.clone(),
    };

    let mut cmd = Command::new(&croc);
    cmd.args(croc_args(&send_job, mode))
        .current_dir(&work_dir)
        .env("CROC_SECRET", code)
        .env_remove("CROC_RELAY")
        .env_remove("CROC_RELAY6")
        .env_remove("CROC_PASS")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let RelayMode::OwnRelay { password, .. } = mode {
        if !password.is_empty() {
            cmd.env("CROC_PASS", password);
        }
    }
    if receiver::debug_enabled() {
        // Never the text or the code: describe the job instead of printing argv
        eprintln!("[croc-send] run: {} ({mode:?}, code: {} chars)", job.describe(), code.chars().count());
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => return SendOutcome::Failed(format!("failed to run croc: {err}")),
    };
    let stdout = child.stdout.take().expect("piped stdout");
    let stdout_reader = std::thread::spawn(move || receiver::read_all(stdout));
    let (signals_tx, signals) = mpsc::channel();
    let stderr = child.stderr.take().expect("piped stderr");
    let stderr_reader = std::thread::spawn(move || watch_sender_stderr(stderr, signals_tx));
    *shared_child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
    let try_wait = || {
        let mut child = shared_child.lock().unwrap_or_else(|e| e.into_inner());
        child.as_mut().and_then(|c| c.try_wait().ok().flatten())
    };

    let deadline = Instant::now() + max_wait(mode);
    let mut rate_limited = false;
    let outcome = loop {
        if cancel.load(Ordering::Relaxed) {
            break SendOutcome::Cancelled;
        }
        if let Some(status) = try_wait() {
            break if status.success() {
                SendOutcome::Delivered { left_out }
            } else {
                SendOutcome::Failed(format!("croc exited with {status}"))
            };
        }
        match signals.recv_timeout(TICK) {
            Ok(SenderSignal::Connected) => connected.store(true, Ordering::Relaxed),
            Ok(SenderSignal::RateLimited) => {
                rate_limited = true;
                break SendOutcome::Failed("the relay is rate limiting — try again in a minute".into());
            }
            Err(_) if !connected.load(Ordering::Relaxed) && Instant::now() >= deadline => {
                break SendOutcome::NoReceiver;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(TICK),
        }
    };

    let status = {
        let mut child = shared_child.lock().unwrap_or_else(|e| e.into_inner());
        let child = child.as_mut().expect("child stored above");
        let _ = child.kill(); // no-op if it already exited
        child.wait()
    };
    let _ = stderr_reader.join();
    let stdout_text = stdout_reader.join().unwrap_or_default();
    let stdout_text = stdout_text.trim();

    // Errors are on stdout ("could not connect to …: relay admission rate limited", …)
    let outcome = match outcome {
        SendOutcome::Failed(_) if !rate_limited && receiver::is_rate_limited(stdout_text) => {
            SendOutcome::Failed("the relay is rate limiting — try again in a minute".into())
        }
        SendOutcome::Failed(_) if !stdout_text.is_empty() => {
            SendOutcome::Failed(explain_failure(stdout_text))
        }
        other => other,
    };
    remove_text_files(&work_dir); // a killed `croc send --text` leaves the text here
    drop(prepared); // croc has exited: remove the staging folder
    if receiver::debug_enabled() {
        eprintln!("[croc-send]   → {outcome:?} ({status:?})");
    }
    outcome
}

/// Private working directory for `croc send` (see the module docs): `$XDG_RUNTIME_DIR`
/// (per-user, in RAM) on Linux, the user's cache folder otherwise; owner-only access.
fn work_dir() -> std::io::Result<PathBuf> {
    let dir = dirs_next::runtime_dir()
        .or_else(dirs_next::cache_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("airecho-send");
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

/// Delete croc's `croc-stdin-*` text files from `dir`.
fn remove_text_files(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with("croc-stdin-") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

enum SenderSignal {
    Connected,
    RateLimited,
}

/// Watches croc's stderr for "a receiver connected" and rate limiting. The text is
/// never stored or logged (it contains the code).
fn watch_sender_stderr(mut stderr: impl Read, signals: mpsc::Sender<SenderSignal>) {
    let mut pending = String::new();
    let mut buf = [0u8; 4096];
    let mut connected_sent = false;
    while let Ok(n) = stderr.read(&mut buf) {
        if n == 0 {
            break;
        }
        pending.push_str(&String::from_utf8_lossy(&buf[..n]));
        while let Some(pos) = pending.find(['\r', '\n']) {
            let segment: String = pending.drain(..=pos).collect();
            let segment = segment.trim();
            if receiver::is_rate_limited(segment) {
                let _ = signals.send(SenderSignal::RateLimited);
            } else if !connected_sent && is_receiver_connected(segment) {
                connected_sent = true;
                let _ = signals.send(SenderSignal::Connected);
            }
        }
        if pending.len() > 8192 {
            pending.clear();
        }
    }
}

/// User-facing reason for a failed send, from croc's stdout.
fn explain_failure(stdout_text: &str) -> String {
    // croc sends "refusing files" for *any* error on the receiving side: a declined
    // "Accept …? (Y/n)" (a receiver without --yes and no terminal declines), a name it
    // won't create (AirEcho avoids those, see croc_names), a full disk, …
    if stdout_text.contains("refusing files") {
        return "the receiver rejected the transfer \u{2014} its croc output shows why. \
                A croc receiver also needs --yes to accept files"
            .into();
    }
    stdout_text.lines().last().unwrap_or(stdout_text).to_owned()
}

/// `Sending (->192.168.0.110)` — printed once a receiver has joined.
fn is_receiver_connected(segment: &str) -> bool {
    segment.starts_with("Sending (->")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_receiver_connected() {
        assert!(is_receiver_connected("Sending (->127.0.0.1)"));
        assert!(is_receiver_connected("Sending (->[fe80::1]:9009)"));
        assert!(!is_receiver_connected("Sending 'text' (12 B)"));
        assert!(!is_receiver_connected("Sending 2 files (18 B)"));
    }

    #[test]
    fn args_follow_mode_and_never_contain_the_code() {
        let text = SendJob::Text("hello \"world\"".into());
        let local = croc_args(&text, &RelayMode::Local { sender_ip: Some("10.0.0.2".into()) });
        assert_eq!(
            local,
            ["--yes", "--ignore-stdin", "--disable-clipboard", "--local", "send", "--text", "hello \"world\""]
        );
        let own = RelayMode::OwnRelay { address: "relay.example:9009".into(), password: "pw".into() };
        let files = SendJob::Files(vec!["/tmp/a b.txt".into()]);
        assert_eq!(
            croc_args(&files, &own),
            ["--yes", "--ignore-stdin", "--disable-clipboard", "--relay", "relay.example:9009", "send", "/tmp/a b.txt"]
        );
        assert!(!croc_args(&files, &own).iter().any(|a| a == "pw"), "password goes in CROC_PASS");
    }

    #[test]
    fn explains_refused_files() {
        // Exact croc 11.5.4 output when the receiver errors or runs without --yes
        assert!(explain_failure("peer error: refusing files").contains("rejected"));
        assert_eq!(explain_failure("first\ncould not connect to x: y"), "could not connect to x: y");
    }

    #[test]
    fn describe_never_reveals_text() {
        assert_eq!(SendJob::Text("secret".into()).describe(), "copied text");
        assert_eq!(SendJob::Files(vec!["/x/photo.jpg".into()]).describe(), "\u{201c}photo.jpg\u{201d}");
        assert_eq!(SendJob::Files(vec!["/a".into(), "/b".into()]).describe(), "2 items");
    }

    // ── End-to-end with real croc: cargo test -- --ignored --test-threads=1 ──────

    fn receive_once(code: &str, out: &std::path::Path) -> (bool, String) {
        // Receiver dials the sender's built-in relay directly, like AirEcho with a Sender IP
        let output = Command::new(receiver::find_croc().expect("croc installed"))
            .args(["--yes", "--ignore-stdin", "--ip", "127.0.0.1", "--out"])
            .arg(out)
            .env("CROC_SECRET", code)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (output.status.success(), String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn send_and_receive(job: SendJob, code: &str, out: &std::path::Path) -> (SendOutcome, String) {
        let cancel = Arc::new(AtomicBool::new(false));
        let connected = Arc::new(AtomicBool::new(false));
        let sender = {
            let (job, code, cancel, connected) = (job, code.to_owned(), cancel.clone(), connected.clone());
            std::thread::spawn(move || {
                let child = Mutex::new(None);
                run_send(&job, &RelayMode::Local { sender_ip: None }, &code, &cancel, &connected, &child)
            })
        };
        std::thread::sleep(Duration::from_secs(2)); // sender ready (~0.3 s)
        let (ok, stdout) = receive_once(code, out);
        assert!(ok, "receiver failed");
        (sender.join().unwrap(), stdout)
    }

    #[test]
    #[ignore]
    fn sends_text_to_a_receiver() {
        let out = std::env::temp_dir().join(format!("airecho-send-text-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        let text = "line one\n  \"quoted\" $HOME ü€\n";
        let code = format!("airecho-send-e2e-{}", std::process::id());
        let (outcome, received) = send_and_receive(SendJob::Text(text.into()), &code, &out);
        assert_eq!(outcome, SendOutcome::Delivered { left_out: 0 });
        assert_eq!(received, text, "receiver gets the text byte for byte");
    }

    #[test]
    #[ignore]
    fn sends_files_to_a_receiver() {
        let base = std::env::temp_dir().join(format!("airecho-send-files-{}", std::process::id()));
        let (src, out) = (base.join("src"), base.join("out"));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(src.join("a.txt"), "aaa").unwrap();
        std::fs::write(src.join("b file.txt"), "bbb").unwrap();
        // macOS screenshot style (U+202F before AM): croc's receiver refuses this name
        std::fs::write(src.join("Shot 10.15\u{202f}AM.png"), "png").unwrap();
        let job = SendJob::Files(vec![src.join("a.txt"), src.join("b file.txt"), src.join("Shot 10.15\u{202f}AM.png")]);
        let code = format!("airecho-send-files-e2e-{}", std::process::id());
        let (outcome, _) = send_and_receive(job, &code, &out);
        assert_eq!(outcome, SendOutcome::Delivered { left_out: 0 });
        assert_eq!(std::fs::read_to_string(out.join("a.txt")).unwrap(), "aaa");
        assert_eq!(std::fs::read_to_string(out.join("b file.txt")).unwrap(), "bbb");
        assert_eq!(std::fs::read_to_string(out.join("Shot 10.15 AM.png")).unwrap(), "png");
        assert!(src.join("Shot 10.15\u{202f}AM.png").exists(), "original untouched");
    }

    #[test]
    #[ignore]
    fn cancel_stops_a_waiting_send_quickly() {
        let cancel = Arc::new(AtomicBool::new(false));
        let connected = AtomicBool::new(false);
        {
            let cancel = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(1));
                cancel.store(true, Ordering::Relaxed);
            });
        }
        let started = Instant::now();
        let job = SendJob::Text("nobody receives this".into());
        let mode = RelayMode::Local { sender_ip: None };
        let outcome = run_send(&job, &mode, "airecho-cancel-e2e", &cancel, &connected, &Mutex::new(None));
        assert_eq!(outcome, SendOutcome::Cancelled);
        let leftovers: Vec<_> = std::fs::read_dir(work_dir().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("croc-stdin-"))
            .collect();
        assert!(leftovers.is_empty(), "the text must not be left on disk: {leftovers:?}");
        // 1 s until cancel + ≤ 200 ms to notice + croc send's own exit, which always takes
        // ~2 s after SIGKILL (measured with croc 11.5.4) — still off the GTK thread
        assert!(started.elapsed() < Duration::from_secs(4), "{:?}", started.elapsed());
    }
}
