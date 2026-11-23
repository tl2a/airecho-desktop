/// build.rs — compiles Blueprint UI files and bundles them as a GResource
/// so that `cargo build` / `cargo run` work without Meson.
///
/// Pipeline:
///   data/ui/*.blp
///     --[blueprint-compiler compile]--> $OUT_DIR/*.ui
///     --[glib-compile-resources]------> $OUT_DIR/airecho.gresource
///
/// The gresource is then linked into the binary via the `gio::resources_register_include!`
/// macro in main.rs.

use std::{
    env, fs,
    path::PathBuf,
    process::Command,
};

#[cfg(not(windows))]
use std::path::Path;

const APP_ID: &str = "in.tl2a.airEcho";

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // ── Locate tools ──────────────────────────────────────────────────────────
    let (blueprint_compiler, blueprint_script) = find_blueprint_compiler();
    let glib_compile_resources = find_binary("glib-compile-resources");

    // ── Compile each .blp → .ui ───────────────────────────────────────────────
    let blp_src_dir = manifest_dir.join("data").join("ui");
    let blp_files: Vec<_> = fs::read_dir(&blp_src_dir)
        .unwrap_or_else(|_| panic!("cannot read {}", blp_src_dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("blp"))
        .collect();

    for blp in &blp_files {
        let stem = blp.file_stem().unwrap().to_str().unwrap();
        let ui_out = out_dir.join(format!("{stem}.ui"));

        println!("cargo:rerun-if-changed={}", blp.display());

        let mut command = Command::new(&blueprint_compiler);
        if let Some(script) = &blueprint_script {
            command.arg(script).env("PYTHONUTF8", "1");
        }
        let status = command
            .args(["compile", "--output"])
            .arg(&ui_out)
            .arg(blp)
            .status()
            .unwrap_or_else(|e| panic!("failed to run blueprint-compiler: {e}"));

        if !status.success() {
            panic!("blueprint-compiler failed for {}", blp.display());
        }
    }

    // ── Compile .gresource ────────────────────────────────────────────────────
    let gresource_xml = manifest_dir
        .join("data")
        .join(format!("{APP_ID}.gresource.xml"));

    println!("cargo:rerun-if-changed={}", gresource_xml.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir
            .join("data/icons/in.tl2a.airEcho.svg")
            .display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("data/macos/about-icon.png").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("data/macos/index.theme").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir
            .join("data/icons/in.tl2a.airEcho-status-icon-active.svg")
            .display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir
            .join("data/icons/in.tl2a.airEcho-status-icon-inactive.svg")
            .display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir
            .join("data/icons/in.tl2a.airEcho-status-icon-active-16.png")
            .display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir
            .join("data/icons/in.tl2a.airEcho-status-icon-inactive-16.png")
            .display()
    );

    let gresource_out = out_dir.join(format!("{APP_ID}.gresource"));

    // Static resources listed in the manifest (e.g. style.css) live in data/
    let data_src_dir = manifest_dir.join("data");
    println!("cargo:rerun-if-changed={}", data_src_dir.join("style.css").display());

    // Generated .ui files are looked up first, then hand-written files in data/
    let status = Command::new(&glib_compile_resources)
        .arg(format!("--sourcedir={}", out_dir.display()))
        .arg(format!("--sourcedir={}", data_src_dir.display()))
        .arg(format!("--target={}", gresource_out.display()))
        .arg(&gresource_xml)
        .status()
        .unwrap_or_else(|e| panic!("failed to run glib-compile-resources: {e}"));

    if !status.success() {
        panic!("glib-compile-resources failed");
    }

    // ── Expose the path so `include_bytes!` / `resources_register_include!` works ──
    println!(
        "cargo:rustc-env=GRESOURCE_FILE={}",
        gresource_out.display()
    );
}

fn find_blueprint_compiler() -> (PathBuf, Option<PathBuf>) {
    if let Ok(executable) = which_simple("blueprint-compiler") {
        return (executable, None);
    }

    #[cfg(windows)]
    if let Ok(script) = which_script("blueprint-compiler") {
        let python = which_simple("python3")
            .or_else(|_| which_simple("python"))
            .unwrap_or_else(|_| panic!("Cannot find Python to run `blueprint-compiler`"));
        return (python, Some(script));
    }

    (find_binary("blueprint-compiler"), None)
}

/// Find a binary by name, also checking common Unix system paths outside $PATH.
fn find_binary(name: &str) -> PathBuf {
    // 1. Check $PATH first
    if let Ok(p) = which_simple(name) {
        return p;
    }

    // 2. Common extra locations on Unix-like systems
    #[cfg(not(windows))]
    {
        let extra = [
            format!("{}/.local/bin/{name}", env::var("HOME").unwrap_or_default()),
            format!("/usr/local/bin/{name}"),
            format!("/usr/bin/{name}"),
        ];
        for candidate in &extra {
            let p = Path::new(candidate);
            if p.is_file() {
                return p.to_path_buf();
            }
        }
    }

    panic!(
        "Cannot find `{name}`. Make sure it is installed and on $PATH \
         (or in ~/.local/bin)."
    );
}

fn which_simple(name: &str) -> Result<PathBuf, ()> {
    let path_var = env::var_os("PATH").unwrap_or_default();
    let executable = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(&executable);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(())
}

#[cfg(windows)]
fn which_script(name: &str) -> Result<PathBuf, ()> {
    let path_var = env::var_os("PATH").unwrap_or_default();
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(())
}
