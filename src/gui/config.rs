use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::hotkey::{DEFAULT as DEFAULT_HOTKEY, Hotkey};
use crate::state::{Button, Settings};

#[derive(Clone, Debug)]
pub struct Loaded {
    pub settings: Settings,
    pub hotkey: String,
    pub warning: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedSettings {
    cps: f64,
    button: String,
    hold_ms: f64,
    jitter_pct: f64,
    #[serde(default = "default_hotkey")]
    hotkey: String,
}

impl PersistedSettings {
    fn new(settings: &Settings, hotkey: &str) -> Self {
        Self {
            cps: settings.cps,
            button: settings.button.to_string(),
            hold_ms: settings.hold_ms(),
            jitter_pct: settings.jitter_pct,
            hotkey: hotkey.to_string(),
        }
    }
}

fn default_hotkey() -> String {
    DEFAULT_HOTKEY.to_string()
}

pub fn load() -> Loaded {
    let path = match config_path() {
        Ok(path) => path,
        Err(error) => {
            return Loaded {
                settings: Settings::default(),
                hotkey: default_hotkey(),
                warning: Some(format!("Не удалось определить путь к конфигу: {error:#}")),
            };
        }
    };

    match fs::read_to_string(&path) {
        Ok(text) => match parse_document(&text) {
            Ok((settings, raw_hotkey)) => match Hotkey::parse(&raw_hotkey) {
                Ok(hotkey) => Loaded {
                    settings,
                    hotkey: hotkey.to_string(),
                    warning: None,
                },
                Err(error) => Loaded {
                    settings,
                    hotkey: default_hotkey(),
                    warning: Some(format!(
                        "Некорректная горячая клавиша в {}: {error:#}. Используется {}.",
                        path.display(),
                        DEFAULT_HOTKEY
                    )),
                },
            },
            Err(error) => Loaded {
                settings: Settings::default(),
                hotkey: default_hotkey(),
                warning: Some(format!(
                    "Не удалось прочитать {}: {error:#}. Используются значения по умолчанию.",
                    path.display()
                )),
            },
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Loaded {
            settings: Settings::default(),
            hotkey: default_hotkey(),
            warning: None,
        },
        Err(error) => Loaded {
            settings: Settings::default(),
            hotkey: default_hotkey(),
            warning: Some(format!(
                "Не удалось прочитать {}: {error}. Используются значения по умолчанию.",
                path.display()
            )),
        },
    }
}

pub fn save(settings: &Settings, hotkey: &str) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let text = serialize(settings, hotkey)?;
    fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
pub fn parse(text: &str) -> Result<Settings> {
    Ok(parse_document(text)?.0)
}

fn parse_document(text: &str) -> Result<(Settings, String)> {
    let persisted: PersistedSettings =
        toml::from_str(text).context("invalid TOML configuration")?;
    let button = persisted.button.parse::<Button>()?;

    let settings = Settings::new(
        persisted.cps,
        button,
        persisted.hold_ms,
        persisted.jitter_pct,
    )?;

    Ok((settings, persisted.hotkey))
}

pub fn serialize(settings: &Settings, hotkey: &str) -> Result<String> {
    toml::to_string_pretty(&PersistedSettings::new(settings, hotkey))
        .context("failed to serialize GUI settings")
}

fn config_path() -> Result<PathBuf> {
    let directory = config_directory()
        .ok_or_else(|| anyhow::anyhow!("the user configuration directory is not set"))?;
    Ok(directory.join("autoclicker").join("gui.toml"))
}

#[cfg(unix)]
fn config_directory() -> Option<PathBuf> {
    if let Some(directory) = non_empty_env("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(directory));
    }

    non_empty_env("HOME").map(|home| PathBuf::from(home).join(".config"))
}

#[cfg(windows)]
fn config_directory() -> Option<PathBuf> {
    non_empty_env("APPDATA").map(PathBuf::from)
}

fn non_empty_env(name: &str) -> Option<std::ffi::OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_toml() {
        let settings = Settings::new(123.456, Button::Middle, 7.25, 33.5).unwrap();
        let text = serialize(&settings, "ctrl+shift+f8").unwrap();
        let (parsed, hotkey) = parse_document(&text).unwrap();
        assert_eq!(parsed, settings);
        assert_eq!(hotkey, "ctrl+shift+f8");
    }

    #[test]
    fn missing_hotkey_uses_the_default() {
        let (_, hotkey) =
            parse_document("cps = 50\nbutton = \"left\"\nhold_ms = 10\njitter_pct = 0\n").unwrap();
        assert_eq!(hotkey, DEFAULT_HOTKEY);
    }

    #[test]
    fn invalid_configuration_is_rejected() {
        assert!(parse("cps = 0\nbutton = \"left\"\nhold_ms = 10\njitter_pct = 0\n").is_err());
        assert!(parse("cps = 50\nbutton = \"side\"\nhold_ms = 10\njitter_pct = 0\n").is_err());
        assert!(parse("cps = 50\nbutton = \"left\"\nhold_ms = 10\n").is_err());
        assert!(parse("not toml at all").is_err());
    }
}
