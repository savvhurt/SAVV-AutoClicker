use std::collections::VecDeque;
use std::fmt;
use std::str::FromStr;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

pub const PROTOCOL: u32 = 2;
pub const MAX_LIMIT: u64 = 1_000_000_000;
pub const MAX_DELAY_MS: f64 = 10_000.0;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    Manual,
    Limit,
    Hold,
    Quit,
    Error,
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Limit => "limit",
            Self::Hold => "hold",
            Self::Quit => "quit",
            Self::Error => "error",
        }
    }

    fn from_wire(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "manual" => Some(Self::Manual),
            "limit" => Some(Self::Limit),
            "hold" => Some(Self::Hold),
            "quit" => Some(Self::Quit),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub cps: f64,
    pub button: Button,
    pub hold: Duration,
    pub jitter_pct: f64,
    pub limit: u64,
    pub delay: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            cps: 50.0,
            button: Button::Left,
            hold: Duration::from_millis(10),
            jitter_pct: 0.0,
            limit: 0,
            delay: Duration::ZERO,
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
            limit: 0,
            delay: Duration::ZERO,
        })
    }

    pub fn with_limit(mut self, limit: u64) -> Result<Self> {
        if limit > MAX_LIMIT {
            bail!("limit must be between 0 and {MAX_LIMIT} (0 means unlimited)");
        }
        self.limit = limit;
        Ok(self)
    }

    pub fn with_delay_ms(mut self, delay_ms: f64) -> Result<Self> {
        if !delay_ms.is_finite() || !(0.0..=MAX_DELAY_MS).contains(&delay_ms) {
            bail!("delay-ms must be between 0 and {MAX_DELAY_MS}");
        }
        self.delay = Duration::from_secs_f64(delay_ms / 1000.0);
        Ok(self)
    }

    pub fn hold_ms(&self) -> f64 {
        self.hold.as_secs_f64() * 1000.0
    }

    pub fn delay_ms(&self) -> f64 {
        self.delay.as_secs_f64() * 1000.0
    }

    pub fn to_wire_command(&self, command: &str) -> String {
        let mut wire = format!(
            "{command} {} {} {} {}",
            format_number(self.cps),
            self.button,
            format_number(self.hold_ms()),
            format_number(self.jitter_pct)
        );

        // Keep the legacy four-field command when the optional features are
        // unused so an older daemon can still handle basic `on`/`set` calls.
        if self.limit != 0 || !self.delay.is_zero() {
            wire.push_str(&format!(
                " {} {}",
                self.limit,
                format_number(self.delay_ms())
            ));
        }

        wire
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
    pub protocol: u32,
    pub enabled: bool,
    pub settings: Settings,
    pub actual_cps: f64,
    pub clicks: u64,
    pub session_clicks: u64,
    pub starting: bool,
    pub delay_left_ms: u64,
    pub stop_reason: Option<StopReason>,
    pub hotkey: String,
    pub hotkey_active: String,
    pub hold_hotkey: String,
    pub hold_hotkey_active: String,
    pub hold_active: bool,
    pub hotkey_paused: bool,
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
        let hotkey_active = if self.hotkey_active.is_empty() {
            String::from("none")
        } else {
            self.hotkey_active.replace(['\n', '\r'], " ")
        };
        let hold_hotkey = self.hold_hotkey.replace(['\n', '\r'], " ");
        let hold_hotkey_active = if self.hold_hotkey_active.is_empty() {
            String::from("none")
        } else {
            self.hold_hotkey_active.replace(['\n', '\r'], " ")
        };
        let hotkey_error = self
            .hotkey_error
            .as_deref()
            .unwrap_or("none")
            .replace(['\n', '\r'], " ");
        let stop_reason = self.stop_reason.map(StopReason::as_str).unwrap_or("none");

        format!(
            "OK\nprotocol: {}\nenabled: {}\ncps: {}\nbutton: {}\nhold_ms: {}\njitter_pct: {}\nlimit: {}\ndelay_ms: {}\nactual_cps: {:.1}\nclicks: {}\nsession_clicks: {}\nstarting: {}\ndelay_left_ms: {}\nstop_reason: {}\nhotkey: {}\nhotkey_active: {}\nhold_hotkey: {}\nhold_hotkey_active: {}\nhold_active: {}\nhotkey_paused: {}\nhotkey_error: {}\nerror: {}\n",
            self.protocol,
            self.enabled,
            format_number(self.settings.cps),
            self.settings.button,
            format_number(self.settings.hold_ms()),
            format_number(self.settings.jitter_pct),
            self.settings.limit,
            format_number(self.settings.delay_ms()),
            self.actual_cps,
            self.clicks,
            self.session_clicks,
            self.starting,
            self.delay_left_ms,
            stop_reason,
            hotkey,
            hotkey_active,
            hold_hotkey,
            hold_hotkey_active,
            self.hold_active,
            self.hotkey_paused,
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

        let mut protocol = None;
        let mut enabled = None;
        let mut cps = None;
        let mut button = None;
        let mut hold_ms = None;
        let mut jitter_pct = None;
        let mut limit = None;
        let mut delay_ms = None;
        let mut actual_cps = None;
        let mut clicks = None;
        let mut session_clicks = None;
        let mut starting = None;
        let mut delay_left_ms = None;
        let mut stop_reason = None;
        let mut hotkey = None;
        let mut hotkey_active = None;
        let mut hold_hotkey = None;
        let mut hold_hotkey_active = None;
        let mut hold_active = None;
        let mut hotkey_paused = None;
        let mut hotkey_error = None;
        let mut last_error = None;

        for line in body.lines() {
            let Some((key, value)) = line.split_once(':') else {
                bail!("malformed status line `{line}`");
            };
            let value = value.trim();

            match key.trim() {
                "protocol" => protocol = Some(value.parse::<u32>()?),
                "enabled" => enabled = Some(value.parse::<bool>()?),
                "cps" => cps = Some(value.parse::<f64>()?),
                "button" => button = Some(value.parse::<Button>()?),
                "hold_ms" => hold_ms = Some(value.parse::<f64>()?),
                "jitter_pct" => jitter_pct = Some(value.parse::<f64>()?),
                "limit" => limit = Some(value.parse::<u64>()?),
                "delay_ms" => delay_ms = Some(value.parse::<f64>()?),
                "actual_cps" => actual_cps = Some(value.parse::<f64>()?),
                "clicks" => clicks = Some(value.parse::<u64>()?),
                "session_clicks" => session_clicks = Some(value.parse::<u64>()?),
                "starting" => starting = Some(value.parse::<bool>()?),
                "delay_left_ms" => delay_left_ms = Some(value.parse::<u64>()?),
                "stop_reason" => {
                    stop_reason = if value.eq_ignore_ascii_case("none") {
                        None
                    } else {
                        StopReason::from_wire(value)
                    };
                }
                "hotkey" => hotkey = Some(value.to_string()),
                "hotkey_active" => {
                    hotkey_active =
                        Some(if value.is_empty() || value.eq_ignore_ascii_case("none") {
                            String::new()
                        } else {
                            value.to_string()
                        });
                }
                "hold_hotkey" => hold_hotkey = Some(value.to_string()),
                "hold_hotkey_active" => {
                    hold_hotkey_active =
                        Some(if value.is_empty() || value.eq_ignore_ascii_case("none") {
                            String::new()
                        } else {
                            value.to_string()
                        });
                }
                "hold_active" => hold_active = Some(value.parse::<bool>()?),
                "hotkey_paused" => hotkey_paused = Some(value.parse::<bool>()?),
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
        )?
        .with_limit(limit.unwrap_or(0))?
        .with_delay_ms(delay_ms.unwrap_or(0.0))?;

        Ok(Self {
            protocol: protocol.unwrap_or(0),
            enabled: enabled.ok_or_else(|| anyhow::anyhow!("status is missing `enabled`"))?,
            settings,
            actual_cps: actual_cps
                .ok_or_else(|| anyhow::anyhow!("status is missing `actual_cps`"))?,
            clicks: clicks.ok_or_else(|| anyhow::anyhow!("status is missing `clicks`"))?,
            session_clicks: session_clicks.unwrap_or(0),
            starting: starting.unwrap_or(false),
            delay_left_ms: delay_left_ms.unwrap_or(0),
            stop_reason,
            hotkey: hotkey.unwrap_or_default(),
            hotkey_active: hotkey_active.unwrap_or_default(),
            hold_hotkey: hold_hotkey.unwrap_or_default(),
            hold_hotkey_active: hold_hotkey_active.unwrap_or_default(),
            hold_active: hold_active.unwrap_or(false),
            hotkey_paused: hotkey_paused.unwrap_or(false),
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
    session_clicks: u64,
    click_times: VecDeque<Instant>,
    delay_until: Option<Instant>,
    stop_reason: Option<StopReason>,
    hold_active: bool,
    hotkey: String,
    hotkey_active: String,
    hold_hotkey: String,
    hold_hotkey_active: String,
    hotkey_paused: bool,
    hotkey_error: Option<String>,
    hotkey_reload: bool,
    hold_hotkey_reload: bool,
    last_error: Option<String>,
}

pub struct SharedState {
    inner: Mutex<Inner>,
    changed: Condvar,
}

impl SharedState {
    pub fn new(settings: Settings, enabled: bool) -> Self {
        let delay_until = if enabled && !settings.delay.is_zero() {
            Some(Instant::now() + settings.delay)
        } else {
            None
        };

        Self {
            inner: Mutex::new(Inner {
                enabled,
                quit: false,
                settings,
                version: 0,
                clicks: 0,
                session_clicks: 0,
                click_times: VecDeque::new(),
                delay_until,
                stop_reason: None,
                hold_active: false,
                hotkey: crate::hotkey::DEFAULT.to_string(),
                hotkey_active: String::new(),
                hold_hotkey: String::new(),
                hold_hotkey_active: String::new(),
                hotkey_paused: false,
                hotkey_error: None,
                hotkey_reload: false,
                hold_hotkey_reload: false,
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
            if inner.enabled {
                Self::stop_locked(&mut inner, StopReason::Manual);
                false
            } else {
                Self::begin_session_locked(&mut inner);
                true
            }
        };
        self.changed.notify_all();
        enabled
    }

    pub fn on(&self, settings: Settings) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.settings = settings;
            Self::begin_session_locked(&mut inner);
        }
        self.changed.notify_all();
    }

    pub fn set_settings(&self, settings: Settings) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.settings = settings;
            inner.version = inner.version.wrapping_add(1);
            inner.click_times.clear();

            if inner.enabled
                && inner.settings.limit > 0
                && inner.session_clicks >= inner.settings.limit
            {
                Self::stop_locked(&mut inner, StopReason::Limit);
            }
        }
        self.changed.notify_all();
    }

    pub fn off(&self) {
        {
            let mut inner = self.inner.lock().unwrap();
            Self::stop_locked(&mut inner, StopReason::Manual);
        }
        self.changed.notify_all();
    }

    pub fn stop(&self, reason: StopReason) {
        {
            let mut inner = self.inner.lock().unwrap();
            Self::stop_locked(&mut inner, reason);
        }
        self.changed.notify_all();
    }

    fn stop_locked(inner: &mut Inner, reason: StopReason) {
        inner.enabled = false;
        inner.delay_until = None;
        inner.stop_reason = Some(reason);
        inner.hold_active = false;
        inner.version = inner.version.wrapping_add(1);
        inner.click_times.clear();
    }

    fn begin_session_locked(inner: &mut Inner) {
        inner.enabled = true;
        inner.version = inner.version.wrapping_add(1);
        inner.session_clicks = 0;
        inner.click_times.clear();
        inner.delay_until = if inner.settings.delay.is_zero() {
            None
        } else {
            Some(Instant::now() + inner.settings.delay)
        };
        inner.stop_reason = None;
        inner.hold_active = false;
        inner.last_error = None;
    }

    pub fn quit(&self) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.quit = true;
            Self::stop_locked(&mut inner, StopReason::Quit);
        }
        self.changed.notify_all();
    }

    pub fn hold_press(&self) -> bool {
        let started = {
            let mut inner = self.inner.lock().unwrap();
            if inner.quit || inner.enabled {
                inner.hold_active = false;
                false
            } else {
                Self::begin_session_locked(&mut inner);
                inner.hold_active = true;
                true
            }
        };
        self.changed.notify_all();
        started
    }

    pub fn hold_release(&self) {
        {
            let mut inner = self.inner.lock().unwrap();
            if !inner.hold_active {
                return;
            }
            Self::stop_locked(&mut inner, StopReason::Hold);
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

    /// Wait until the start delay for the current session has elapsed. The
    /// delay survives settings changes, so moving a slider does not restart it.
    pub fn wait_for_start(&self, version: u64) -> bool {
        let mut inner = self.inner.lock().unwrap();
        loop {
            if !inner.enabled || inner.quit || inner.version != version {
                return false;
            }

            let Some(until) = inner.delay_until else {
                return true;
            };

            let now = Instant::now();
            if now >= until {
                inner.delay_until = None;
                return true;
            }

            let wait = (until - now).min(Duration::from_millis(50));
            let (guard, _) = self.changed.wait_timeout(inner, wait).unwrap();
            inner = guard;
        }
    }

    pub fn record_click(&self) -> bool {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap();
        inner.clicks = inner.clicks.saturating_add(1);
        inner.session_clicks = inner.session_clicks.saturating_add(1);
        inner.click_times.push_back(now);

        while let Some(front) = inner.click_times.front() {
            if now.saturating_duration_since(*front) > Duration::from_secs(1) {
                inner.click_times.pop_front();
            } else {
                break;
            }
        }

        inner.settings.limit > 0 && inner.session_clicks >= inner.settings.limit
    }

    pub fn set_error(&self, message: impl Into<String>) {
        self.inner.lock().unwrap().last_error = Some(message.into());
    }

    pub fn set_hotkey_requested(&self, value: impl Into<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.hotkey = value.into();
        inner.hotkey_error = None;
        inner.hotkey_reload = false;
    }

    pub fn hotkey_requested(&self) -> String {
        self.inner.lock().unwrap().hotkey.clone()
    }

    pub fn set_hotkey_active(&self, value: impl Into<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.hotkey_active = value.into();
        inner.hotkey_paused = false;
        inner.hotkey_error = None;
    }

    pub fn clear_hotkey_active(&self) {
        self.inner.lock().unwrap().hotkey_active.clear();
    }

    pub fn set_hold_hotkey_requested(&self, value: impl Into<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.hold_hotkey = value.into();
        inner.hotkey_error = None;
        inner.hold_hotkey_reload = false;
    }

    pub fn hold_hotkey_requested(&self) -> String {
        self.inner.lock().unwrap().hold_hotkey.clone()
    }

    pub fn set_hold_hotkey_active(&self, value: impl Into<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.hold_hotkey_active = value.into();
        inner.hotkey_paused = false;
        inner.hotkey_error = None;
    }

    pub fn clear_hold_hotkey_active(&self) {
        self.inner.lock().unwrap().hold_hotkey_active.clear();
    }

    pub fn set_hotkey_paused(&self, paused: bool) {
        self.inner.lock().unwrap().hotkey_paused = paused;
    }

    pub fn request_hotkey(&self, value: impl Into<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.hotkey = value.into();
        inner.hotkey_error = None;
        inner.hotkey_reload = true;
    }

    pub fn request_hold_hotkey(&self, value: impl Into<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.hold_hotkey = value.into();
        inner.hotkey_error = None;
        inner.hold_hotkey_reload = true;
    }

    #[cfg(windows)]
    pub(crate) fn take_hotkey_request(&self) -> Option<String> {
        let mut inner = self.inner.lock().unwrap();
        if inner.hotkey_reload {
            inner.hotkey_reload = false;
            Some(inner.hotkey.clone())
        } else {
            None
        }
    }

    #[cfg(windows)]
    pub(crate) fn take_hold_hotkey_request(&self) -> Option<String> {
        let mut inner = self.inner.lock().unwrap();
        if inner.hold_hotkey_reload {
            inner.hold_hotkey_reload = false;
            Some(inner.hold_hotkey.clone())
        } else {
            None
        }
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

        let starting = inner.enabled && inner.delay_until.is_some();
        let delay_left_ms = inner
            .delay_until
            .map(|until| until.saturating_duration_since(now).as_millis() as u64)
            .unwrap_or(0);

        Status {
            protocol: PROTOCOL,
            enabled: inner.enabled,
            settings: inner.settings,
            actual_cps: if inner.enabled && !starting {
                inner.click_times.len() as f64
            } else {
                0.0
            },
            clicks: inner.clicks,
            session_clicks: inner.session_clicks,
            starting,
            delay_left_ms,
            stop_reason: inner.stop_reason,
            hotkey: inner.hotkey.clone(),
            hotkey_active: inner.hotkey_active.clone(),
            hold_hotkey: inner.hold_hotkey.clone(),
            hold_hotkey_active: inner.hold_hotkey_active.clone(),
            hold_active: inner.hold_active,
            hotkey_paused: inner.hotkey_paused,
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
        assert!(Settings::default().with_limit(MAX_LIMIT + 1).is_err());
        assert!(
            Settings::default()
                .with_delay_ms(MAX_DELAY_MS + 1.0)
                .is_err()
        );
        assert!(Settings::default().with_delay_ms(f64::NAN).is_err());
    }

    #[test]
    fn optional_settings_extend_the_wire_command() {
        let base = Settings::new(125.0, Button::Left, 10.0, 0.0).unwrap();
        assert_eq!(base.to_wire_command("ON"), "ON 125 left 10 0");

        let extended = base.with_limit(500).unwrap().with_delay_ms(250.0).unwrap();
        assert_eq!(extended.to_wire_command("ON"), "ON 125 left 10 0 500 250");
    }

    #[test]
    fn toggle_and_status_are_consistent() {
        let shared = SharedState::new(Settings::default(), false);
        assert!(!shared.status().enabled);
        assert!(shared.toggle());
        assert!(shared.status().enabled);
        shared.off();
        assert!(!shared.status().enabled);
        assert_eq!(shared.status().stop_reason, Some(StopReason::Manual));
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
        let settings = Settings::new(123.456, Button::Middle, 7.25, 33.5)
            .unwrap()
            .with_limit(500)
            .unwrap()
            .with_delay_ms(250.0)
            .unwrap();
        let status = Status {
            protocol: PROTOCOL,
            enabled: true,
            settings,
            actual_cps: 99.9,
            clicks: 42,
            session_clicks: 17,
            starting: true,
            delay_left_ms: 123,
            stop_reason: Some(StopReason::Limit),
            hotkey: "ctrl+alt+c".to_string(),
            hotkey_active: "ctrl+shift+f8".to_string(),
            hold_hotkey: "f9".to_string(),
            hold_hotkey_active: "f9".to_string(),
            hold_active: true,
            hotkey_paused: false,
            hotkey_error: Some("hotkey is already in use".to_string()),
            last_error: Some("press failed: permission denied".to_string()),
        };

        let parsed = Status::from_wire(&status.to_wire()).unwrap();

        assert_eq!(parsed, status);
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

    #[test]
    fn status_wire_accepts_legacy_daemon_without_new_fields() {
        let parsed = Status::from_wire(
            "OK\nenabled: false\ncps: 50\nbutton: left\nhold_ms: 10\njitter_pct: 0\nactual_cps: 0.0\nclicks: 0\nerror: none\n",
        )
        .unwrap();

        assert_eq!(parsed.protocol, 0);
        assert_eq!(parsed.hotkey, "");
        assert_eq!(parsed.hotkey_active, "");
        assert_eq!(parsed.settings.limit, 0);
        assert_eq!(parsed.settings.delay, Duration::ZERO);
        assert_eq!(parsed.session_clicks, 0);
        assert!(!parsed.starting);
        assert!(!parsed.hotkey_paused);
        assert!(!parsed.hold_active);
    }

    #[test]
    fn requested_and_active_hotkeys_are_separate() {
        let shared = SharedState::new(Settings::default(), false);
        shared.set_hotkey_requested("ctrl+alt+c");
        shared.set_hotkey_active("ctrl+alt+c");

        shared.request_hotkey("ctrl+shift+f8");

        let status = shared.status();
        assert_eq!(status.hotkey, "ctrl+shift+f8");
        assert_eq!(status.hotkey_active, "ctrl+alt+c");
        assert!(status.hotkey_error.is_none());

        shared.set_hotkey_active("ctrl+shift+f8");
        assert_eq!(shared.status().hotkey_active, "ctrl+shift+f8");
    }

    #[test]
    fn hold_only_stops_clicks_it_started() {
        let shared = SharedState::new(Settings::default(), false);

        shared.hold_press();
        assert!(shared.status().enabled);
        assert!(shared.status().hold_active);

        shared.hold_release();
        assert!(!shared.status().enabled);
        assert_eq!(shared.status().stop_reason, Some(StopReason::Hold));

        assert!(shared.toggle());
        shared.hold_press();
        assert!(shared.status().enabled);
        assert!(!shared.status().hold_active);

        shared.hold_release();
        assert!(shared.status().enabled);
    }

    #[test]
    fn delay_and_limit_are_exposed_in_status() {
        let settings = Settings::default()
            .with_limit(10)
            .unwrap()
            .with_delay_ms(300.0)
            .unwrap();
        let shared = SharedState::new(settings, true);

        let status = shared.status();
        assert!(status.starting);
        assert!(status.delay_left_ms > 0);
        assert_eq!(status.session_clicks, 0);

        assert!(shared.wait_for_start(shared.snapshot().version));
        let status = shared.status();
        assert!(!status.starting);
        assert_eq!(status.delay_left_ms, 0);
    }
}
