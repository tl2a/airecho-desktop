use std::{fs, io, path::PathBuf};

pub fn setup_environment() {
    set_app_user_model_id();

    let Some(executable) = std::env::current_exe().ok() else {
        eprintln!("[windows] cannot locate the running executable");
        return;
    };
    let Some(prefix) = executable.parent() else {
        eprintln!("[windows] executable has no parent directory");
        return;
    };

    let share = prefix.join("share");
    let schemas = share.join("glib-2.0/schemas");
    if !schemas.join("gschemas.compiled").is_file() {
        return;
    }

    let loaders = prefix.join("lib/gdk-pixbuf-2.0/2.10.0/loaders");
    unsafe {
        std::env::set_var("XDG_DATA_DIRS", &share);
        std::env::set_var("GSETTINGS_SCHEMA_DIR", &schemas);
        std::env::set_var("GTK_DATA_PREFIX", prefix);
        std::env::set_var("GTK_EXE_PREFIX", prefix);
        std::env::set_var("GDK_PIXBUF_MODULEDIR", &loaders);
        std::env::set_var("FONTCONFIG_PATH", prefix.join("etc/fonts"));
        std::env::set_var("FONTCONFIG_FILE", prefix.join("etc/fonts/fonts.conf"));
    }

    match write_pixbuf_loaders_cache(prefix, &loaders) {
        Ok(Some(cache)) => unsafe {
            std::env::set_var("GDK_PIXBUF_MODULE_FILE", cache);
        },
        Ok(None) => eprintln!("[windows] bundled gdk-pixbuf loader cache template is missing"),
        Err(error) => eprintln!("[windows] failed to prepare gdk-pixbuf loaders: {error}"),
    }
}

fn set_app_user_model_id() {
    let app_id: Vec<u16> = crate::APP_ID
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe { SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr()) };
    if result < 0 {
        eprintln!(
            "[windows] failed to set AppUserModelID (HRESULT 0x{:08X})",
            result as u32
        );
    }
}

fn write_pixbuf_loaders_cache(
    prefix: &std::path::Path,
    loaders: &std::path::Path,
) -> io::Result<Option<PathBuf>> {
    let template_path = prefix.join("lib/gdk-pixbuf-2.0/2.10.0/loaders.cache.in");
    if !template_path.is_file() {
        return Ok(None);
    }

    let template = fs::read_to_string(template_path)?;
    if !template.contains("@LOADERS_DIR@") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "loader cache template has no @LOADERS_DIR@ placeholder",
        ));
    }

    let cache_dir = dirs_next::cache_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "cache directory is unavailable"))?
        .join(crate::APP_ID);
    fs::create_dir_all(&cache_dir)?;
    let cache = cache_dir.join("gdk-pixbuf-loaders.cache");
    let escaped_loaders = loaders.to_string_lossy().replace('\\', "\\\\");
    fs::write(&cache, template.replace("@LOADERS_DIR@", &escaped_loaders))?;
    Ok(Some(cache))
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
}
