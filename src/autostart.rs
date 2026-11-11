//! Linux autostart integration through a freedesktop `.desktop` file.

use anyhow::Result;

#[cfg(target_os = "linux")]
use std::env;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use anyhow::Context;

pub const SUPPORTED: bool = cfg!(target_os = "linux");

#[cfg(target_os = "linux")]
const ICON: &[u8] = include_bytes!("../assets/icon/autoclicker-256.png");

#[cfg(target_os = "linux")]
pub fn is_enabled() -> bool {
    autostart_path().is_ok_and(|path| path.exists())
}

#[cfg(not(target_os = "linux"))]
pub fn is_enabled() -> bool {
    false
}

#[cfg(target_os = "linux")]
pub fn set_enabled(enabled: bool) -> Result<()> {
    let executable = env::current_exe().context("cannot resolve the current executable path")?;

    if enabled {
        install_menu_entry(&executable)?;
        install_icon()?;

        let path = autostart_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let entry = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Autoclicker\n\
             Comment=Virtual mouse autoclicker\n\
             Exec={} daemon --detach\n\
             Icon=autoclicker\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n\
             NoDisplay=false\n",
            desktop_quote(&executable)
        );
        write_atomic(&path, entry.as_bytes())?;
        Ok(())
    } else {
        match autostart_path() {
            Ok(path) => match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => {
                    Err(error).with_context(|| format!("failed to remove {}", path.display()))
                }
            },
            Err(error) => Err(error),
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn set_enabled(_enabled: bool) -> Result<()> {
    anyhow::bail!("autostart is available on Linux only")
}

#[cfg(target_os = "linux")]
fn install_menu_entry(executable: &Path) -> Result<()> {
    let path = data_home()?
        .join("applications")
        .join("autoclicker.desktop");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let entry = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Autoclicker\n\
         Comment=Virtual mouse autoclicker\n\
         Exec={}\n\
         Icon=autoclicker\n\
         Terminal=false\n\
         Categories=Utility;\n",
        desktop_quote(executable)
    );
    write_atomic(&path, entry.as_bytes())
}

#[cfg(target_os = "linux")]
fn install_icon() -> Result<()> {
    let path = data_home()?.join("icons/hicolor/256x256/apps/autoclicker.png");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    write_atomic(&path, ICON)
}

#[cfg(target_os = "linux")]
fn autostart_path() -> Result<PathBuf> {
    Ok(config_home()?.join("autostart").join("autoclicker.desktop"))
}

#[cfg(target_os = "linux")]
fn config_home() -> Result<PathBuf> {
    if let Some(directory) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(directory));
    }
    home().map(|home| home.join(".config"))
}

#[cfg(target_os = "linux")]
fn data_home() -> Result<PathBuf> {
    if let Some(directory) = env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(directory));
    }
    home().map(|home| home.join(".local").join("share"))
}

#[cfg(target_os = "linux")]
fn home() -> Result<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("the HOME environment variable is not set"))
}

#[cfg(target_os = "linux")]
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", path.display()))?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default(),
        std::process::id()
    ));

    fs::write(&temporary, bytes)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        anyhow::anyhow!("failed to replace {}: {error}", path.display())
    })
}

#[cfg(target_os = "linux")]
fn desktop_quote(path: &Path) -> String {
    let text = path.to_string_lossy();
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '\\' | '"' | '`' | '$' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '%' => {
                // Field codes in the Exec key start with `%`; a literal
                // percent sign has to be written as `%%`.
                quoted.push('%');
                quoted.push('%');
            }
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_quoting_escapes_special_characters() {
        let quoted = desktop_quote(Path::new("/opt/my app/$bin\"x%y"));
        assert_eq!(quoted, "\"/opt/my app/\\$bin\\\"x%%y\"");
    }
}
