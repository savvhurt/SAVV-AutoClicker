use std::collections::VecDeque;
use std::fmt;
use std::str::FromStr;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

impl Button {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Middle => "middle",
        }
    }
}

impl fmt::Display for Button {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Button {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "middle" => Ok(Self::Middle),
            _ => bail!("unknown button `{value}` (expected left, right or middle)"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub cps: f64,
    pub button: Button,
    pub hold: Duration,
    pub jitter_pct: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            cps: 50.0,
            button: Button::Left,
            hold: Duration::from_millis(10),
            jitter_pct: 0.0,
        }
    }
}

impl Settings {
    pub fn new(cps: f64, button: Button, hold_ms: f64, jitter_pct: f64) -> Result<Self> {
        if !cps.is_finite() || !(0.2..=1000.0).contains(&cps) {
            bail!("cps must be between 0.2 and 1000");
        }
        if !hold_ms.is_finite() || !(0.1..=5000.0).contains(&hold_ms) {
            bail!("hold-ms must be between 0.1 and 5000");
        }
        if !jitter_pct.is_finite() || !(0.0..=100.0).contains(&jitter_pct) {
            bail!("jitter-pct must be between 0 and 100");
        }

        Ok(Self {
            cps,
            button,
            hold: Duration::from_secs_f64(hold_ms / 1000.0),
            jitter_pct,
        })
    }

    pub fn hold_ms(&self) -> f64 {
        self.hold.as_secs_f64() * 1000.0
    }

    pub fn to_wire_command(&self, command: &str) -> String {
        format!(
            "{command} {} {} {} {}",
            format_number(self.cps),
            self.button,
            format_number(self.hold_ms()),
            format_number(self.jitter_pct)
        )
    }
}

fn format_number(value: f64) -> String {
    let mut text = format!("{value:.3}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    pub enabled: bool,
    pub quit: bool,
    pub settings: Settings,
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub enabled: bool,
    pub settings: Settings,
    pub actual_cps: f64,
    pub clicks: u64,
    pub hotkey: String,
    pub hotkey_error: Option<String>,
    pub last_error: Option<String>,
}

impl Status {
    pub fn to_wire(&self) -> String {
        let error = self
            .last_error
            .as_deref()
            .unwrap_or("none")
            .replace(['\n', '\r'], " ");
        let hotkey = self.hotkey.replace(['\n', '\r'], " ");
        let hotkey_error = self
            .hotkey_error
            .as_deref()
            .unwrap_or("none")
            .replace(['\n', '\r'], " ");

        format!(
            "OK\nenabled: {}\ncps: {}\nbutton: {}\nhold_ms: {}\njitter_pct: {}\nactual_cps: {:.1}\nclicks: {}\nhotkey: {}\nhotkey_error: {}\nerror: {}\n",
            self.enabled,
            format_number(self.settings.cps),
            self.settings.button,
            format_number(self.settings.hold_ms()),
            format_number(self.settings.jitter_pct),
            self.actual_cps,
            self.clicks,
            hotkey,
            hotkey_error,
            error
        )
    }

    pub fn from_wire(response: &str) -> Result<Self> {
        let response = response.trim_end_matches(['\n', '\r']);
        let body = if let Some(body) = response.strip_prefix("OK\n") {
            body
        } else if response == "OK" {
            ""
        } else if let Some(error) = response.strip_prefix("ERR ") {
            bail!("{}", error.trim());
        } else {
            bail!("unexpected daemon response: {}", response.trim());
        };

        let mut enabled = None;
        let mut cps = None;
        let mut button = None;
        let mut hold_ms = None;
        let mut jitter_pct = None;
        let mut actual_cps = None;
        let mut clicks = None;
        let mut hotkey = None;
        let mut hotkey_error = None;
        let mut last_error = None;

        for line in body.lines() {
            let Some((key, value)) = line.split_once(':') else {
                bail!("malformed status line `{line}`");
            };
            let value = value.trim();

            match key.trim() {
                "enabled" => enabled = Some(value.parse::<bool>()?),
                "cps" => cps = Some(value.parse::<f64>()?),
                "button" => button = Some(value.parse::<Button>()?),
                "hold_ms" => hold_ms = Some(value.parse::<f64>()?),
                "jitter_pct" => jitter_pct = Some(value.parse::<f64>()?),
                "actual_cps" => actual_cps = Some(value.parse::<f64>()?),
                "clicks" => clicks = Some(value.parse::<u64>()?),
                "hotkey" => hotkey = Some(value.to_string()),
                "hotkey_error" => {
                    hotkey_error = if value.is_empty() || value.eq_ignore_ascii_case("none") {
                        None
                    } else {
                        Some(value.to_string())
                    };
                }
                "error" => {
                    last_error = if value.is_empty() || value.eq_ignore_ascii_case("none") {
                        None
                    } else {
                        Some(value.to_string())
                    };
                }
                _ => {}
            }
        }

        let settings = Settings::new(
            cps.ok_or_else(|| anyhow::anyhow!("status is missing `cps`"))?,
            button.ok_or_else(|| anyhow::anyhow!("status is missing `button`"))?,
            hold_ms.ok_or_else(|| anyhow::anyhow!("status is missing `hold_ms`"))?,
            jitter_pct.ok_or_else(|| anyhow::anyhow!("status is missing `jitter_pct`"))?,
        )?;

        Ok(Self {
            enabled: enabled.ok_or_else(|| anyhow::anyhow!("status is missing `enabled`"))?,
            settings,
            actual_cps: actual_cps
                .ok_or_else(|| anyhow::anyhow!("status is missing `actual_cps`"))?,
            clicks: clicks.ok_or_else(|| anyhow::anyhow!("status is missing `clicks`"))?,
            hotkey: hotkey.unwrap_or_default(),
            hotkey_error,
            last_error,
        })
    }
}

struct Inner {
    enabled: bool,
    quit: bool,
    settings: Settings,
    version: u64,
    clicks: u64,
    click_times: VecDeque<Instant>,
    hotkey: String,
    hotkey_error: Option<String>,
    hotkey_reload: bool,
    last_error: Option<String>,
}

pub struct SharedState {
    inner: Mutex<Inner>,
    changed: Condvar,
}

impl SharedState {
    pub fn new(settings: Settings, enabled: bool) -> Self {
        Self {
            inner: Mutex::new(Inner {
                enabled,
                quit: false,
                settings,
                version: 0,
                clicks: 0,
                click_times: VecDeque::new(),
                hotkey: crate::hotkey::DEFAULT.to_string(),
                hotkey_error: None,
                hotkey_reload: false,
                last_error: None,
            }),
            changed: Condvar::new(),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let inner = self.inner.lock().unwrap();
        Snapshot {
            enabled: inner.enabled,
            quit: inner.quit,
            settings: inner.settings,
            version: inner.version,
        }
    }

    pub fn toggle(&self) -> bool {
        let enabled = {
            let mut inner = self.inner.lock().unwrap();
            inner.enabled = !inner.enabled;
            inner.version = inner.version.wrapping_add(1);
            inner.click_times.clear();
            if inner.enabled {
                inner.last_error = None;
            }
            inner.enabled
        };
        self.changed.notify_all();
        enabled
    }

    pub fn on(&self, settings: Settings) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.settings = settings;
            inner.enabled = true;
            inner.version = inner.version.wrapping_add(1);
            inner.click_times.clear();
            inner.last_error = None;
        }
        self.changed.notify_all();
    }

    pub fn set_settings(&self, settings: Settings) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.settings = settings;
            inner.version = inner.version.wrapping_add(1);
            inner.click_times.clear();
        }
        self.changed.notify_all();
    }

    pub fn off(&self) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.enabled = false;
            inner.version = inner.version.wrapping_add(1);
            inner.click_times.clear();
        }
        self.changed.notify_all();
    }

    pub fn quit(&self) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.quit = true;
            inner.enabled = false;
            inner.version = inner.version.wrapping_add(1);
            inner.click_times.clear();
        }
        self.changed.notify_all();
    }

    pub fn is_quit(&self) -> bool {
        self.inner.lock().unwrap().quit
    }

    pub fn wait_for_quit(&self) {
        let mut inner = self.inner.lock().unwrap();
        while !inner.quit {
            inner = self.changed.wait(inner).unwrap();
        }
    }

    pub fn wait_for_change(&self, observed_version: u64) {
        let mut inner = self.inner.lock().unwrap();
        while !inner.quit && inner.version == observed_version {
            inner = self.changed.wait(inner).unwrap();
        }
    }

    pub fn should_stop(&self, version: u64) -> bool {
        let inner = self.inner.lock().unwrap();
        !inner.enabled || inner.quit || inner.version != version
    }

    pub fn record_click(&self) {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap();
        inner.clicks = inner.clicks.saturating_add(1);
        inner.click_times.push_back(now);

        while let Some(front) = inner.click_times.front() {
            if now.saturating_duration_since(*front) > Duration::from_secs(1) {
                inner.click_times.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn set_error(&self, message: impl Into<String>) {
        self.inner.lock().unwrap().last_error = Some(message.into());
    }

    pub fn set_hotkey(&self, value: impl Into<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.hotkey = value.into();
        inner.hotkey_error = None;
        inner.hotkey_reload = false;
    }

    pub fn request_hotkey(&self, value: impl Into<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.hotkey = value.into();
        inner.hotkey_error = None;
        inner.hotkey_reload = true;
    }

    #[cfg(windows)]
    pub(crate) fn take_hotkey_request(&self) -> Option<String> {
        let mut inner = self.inner.lock().unwrap();
        if !inner.hotkey_reload {
            return None;
        }
        inner.hotkey_reload = false;
        Some(inner.hotkey.clone())
    }

    pub fn set_hotkey_error(&self, message: impl Into<String>) {
        self.inner.lock().unwrap().hotkey_error = Some(message.into());
    }

    pub fn clear_hotkey_error(&self) {
        self.inner.lock().unwrap().hotkey_error = None;
    }

    pub fn total_clicks(&self) -> u64 {
        self.inner.lock().unwrap().clicks
    }

    pub fn status(&self) -> Status {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap();

        while let Some(front) = inner.click_times.front() {
            if now.saturating_duration_since(*front) > Duration::from_secs(1) {
                inner.click_times.pop_front();
            } else {
                break;
            }
        }

        Status {
            enabled: inner.enabled,
            settings: inner.settings,
            actual_cps: if inner.enabled {
                inner.click_times.len() as f64
            } else {
                0.0
            },
            clicks: inner.clicks,
            hotkey: inner.hotkey.clone(),
            hotkey_error: inner.hotkey_error.clone(),
            last_error: inner.last_error.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_reject_out_of_range_values() {
        assert!(Settings::new(0.0, Button::Left, 10.0, 0.0).is_err());
        assert!(Settings::new(1001.0, Button::Left, 10.0, 0.0).is_err());
        assert!(Settings::new(50.0, Button::Left, 0.0, 0.0).is_err());
        assert!(Settings::new(50.0, Button::Left, 10.0, 101.0).is_err());
        assert!(Settings::new(f64::NAN, Button::Left, 10.0, 0.0).is_err());
    }

    #[test]
    fn toggle_and_status_are_consistent() {
        let shared = SharedState::new(Settings::default(), false);
        assert!(!shared.status().enabled);
        assert!(shared.toggle());
        assert!(shared.status().enabled);
        shared.off();
        assert!(!shared.status().enabled);
    }

    #[test]
    fn set_settings_does_not_toggle_clicking() {
        let shared = SharedState::new(Settings::default(), false);
        let settings = Settings::new(125.5, Button::Right, 5.25, 12.5).unwrap();

        shared.set_settings(settings);

        let status = shared.status();
        assert!(!status.enabled);
        assert_eq!(status.settings, settings);
    }

    #[test]
    fn status_wire_round_trips() {
        let status = Status {
            enabled: true,
            settings: Settings::new(123.456, Button::Middle, 7.25, 33.5).unwrap(),
            actual_cps: 99.9,
            clicks: 42,
            hotkey: "ctrl+alt+c".to_string(),
            hotkey_error: Some("hotkey is already in use".to_string()),
            last_error: Some("press failed: permission denied".to_string()),
        };

        let parsed = Status::from_wire(&status.to_wire()).unwrap();

        assert_eq!(parsed.enabled, status.enabled);
        assert_eq!(parsed.settings, status.settings);
        assert_eq!(parsed.actual_cps, status.actual_cps);
        assert_eq!(parsed.clicks, status.clicks);
        assert_eq!(parsed.hotkey, status.hotkey);
        assert_eq!(parsed.hotkey_error, status.hotkey_error);
        assert_eq!(parsed.last_error, status.last_error);
    }

    #[test]
    fn status_wire_reports_errors() {
        let error = Status::from_wire("ERR invalid cps\n").unwrap_err();
        assert!(error.to_string().contains("invalid cps"));
    }

    #[test]
    fn status_wire_requires_fields() {
        assert!(Status::from_wire("OK\nenabled: true\n").is_err());
    }
}
