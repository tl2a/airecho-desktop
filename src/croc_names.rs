//! File names croc can deliver.
//!
//! croc 11.5's receiver refuses the **whole transfer** if any name in it can't be created
//! safely on every OS (`receivefs.Normalize` / `validateReceiveMetadata` in croc's source):
//!
//! - a character that isn't printable — every space except U+0020 (non-breaking spaces,
//!   U+202F …), zero-width and direction marks, control characters;
//! - `:` (Finder shows it as `/`), a trailing `.` or space, Windows device names (`CON`,
//!   `aux.txt`, `COM1` …);
//! - a `.git`, `.ssh` or `.gnupg` component anywhere;
//! - two names in one folder that differ only in case;
//! - a symlink whose target is absolute or contains `..`.
//!
//! The sender doesn't check. In Local mode it then fails with `peer error: refusing files`;
//! on a relay it even exits 0 although nothing arrived. The common case is macOS
//! screenshots: `Screenshot 2026-10-19 at 10.15.32 AM.png` has U+202F before `AM`.
//!
//! [`prepare`] sends such items under safe names: it mirrors them into a private folder
//! with cleaned names — hard links (instant, no extra space), or a copy across volumes —
//! and croc sends the mirror. Items whose names are already fine are sent from where they
//! are. Only Unicode code points that are unassigned aren't recognised (no Unicode tables).

use std::{
    collections::HashSet,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// Staging folders older than this are leftovers (AirEcho quit mid-send) and are removed.
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const STAGING_PREFIX: &str = "files-";

/// What to pass to `croc send`. Dropping it deletes the staging folder (links/copies only,
/// never the originals) — keep it alive until croc has exited.
#[derive(Debug)]
pub struct Prepared {
    pub paths: Vec<PathBuf>,
    /// Names that were changed.
    pub renamed: usize,
    /// Items croc never accepts (`.git`/`.ssh`/`.gnupg`, symlinks pointing outside).
    pub left_out: usize,
    staging: Option<PathBuf>,
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if let Some(dir) = &self.staging {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

/// Make `files` sendable (see the module docs). `work_dir` is AirEcho's private folder.
pub fn prepare(files: &[PathBuf], work_dir: &Path) -> io::Result<Prepared> {
    remove_stale_staging(work_dir);

    // A selected symlink means its target (croc would send the link itself)
    let files: Vec<PathBuf> =
        files.iter().map(|f| fs::canonicalize(f).unwrap_or_else(|_| f.clone())).collect();
    let names: Vec<OsString> =
        files.iter().map(|f| f.file_name().map(OsString::from).unwrap_or_default()).collect();
    let finals = assign_names(&names);

    let mut prepared = Prepared { paths: Vec::new(), renamed: 0, left_out: 0, staging: None };
    for ((file, name), final_name) in files.iter().zip(&names).zip(finals) {
        let Some(final_name) = final_name else {
            prepared.left_out += 1;
            continue;
        };
        if Some(final_name.as_str()) == name.to_str() && tree_ok(file)? {
            prepared.paths.push(file.clone());
            continue;
        }
        if prepared.staging.is_none() {
            prepared.staging = Some(create_staging(work_dir)?);
        }
        if Some(final_name.as_str()) != name.to_str() {
            prepared.renamed += 1;
        }
        let dst = prepared.staging.as_ref().expect("created above").join(&final_name);
        mirror(file, &dst, &mut prepared)?;
        prepared.paths.push(dst);
    }
    Ok(prepared)
}

/// `name` as croc accepts it, or `None` for a name croc refuses anywhere in a transfer.
pub fn safe_name(name: &str) -> Option<String> {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            ' ' => out.push(' '),
            c if c.is_whitespace() => out.push(' '), // U+202F, no-break space, tab, newline …
            ':' | '\\' => out.push('-'),             // `\` would become a folder separator
            c if is_format_char(c) => {}             // zero-width / direction marks: invisible
            c if c.is_control() || is_private_use(c) || is_noncharacter(c) => out.push('_'),
            c => out.push(c),
        }
    }
    let mut out = match out.trim_end_matches(['.', ' ']) {
        "" => "_".to_owned(),
        trimmed => trimmed.to_owned(),
    };
    if is_windows_device_name(&out) {
        out.insert(0, '_');
    }
    match out.to_lowercase().as_str() {
        ".git" | ".ssh" | ".gnupg" => None,
        _ => Some(out),
    }
}

/// Final names for the entries of one folder: safe, and unique ignoring case. Names that
/// are already fine keep priority over cleaned ones. `None` = left out.
fn assign_names(names: &[OsString]) -> Vec<Option<String>> {
    let safe: Vec<Option<String>> =
        names.iter().map(|n| safe_name(&n.to_string_lossy())).collect();
    let mut taken = HashSet::new();
    let mut finals: Vec<Option<Option<String>>> = vec![None; names.len()];
    for (i, (name, safe)) in names.iter().zip(&safe).enumerate() {
        match safe {
            None => finals[i] = Some(None),
            Some(s) if name.to_str() == Some(s) && taken.insert(s.to_lowercase()) => {
                finals[i] = Some(Some(s.clone()));
            }
            Some(_) => {}
        }
    }
    for (i, safe) in safe.iter().enumerate() {
        if finals[i].is_none() {
            let name = safe.as_ref().expect("left-out names are set above");
            finals[i] = Some(Some(unique(name, &mut taken)));
        }
    }
    finals.into_iter().map(|f| f.expect("every entry assigned")).collect()
}

/// `name`, or `name (2).ext`, `name (3).ext` … — the first not in `taken` (ignoring case).
fn unique(name: &str, taken: &mut HashSet<String>) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    let mut candidate = name.to_owned();
    let mut n = 2;
    while !taken.insert(candidate.to_lowercase()) {
        candidate = format!("{stem} ({n}){ext}");
        n += 1;
    }
    candidate
}

/// Whether croc accepts everything inside `path` as it is.
fn tree_ok(path: &Path) -> io::Result<bool> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(symlink_target_ok(&fs::read_link(path)?));
    }
    if !meta.is_dir() {
        return Ok(true);
    }
    let entries = sorted_entries(path)?;
    let names: Vec<OsString> = entries.iter().map(|e| e.file_name().unwrap_or_default().into()).collect();
    for ((entry, name), final_name) in entries.iter().zip(&names).zip(assign_names(&names)) {
        if final_name.as_deref() != name.to_str() || !tree_ok(entry)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Recreate `src` at `dst` with safe names: folders are created, files hard-linked (copied
/// if that fails, e.g. across volumes), acceptable symlinks recreated as they are.
fn mirror(src: &Path, dst: &Path, prepared: &mut Prepared) -> io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    let file_type = meta.file_type();
    if file_type.is_dir() {
        fs::create_dir(dst)?;
        let entries = sorted_entries(src)?;
        let names: Vec<OsString> =
            entries.iter().map(|e| e.file_name().unwrap_or_default().into()).collect();
        for ((entry, name), final_name) in entries.iter().zip(&names).zip(assign_names(&names)) {
            match final_name {
                None => prepared.left_out += 1,
                Some(final_name) => {
                    if Some(final_name.as_str()) != name.to_str() {
                        prepared.renamed += 1;
                    }
                    mirror(entry, &dst.join(final_name), prepared)?;
                }
            }
        }
    } else if file_type.is_symlink() {
        let target = fs::read_link(src)?;
        if !symlink_target_ok(&target) {
            prepared.left_out += 1;
            return Ok(());
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, dst)?;
    } else if file_type.is_file() && fs::hard_link(src, dst).is_err() {
        fs::copy(src, dst)?;
    }
    // Sockets, FIFOs, devices: croc can't send them either
    Ok(())
}

/// croc only accepts relative symlink targets without `..`, made of acceptable names.
fn symlink_target_ok(target: &Path) -> bool {
    let target = target.to_string_lossy();
    !target.starts_with('/')
        && target
            .split(['/', '\\'])
            .filter(|c| !c.is_empty() && *c != ".")
            .all(|c| c != ".." && safe_name(c).as_deref() == Some(c))
}

fn sorted_entries(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut entries: Vec<PathBuf> =
        fs::read_dir(dir)?.map(|e| e.map(|e| e.path())).collect::<io::Result<_>>()?;
    entries.sort();
    Ok(entries)
}

fn create_staging(work_dir: &Path) -> io::Result<PathBuf> {
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let dir = work_dir.join(format!("{STAGING_PREFIX}{}-{nanos}", std::process::id()));
    fs::create_dir(&dir)?;
    Ok(dir)
}

/// Delete staging folders left behind when AirEcho quit during a send. Only old ones: a
/// second AirEcho process may be sending right now.
fn remove_stale_staging(work_dir: &Path) {
    let Ok(entries) = fs::read_dir(work_dir) else { return };
    for entry in entries.flatten() {
        let is_staging = entry.file_name().to_string_lossy().starts_with(STAGING_PREFIX);
        let age = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
        if is_staging && age.is_some_and(|age| age > STALE_AFTER) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Unicode "Cf" (format) characters: invisible, rejected by croc.
fn is_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{AD}'
            | '\u{600}'..='\u{605}'
            | '\u{61C}'
            | '\u{6DD}'
            | '\u{70F}'
            | '\u{890}'..='\u{891}'
            | '\u{8E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

fn is_private_use(c: char) -> bool {
    matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}' | '\u{100000}'..='\u{10FFFD}')
}

fn is_noncharacter(c: char) -> bool {
    matches!(c, '\u{FDD0}'..='\u{FDEF}') || (c as u32) & 0xFFFE == 0xFFFE
}

/// `CON`, `aux.txt`, `COM1.log`, `LPT²` … — the part before the first `.`, any case.
fn is_windows_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().to_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$") {
        return true;
    }
    let chars: Vec<char> = stem.chars().collect();
    chars.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(chars[3], '1'..='9' | '¹' | '²' | '³')
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREENSHOT: &str = "Screenshot 2026-10-19 at 10.15.32\u{202f}AM.png";

    #[test]
    fn cleans_names_croc_refuses() {
        assert_eq!(safe_name(SCREENSHOT).unwrap(), "Screenshot 2026-10-19 at 10.15.32 AM.png");
        assert_eq!(safe_name("a\u{a0}b\tc").unwrap(), "a b c");
        assert_eq!(safe_name("zero\u{200b}width\u{200e}").unwrap(), "zerowidth");
        assert_eq!(safe_name("10:30 notes").unwrap(), "10-30 notes");
        assert_eq!(safe_name("a\\b").unwrap(), "a-b");
        assert_eq!(safe_name("trailing. ").unwrap(), "trailing");
        assert_eq!(safe_name("...").unwrap(), "_");
        assert_eq!(safe_name("bell\u{7}").unwrap(), "bell_");
        assert_eq!(safe_name("aux.txt").unwrap(), "_aux.txt");
        assert_eq!(safe_name("com1").unwrap(), "_com1");
        assert_eq!(safe_name(".git"), None);
        assert_eq!(safe_name(".SSH"), None);
        assert_eq!(safe_name(".gnupg "), None);
    }

    #[test]
    fn keeps_names_croc_accepts() {
        for name in ["plain space.txt", "Über ü € 漢字 😀.pdf", ".hidden", "console.log", "COM10", ".gitignore"] {
            assert_eq!(safe_name(name).as_deref(), Some(name), "{name}");
        }
    }

    #[test]
    fn names_stay_unique_ignoring_case() {
        let names: Vec<OsString> = ["a:b.txt", "a-b.txt", "A.txt", "a.txt", ".git"].map(Into::into).into();
        assert_eq!(
            assign_names(&names),
            [Some("a-b (2).txt".into()), Some("a-b.txt".into()), Some("A.txt".into()), Some("a (2).txt".into()), None]
        );
    }

    #[test]
    fn symlink_targets() {
        assert!(symlink_target_ok(Path::new("Versions/Current")));
        assert!(!symlink_target_ok(Path::new("/etc/hosts")));
        assert!(!symlink_target_ok(Path::new("../outside")));
        assert!(!symlink_target_ok(Path::new("a\u{202f}b")));
    }

    #[test]
    fn stages_only_what_needs_it() {
        let base = std::env::temp_dir().join(format!("airecho-names-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (src, work) = (base.join("src"), base.join("work"));
        fs::create_dir_all(src.join("shots/.git")).unwrap();
        fs::create_dir_all(&work).unwrap();
        fs::write(src.join("fine.txt"), "fine").unwrap();
        fs::write(src.join(SCREENSHOT), "png").unwrap();
        fs::write(src.join("shots").join(SCREENSHOT), "inner").unwrap();
        fs::write(src.join("shots/.git/HEAD"), "ref").unwrap();
        std::os::unix::fs::symlink("/etc/hosts", src.join("shots/outside")).unwrap();
        std::os::unix::fs::symlink("fine link target", src.join("shots/inside")).unwrap();

        let files = [src.join("fine.txt"), src.join(SCREENSHOT), src.join("shots")];
        let prepared = prepare(&files, &work).unwrap();
        let staging = prepared.staging.clone().expect("staged");
        assert_eq!(prepared.paths[0], fs::canonicalize(src.join("fine.txt")).unwrap(), "sent in place");
        assert_eq!(prepared.paths[1], staging.join("Screenshot 2026-10-19 at 10.15.32 AM.png"));
        assert_eq!(fs::read_to_string(&prepared.paths[1]).unwrap(), "png");
        let shots = &prepared.paths[2];
        assert_eq!(fs::read_to_string(shots.join("Screenshot 2026-10-19 at 10.15.32 AM.png")).unwrap(), "inner");
        assert!(!shots.join(".git").exists());
        assert!(fs::symlink_metadata(shots.join("outside")).is_err());
        assert_eq!(fs::read_link(shots.join("inside")).unwrap(), Path::new("fine link target"));
        assert_eq!((prepared.renamed, prepared.left_out), (2, 2));

        drop(prepared);
        assert!(!staging.exists(), "staging removed");
        assert_eq!(fs::read_to_string(src.join(SCREENSHOT)).unwrap(), "png", "originals untouched");
        assert!(src.join("shots/.git/HEAD").exists());

        let clean = prepare(&[src.join("fine.txt")], &work).unwrap();
        assert!(clean.staging.is_none());
        fs::remove_dir_all(&base).unwrap();
    }
}
