# Sending

How Kopico sends copied text and files with croc. Code: `src/transfers.rs` (coordination),
`src/sender.rs` (croc), `src/clipboard.rs` (watching copies), `src/macos_services.rs`
(Finder menu).

## What gets sent

| Trigger | Sent as | Needs |
|---|---|---|
| You **copy text** anywhere | `croc send --text "<text>"` | Kopico on + Advanced → *Send Copied Text* on (default) |
| Finder → right-click → **Send with Kopico** (Services / Quick Actions) | `croc send <files>` | macOS, Kopico started from its bundle |
| Finder → right-click → **Open With → Kopico Desktop** | `croc send <files>` | macOS |
| Nautilus, Dolphin, Nemo, or Thunar → right-click → **Send with Kopico** | `croc send <files>` | Linux; `nautilus-python` for Nautilus; `./dev-install.sh` |
| `kopico-desktop <files>` | `croc send <files>` | Linux (macOS: starts a second Kopico — use Finder) |
| Advanced → **Send Files…** | `croc send <files>` | — |

Every send uses the receive code and the receive mode:

```text
CROC_SECRET=<code> croc --yes --ignore-stdin --disable-clipboard --local send --text "…"
                                                                 ^ --relay <addr> (own relay) / nothing (public)
```

The other device just keeps receiving — e.g. another Kopico with the same code, or
`CROC_SECRET=<code> croc --ip <this-machine>:9009 --yes` by hand. **`--yes` is required
for files:** without it, a croc receiver that has no terminal to ask in declines files
(`peer error: refusing files` on the sending side) — text is never asked about.

## File names croc refuses

croc 11.5's **receiver** rejects the *whole transfer* if any name in it can't be created
safely on every OS; the sender doesn't check. In Local mode the sender then fails with
`peer error: refusing files` (the receiver prints e.g. `unsafe receive path …: non-printable
character U+202F`); **on a relay the sender even exits 0** although nothing arrived.
Refused (croc's `receivefs` rules):

| Name | Example |
|---|---|
| Any space other than U+0020, invisible/format or control characters | **macOS screenshots and screen recordings**: `Screenshot … 10.15.32 AM.png` has U+202F before `AM` |
| `:` (Finder shows it as `/`), a trailing `.` or space | `10:30 notes`, `draft.` |
| Windows device names, any extension | `CON`, `aux.txt`, `COM1.log` |
| `.git`, `.ssh`, `.gnupg` anywhere in a folder | sending a source repository |
| Names in one folder that differ only in case | `A.txt` + `a.txt` (Linux) |
| A symlink with an absolute target or `..` | `link → /etc/hosts`, `link → ../x` |

Kopico (`src/croc_names.rs`) checks every name before a send. Items that are fine are sent
from where they are. Any other item is **mirrored into the private send folder** with safe
names — `U+202F`/no-break spaces → space, `:` `\` → `-`, invisible characters removed,
trailing `.`/space trimmed, `_` before device names, ` (2)` for case clashes — using hard
links (instant, no extra space; a copy only across volumes). The mirror is deleted when
croc exits (leftovers from a quit mid-send after a day). `.git`/`.ssh`/`.gnupg` folders and
symlinks pointing outside are **left out**, and the "Sent …" message says how many.
Originals are never renamed or changed.

## Sending and receiving share one code

A `croc send` and Kopico's own receive loop with the same code would find each other —
Kopico would receive its own transfer. So `Transfers`:

1. **stops the receiver** before a send — but if a transfer is *coming in* right now
   (croc said `authenticating code…`), it waits for that to finish first;
2. runs **one `croc send` at a time** from a queue;
3. **restarts the receiver** once the queue is empty (if Kopico is on).

Queue rules:

- **Newer copied text replaces older text** that nobody has picked up yet (the waiting
  `croc send` is cancelled). Text already being transferred is not interrupted.
- **Files are queued** in order and always sent, even while Kopico is switched off — they
  are an explicit request.
- Switching Kopico **off** cancels copied-text sends; **quitting** kills any running
  `croc send` (nothing keeps running in the background).

## No echo

Two Kopicos with the same code would otherwise bounce text forever: A copies → B receives
and puts it on its clipboard → B's watcher sees a "copy" → B sends it back → …

Kopico remembers the last text it **received or sent**; a clipboard change to exactly that
text is not a new copy. This also covers Kopico's own clipboard write when text arrives,
and copying the same text twice.

## Watching the clipboard

| | How | Limits |
|---|---|---|
| macOS | checks `NSPasteboard.changeCount` every 500 ms (macOS has no change notification; clipboard managers poll the same way) | — |
| Linux, X11 | GDK `changed` signal | — |
| Linux, Wayland | GDK `changed` signal | GNOME (no data-control protocol): only while Kopico's window has focus |

Copies that existed before Kopico started are not sent. Content marked **private** by
password managers is never sent: `org.nspasteboard.ConcealedType` / `TransientType` /
`AutoGeneratedType` on macOS (1Password, Bitwarden, KeePassXC, … set these), and
`x-kde-passwordManagerHint` on KDE. Text over **64 KB** is not sent (it is passed to croc
as a command-line argument).

## croc's sender, as Kopico reads it

Verified against **croc 11.5.4**:

```text
On the other computer, run:  croc <code> …   ← ready (~0.3 s), waits for a receiver forever
Sending 'text' (12 B) | Sending 2 files (18 B)
Sending (->192.168.0.110)                    ← a receiver connected
a.txt 100% |████| (11/11 B, …) 1/2           ← progress (files)
```

- Exit 0 = delivered.
- **Wait limit** before giving up — only until `Sending (->…)`; a transfer in progress is
  never killed: 30 s (local, own relay), 75 s (public relay; the other side polls every
  30 s).
- **`--disable-clipboard`** is required: by default croc copies the code to the clipboard,
  which the watcher would then send.
- stderr contains the code, so it is never logged; `KOPICO_DEBUG=1` logs
  `[croc-send] run: copied text (…, code: N chars)` and the outcome, never the text.
- A killed `croc send` takes ~2 s to exit (measured); cancelling therefore completes ~2 s
  later, off the UI thread.
- **`--text` leaves the text on disk when killed:** croc writes it to `croc-stdin-<n>` in
  its working directory and deletes it only after a delivered send. Kopico runs croc in a
  private folder (`$XDG_RUNTIME_DIR/kopico-send` on Linux, `~/Library/Caches/kopico-send`
  on macOS, mode 700) and deletes `croc-stdin-*` there before and after every send.
- `--local` sends start croc's own relay on ports 9009–9013 — the other side's *Sender
  IP* points there.

## Results

| Outcome | Copied text | Files |
|---|---|---|
| Delivered | toast "Sent copied text" | notification + toast "Sent “x”" (+ how many items were left out, if any) |
| Nobody picked it up | toast | notification + toast |
| Failed (croc missing, relay rate limit, …) | toast | notification + toast |
| Cancelled | silent (replaced by newer text) | toast |

Copied text never causes system notifications — one per copy would be noise.

## Tests

```sh
cargo test sender                                   # argument building, parsing
cargo test sender -- --ignored --test-threads=1     # real croc: text, files, cancel
```

The full flow (copy → receiver pauses → send → receiver resumes; no echo; Finder Open
With and Services; quit mid-send) was verified on macOS 26 against real croc receivers.
The Linux file-manager entries are untested so far.
