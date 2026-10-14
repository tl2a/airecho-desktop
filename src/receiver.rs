//! Background croc receive loop.
//!
//! # How it works
//!
//! croc's receive side does not sit and listen — each invocation either finds a
//! sender within a second or two and receives, or times out and exits.  We
//! therefore run croc repeatedly on a background thread:
//!
//! | Mode        | Poll interval | Back-off on rate-limit |
//! |-------------|---------------|------------------------|
//! | Local (LAN) |  3 s          | n/a                    |
//! | PublicRelay | 30 s          | 60 s                   |
//!
//! Public relays allow ~30 joins/min — the 30 s interval keeps us well inside
//! that budget.  A rate-limit message triggers a 60 s back-off.
//!
//! Files are saved to `~/Downloads/Kopico/`.  Received filenames are posted
//! back to the GTK main thread through a `glib::MainContext::channel`, avoiding
//! any GTK-object capture on the background thread.

use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use gtk4::glib;

// ── Timing constants ───────────────────────────────────────────────────────────

const TIMEOUT_LOCAL: Duration = Duration::from_secs(2);
const TIMEOUT_RELAY: Duration = Duration::from_secs(25);
const INTERVAL_LOCAL: Duration = Duration::from_secs(3);
const INTERVAL_RELAY: Duration = Duration::from_secs(30);
const BACKOFF_RELAY: Duration = Duration::from_secs(60);

// ── Public API ─────────────────────────────────────────────────────────────────

/// Which relay croc should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayMode {
    /// LAN-only (`croc --local`).  Fast poll, no internet required.
    Local,
    /// Public croc relay.  30 s poll to stay within the rate limit.
    PublicRelay,
}

/// A live receive loop.  Drop (or call [`ReceiverHandle::stop`]) to shut it down.
pub struct ReceiverHandle {
    running: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ReceiverHandle {
    /// Spawn the background receive loop.
    ///
    /// * `mode`     — which relay to use
    /// * `out_dir`  — directory where received files are saved
    /// * `code`     — croc share code (both sender and receiver must agree)
    /// * `on_file`  — called on the **GTK main thread** for each received filename;
    ///               may capture GTK objects freely.
    pub fn start(
        mode: RelayMode,
        out_dir: PathBuf,
        code: String,
        on_file: impl Fn(String) + 'static,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let flag = running.clone();

        // Wrap in Arc so we can clone it into each glib::idle_add_once call.
        // Arc<dyn Fn + Send + Sync> satisfies the 'static + Send requirement on
        // the closure passed to idle_add_once.
        let cb: Arc<dyn Fn(String) + Send + Sync + 'static> = Arc::new(on_file);

        let (tx, rx) = std::sync::mpsc::channel::<String>();

        // Bg thread: runs croc in a loop, sends filenames over tx.
        let thread = std::thread::Builder::new()
            .name("croc-receive".into())
            .spawn(move || receive_loop(flag, mode, out_dir, code, tx))
            .expect("failed to spawn croc-receive thread");

        // Main-thread poller: drain rx via glib idle callbacks.
        // We register a single repeated idle source that consumes all pending
        // items and re-arms itself until the channel is closed.
        glib::idle_add(move || {
            loop {
                match rx.try_recv() {
                    Ok(filename) => {
                        let cb = cb.clone();
                        glib::idle_add_once(move || cb(filename));
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        // Nothing right now — come back later
                        return glib::ControlFlow::Continue;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // Bg thread is gone — remove this idle source
                        return glib::ControlFlow::Break;
                    }
                }
            }
        });

        Self {
            running,
            thread: Some(thread),
        }
    }

    /// Signal the loop to stop and wait for the thread to exit.
    pub fn stop(mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for ReceiverHandle {
    fn drop(&mut self) {
        // Signal without joining to avoid blocking the GTK main thread.
        self.running.store(false, Ordering::Relaxed);
    }
}

/// Default output directory: `~/Downloads/Kopico`.
pub fn default_out_dir() -> PathBuf {
    let mut p = dirs_next::download_dir()
        .or_else(dirs_next::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    p.push("Kopico");
    p
}

// ── Background thread ──────────────────────────────────────────────────────────

fn receive_loop(
    running: Arc<AtomicBool>,
    mode: RelayMode,
    out_dir: PathBuf,
    code: String,
    sender: std::sync::mpsc::Sender<String>,
) {
    let (timeout, interval) = match mode {
        RelayMode::Local => (TIMEOUT_LOCAL, INTERVAL_LOCAL),
        RelayMode::PublicRelay => (TIMEOUT_RELAY, INTERVAL_RELAY),
    };

    std::fs::create_dir_all(&out_dir).ok();

    while running.load(Ordering::Relaxed) {
        let cycle_start = Instant::now();

        match run_croc(&out_dir, mode, &code, timeout) {
            CrocResult::Received(files) => {
                for file in files {
                    // If the receiver has been detached (app shutting down) we stop.
                    if sender.send(file).is_err() {
                        return;
                    }
                }
            }
            CrocResult::RateLimited => {
                eprintln!(
                    "[croc-receive] rate-limited — backing off {}s",
                    BACKOFF_RELAY.as_secs()
                );
                sleep_interruptible(&running, BACKOFF_RELAY);
                continue;
            }
            CrocResult::NoSender | CrocResult::Error => {}
        }

        let elapsed = cycle_start.elapsed();
        if elapsed < interval {
            sleep_interruptible(&running, interval - elapsed);
        }
    }
}

// ── croc subprocess ────────────────────────────────────────────────────────────

#[derive(Debug)]
enum CrocResult {
    Received(Vec<String>),
    NoSender,
    RateLimited,
    Error,
}

fn run_croc(out_dir: &PathBuf, mode: RelayMode, code: &str, timeout: Duration) -> CrocResult {
    let mut cmd = Command::new("croc");
    cmd.arg("--yes")
        .arg("--overwrite")
        .arg("--out")
        .arg(out_dir)
        // croc refuses codes on the command line for security — use CROC_SECRET instead
        .env("CROC_SECRET", code)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if mode == RelayMode::Local {
        cmd.arg("--local");
    }

    let mut child: Child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[croc-receive] failed to spawn croc: {e}");
            return CrocResult::Error;
        }
    };

    let stdout = child.stdout.take().map(BufReader::new);
    let stderr = child.stderr.take().map(BufReader::new);

    let deadline = Instant::now() + timeout;
    let mut received_files: Vec<String> = Vec::new();
    let mut rate_limited = false;

    if let Some(mut reader) = stdout {
        let mut line = String::new();
        loop {
            if Instant::now() >= deadline {
                break;
            }
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => parse_croc_line(line.trim(), &mut received_files, &mut rate_limited),
                Err(_) => break,
            }
        }
    }

    if let Some(mut reader) = stderr {
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if is_rate_limited(&line) {
                rate_limited = true;
            }
            line.clear();
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    if rate_limited {
        return CrocResult::RateLimited;
    }
    if !received_files.is_empty() {
        return CrocResult::Received(received_files);
    }
    CrocResult::NoSender
}

fn parse_croc_line(line: &str, files: &mut Vec<String>, rate_limited: &mut bool) {
    if is_rate_limited(line) {
        *rate_limited = true;
        return;
    }
    // croc prints: `Received 'filename'` or `Saving 'filename'`
    if line.starts_with("Received") || line.starts_with("Saving") {
        if let Some(name) = extract_quoted(line) {
            files.push(name);
        }
    }
}

fn is_rate_limited(s: &str) -> bool {
    s.contains("rate limit") || s.contains("too many") || s.contains("429")
}

fn extract_quoted(line: &str) -> Option<String> {
    let start = line.find('\'')?;
    let rest = &line[start + 1..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

fn sleep_interruptible(running: &AtomicBool, duration: Duration) {
    const TICK: Duration = Duration::from_millis(200);
    let deadline = Instant::now() + duration;
    while running.load(Ordering::Relaxed) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        std::thread::sleep(remaining.min(TICK));
    }
}
