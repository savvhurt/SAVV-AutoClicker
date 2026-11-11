use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::hotkey::{DEFAULT as DEFAULT_HOTKEY, Hotkey};
use crate::i18n::Language;
use crate::state::{Button, ClickMode, DEFAULT_POINT_INTERVAL_MS, Point, Settings};

pub const MIN_UI_SCALE: f32 = 0.8;
pub const MAX_UI_SCALE: f32 = 1.5;
pub const UI_SCALES: [f32; 6] = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5];

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

#[derive(Clone, Debug)]
pub struct Config {
    pub settings: Settings,
    pub hotkey: String,
    pub hold_hotkey: String,
    pub theme: ThemeMode,
    pub language: Language,
    pub ui_scale: f32,
    pub minimize_to_tray: bool,
}

#[derive(Clone, Debug)]
pub struct Loaded {
    pub config: Config,
    pub warning: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
struct PersistedPoint {
    x: i32,
    y: i32,
}

impl From<Point> for PersistedPoint {
    fn from(point: Point) -> Self {
        Self {
            x: point.x,
            y: point.y,
        }
    }
}

impl From<PersistedPoint> for Point {
    fn from(point: PersistedPoint) -> Self {
        Self::new(point.x, point.y)
    }
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
    #[serde(default = "default_multi_click")]
    multi_click: u8,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    target: Option<PersistedPoint>,
    #[serde(default)]
    points: Vec<PersistedPoint>,
    #[serde(default = "default_point_interval")]
    point_interval_ms: u32,
    #[serde(default = "default_hotkey")]
    hotkey: String,
    #[serde(default)]
    hold_hotkey: String,
    #[serde(default = "default_theme")]
    theme: String,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default = "default_ui_scale")]
    ui_scale: f32,
    #[serde(default)]
    minimize_to_tray: bool,
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
            multi_click: config.settings.multi_click,
            mode: config.settings.mode.as_str().to_string(),
            target: config.settings.target.map(PersistedPoint::from),
            points: config
                .settings
                .points
                .iter()
                .flatten()
                .map(|point| PersistedPoint::from(*point))
                .collect(),
            point_interval_ms: config.settings.point_interval_ms().round() as u32,
            hotkey: config.hotkey.clone(),
            hold_hotkey: config.hold_hotkey.clone(),
            theme: config.theme.as_str().to_string(),
            language: config.language.as_str().to_string(),
            ui_scale: config.ui_scale,
            minimize_to_tray: config.minimize_to_tray,
        }
    }
}

fn default_hotkey() -> String {
    DEFAULT_HOTKEY.to_string()
}

fn default_theme() -> String {
    ThemeMode::Light.as_str().to_string()
}

fn default_multi_click() -> u8 {
    1
}

fn default_point_interval() -> u32 {
    DEFAULT_POINT_INTERVAL_MS
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_language() -> String {
    Language::Auto.as_str().to_string()
}

pub fn load() -> Loaded {
    let fallback = || Config {
        settings: Settings::default(),
        hotkey: default_hotkey(),
        hold_hotkey: String::new(),
        theme: ThemeMode::Light,
        language: Language::Auto,
        ui_scale: 1.0,
        minimize_to_tray: false,
    };

    let path = match config_path() {
        Ok(path) => path,
        Err(error) => {
            return Loaded {
                config: fallback(),
                warning: Some(format!("Could not determine the config path: {error:#}")),
            };
        }
    };

    let (read_path, migrated, text) = match fs::read_to_string(&path) {
        Ok(text) => (path.clone(), false, text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let Some(legacy) = legacy_config_path().filter(|legacy| legacy.exists()) else {
                return Loaded {
                    config: fallback(),
                    warning: None,
                };
            };
            match fs::read_to_string(&legacy) {
                Ok(text) => (legacy, true, text),
                Err(error) => {
                    return Loaded {
                        config: fallback(),
                        warning: Some(format!(
                            "Could not read {}: {error}. Using defaults.",
                            legacy.display()
                        )),
                    };
                }
            }
        }
        Err(error) => {
            return Loaded {
                config: fallback(),
                warning: Some(format!(
                    "Could not read {}: {error}. Using defaults.",
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
                    "Could not parse {}: {error:#}. Using defaults.",
                    read_path.display()
                )),
            };
        }
    };

    let mut warnings = Vec::new();
    if migrated {
        warnings.push(format!("Imported settings from {}.", read_path.display()));
    }

    let settings = match parse_settings(&persisted) {
        Ok(settings) => settings,
        Err(error) => {
            warnings.push(format!("Invalid click settings: {error:#}"));
            Settings::default()
        }
    };

    let hotkey = match Hotkey::parse(&persisted.hotkey) {
        Ok(hotkey) => hotkey.to_string(),
        Err(error) => {
            warnings.push(format!(
                "Invalid hotkey: {error:#}. Using {}.",
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
                warnings.push(format!("Invalid hold key: {error:#}."));
                String::new()
            }
        }
    };

    let theme = match ThemeMode::parse(&persisted.theme) {
        Some(theme) => theme,
        None => {
            warnings.push(format!("Unknown theme `{}`. Using light.", persisted.theme));
            ThemeMode::Light
        }
    };

    let language = match Language::parse(&persisted.language) {
        Some(language) => language,
        None => {
            warnings.push(format!(
                "Unknown language `{}`. Using automatic detection.",
                persisted.language
            ));
            Language::Auto
        }
    };

    let ui_scale = if persisted.ui_scale.is_finite() {
        persisted.ui_scale.clamp(MIN_UI_SCALE, MAX_UI_SCALE)
    } else {
        warnings.push("Invalid UI scale. Using 100%.".to_string());
        1.0
    };

    Loaded {
        config: Config {
            settings,
            hotkey,
            hold_hotkey,
            theme,
            language,
            ui_scale,
            minimize_to_tray: persisted.minimize_to_tray,
        },
        warning: (!warnings.is_empty()).then(|| warnings.join("\n")),
    }
}

fn parse_settings(persisted: &PersistedSettings) -> Result<Settings> {
    let mode = if persisted.mode.trim().is_empty() {
        if persisted.target.is_some() {
            ClickMode::Point
        } else {
            ClickMode::Cursor
        }
    } else {
        ClickMode::parse(&persisted.mode)
            .ok_or_else(|| anyhow::anyhow!("unknown mode `{}`", persisted.mode))?
    };

    let points = persisted
        .points
        .iter()
        .copied()
        .map(Point::from)
        .collect::<Vec<_>>();
    let interval = if persisted.point_interval_ms == 0 {
        DEFAULT_POINT_INTERVAL_MS
    } else {
        persisted.point_interval_ms
    };

    Settings::new(
        persisted.cps,
        persisted.button.parse::<Button>()?,
        persisted.hold_ms,
        persisted.jitter_pct,
    )?
    .with_limit(persisted.limit)?
    .with_delay_ms(persisted.delay_ms)?
    .with_multi_click(persisted.multi_click)?
    .with_mode(mode)
    .with_target(persisted.target.map(Point::from))?
    .with_points(&points)?
    .with_point_interval_ms(interval)
}

pub fn config_path() -> Result<PathBuf> {
    let directory = config_directory()
        .ok_or_else(|| anyhow::anyhow!("the user configuration directory is not set"))?;
    Ok(directory.join("savv-autoclicker").join("gui.toml"))
}

fn legacy_config_path() -> Option<PathBuf> {
    config_directory().map(|directory| directory.join("autoclicker").join("gui.toml"))
}

#[cfg(test)]
pub fn parse(text: &str) -> Result<Settings> {
    let persisted: PersistedSettings =
        toml::from_str(text).context("invalid TOML configuration")?;
    parse_settings(&persisted)
}

pub fn serialize(config: &Config) -> Result<String> {
    toml::to_string_pretty(&PersistedSettings::new(config))
        .context("failed to serialize GUI settings")
}

pub fn save(config: &Config) -> Result<()> {
    let path = config_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let text = serialize(config)?;
    let temporary = parent.join(format!(".gui.toml.{}.tmp", std::process::id()));
    fs::write(&temporary, text)
        .with_context(|| format!("failed to write {}", temporary.display()))?;

    match fs::rename(&temporary, &path) {
        Ok(()) => Ok(()),
        Err(error) => {
            // `rename` does not replace an existing file on Windows.
            if path.exists()
                && fs::remove_file(&path).is_ok()
                && fs::rename(&temporary, &path).is_ok()
            {
                return Ok(());
            }
            let _ = fs::remove_file(&temporary);
            Err(error).with_context(|| format!("failed to replace {}", path.display()))
        }
    }
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
                .unwrap()
                .with_multi_click(2)
                .unwrap()
                .with_mode(ClickMode::Sequence)
                .with_points(&[Point::new(10, -20), Point::new(30, 40)])
                .unwrap()
                .with_point_interval_ms(175)
                .unwrap(),
            hotkey: "f8".to_string(),
            hold_hotkey: "alt+space".to_string(),
            theme: ThemeMode::Dark,
            language: Language::De,
            ui_scale: 1.25,
            minimize_to_tray: true,
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
        assert_eq!(parsed.language, "de");
        assert_eq!(parsed.multi_click, 2);
        assert_eq!(parsed.mode, "sequence");
        assert_eq!(parsed.points.len(), 2);
        assert_eq!(parsed.point_interval_ms, 175);
        assert!((parsed.ui_scale - 1.25).abs() < 1e-6);
        assert!(parsed.minimize_to_tray);
        assert!(!text.contains("profiles"));
        assert!(!text.contains("active_profile"));

        let settings = parse(&text).unwrap();
        assert_eq!(settings.limit, 500);
        assert!((settings.delay_ms() - 250.0).abs() < 1e-9);
        assert_eq!(settings.multi_click, 2);
        assert_eq!(settings.mode, ClickMode::Sequence);
        assert_eq!(settings.point_count(), 2);
        assert_eq!(settings.point_at(0), Some(Point::new(10, -20)));
        assert!((settings.point_interval_ms() - 175.0).abs() < 1e-9);
    }

    #[test]
    fn missing_new_fields_use_defaults() {
        let settings =
            parse("cps = 50\nbutton = \"left\"\nhold_ms = 10\njitter_pct = 0\n").unwrap();
        assert_eq!(settings.limit, 0);
        assert_eq!(settings.delay_ms(), 0.0);
        assert_eq!(settings.multi_click, 1);
        assert_eq!(settings.mode, ClickMode::Cursor);
        assert_eq!(settings.target, None);
        assert_eq!(settings.point_count(), 0);
    }

    #[test]
    fn legacy_target_without_mode_becomes_point_mode() {
        let settings = parse(
            "cps = 50\nbutton = \"left\"\nhold_ms = 10\njitter_pct = 0\ntarget = { x = 5, y = -6 }\n",
        )
        .unwrap();
        assert_eq!(settings.mode, ClickMode::Point);
        assert_eq!(settings.target, Some(Point::new(5, -6)));
    }

    #[test]
    fn invalid_configuration_is_rejected() {
        assert!(parse("cps = 0\nbutton = \"left\"\nhold_ms = 10\njitter_pct = 0\n").is_err());
        assert!(parse("cps = 50\nbutton = \"side\"\nhold_ms = 10\njitter_pct = 0\n").is_err());
        assert!(parse("cps = 50\nbutton = \"left\"\nhold_ms = 10\n").is_err());
        assert!(parse("not toml at all").is_err());
    }

    #[test]
    fn legacy_profile_fields_are_ignored() {
        let text = "cps = 100\nbutton = \"right\"\nhold_ms = 8\njitter_pct = 0\nhotkey = \"f8\"\nactive_profile = \"Точный\"\n[[profiles]]\nname = \"Точный\"\ncps = 10.0\nbutton = \"left\"\nhold_ms = 40.0\njitter_pct = 0.0\n";
        let persisted: PersistedSettings = toml::from_str(text).unwrap();
        assert_eq!(persisted.theme, "light");
        assert_eq!(persisted.hold_hotkey, "");

        let settings = parse(text).unwrap();
        assert_eq!(settings.cps, 100.0);
        assert_eq!(settings.button, Button::Right);
    }

    #[test]
    fn theme_aliases_round_trip() {
        assert_eq!(ThemeMode::parse("Тёмная"), Some(ThemeMode::Dark));
        assert_eq!(ThemeMode::parse("LIGHT"), Some(ThemeMode::Light));
        assert_eq!(ThemeMode::parse("neon"), None);
        assert_eq!(ThemeMode::Light.toggled(), ThemeMode::Dark);
    }
}
