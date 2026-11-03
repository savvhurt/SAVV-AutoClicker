use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::state::{Button, Settings};

#[derive(Clone, Debug)]
pub struct Loaded {
    pub settings: Settings,
    pub warning: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedSettings {
    cps: f64,
    button: String,
    hold_ms: f64,
    jitter_pct: f64,
}

impl From<&Settings> for PersistedSettings {
    fn from(settings: &Settings) -> Self {
        Self {
            cps: settings.cps,
            button: settings.button.to_string(),
            hold_ms: settings.hold_ms(),
            jitter_pct: settings.jitter_pct,
        }
    }
}

pub fn load() -> Loaded {
    let path = match config_path() {
        Ok(path) => path,
        Err(error) => {
            return Loaded {
                settings: Settings::default(),
                warning: Some(format!("Не удалось определить путь к конфигу: {error:#}")),
            };
        }
    };

    match fs::read_to_string(&path) {
        Ok(text) => match parse(&text) {
            Ok(settings) => Loaded {
                settings,
                warning: None,
            },
            Err(error) => Loaded {
                settings: Settings::default(),
                warning: Some(format!(
                    "Не удалось прочитать {}: {error:#}. Используются значения по умолчанию.",
                    path.display()
                )),
            },
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Loaded {
            settings: Settings::default(),
            warning: None,
        },
        Err(error) => Loaded {
            settings: Settings::default(),
            warning: Some(format!(
                "Не удалось прочитать {}: {error}. Используются значения по умолчанию.",
                path.display()
            )),
        },
    }
}

pub fn save(settings: &Settings) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let text = serialize(settings)?;
    fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub fn parse(text: &str) -> Result<Settings> {
    let persisted: PersistedSettings =
        toml::from_str(text).context("invalid TOML configuration")?;
    let button = persisted.button.parse::<Button>()?;

    Settings::new(
        persisted.cps,
        button,
        persisted.hold_ms,
        persisted.jitter_pct,
    )
}

pub fn serialize(settings: &Settings) -> Result<String> {
    toml::to_string_pretty(&PersistedSettings::from(settings))
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
        let text = serialize(&settings).unwrap();
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed, settings);
    }

    #[test]
    fn invalid_configuration_is_rejected() {
        assert!(parse("cps = 0\nbutton = \"left\"\nhold_ms = 10\njitter_pct = 0\n").is_err());
        assert!(parse("cps = 50\nbutton = \"side\"\nhold_ms = 10\njitter_pct = 0\n").is_err());
        assert!(parse("cps = 50\nbutton = \"left\"\nhold_ms = 10\n").is_err());
        assert!(parse("not toml at all").is_err());
    }
}
