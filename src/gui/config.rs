use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::hotkey::{DEFAULT as DEFAULT_HOTKEY, Hotkey};
use crate::state::{Button, Settings};

pub const DEFAULT_PROFILE: &str = "По умолчанию";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
}

impl ThemeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "light" | "светлая" => Some(Self::Light),
            "dark" | "тёмная" | "темная" => Some(Self::Dark),
            _ => None,
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub name: String,
    pub settings: Settings,
}

impl Profile {
    pub fn new(name: impl Into<String>, settings: Settings) -> Self {
        Self {
            name: name.into(),
            settings,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub settings: Settings,
    pub hotkey: String,
    pub hold_hotkey: String,
    pub theme: ThemeMode,
    pub profiles: Vec<Profile>,
    pub active_profile: String,
}

#[derive(Clone, Debug)]
pub struct Loaded {
    pub config: Config,
    pub warning: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedSettings {
    cps: f64,
    button: String,
    hold_ms: f64,
    jitter_pct: f64,
    #[serde(default)]
    limit: u64,
    #[serde(default)]
    delay_ms: f64,
    #[serde(default = "default_hotkey")]
    hotkey: String,
    #[serde(default)]
    hold_hotkey: String,
    #[serde(default = "default_theme")]
    theme: String,
    #[serde(default)]
    active_profile: String,
    #[serde(default)]
    profiles: Vec<PersistedProfile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedProfile {
    name: String,
    cps: f64,
    button: String,
    hold_ms: f64,
    jitter_pct: f64,
    #[serde(default)]
    limit: u64,
    #[serde(default)]
    delay_ms: f64,
}

impl PersistedSettings {
    fn new(config: &Config) -> Self {
        Self {
            cps: config.settings.cps,
            button: config.settings.button.to_string(),
            hold_ms: config.settings.hold_ms(),
            jitter_pct: config.settings.jitter_pct,
            limit: config.settings.limit,
            delay_ms: config.settings.delay_ms(),
            hotkey: config.hotkey.clone(),
            hold_hotkey: config.hold_hotkey.clone(),
            theme: config.theme.as_str().to_string(),
            active_profile: config.active_profile.clone(),
            profiles: config
                .profiles
                .iter()
                .map(PersistedProfile::from_profile)
                .collect(),
        }
    }
}

impl PersistedProfile {
    fn from_profile(profile: &Profile) -> Self {
        Self {
            name: profile.name.clone(),
            cps: profile.settings.cps,
            button: profile.settings.button.to_string(),
            hold_ms: profile.settings.hold_ms(),
            jitter_pct: profile.settings.jitter_pct,
            limit: profile.settings.limit,
            delay_ms: profile.settings.delay_ms(),
        }
    }

    fn to_profile(&self) -> Result<Profile> {
        let name = self.name.trim();
        if name.is_empty() {
            anyhow::bail!("profile name cannot be empty");
        }
        let settings = Settings::new(
            self.cps,
            self.button.parse::<Button>()?,
            self.hold_ms,
            self.jitter_pct,
        )?
        .with_limit(self.limit)?
        .with_delay_ms(self.delay_ms)?;
        Ok(Profile::new(name, settings))
    }
}

fn default_hotkey() -> String {
    DEFAULT_HOTKEY.to_string()
}

fn default_theme() -> String {
    ThemeMode::Light.as_str().to_string()
}

pub fn load() -> Loaded {
    let fallback = || Config {
        settings: Settings::default(),
        hotkey: default_hotkey(),
        hold_hotkey: String::new(),
        theme: ThemeMode::Light,
        profiles: vec![Profile::new(DEFAULT_PROFILE, Settings::default())],
        active_profile: DEFAULT_PROFILE.to_string(),
    };

    let path = match config_path() {
        Ok(path) => path,
        Err(error) => {
            return Loaded {
                config: fallback(),
                warning: Some(format!("Не удалось определить путь к конфигу: {error:#}")),
            };
        }
    };

    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Loaded {
                config: fallback(),
                warning: None,
            };
        }
        Err(error) => {
            return Loaded {
                config: fallback(),
                warning: Some(format!(
                    "Не удалось прочитать {}: {error}. Используются значения по умолчанию.",
                    path.display()
                )),
            };
        }
    };

    let persisted: PersistedSettings = match toml::from_str(&text) {
        Ok(persisted) => persisted,
        Err(error) => {
            return Loaded {
                config: fallback(),
                warning: Some(format!(
                    "Не удалось прочитать {}: {error:#}. Используются значения по умолчанию.",
                    path.display()
                )),
            };
        }
    };

    let mut warnings = Vec::new();

    let settings = match parse_settings(
        persisted.cps,
        &persisted.button,
        persisted.hold_ms,
        persisted.jitter_pct,
        persisted.limit,
        persisted.delay_ms,
    ) {
        Ok(settings) => settings,
        Err(error) => {
            warnings.push(format!("Настройки кликов некорректны: {error:#}"));
            Settings::default()
        }
    };

    let hotkey = match Hotkey::parse(&persisted.hotkey) {
        Ok(hotkey) => hotkey.to_string(),
        Err(error) => {
            warnings.push(format!(
                "Некорректная горячая клавиша: {error:#}. Используется {}.",
                DEFAULT_HOTKEY
            ));
            default_hotkey()
        }
    };

    let hold_hotkey = if persisted.hold_hotkey.trim().is_empty() {
        String::new()
    } else {
        match Hotkey::parse(&persisted.hold_hotkey) {
            Ok(hotkey) => hotkey.to_string(),
            Err(error) => {
                warnings.push(format!("Некорректная клавиша удержания: {error:#}."));
                String::new()
            }
        }
    };

    let theme = match ThemeMode::parse(&persisted.theme) {
        Some(theme) => theme,
        None => {
            warnings.push(format!(
                "Неизвестная тема `{}`. Используется светлая.",
                persisted.theme
            ));
            ThemeMode::Light
        }
    };

    let mut profiles = Vec::new();
    for profile in &persisted.profiles {
        match profile.to_profile() {
            Ok(profile) => {
                if profiles
                    .iter()
                    .any(|existing: &Profile| existing.name == profile.name)
                {
                    warnings.push(format!(
                        "Профиль `{}` пропущен: имя повторяется.",
                        profile.name
                    ));
                } else {
                    profiles.push(profile);
                }
            }
            Err(error) => warnings.push(format!("Профиль `{}` пропущен: {error:#}.", profile.name)),
        }
    }

    if profiles.is_empty() {
        profiles.push(Profile::new(DEFAULT_PROFILE, settings));
    }

    let active_profile = if profiles
        .iter()
        .any(|profile| profile.name == persisted.active_profile)
    {
        persisted.active_profile
    } else {
        profiles[0].name.clone()
    };

    Loaded {
        config: Config {
            settings,
            hotkey,
            hold_hotkey,
            theme,
            profiles,
            active_profile,
        },
        warning: (!warnings.is_empty()).then(|| warnings.join("\n")),
    }
}

fn parse_settings(
    cps: f64,
    button: &str,
    hold_ms: f64,
    jitter_pct: f64,
    limit: u64,
    delay_ms: f64,
) -> Result<Settings> {
    Settings::new(cps, button.parse::<Button>()?, hold_ms, jitter_pct)?
        .with_limit(limit)?
        .with_delay_ms(delay_ms)
}

pub fn save(config: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let text = serialize(config)?;
    fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
pub fn parse(text: &str) -> Result<Settings> {
    let persisted: PersistedSettings =
        toml::from_str(text).context("invalid TOML configuration")?;
    parse_settings(
        persisted.cps,
        &persisted.button,
        persisted.hold_ms,
        persisted.jitter_pct,
        persisted.limit,
        persisted.delay_ms,
    )
}

pub fn serialize(config: &Config) -> Result<String> {
    toml::to_string_pretty(&PersistedSettings::new(config))
        .context("failed to serialize GUI settings")
}

pub fn config_path() -> Result<PathBuf> {
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

    fn sample_config() -> Config {
        Config {
            settings: Settings::new(123.456, Button::Middle, 7.25, 33.5)
                .unwrap()
                .with_limit(500)
                .unwrap()
                .with_delay_ms(250.0)
                .unwrap(),
            hotkey: "f8".to_string(),
            hold_hotkey: "alt+space".to_string(),
            theme: ThemeMode::Dark,
            profiles: vec![
                Profile::new(
                    "Точный",
                    Settings::new(10.0, Button::Left, 40.0, 0.0)
                        .unwrap()
                        .with_limit(20)
                        .unwrap(),
                ),
                Profile::new("По умолчанию", Settings::default()),
            ],
            active_profile: "Точный".to_string(),
        }
    }

    #[test]
    fn settings_round_trip_through_toml() {
        let config = sample_config();
        let text = serialize(&config).unwrap();
        let parsed: PersistedSettings = toml::from_str(&text).unwrap();

        assert_eq!(parsed.hotkey, "f8");
        assert_eq!(parsed.hold_hotkey, "alt+space");
        assert_eq!(parsed.theme, "dark");
        assert_eq!(parsed.active_profile, "Точный");
        assert_eq!(parsed.profiles.len(), 2);
        assert_eq!(parsed.profiles[0].limit, 20);

        let settings = parse(&text).unwrap();
        assert_eq!(settings.limit, 500);
        assert!((settings.delay_ms() - 250.0).abs() < 1e-9);
    }

    #[test]
    fn missing_new_fields_use_defaults() {
        let settings =
            parse("cps = 50\nbutton = \"left\"\nhold_ms = 10\njitter_pct = 0\n").unwrap();
        assert_eq!(settings.limit, 0);
        assert_eq!(settings.delay_ms(), 0.0);
    }

    #[test]
    fn invalid_configuration_is_rejected() {
        assert!(parse("cps = 0\nbutton = \"left\"\nhold_ms = 10\njitter_pct = 0\n").is_err());
        assert!(parse("cps = 50\nbutton = \"side\"\nhold_ms = 10\njitter_pct = 0\n").is_err());
        assert!(parse("cps = 50\nbutton = \"left\"\nhold_ms = 10\n").is_err());
        assert!(parse("not toml at all").is_err());
    }

    #[test]
    fn old_config_gets_a_default_profile() {
        let text = "cps = 100\nbutton = \"right\"\nhold_ms = 8\njitter_pct = 0\nhotkey = \"f8\"\n";
        let persisted: PersistedSettings = toml::from_str(text).unwrap();
        assert!(persisted.profiles.is_empty());
        assert_eq!(persisted.theme, "light");
        assert_eq!(persisted.hold_hotkey, "");
    }

    #[test]
    fn theme_aliases_round_trip() {
        assert_eq!(ThemeMode::parse("Тёмная"), Some(ThemeMode::Dark));
        assert_eq!(ThemeMode::parse("LIGHT"), Some(ThemeMode::Light));
        assert_eq!(ThemeMode::parse("neon"), None);
        assert_eq!(ThemeMode::Light.toggled(), ThemeMode::Dark);
    }
}
