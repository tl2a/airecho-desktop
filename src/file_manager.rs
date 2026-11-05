use std::path::{Path, PathBuf};
use std::process::Command;

use adw::Application;
use adw::prelude::*;
use gtk4::gio;

#[derive(Clone, Copy)]
enum FileManager {
    Nautilus,
    Nemo,
    Dolphin,
    Thunar,
}

pub fn register_actions(app: &Application) {
    for (name, file_manager) in [
        ("install-nautilus", FileManager::Nautilus),
        ("install-nemo", FileManager::Nemo),
        ("install-dolphin", FileManager::Dolphin),
        ("install-thunar", FileManager::Thunar),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let app_w = app.downgrade();
        action.connect_activate(move |_, _| {
            let Some(app) = app_w.upgrade() else { return };
            let message = match install(file_manager) {
                Ok(message) => message,
                Err(error) => format!("Could not install file-manager integration: {error}"),
            };
            crate::show_toast(&app, &message);
        });
        app.add_action(&action);
    }
}

fn install(file_manager: FileManager) -> Result<String, String> {
    let executable = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| std::env::current_exe().map_err(|error| error.to_string()))?;
    let executable = std::fs::canonicalize(&executable).map_err(|error| {
        format!(
            "cannot resolve AirEcho executable {}: {error}",
            executable.display()
        )
    })?;
    let home = dirs_next::home_dir().ok_or("cannot determine the home directory")?;

    match file_manager {
        FileManager::Nautilus => {
            let destination = home.join(".local/share/nautilus-python/extensions/airecho-send.py");
            let template = include_str!("../data/linux/nautilus/airecho-send.py");
            let contents = template.replace(
                "\"@AIRECHO_BIN@\"",
                &python_string_literal(&executable.to_string_lossy()),
            );
            write_user_file(&destination, contents.as_bytes())?;
            Ok("Nautilus integration installed. It requires nautilus-python; restart Files to load it.".into())
        }
        FileManager::Nemo => {
            let destination = home.join(".local/share/nemo/actions/airecho-send.nemo_action");
            let template = include_str!("../data/linux/nemo/airecho-send.nemo_action");
            let contents = template.replace("@AIRECHO_BIN@", &desktop_exec_argument(&executable));
            write_user_file(&destination, contents.as_bytes())?;
            Ok("Nemo integration installed. Restart Nemo to load it.".into())
        }
        FileManager::Dolphin => {
            let destination = home.join(".local/share/kio/servicemenus/airecho-send.desktop");
            let template = include_str!("../data/linux/kio/airecho-send.desktop");
            install_dolphin_service_menu(&destination, template, &executable, false)?;
            let mut installed = vec!["native Dolphin"];
            let mut flatpak_installed = false;
            if flatpak_app_installed("org.kde.dolphin") {
                let flatpak_data = home.join(".var/app/org.kde.dolphin/data");
                let flatpak_destination =
                    flatpak_data.join("kio/servicemenus/airecho-send.desktop");
                install_dolphin_service_menu(&flatpak_destination, template, &executable, true)?;
                refresh_flatpak_dolphin_service_cache()?;
                installed.push("Flatpak Dolphin");
                flatpak_installed = true;
            }
            refresh_dolphin_service_cache()?;
            let mut message = format!(
                "Dolphin integration installed for {}. Restart Dolphin if it does not appear.",
                installed.join(" and ")
            );
            if flatpak_installed {
                message.push_str(" Flatpak Dolphin will launch AirEcho on the host.");
            }
            Ok(message)
        }
        FileManager::Thunar => install_thunar(&home, &executable),
    }
}

fn install_dolphin_service_menu(
    destination: &Path,
    template: &str,
    executable: &Path,
    use_flatpak_spawn: bool,
) -> Result<(), String> {
    let contents = dolphin_service_menu_contents(template, executable, use_flatpak_spawn);
    write_user_file(destination, contents.as_bytes())?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("cannot make {} executable: {error}", destination.display()))
}

fn dolphin_service_menu_contents(
    template: &str,
    executable: &Path,
    use_flatpak_spawn: bool,
) -> String {
    let executable = desktop_exec_argument(executable);
    let command = if use_flatpak_spawn {
        format!("flatpak-spawn --host {executable}")
    } else {
        executable
    };
    template.replace("@AIRECHO_BIN@", &command)
}

fn flatpak_app_installed(app_id: &str) -> bool {
    Command::new("flatpak")
        .args(["info", app_id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn write_user_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("integration path has no parent directory")?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    std::fs::write(path, contents)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn python_string_literal(value: &str) -> String {
    let mut literal = String::from("'");
    for character in value.chars() {
        match character {
            '\\' => literal.push_str("\\\\"),
            '\'' => literal.push_str("\\'"),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\t' => literal.push_str("\\t"),
            character if character.is_control() && (character as u32) <= 0xffff => {
                literal.push_str(&format!("\\u{:04x}", character as u32));
            }
            character if character.is_control() => {
                literal.push_str(&format!("\\U{:08x}", character as u32));
            }
            character => literal.push(character),
        }
    }
    literal.push('\'');
    literal
}

fn desktop_exec_argument(path: &Path) -> String {
    let mut argument = String::from("\"");
    for character in path.to_string_lossy().chars() {
        match character {
            '\\' | '"' | '`' | '$' => {
                argument.push('\\');
                argument.push(character);
            }
            '%' => argument.push_str("%%"),
            character => argument.push(character),
        }
    }
    argument.push('"');
    argument
}

fn refresh_dolphin_service_cache() -> Result<(), String> {
    let path_entries = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    let command = ["kbuildsycoca6", "kbuildsycoca5"]
        .into_iter()
        .find_map(|name| {
            path_entries
                .iter()
                .map(|directory| directory.join(name))
                .find(|candidate| candidate.is_file())
        });
    let Some(command) = command else {
        return Ok(());
    };
    let status = Command::new(&command)
        .arg("--noincremental")
        .status()
        .map_err(|error| {
            format!(
                "could not run {} to refresh Dolphin's service-menu cache: {error}",
                command.display()
            )
        })?;
    if !status.success() {
        return Err(format!(
            "Dolphin's service-menu cache refresh exited with {status}"
        ));
    }

    Ok(())
}

fn refresh_flatpak_dolphin_service_cache() -> Result<(), String> {
    let status = Command::new("flatpak")
        .args([
            "run",
            "--command=kbuildsycoca6",
            "org.kde.dolphin",
            "--noincremental",
        ])
        .status()
        .map_err(|error| {
            format!("could not refresh Flatpak Dolphin's service-menu cache: {error}")
        })?;
    if !status.success() {
        return Err(format!(
            "Flatpak Dolphin's service-menu cache refresh exited with {status}"
        ));
    }
    Ok(())
}

fn install_thunar(home: &Path, executable: &Path) -> Result<String, String> {
    const INSTALLER: &str = r#"
import os
import shlex
import sys
import xml.etree.ElementTree as ET

config = sys.argv[1]
parser = ET.XMLParser(target=ET.TreeBuilder(insert_comments=True))
if os.path.exists(config):
    tree = ET.parse(config, parser=parser)
    root = tree.getroot()
    if root.tag != "actions":
        raise SystemExit(f"Unexpected Thunar custom-action root in {config}: {root.tag}")
else:
    root = ET.Element("actions")
    tree = ET.ElementTree(root)

action = ET.Element("action")
for tag, text in (
    ("icon", "document-send"),
    ("name", "Send with AirEcho"),
    ("unique-id", "airecho-send"),
    ("command", f"{shlex.quote(os.environ['AIRECHO_BIN'])} %F"),
    ("description", "Send selected files with AirEcho"),
):
    ET.SubElement(action, tag).text = text
range_element = ET.SubElement(action, "range")
ET.SubElement(range_element, "file")
ET.SubElement(range_element, "directory")
ET.SubElement(action, "patterns").text = "*"
ET.SubElement(action, "startup-notify")

for index, existing in enumerate(root.findall("action")):
    if existing.findtext("unique-id") in {"airecho-send", "kopico-desktop-send"}:
        root.remove(existing)
        root.insert(index, action)
        break
else:
    root.append(action)

ET.indent(tree, space=" ")
tree.write(config, encoding="UTF-8", xml_declaration=True)
"#;

    let config = home.join(".config/Thunar/uca.xml");
    let parent = config
        .parent()
        .ok_or("Thunar configuration path has no parent")?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    let output = Command::new("python3")
        .arg("-c")
        .arg(INSTALLER)
        .arg(&config)
        .env("AIRECHO_BIN", executable)
        .output()
        .map_err(|error| format!("could not run Python to update Thunar actions: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Thunar configuration update failed: {}",
            stderr.trim()
        ));
    }
    Ok("Thunar integration installed. Restart Thunar to load it.".into())
}

#[cfg(test)]
mod tests {
    use super::{desktop_exec_argument, dolphin_service_menu_contents, python_string_literal};
    use std::path::Path;

    const DOLPHIN_TEMPLATE: &str = "[Desktop Action sendWithAirEcho]\nExec=@AIRECHO_BIN@ %F\n";

    #[test]
    fn flatpak_dolphin_action_launches_airecho_on_host() {
        assert_eq!(
            dolphin_service_menu_contents(
                DOLPHIN_TEMPLATE,
                Path::new("/home/user/Air Echo/airecho.AppImage"),
                true
            ),
            "[Desktop Action sendWithAirEcho]\nExec=flatpak-spawn --host \"/home/user/Air Echo/airecho.AppImage\" %F\n"
        );
    }

    #[test]
    fn native_dolphin_action_launches_airecho_directly() {
        assert_eq!(
            dolphin_service_menu_contents(DOLPHIN_TEMPLATE, Path::new("/home/user/airecho"), false),
            "[Desktop Action sendWithAirEcho]\nExec=\"/home/user/airecho\" %F\n"
        );
    }

    #[test]
    fn desktop_exec_argument_escapes_special_characters() {
        assert_eq!(
            desktop_exec_argument(Path::new("/home/user/Air Echo/$app%")),
            "\"/home/user/Air Echo/\\$app%%\""
        );
    }

    #[test]
    fn python_string_literal_escapes_quotes_and_control_characters() {
        assert_eq!(python_string_literal("Air'Echo\n"), "'Air\\'Echo\\n'");
    }
}
