//! Autostart integration: a freedesktop `.desktop` file on Linux and the
//! per-user `Run` registry key on Windows.

use anyhow::Result;

#[cfg(target_os = "linux")]
use std::env;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use anyhow::Context;

pub const SUPPORTED: bool = cfg!(any(target_os = "linux", windows));

#[cfg(target_os = "linux")]
const ICON: &[u8] = include_bytes!("../assets/icon/v-autoclicker-256.png");

#[cfg(target_os = "linux")]
const NAME: &str = "V-AutoClicker";

#[cfg(target_os = "linux")]
const ICON_NAME: &str = "v-autoclicker";

#[cfg(target_os = "linux")]
pub fn is_enabled() -> bool {
    autostart_path().is_ok_and(|path| path.exists())
}

#[cfg(windows)]
pub fn is_enabled() -> bool {
    windows_registry::is_enabled()
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn is_enabled() -> bool {
    false
}

#[cfg(target_os = "linux")]
pub fn set_enabled(enabled: bool, _start_hidden: bool) -> Result<()> {
    let executable = env::current_exe().context("cannot resolve the current executable path")?;

    if enabled {
        install_menu_entry(&executable)?;
        install_icon()?;
        remove_legacy_entries();

        let path = autostart_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        // Start the GUI, not a detached daemon: the tray setting lives in the
        // GUI config and the window is where the user expects the app.
        let entry = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name={NAME}\n\
             Comment=Virtual mouse autoclicker\n\
             Exec={}\n\
             Icon={ICON_NAME}\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n\
             NoDisplay=false\n",
            desktop_quote(&executable)
        );
        write_atomic(&path, entry.as_bytes())
    } else {
        remove_legacy_entries();
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

#[cfg(windows)]
pub fn set_enabled(enabled: bool, start_hidden: bool) -> Result<()> {
    windows_registry::set_enabled(enabled, start_hidden)
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn set_enabled(_enabled: bool, _start_hidden: bool) -> Result<()> {
    anyhow::bail!("autostart is available on Linux and Windows only")
}

#[cfg(target_os = "linux")]
fn install_menu_entry(executable: &Path) -> Result<()> {
    let path = data_home()?
        .join("applications")
        .join(format!("{ICON_NAME}.desktop"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let entry = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={NAME}\n\
         Comment=Virtual mouse autoclicker\n\
         Exec={}\n\
         Icon={ICON_NAME}\n\
         Terminal=false\n\
         Categories=Utility;\n",
        desktop_quote(executable)
    );
    write_atomic(&path, entry.as_bytes())
}

#[cfg(target_os = "linux")]
fn install_icon() -> Result<()> {
    let path = data_home()?
        .join("icons/hicolor/256x256/apps")
        .join(format!("{ICON_NAME}.png"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    write_atomic(&path, ICON)
}

#[cfg(target_os = "linux")]
fn remove_legacy_entries() {
    const LEGACY: [&str; 2] = ["savv-autoclicker", "autoclicker"];

    if let Ok(config) = config_home() {
        for name in LEGACY {
            let _ = fs::remove_file(config.join("autostart").join(format!("{name}.desktop")));
        }
    }
    if let Ok(data) = data_home() {
        for name in LEGACY {
            let _ = fs::remove_file(data.join("applications").join(format!("{name}.desktop")));
            let _ = fs::remove_file(
                data.join("icons/hicolor/256x256/apps")
                    .join(format!("{name}.png")),
            );
        }
    }
}

#[cfg(target_os = "linux")]
fn autostart_path() -> Result<PathBuf> {
    Ok(config_home()?
        .join("autostart")
        .join(format!("{ICON_NAME}.desktop")))
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

#[cfg(windows)]
mod windows_registry {
    use anyhow::{Context, Result, bail};
    use std::env;
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SAM_FLAGS, REG_SZ,
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };
    use windows::core::PCWSTR;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "V-AutoClicker";
    const LEGACY_VALUE_NAMES: [&str; 2] = ["SAVV-AutoClicker", "Autoclicker"];

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open_key(access: REG_SAM_FLAGS) -> Result<Option<HKEY>> {
        let subkey = wide(RUN_KEY);
        let mut key = HKEY::default();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                None,
                access,
                &mut key,
            )
        };

        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        status
            .ok()
            .with_context(|| "failed to open the Windows autostart registry key")?;
        Ok(Some(key))
    }

    pub fn is_enabled() -> bool {
        let Ok(Some(key)) = open_key(KEY_QUERY_VALUE) else {
            return false;
        };
        let name = wide(VALUE_NAME);
        let status =
            unsafe { RegQueryValueExW(key, PCWSTR(name.as_ptr()), None, None, None, None) };
        unsafe {
            let _ = RegCloseKey(key);
        }
        status.is_ok()
    }

    pub fn set_enabled(enabled: bool, start_hidden: bool) -> Result<()> {
        if enabled {
            let executable =
                env::current_exe().context("cannot resolve the current executable path")?;
            let mut command = format!("\"{}\"", executable.display());
            if start_hidden {
                command.push_str(" --hidden");
            }

            let Some(key) = open_key(KEY_SET_VALUE)? else {
                bail!("the Windows autostart registry key does not exist");
            };
            let name = wide(VALUE_NAME);
            // REG_SZ data includes the terminating NUL.
            let data = wide(&command);
            let bytes: Vec<u8> = data.iter().flat_map(|unit| unit.to_le_bytes()).collect();
            let status =
                unsafe { RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&bytes)) };
            unsafe {
                let _ = RegCloseKey(key);
            }
            status
                .ok()
                .with_context(|| "failed to write the Windows autostart registry value")?;
        } else {
            if let Some(key) = open_key(KEY_SET_VALUE)? {
                let name = wide(VALUE_NAME);
                let status = unsafe { RegDeleteValueW(key, PCWSTR(name.as_ptr())) };
                unsafe {
                    let _ = RegCloseKey(key);
                }
                if status.is_err() && status != ERROR_FILE_NOT_FOUND {
                    status
                        .ok()
                        .with_context(|| "failed to remove the Windows autostart registry value")?;
                }
            }
        }

        // Drop pre-rename values if an old build created them.
        if let Some(key) = open_key(KEY_SET_VALUE)? {
            for value in LEGACY_VALUE_NAMES {
                let name = wide(value);
                unsafe {
                    let _ = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
                }
            }
            unsafe {
                let _ = RegCloseKey(key);
            }
        }

        Ok(())
    }
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
