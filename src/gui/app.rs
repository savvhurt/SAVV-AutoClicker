use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2,
};
use egui_software_backend::{SoftwareBackend, SoftwareBackendAppConfiguration};

use super::config::{self, MAX_UI_SCALE, MIN_UI_SCALE, ThemeMode, UI_SCALES};
use super::worker::{Command, Event, Worker};
use super::{theme, widgets};

use crate::i18n::{self, Key, Lang, Language};
use crate::state::{
    Button, ClickMode, MAX_DELAY_MS, MAX_LIMIT, MAX_POINT_INTERVAL_MS, MAX_POINTS, Point, Settings,
    Status, StopReason,
};

const APPLY_DEBOUNCE: Duration = Duration::from_millis(250);
const SYNC_GRACE: Duration = Duration::from_millis(800);
const TOGGLE_TIMEOUT: Duration = Duration::from_secs(3);
const CONTENT_WIDTH: f32 = 424.0;
const WINDOW_SIZE: [f32; 2] = [460.0, 560.0];
const MIN_WINDOW_SIZE: [f32; 2] = [420.0, 520.0];
const LABEL_WIDTH: f32 = 90.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Tab {
    Clicks,
    Mode,
    Hotkeys,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
enum RecordTarget {
    Toggle,
    Hold,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipetteTarget {
    Point,
    Sequence(usize),
    NewSequence,
}

struct Pipette {
    target: PipetteTarget,
    receiver: Receiver<std::result::Result<Option<Point>, String>>,
}

struct SliderSpec<'a> {
    range: std::ops::RangeInclusive<f64>,
    logarithmic: bool,
    step: f64,
    decimals: usize,
    suffix: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DraftSettings {
    cps: f64,
    button: Button,
    hold_ms: f64,
    jitter_pct: f64,
    limit: u64,
    delay_ms: f64,
    multi_click: u8,
    mode: ClickMode,
    target: Option<Point>,
    points: [Option<Point>; MAX_POINTS],
    point_interval_ms: f64,
}

impl From<Settings> for DraftSettings {
    fn from(settings: Settings) -> Self {
        Self {
            cps: settings.cps,
            button: settings.button,
            hold_ms: settings.hold_ms(),
            jitter_pct: settings.jitter_pct,
            limit: settings.limit,
            delay_ms: settings.delay_ms(),
            multi_click: settings.multi_click,
            mode: settings.mode,
            target: settings.target,
            points: settings.points,
            point_interval_ms: settings.point_interval_ms(),
        }
    }
}

impl DraftSettings {
    fn points_vec(&self) -> Vec<Point> {
        self.points.iter().flatten().copied().collect()
    }

    fn point_count(&self) -> usize {
        self.points.iter().flatten().count()
    }

    fn point_at(&self, index: usize) -> Option<Point> {
        self.points.iter().flatten().nth(index).copied()
    }

    fn set_point(&mut self, index: usize, point: Point) {
        for (current, existing) in self.points.iter_mut().flatten().enumerate() {
            if current == index {
                *existing = point;
                return;
            }
        }
    }

    fn add_point(&mut self, point: Point) -> Option<usize> {
        let slot = self.points.iter_mut().find(|slot| slot.is_none())?;
        *slot = Some(point);
        Some(self.point_count() - 1)
    }

    fn remove_point(&mut self, index: usize) {
        let mut points = self.points_vec();
        if index < points.len() {
            points.remove(index);
        }
        self.points = [None; MAX_POINTS];
        for (slot, point) in self.points.iter_mut().zip(points) {
            *slot = Some(point);
        }
    }

    fn to_settings(self) -> Result<Settings, String> {
        let points = self.points_vec();
        let interval = self
            .point_interval_ms
            .round()
            .clamp(1.0, MAX_POINT_INTERVAL_MS as f64) as u32;

        Settings::new(self.cps, self.button, self.hold_ms, self.jitter_pct)
            .and_then(|settings| settings.with_limit(self.limit))
            .and_then(|settings| settings.with_delay_ms(self.delay_ms))
            .and_then(|settings| settings.with_multi_click(self.multi_click))
            .map(|settings| settings.with_mode(self.mode))
            .and_then(|settings| settings.with_target(self.target))
            .and_then(|settings| settings.with_points(&points))
            .and_then(|settings| settings.with_point_interval_ms(interval))
            .map_err(|error| format!("{error:#}"))
    }
}

pub struct GuiApp {
    settings: DraftSettings,
    tab: Tab,
    hotkey: String,
    hold_hotkey: String,
    recording: Option<RecordTarget>,
    theme_mode: ThemeMode,
    language: Language,
    lang: Lang,
    ui_scale: f32,
    applied_scale: Option<f32>,
    minimize_to_tray: bool,
    #[cfg_attr(not(windows), allow(dead_code))]
    start_hidden: bool,
    #[cfg(windows)]
    elevated: bool,
    #[cfg(windows)]
    tray: Option<super::tray::Tray>,
    pipette: Option<Pipette>,
    confirm_defaults: bool,
    autostart_enabled: bool,
    online: bool,
    enabled: bool,
    actual_cps: f64,
    clicks: u64,
    session_clicks: u64,
    starting: bool,
    delay_left_ms: u64,
    stop_reason: Option<StopReason>,
    hold_active: bool,
    daemon_error: Option<String>,
    daemon_hotkey_active: String,
    daemon_hold_hotkey_active: String,
    daemon_hotkey_paused: bool,
    daemon_hotkey_error: Option<String>,
    daemon_settings: Option<Settings>,
    daemon_protocol: u32,
    #[cfg_attr(not(windows), allow(dead_code))]
    legacy_restart_attempted: bool,
    #[cfg_attr(not(windows), allow(dead_code))]
    legacy_restart_pending: bool,
    #[cfg_attr(not(windows), allow(dead_code))]
    last_hold_hotkey_sync: Option<Instant>,
    last_hotkey_sync: Option<Instant>,
    notice: Option<String>,
    offline_reason: Option<String>,
    initialized: bool,
    draft_changed_at: Option<Instant>,
    last_apply_sent: Option<Instant>,
    pending_toggle: Option<(bool, Instant)>,
    #[cfg(not(windows))]
    confirm_quit: bool,
    commands: Sender<Command>,
    events: Receiver<Event>,
    worker: Option<thread::JoinHandle<()>>,
}

impl GuiApp {
    fn new(ctx: egui::Context, start_hidden: bool) -> Self {
        let loaded = config::load();
        let config = loaded.config;
        let lang = config.language.resolve();
        theme::install(&ctx, config.theme);
        let worker = Worker::spawn(
            ctx.clone(),
            config.hotkey.clone(),
            config.hold_hotkey.clone(),
            config.language,
        )
        .expect("failed to start the SAVV-AutoClicker GUI IPC worker");
        let autostart_enabled = crate::autostart::is_enabled();

        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut app = Self {
            settings: config.settings.into(),
            tab: Tab::Clicks,
            hotkey: config.hotkey,
            hold_hotkey: config.hold_hotkey,
            recording: None,
            theme_mode: config.theme,
            language: config.language,
            lang,
            ui_scale: config.ui_scale,
            applied_scale: None,
            minimize_to_tray: config.minimize_to_tray,
            start_hidden,
            #[cfg(windows)]
            elevated: crate::platform::is_elevated(),
            #[cfg(windows)]
            tray: None,
            pipette: None,
            confirm_defaults: false,
            autostart_enabled,
            online: false,
            enabled: false,
            actual_cps: 0.0,
            clicks: 0,
            session_clicks: 0,
            starting: false,
            delay_left_ms: 0,
            stop_reason: None,
            hold_active: false,
            daemon_error: None,
            daemon_hotkey_active: String::new(),
            daemon_hold_hotkey_active: String::new(),
            daemon_hotkey_paused: false,
            daemon_hotkey_error: None,
            daemon_settings: None,
            daemon_protocol: 0,
            legacy_restart_attempted: false,
            legacy_restart_pending: false,
            last_hold_hotkey_sync: None,
            last_hotkey_sync: None,
            notice: loaded.warning,
            offline_reason: None,
            initialized: false,
            draft_changed_at: None,
            last_apply_sent: None,
            pending_toggle: None,
            #[cfg(not(windows))]
            confirm_quit: false,
            commands: worker.commands,
            events: worker.events,
            worker: worker.handle,
        };

        #[cfg(windows)]
        {
            // Start the icon before the first frame: an autostart launch can be
            // hidden, and a hidden winit window does not necessarily receive a
            // natural redraw that would otherwise create the icon.
            app.sync_tray(&ctx);
            if app.start_hidden {
                ctx.request_repaint();
            }

            if !app.elevated {
                let warning = app.t(Key::ElevationWarn).to_string();
                app.notice = Some(match app.notice.take() {
                    Some(existing) => format!("{existing}\n{warning}"),
                    None => warning,
                });
            }
        }

        app
    }

    fn t(&self, key: Key) -> &'static str {
        i18n::tr(self.lang, key)
    }

    /// Fill `{}`/`{:.1}` placeholders in a translated template in order.
    fn fill(&self, key: Key, args: &[String]) -> String {
        let mut result = String::new();
        let mut rest = self.t(key);

        for argument in args {
            let Some(start) = rest.find('{') else {
                break;
            };
            let Some(end) = rest[start..].find('}') else {
                break;
            };
            result.push_str(&rest[..start]);
            result.push_str(argument);
            rest = &rest[start + end + 1..];
        }

        result.push_str(rest);
        result
    }

    fn clear_daemon_state(&mut self) {
        self.online = false;
        self.enabled = false;
        self.actual_cps = 0.0;
        self.starting = false;
        self.delay_left_ms = 0;
        self.hold_active = false;
        self.stop_reason = None;
        self.daemon_error = None;
        self.daemon_settings = None;
        self.daemon_protocol = 0;
        self.daemon_hotkey_active.clear();
        self.daemon_hold_hotkey_active.clear();
        self.daemon_hotkey_paused = false;
        self.daemon_hotkey_error = None;
        self.last_hotkey_sync = None;
        self.last_hold_hotkey_sync = None;
        self.initialized = false;
        self.last_apply_sent = None;
    }

    fn drain_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Status(status) => self.apply_status(*status),
                Event::Offline(reason) => {
                    self.clear_daemon_state();
                    self.offline_reason = Some(reason);
                    self.pending_toggle = None;
                }
                Event::Error(error) => {
                    self.notice = Some(error);
                    self.pending_toggle = None;
                }
                #[cfg(not(windows))]
                Event::Stopped => {
                    self.clear_daemon_state();
                    self.offline_reason = None;
                    self.pending_toggle = None;
                    self.notice = Some(self.t(Key::DaemonStoppedNotice).to_string());
                }
            }
        }

        if self
            .pending_toggle
            .is_some_and(|(_, sent_at)| sent_at.elapsed() > TOGGLE_TIMEOUT)
        {
            self.pending_toggle = None;
            self.notice = Some(self.t(Key::ToggleTimeout).to_string());
        }

        if self.pending_toggle.is_some() || self.starting {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn apply_status(&mut self, status: Status) {
        let first_status = !self.initialized;
        let draft_was_dirty = self.draft_changed_at.is_some();

        self.online = true;
        self.initialized = true;
        self.offline_reason = None;
        self.enabled = status.enabled;
        self.actual_cps = status.actual_cps;
        self.clicks = status.clicks;
        self.session_clicks = status.session_clicks;
        self.starting = status.starting;
        self.delay_left_ms = status.delay_left_ms;
        self.stop_reason = status.stop_reason;
        self.hold_active = status.hold_active;
        self.daemon_error = status.last_error.clone();
        self.daemon_hotkey_active = status.hotkey_active.clone();
        self.daemon_hold_hotkey_active = status.hold_hotkey_active.clone();
        self.daemon_hotkey_paused = status.hotkey_paused;
        self.daemon_hotkey_error = status.hotkey_error.clone();
        self.daemon_settings = Some(status.settings);
        self.daemon_protocol = status.protocol;

        if status.protocol < crate::state::PROTOCOL && !self.legacy_restart_attempted {
            // Daemons before protocol 2 reject the new click limit and start
            // delay. Restart them once, preserving a running session.
            self.legacy_restart_attempted = true;
            self.legacy_restart_pending = true;
            self.initialized = false;
            self.daemon_settings = None;
            self.daemon_hotkey_error = None;
            let resume = status.enabled.then_some(status.settings);
            self.notice = Some(self.t(Key::LegacyRestart).to_string());
            let _ = self.commands.send(Command::RestartDaemon {
                hotkey: self.hotkey.clone(),
                hold_hotkey: self.hold_hotkey.clone(),
                resume,
            });
            return;
        }

        if self.legacy_restart_pending && status.protocol >= crate::state::PROTOCOL {
            self.legacy_restart_pending = false;
            self.notice = None;
        }

        if status.hotkey == self.hotkey {
            self.last_hotkey_sync = None;
        } else if self
            .last_hotkey_sync
            .is_none_or(|sent_at| sent_at.elapsed() >= SYNC_GRACE)
        {
            let _ = self.commands.send(Command::SetHotkey(self.hotkey.clone()));
            self.last_hotkey_sync = Some(Instant::now());
        }

        #[cfg(windows)]
        {
            if status.hold_hotkey == self.hold_hotkey {
                self.last_hold_hotkey_sync = None;
            } else if self
                .last_hold_hotkey_sync
                .is_none_or(|sent_at| sent_at.elapsed() >= SYNC_GRACE)
            {
                let _ = self
                    .commands
                    .send(Command::SetHoldHotkey(self.hold_hotkey.clone()));
                self.last_hold_hotkey_sync = Some(Instant::now());
            }
        }

        if first_status {
            if status.enabled {
                if !draft_was_dirty {
                    self.settings = status.settings.into();
                }
            } else if let Ok(current) = self.settings.to_settings() {
                if !settings_equivalent(&current, &status.settings) {
                    let _ = self.commands.send(Command::Apply(current));
                    self.last_apply_sent = Some(Instant::now());
                }
            }
        } else if self.draft_changed_at.is_none()
            && self
                .last_apply_sent
                .is_none_or(|sent_at| sent_at.elapsed() >= SYNC_GRACE)
        {
            self.settings = status.settings.into();
        }

        if let Ok(current) = self.settings.to_settings() {
            if settings_equivalent(&current, &status.settings) {
                self.last_apply_sent = None;
                if self.draft_changed_at.take().is_some() {
                    self.save_config();
                }
            }
        }

        if self
            .pending_toggle
            .is_some_and(|(expected, _)| expected == status.enabled)
        {
            self.pending_toggle = None;
        }
    }

    fn maybe_apply_settings(&mut self) {
        let Some(changed_at) = self.draft_changed_at else {
            return;
        };
        if changed_at.elapsed() < APPLY_DEBOUNCE {
            return;
        }
        self.flush_settings();
    }

    fn flush_settings(&mut self) {
        let settings = match self.settings.to_settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.draft_changed_at = None;
                self.notice = Some(error);
                return;
            }
        };

        self.draft_changed_at = None;

        if self.online {
            let already_current = self
                .daemon_settings
                .is_some_and(|current| settings_equivalent(&current, &settings));
            if !already_current {
                let _ = self.commands.send(Command::Apply(settings));
                self.last_apply_sent = Some(Instant::now());
            }
        }

        self.save_config();
    }

    fn config_snapshot(&self) -> Option<config::Config> {
        self.settings
            .to_settings()
            .ok()
            .map(|settings| config::Config {
                settings,
                hotkey: self.hotkey.clone(),
                hold_hotkey: self.hold_hotkey.clone(),
                theme: self.theme_mode,
                language: self.language,
                ui_scale: self.ui_scale,
                minimize_to_tray: self.minimize_to_tray,
            })
    }

    fn save_config(&mut self) {
        let Some(config) = self.config_snapshot() else {
            return;
        };
        if let Err(error) = config::save(&config) {
            self.notice = Some(self.fill(Key::SaveFailed, &[format!("{error:#}")]));
        }
    }

    fn mark_dirty(&mut self) {
        self.draft_changed_at = Some(Instant::now());
        self.notice = None;
    }

    fn toggle_theme(&mut self, ctx: &egui::Context) {
        self.theme_mode = self.theme_mode.toggled();
        theme::set_mode(ctx, self.theme_mode);
        self.save_config();
        let label = match self.theme_mode {
            ThemeMode::Light => self.t(Key::ThemeLight),
            ThemeMode::Dark => self.t(Key::ThemeDark),
        };
        self.notice = Some(self.fill(Key::ThemeNotice, &[label.to_string()]));
    }

    fn set_language(&mut self, language: Language) {
        self.language = language;
        self.lang = language.resolve();
        #[cfg(windows)]
        super::tray::set_language(self.lang);
        let _ = self.commands.send(Command::SetLanguage(language));
        self.save_config();
    }

    fn apply_ui_scale(&mut self, ctx: &egui::Context) {
        let scale = self.ui_scale.clamp(MIN_UI_SCALE, MAX_UI_SCALE);
        self.ui_scale = scale;
        ctx.set_zoom_factor(scale);

        // The software backend sizes the window in logical points. Scale the
        // outer window as well so the layout keeps its designed size.
        if self.applied_scale != Some(scale) {
            self.applied_scale = Some(scale);
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(Vec2::new(
                MIN_WINDOW_SIZE[0] * scale,
                MIN_WINDOW_SIZE[1] * scale,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
                WINDOW_SIZE[0] * scale,
                WINDOW_SIZE[1] * scale,
            )));
        }
    }

    fn start_pipette(&mut self, target: PipetteTarget) {
        if self.pipette.is_some() {
            return;
        }

        let (sender, receiver) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("savv-autoclicker-pipette".to_string())
            .spawn(move || {
                let result = crate::backend::capture_point().map_err(|error| format!("{error:#}"));
                let _ = sender.send(result);
            });

        match spawned {
            Ok(_) => {
                self.pipette = Some(Pipette { target, receiver });
                self.notice = Some(self.t(Key::PickWaiting).to_string());
            }
            Err(error) => {
                self.notice = Some(self.fill(Key::PickFailed, &[format!("{error:#}")]));
            }
        }
    }

    fn poll_pipette(&mut self, ctx: &egui::Context) {
        let (target, result) = match self.pipette.as_ref() {
            Some(pipette) => (pipette.target, pipette.receiver.try_recv()),
            None => return,
        };

        match result {
            Ok(Ok(Some(point))) => {
                self.pipette = None;
                match target {
                    PipetteTarget::Point => {
                        self.settings.mode = ClickMode::Point;
                        self.settings.target = Some(point);
                    }
                    PipetteTarget::Sequence(index) => {
                        self.settings.set_point(index, point);
                    }
                    PipetteTarget::NewSequence => {
                        if self.settings.add_point(point).is_none() {
                            self.notice = Some(self.t(Key::PointsFull).to_string());
                        }
                    }
                }
                self.mark_dirty();
                self.notice = Some(self.fill(
                    Key::CurrentPicked,
                    &[point.x.to_string(), point.y.to_string()],
                ));
            }
            Ok(Ok(None)) => {
                self.pipette = None;
                self.notice = Some(self.t(Key::PickCancelled).to_string());
            }
            Ok(Err(error)) => {
                self.pipette = None;
                self.notice = Some(self.fill(Key::PickFailed, &[error]));
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pipette = None;
                self.notice = Some(self.t(Key::PickCancelled).to_string());
            }
        }
    }

    fn add_sequence_point(&mut self) {
        if self.settings.point_count() >= MAX_POINTS {
            self.notice = Some(self.t(Key::PointsFull).to_string());
            return;
        }

        self.start_pipette(PipetteTarget::NewSequence);
    }

    fn restore_defaults(&mut self, ctx: &egui::Context) {
        let defaults = Settings::default();
        self.settings = defaults.into();
        self.pipette = None;
        self.recording = None;
        self.hotkey = crate::hotkey::DEFAULT.to_string();
        self.hold_hotkey.clear();
        self.theme_mode = ThemeMode::Light;
        theme::set_mode(ctx, self.theme_mode);
        self.language = Language::Auto;
        self.lang = self.language.resolve();
        self.ui_scale = 1.0;
        self.apply_ui_scale(ctx);
        self.daemon_hotkey_error = None;
        self.last_hotkey_sync = Some(Instant::now());
        self.last_hold_hotkey_sync = Some(Instant::now());
        let _ = self.commands.send(Command::SetLanguage(self.language));
        let _ = self.commands.send(Command::SetHotkey(self.hotkey.clone()));
        let _ = self.commands.send(Command::SetHoldHotkey(String::new()));
        #[cfg(windows)]
        super::tray::set_language(self.lang);
        self.mark_dirty();
        self.flush_settings();
        self.notice = Some(self.t(Key::DefaultsDone).to_string());
    }

    #[cfg(windows)]
    fn sync_tray(&mut self, ctx: &egui::Context) {
        if self.minimize_to_tray {
            if self.tray.is_none() {
                match super::tray::Tray::start(ctx.clone(), self.lang) {
                    Ok(tray) => self.tray = Some(tray),
                    Err(error) => {
                        self.minimize_to_tray = false;
                        self.notice = Some(self.fill(Key::TrayFailed, &[format!("{error:#}")]));
                        self.save_config();
                    }
                }
            }
        } else {
            self.tray = None;
        }
    }

    #[cfg(windows)]
    fn handle_minimize(&mut self, ctx: &egui::Context) {
        let minimized = ctx.input(|input| input.viewport().minimized.unwrap_or(false));
        if minimized && self.minimize_to_tray && self.tray.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        }
    }

    #[cfg(windows)]
    fn start_recording(&mut self, target: RecordTarget) {
        self.recording = Some(target);
        self.notice = None;
        let _ = self.commands.send(Command::PauseHotkey);
    }

    fn stop_recording(&mut self, resume: bool) {
        if self.recording.take().is_none() {
            return;
        }
        if resume {
            #[cfg(windows)]
            let _ = self.commands.send(Command::ResumeHotkey);
        }
    }

    fn capture_hotkey(&mut self, ctx: &egui::Context) {
        let Some(target) = self.recording else {
            return;
        };

        if !ctx.input(|input| input.focused) {
            self.stop_recording(true);
            return;
        }

        let mut pressed = None;
        ctx.input(|input| {
            for event in &input.events {
                if let egui::Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } = event
                {
                    pressed = Some((
                        physical_key.unwrap_or(*key),
                        modifiers.ctrl,
                        modifiers.alt,
                        modifiers.shift,
                    ));
                    break;
                }
            }
        });

        let Some((key, ctrl, alt, shift)) = pressed else {
            return;
        };

        let mut combo = String::new();
        for (enabled, name) in [(ctrl, "ctrl"), (alt, "alt"), (shift, "shift")] {
            if enabled {
                if !combo.is_empty() {
                    combo.push('+');
                }
                combo.push_str(name);
            }
        }
        if !combo.is_empty() {
            combo.push('+');
        }
        combo.push_str(&key.name().to_ascii_lowercase());

        self.apply_binding(target, &combo);
    }

    fn apply_binding(&mut self, target: RecordTarget, combo: &str) {
        let hotkey = match crate::hotkey::Hotkey::parse(combo) {
            Ok(hotkey) => hotkey,
            Err(error) => {
                self.stop_recording(true);
                self.notice = Some(self.fill(Key::BindingFailed, &[format!("{error:#}")]));
                return;
            }
        };
        let combo = hotkey.to_string();

        match target {
            RecordTarget::Toggle => {
                if !self.hold_hotkey.is_empty() && combo == self.hold_hotkey {
                    self.stop_recording(true);
                    self.notice = Some(self.t(Key::KeyAlreadyHold).to_string());
                    return;
                }
                self.stop_recording(true);
                if combo == self.hotkey && self.daemon_hotkey_error.is_none() {
                    return;
                }
                self.hotkey = combo;
                self.daemon_hotkey_error = None;
                self.last_hotkey_sync = Some(Instant::now());
                let _ = self.commands.send(Command::SetHotkey(self.hotkey.clone()));
            }
            RecordTarget::Hold => {
                if combo == self.hotkey {
                    self.stop_recording(true);
                    self.notice = Some(self.t(Key::KeyAlreadyToggle).to_string());
                    return;
                }
                self.stop_recording(true);
                if combo == self.hold_hotkey && self.daemon_hotkey_error.is_none() {
                    return;
                }
                self.hold_hotkey = combo;
                self.daemon_hotkey_error = None;
                self.last_hold_hotkey_sync = Some(Instant::now());
                let _ = self
                    .commands
                    .send(Command::SetHoldHotkey(self.hold_hotkey.clone()));
            }
        }

        let bound = match target {
            RecordTarget::Toggle => self.hotkey.as_str(),
            RecordTarget::Hold => self.hold_hotkey.as_str(),
        };
        let label = crate::hotkey::Hotkey::parse(bound)
            .map(|hotkey| hotkey.label())
            .unwrap_or_else(|_| bound.to_ascii_uppercase());
        self.notice = Some(
            self.fill(
                Key::BoundNotice,
                &[
                    match target {
                        RecordTarget::Toggle => self.t(Key::RecordTargetToggle),
                        RecordTarget::Hold => self.t(Key::RecordTargetHold),
                    }
                    .to_string(),
                    label,
                ],
            ),
        );
        self.save_config();
    }

    #[cfg(windows)]
    fn clear_hold_hotkey(&mut self) {
        self.hold_hotkey.clear();
        self.daemon_hotkey_error = None;
        self.last_hold_hotkey_sync = Some(Instant::now());
        let _ = self.commands.send(Command::SetHoldHotkey(String::new()));
        self.notice = Some(self.t(Key::HoldDisabled).to_string());
        self.save_config();
    }

    fn request_toggle(&mut self, start: bool) {
        if start {
            match self.settings.to_settings() {
                Ok(settings) => {
                    self.notice = None;
                    self.pending_toggle = Some((true, Instant::now()));
                    let _ = self.commands.send(Command::Start(settings));
                }
                Err(error) => self.notice = Some(error),
            }
        } else {
            self.notice = None;
            self.pending_toggle = Some((false, Instant::now()));
            let _ = self.commands.send(Command::Stop);
        }
    }

    #[cfg(not(windows))]
    fn request_quit(&mut self) {
        self.pending_toggle = None;
        self.notice = Some(self.t(Key::Quitting).to_string());
        let _ = self.commands.send(Command::QuitDaemon);
    }

    fn status_palette(&self) -> (&'static str, Color32, bool) {
        if let Some((start, _)) = self.pending_toggle {
            return (
                if start {
                    self.t(Key::StatusStarting)
                } else {
                    self.t(Key::StatusStopping)
                },
                theme::accent(),
                true,
            );
        }
        if !self.online {
            (self.t(Key::StatusOffline), theme::dim(), false)
        } else if self.starting {
            (self.t(Key::StatusDelay), theme::warning(), true)
        } else if self.enabled {
            (self.t(Key::StatusActive), theme::accent(), true)
        } else {
            (self.t(Key::StatusIdle), theme::warning(), false)
        }
    }

    fn draw_topbar(&mut self, ui: &mut Ui) {
        let mut toggle_theme = false;

        ui.horizontal(|ui| {
            let (mark, _) = ui.allocate_exact_size(Vec2::splat(7.0), Sense::hover());
            ui.painter()
                .rect_filled(mark, CornerRadius::same(2), theme::accent());

            ui.label(
                RichText::new("SAVV-AutoClicker")
                    .font(theme::sans_semibold(16.0))
                    .color(theme::text()),
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (label, color, pulse) = self.status_palette();
                let response = widgets::status_badge(ui, label, color, pulse);
                if let Some(reason) = &self.offline_reason {
                    let _ = response.on_hover_text(reason);
                }

                let theme_label = if self.theme_mode == ThemeMode::Dark {
                    self.t(Key::ThemeLight)
                } else {
                    self.t(Key::ThemeDark)
                };
                if widgets::theme_toggle(ui, self.theme_mode == ThemeMode::Dark, theme_label)
                    .on_hover_text(self.t(Key::ToggleTheme))
                    .clicked()
                {
                    toggle_theme = true;
                }
            });
        });

        if toggle_theme {
            self.toggle_theme(ui.ctx());
        }
    }

    fn draw_hero(&mut self, ui: &mut Ui) {
        let active = self.online && self.enabled;
        let pending = self.pending_toggle.map(|(start, _)| start);

        let (title, description) = if let Some(start) = pending {
            (
                if start {
                    self.t(Key::HeroStarting)
                } else {
                    self.t(Key::HeroStopping)
                }
                .to_string(),
                self.t(Key::HeroWaitDaemon).to_string(),
            )
        } else if self.starting {
            (
                self.fill(
                    Key::HeroDelayTitle,
                    &[format!("{:.1}", self.delay_left_ms as f64 / 1000.0)],
                ),
                self.t(Key::HeroDelayText).to_string(),
            )
        } else if self.hold_active {
            (
                self.t(Key::HeroHoldTitle).to_string(),
                self.t(Key::HeroHoldText).to_string(),
            )
        } else if active {
            let description = if self.settings.limit > 0 {
                self.fill(
                    Key::HeroSessionProgress,
                    &[
                        group_digits(self.session_clicks),
                        group_digits(self.settings.limit),
                    ],
                )
            } else if self.settings.mode == ClickMode::Sequence {
                self.fill(
                    Key::HeroSequenceClicks,
                    &[self.settings.point_count().to_string()],
                )
            } else if self.settings.target.is_some() {
                self.t(Key::HeroTargetClicks).to_string()
            } else {
                self.t(Key::HeroCursorClicks).to_string()
            };
            (self.t(Key::HeroClicking).to_string(), description)
        } else if self.stop_reason == Some(StopReason::Limit) {
            (
                self.t(Key::HeroLimitDone).to_string(),
                self.fill(Key::HeroLimitText, &[group_digits(self.session_clicks)]),
            )
        } else {
            (
                self.t(Key::HeroStopped).to_string(),
                if self.online {
                    self.t(Key::HeroPressStart)
                } else {
                    self.t(Key::HeroDaemonAuto)
                }
                .to_string(),
            )
        };

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let response = widgets::power_button(ui, active, pending.is_some(), 68.0);
            if response.clicked() && pending.is_none() {
                self.request_toggle(!active);
            }

            ui.add_space(12.0);
            let width = ui.available_width();
            ui.allocate_ui_with_layout(
                Vec2::new(width, 68.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(title)
                            .font(theme::sans_semibold(20.0))
                            .color(theme::text()),
                    );

                    ui.add_space(1.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(description)
                                .font(theme::sans(10.5))
                                .color(theme::muted()),
                        )
                        .wrap(),
                    );
                },
            );
        });
        ui.add_space(2.0);
    }

    fn draw_metrics(&self, ui: &mut Ui) {
        theme::rule(ui);
        ui.add_space(2.0);
        ui.columns(3, |columns| {
            metric(
                &mut columns[0],
                self.t(Key::MetricCps),
                &format!("{:.1}", self.actual_cps),
                "CPS",
            );
            metric(
                &mut columns[1],
                self.t(Key::MetricClicks),
                &group_digits(self.clicks),
                self.t(Key::UnitPieces),
            );
            let session = group_digits(self.session_clicks);
            let limit = if self.settings.limit > 0 {
                format!("/ {}", group_digits(self.settings.limit))
            } else {
                self.t(Key::UnitPieces).to_string()
            };
            metric(
                &mut columns[2],
                self.t(Key::MetricSession),
                &session,
                &limit,
            );
        });
        ui.add_space(2.0);
        theme::rule(ui);
    }

    fn notice_data(&self) -> (Color32, &'static str, String) {
        if let Some(error) = &self.daemon_error {
            return (theme::danger(), self.t(Key::NoticeError), error.clone());
        }
        if let Some(notice) = &self.notice {
            return (theme::warning(), self.t(Key::NoticeSystem), notice.clone());
        }
        if let Some(error) = &self.daemon_hotkey_error {
            return (theme::danger(), self.t(Key::NoticeHotkey), error.clone());
        }
        if self.starting {
            return (
                theme::warning(),
                self.t(Key::NoticeStart),
                self.fill(
                    Key::NoticeStartsIn,
                    &[format!("{:.1}", self.delay_left_ms as f64 / 1000.0)],
                ),
            );
        }
        if self.stop_reason == Some(StopReason::Limit) && !self.enabled {
            return (
                theme::warning(),
                self.t(Key::NoticeLimit),
                self.fill(Key::NoticeLimitDone, &[group_digits(self.session_clicks)]),
            );
        }
        if !self.online {
            return (
                theme::dim(),
                self.t(Key::NoticeDaemon),
                self.t(Key::NoticeDaemonDown).to_string(),
            );
        }
        (
            theme::dim(),
            self.t(Key::NoticeReady),
            self.t(Key::NoticeAutoApply).to_string(),
        )
    }

    fn draw_notice(&self, ui: &mut Ui) {
        let (tone, title, text) = self.notice_data();
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), 20.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                widgets::notice_line(ui, tone, title, &text);
            },
        );
    }

    fn draw_tabs(&mut self, ui: &mut Ui) {
        let mut tab = self.tab;
        let changed = widgets::segmented(
            ui,
            "main_tabs",
            &mut tab,
            &[
                (Tab::Clicks, self.t(Key::TabClicks)),
                (Tab::Mode, self.t(Key::TabMode)),
                (Tab::Hotkeys, self.t(Key::TabHotkeys)),
                (Tab::System, self.t(Key::TabSystem)),
            ],
        );
        if changed {
            self.tab = tab;
            self.notice = None;
        }
    }

    fn draw_clicks_tab(&mut self, ui: &mut Ui) {
        // Six parameter rows plus the hint must fit the 420x520 minimum
        // without scrolling, so use a tighter vertical rhythm there.
        ui.spacing_mut().item_spacing.y = if ui.ctx().content_rect().height() < 540.0 {
            1.0
        } else {
            3.0
        };
        let mut draft = self.settings;
        let mut changed = false;

        if draft.mode == ClickMode::Sequence {
            ui.add_space(2.0);
            ui.add(
                egui::Label::new(
                    RichText::new(self.t(Key::SequenceSpeedHint))
                        .font(theme::sans(9.5))
                        .color(theme::dim()),
                )
                .wrap(),
            );
        } else {
            changed |= compact_slider_row(
                ui,
                self.t(Key::RowSpeed),
                self.t(Key::HintSpeed),
                &mut draft.cps,
                SliderSpec {
                    range: 0.2..=1000.0,
                    logarithmic: true,
                    step: 0.1,
                    decimals: 1,
                    suffix: "CPS",
                },
            );
        }

        theme::rule(ui);

        changed |= compact_choice_row(ui, self.t(Key::RowButton), self.t(Key::HintButton), |ui| {
            widgets::segmented(
                ui,
                "mouse_button",
                &mut draft.button,
                &[
                    (Button::Left, self.t(Key::ButtonLeft)),
                    (Button::Right, self.t(Key::ButtonRight)),
                    (Button::Middle, self.t(Key::ButtonMiddle)),
                ],
            )
        });

        theme::rule(ui);

        let ms = self.t(Key::UnitMs);
        changed |= compact_slider_row(
            ui,
            self.t(Key::RowHold),
            self.t(Key::HintHold),
            &mut draft.hold_ms,
            SliderSpec {
                range: 0.1..=5000.0,
                logarithmic: false,
                step: 0.1,
                decimals: 2,
                suffix: ms,
            },
        );

        theme::rule(ui);

        changed |= compact_slider_row(
            ui,
            self.t(Key::RowJitter),
            self.t(Key::HintJitter),
            &mut draft.jitter_pct,
            SliderSpec {
                range: 0.0..=100.0,
                logarithmic: false,
                step: 0.5,
                decimals: 2,
                suffix: "%",
            },
        );

        theme::rule(ui);

        let pieces = self.t(Key::UnitPieces);
        changed |= compact_integer_row(
            ui,
            self.t(Key::RowLimit),
            self.t(Key::HintLimit),
            &mut draft.limit,
            0..=MAX_LIMIT,
            pieces,
        );

        theme::rule(ui);

        changed |= compact_slider_row(
            ui,
            self.t(Key::RowDelay),
            self.t(Key::HintDelay),
            &mut draft.delay_ms,
            SliderSpec {
                range: 0.0..=MAX_DELAY_MS,
                logarithmic: false,
                step: 10.0,
                decimals: 0,
                suffix: ms,
            },
        );

        let period_ms = match draft.mode {
            ClickMode::Sequence => draft.point_interval_ms,
            ClickMode::Cursor | ClickMode::Point => 1000.0 / draft.cps.max(0.2),
        };
        let effective_hold = draft.hold_ms.min((period_ms - 0.2).max(0.1));
        let hint = if draft.hold_ms > effective_hold + 0.001 {
            (
                theme::warning(),
                self.fill(Key::HintHoldClamped, &[trim_number(effective_hold)]),
            )
        } else if draft.limit > 0 {
            (
                theme::muted(),
                self.fill(Key::HintLimitOn, &[group_digits(draft.limit)]),
            )
        } else {
            (theme::dim(), self.t(Key::HintLimitOff).to_string())
        };
        ui.add_space(2.0);
        ui.add(
            egui::Label::new(RichText::new(hint.1).font(theme::sans(9.5)).color(hint.0)).truncate(),
        );

        if changed {
            self.settings = draft;
            self.mark_dirty();
        }
    }

    fn draw_mode_tab(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = if ui.ctx().content_rect().height() < 540.0 {
            1.0
        } else {
            3.0
        };
        let mut draft = self.settings;
        let mut changed = false;
        let mut pipette_target = None;
        let mut add_point = false;
        let mut remove_point = None;

        let mut mode = draft.mode;
        let mode_changed =
            compact_choice_row(ui, self.t(Key::RowTarget), self.t(Key::HintTarget), |ui| {
                widgets::segmented(
                    ui,
                    "click_mode",
                    &mut mode,
                    &[
                        (ClickMode::Cursor, self.t(Key::TargetCursor)),
                        (ClickMode::Point, self.t(Key::TargetPoint)),
                        (ClickMode::Sequence, self.t(Key::TargetSequence)),
                    ],
                )
            });
        if mode_changed {
            match mode {
                ClickMode::Point if draft.target.is_none() => {
                    draft.target = crate::backend::cursor_position().ok();
                }
                ClickMode::Sequence if draft.point_count() == 0 => {
                    // Leave the list empty: the user adds points with the
                    // eyedropper, and readiness asks for two before starting.
                }
                _ => {}
            }
            draft.mode = mode;
            changed = true;
        }

        let blocked = self.target_blocked_reason();
        if blocked.is_some() && draft.mode != ClickMode::Cursor {
            if let Some(reason) = blocked {
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(self.t(reason))
                            .font(theme::sans(9.5))
                            .color(theme::warning()),
                    )
                    .wrap(),
                );
            }
        }

        if draft.mode == ClickMode::Point && draft.target.is_none() {
            draft.target = Some(crate::backend::cursor_position().unwrap_or(Point::new(0, 0)));
            changed = true;
        }

        if draft.mode == ClickMode::Sequence {
            theme::rule(ui);
            changed |= compact_slider_row(
                ui,
                self.t(Key::RowInterval),
                self.t(Key::HintInterval),
                &mut draft.point_interval_ms,
                SliderSpec {
                    range: 1.0..=MAX_POINT_INTERVAL_MS as f64,
                    logarithmic: false,
                    step: 1.0,
                    decimals: 0,
                    suffix: self.t(Key::UnitMs),
                },
            );

            theme::rule(ui);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let _ = row_label(ui, self.t(Key::RowPoints), self.t(Key::HintPoints), 22.0);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let enabled = draft.point_count() < MAX_POINTS;
                    ui.add_enabled_ui(enabled, |ui| {
                        if widgets::text_button(
                            ui,
                            self.t(Key::AddPoint),
                            theme::muted(),
                            theme::accent(),
                        )
                        .clicked()
                        {
                            add_point = true;
                        }
                    });
                    ui.label(
                        RichText::new(format!("{} / {}", draft.point_count(), MAX_POINTS))
                            .font(theme::mono(8.5))
                            .color(theme::dim()),
                    );
                });
            });

            if draft.point_count() == 0 {
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(self.t(Key::PointsEmpty))
                            .font(theme::sans(9.5))
                            .color(theme::muted()),
                    )
                    .wrap(),
                );
            }

            for index in 0..draft.point_count() {
                let Some(mut point) = draft.point_at(index) else {
                    continue;
                };
                let active = self
                    .pipette
                    .as_ref()
                    .is_some_and(|pipette| pipette.target == PipetteTarget::Sequence(index));

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(
                        RichText::new(format!("{:02}", index + 1))
                            .font(theme::mono_medium(9.0))
                            .color(if active {
                                theme::accent()
                            } else {
                                theme::dim()
                            }),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_enabled_ui(blocked.is_none(), |ui| {
                            let response = widgets::pipette_button(ui, active);
                            let response = response.on_hover_text(self.t(Key::PickHint));
                            if response.clicked() {
                                pipette_target = Some(PipetteTarget::Sequence(index));
                            }
                        });
                        if widgets::text_button(ui, "×", theme::dim(), theme::danger())
                            .on_hover_text(self.t(Key::RemovePoint))
                            .clicked()
                        {
                            remove_point = Some(index);
                        }

                        let remaining = ui.available_width();
                        ui.allocate_ui_with_layout(
                            Vec2::new(remaining, 18.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                changed |= widgets::coordinate_field(ui, &mut point.x, "X");
                                changed |= widgets::coordinate_field(ui, &mut point.y, "Y");
                            },
                        );
                    });
                });

                if point != draft.point_at(index).unwrap_or(point) {
                    draft.set_point(index, point);
                    changed = true;
                }
            }
        } else if draft.mode == ClickMode::Point {
            if let Some(point) = &mut draft.target {
                theme::rule(ui);
                let active = self
                    .pipette
                    .as_ref()
                    .is_some_and(|pipette| pipette.target == PipetteTarget::Point);
                let mut point_value = *point;

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let _ = row_label(ui, self.t(Key::RowCoords), self.t(Key::HintCoords), 20.0);

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_enabled_ui(blocked.is_none(), |ui| {
                            let response = widgets::pipette_button(ui, active);
                            let response = response.on_hover_text(self.t(Key::PickHint));
                            if response.clicked() {
                                pipette_target = Some(PipetteTarget::Point);
                            }
                        });
                        let remaining = ui.available_width();
                        ui.allocate_ui_with_layout(
                            Vec2::new(remaining, 18.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                changed |= widgets::coordinate_field(ui, &mut point_value.x, "X");
                                changed |= widgets::coordinate_field(ui, &mut point_value.y, "Y");
                            },
                        );
                    });
                });

                if point_value != *point {
                    *point = point_value;
                    changed = true;
                }
            }
        } else {
            ui.add_space(3.0);
            ui.add(
                egui::Label::new(
                    RichText::new(self.t(Key::CursorModeHint))
                        .font(theme::sans(9.5))
                        .color(theme::dim()),
                )
                .wrap(),
            );
        }

        theme::rule(ui);

        let mut multi = draft.multi_click;
        let multi_changed =
            compact_choice_row(ui, self.t(Key::RowMulti), self.t(Key::HintMulti), |ui| {
                widgets::segmented(
                    ui,
                    "multi_click",
                    &mut multi,
                    &[
                        (1, self.t(Key::MultiOne)),
                        (2, self.t(Key::MultiDouble)),
                        (3, self.t(Key::MultiTriple)),
                    ],
                )
            });
        if multi_changed {
            draft.multi_click = multi;
            changed = true;
        }

        ui.add_space(3.0);
        ui.add(
            egui::Label::new(
                RichText::new(self.t(Key::MultiHint))
                    .font(theme::sans(9.5))
                    .color(theme::dim()),
            )
            .wrap(),
        );

        if let Some(index) = remove_point {
            draft.remove_point(index);
            changed = true;
            if self
                .pipette
                .as_ref()
                .is_some_and(|pipette| pipette.target == PipetteTarget::Sequence(index))
            {
                self.pipette = None;
            }
        }

        if changed {
            self.settings = draft;
            self.mark_dirty();
        }

        if add_point {
            self.add_sequence_point();
        } else if let Some(target) = pipette_target {
            self.start_pipette(target);
        }
    }

    fn draw_system_tab(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = if ui.ctx().content_rect().height() < 540.0 {
            1.0
        } else {
            3.0
        };

        let mut selected = self.language;
        let _ = compact_choice_row(
            ui,
            self.t(Key::RowLanguage),
            self.t(Key::HintLanguage),
            |ui| {
                egui::ComboBox::from_id_salt("language")
                    .width(ui.available_width().min(190.0))
                    .selected_text(selected.native_label())
                    .show_ui(ui, |ui| {
                        for language in Language::ALL {
                            ui.selectable_value(&mut selected, language, language.native_label());
                        }
                    });
                false
            },
        );
        if selected != self.language {
            self.set_language(selected);
        }

        theme::rule(ui);

        let mut toggle_theme = false;
        let _ = compact_choice_row(ui, self.t(Key::RowTheme), self.t(Key::HintTheme), |ui| {
            let label = if self.theme_mode == ThemeMode::Dark {
                self.t(Key::ThemeLight)
            } else {
                self.t(Key::ThemeDark)
            };
            let clicked =
                widgets::theme_toggle(ui, self.theme_mode == ThemeMode::Dark, label).clicked();
            if clicked {
                toggle_theme = true;
            }
            clicked
        });
        if toggle_theme {
            self.toggle_theme(ui.ctx());
        }

        theme::rule(ui);

        let mut scale = self.ui_scale;
        let _ = compact_choice_row(
            ui,
            self.t(Key::RowUiScale),
            self.t(Key::HintUiScale),
            |ui| {
                egui::ComboBox::from_id_salt("ui_scale")
                    .width(ui.available_width().min(120.0))
                    .selected_text(format!("{:.0}%", scale * 100.0))
                    .show_ui(ui, |ui| {
                        for preset in UI_SCALES {
                            ui.selectable_value(
                                &mut scale,
                                preset,
                                format!("{:.0}%", preset * 100.0),
                            );
                        }
                    });
                false
            },
        );
        if (scale - self.ui_scale).abs() > f32::EPSILON {
            self.ui_scale = scale.clamp(MIN_UI_SCALE, MAX_UI_SCALE);
            self.apply_ui_scale(ui.ctx());
            self.save_config();
        }

        theme::rule(ui);

        if crate::autostart::SUPPORTED {
            let mut enabled = self.autostart_enabled;
            let changed = compact_choice_row(
                ui,
                self.t(Key::RowAutostart),
                self.t(Key::HintAutostart),
                |ui| {
                    ui.checkbox(&mut enabled, self.t(Key::Enabled));
                    enabled != self.autostart_enabled
                },
            );

            if changed && enabled != self.autostart_enabled {
                let start_hidden = enabled && self.minimize_to_tray;
                match crate::autostart::set_enabled(enabled, start_hidden) {
                    Ok(()) => {
                        self.autostart_enabled = enabled;
                        self.notice = Some(
                            self.t(if enabled {
                                Key::AutostartOn
                            } else {
                                Key::AutostartOff
                            })
                            .to_string(),
                        );
                    }
                    Err(error) => {
                        self.notice =
                            Some(self.fill(Key::AutostartFailed, &[format!("{error:#}")]));
                    }
                }
            }

            ui.add_space(3.0);
            ui.add(
                egui::Label::new(
                    RichText::new(self.t(Key::AutostartHint))
                        .font(theme::sans(9.5))
                        .color(theme::dim()),
                )
                .wrap(),
            );
        } else {
            let _ = row_label(
                ui,
                self.t(Key::RowAutostart),
                self.t(Key::HintAutostart),
                20.0,
            );
            ui.add_space(3.0);
            ui.add(
                egui::Label::new(
                    RichText::new(self.t(Key::AutostartUnavailable))
                        .font(theme::sans(9.5))
                        .color(theme::warning()),
                )
                .wrap(),
            );
        }

        #[cfg(windows)]
        {
            theme::rule(ui);
            let mut tray = self.minimize_to_tray;
            let changed = compact_choice_row(
                ui,
                self.t(Key::RowMinimizeTray),
                self.t(Key::HintMinimizeTray),
                |ui| {
                    ui.checkbox(&mut tray, self.t(Key::Enabled));
                    tray != self.minimize_to_tray
                },
            );
            if changed {
                self.minimize_to_tray = tray;
                self.sync_tray(ui.ctx());
                if self.autostart_enabled {
                    // Keep the autostart entry in sync with the new tray
                    // preference, otherwise it would open a hidden window
                    // without a tray icon (or show a window unexpectedly).
                    let _ = crate::autostart::set_enabled(true, tray);
                }
                self.save_config();
            }

            ui.add_space(3.0);
            ui.add(
                egui::Label::new(
                    RichText::new(self.t(Key::HintMinimizeTray))
                        .font(theme::sans(9.5))
                        .color(theme::dim()),
                )
                .wrap(),
            );

            if !self.elevated {
                theme::rule(ui);
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(self.t(Key::ElevationWarn))
                            .font(theme::sans(9.5))
                            .color(theme::warning()),
                    )
                    .wrap(),
                );
                ui.add_space(3.0);
                if widgets::text_button(
                    ui,
                    self.t(Key::ElevationRestart),
                    theme::accent(),
                    theme::accent(),
                )
                .clicked()
                {
                    match crate::platform::restart_elevated() {
                        Ok(()) => {
                            self.notice = Some(self.t(Key::ElevationRestarting).to_string());
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        Err(error) => {
                            self.notice =
                                Some(self.fill(Key::ElevationFailed, &[format!("{error:#}")]));
                        }
                    }
                }
            }
        }

        theme::rule(ui);
        ui.add_space(2.0);
        if widgets::text_button(
            ui,
            self.t(Key::DefaultsButton),
            theme::muted(),
            theme::danger(),
        )
        .clicked()
        {
            self.confirm_defaults = true;
        }
    }

    fn draw_defaults_confirmation(&mut self, ctx: &egui::Context) {
        if !self.confirm_defaults {
            return;
        }

        let mut close = false;
        let mut restore = false;

        let modal = egui::Modal::new(egui::Id::new("restore_defaults"))
            .backdrop_color(Color32::from_black_alpha(120))
            .frame(
                Frame::new()
                    .fill(theme::panel())
                    .stroke(Stroke::new(1.0_f32, theme::line()))
                    .corner_radius(CornerRadius::same(16))
                    .inner_margin(Margin::same(24))
                    .shadow(theme::shadow()),
            )
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(
                    RichText::new(self.t(Key::DefaultsTitle))
                        .font(theme::mono_medium(8.5))
                        .color(theme::dim()),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(self.t(Key::DefaultsQuestion))
                        .font(theme::sans_semibold(22.0))
                        .color(theme::text()),
                );
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(self.t(Key::DefaultsText))
                            .font(theme::sans(12.0))
                            .color(theme::muted()),
                    )
                    .wrap(),
                );
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::text_button(
                            ui,
                            self.t(Key::DefaultsConfirm),
                            theme::danger(),
                            theme::danger(),
                        )
                        .clicked()
                        {
                            restore = true;
                        }
                        if widgets::text_button(
                            ui,
                            self.t(Key::Cancel),
                            theme::muted(),
                            theme::text(),
                        )
                        .clicked()
                        {
                            close = true;
                        }
                    });
                });
            });

        if modal.should_close() {
            close = true;
        }

        if restore {
            self.confirm_defaults = false;
            self.restore_defaults(ctx);
        } else if close {
            self.confirm_defaults = false;
        }
    }

    fn target_blocked_reason(&self) -> Option<Key> {
        if crate::backend::target_supported() {
            return None;
        }

        #[cfg(target_os = "linux")]
        {
            if crate::backend::linux::is_wayland() {
                return Some(Key::TargetWayland);
            }
        }

        Some(Key::TargetNoDisplay)
    }

    #[cfg(windows)]
    fn draw_hotkeys_tab(&mut self, ui: &mut Ui) {
        let mut start = None;

        ui.horizontal(|ui| {
            let _ = row_label(
                ui,
                self.t(Key::RowToggleHotkey),
                self.t(Key::HintToggleHotkey),
                20.0,
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let recording = self.recording == Some(RecordTarget::Toggle);
                let response = hotkey_field(
                    ui,
                    &self.hotkey,
                    recording,
                    self.t(Key::RecordWaiting),
                    self.t(Key::NotSet),
                );
                if response.clicked() {
                    start = Some(RecordTarget::Toggle);
                }
                let response = if self.daemon_hotkey_active.is_empty() {
                    response
                } else {
                    response.on_hover_text(
                        self.fill(Key::Registered, &[hotkey_label(&self.daemon_hotkey_active)]),
                    )
                };
                let _ = response;
            });
        });

        theme::rule(ui);

        ui.horizontal(|ui| {
            let _ = row_label(
                ui,
                self.t(Key::RowHoldHotkey),
                self.t(Key::HintHoldHotkey),
                20.0,
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let recording = self.recording == Some(RecordTarget::Hold);
                let response = hotkey_field(
                    ui,
                    &self.hold_hotkey,
                    recording,
                    self.t(Key::RecordWaiting),
                    self.t(Key::NotSet),
                );
                if response.clicked() {
                    start = Some(RecordTarget::Hold);
                }
                if !self.hold_hotkey.is_empty()
                    && widgets::text_button(ui, self.t(Key::Clear), theme::dim(), theme::danger())
                        .clicked()
                {
                    self.clear_hold_hotkey();
                }
                if !self.daemon_hold_hotkey_active.is_empty() {
                    let _ = response.on_hover_text(self.fill(
                        Key::Registered,
                        &[hotkey_label(&self.daemon_hold_hotkey_active)],
                    ));
                }
            });
        });

        let hint = if let Some(target) = self.recording {
            (
                theme::accent(),
                self.fill(
                    Key::RecordHint,
                    &[match target {
                        RecordTarget::Toggle => self.t(Key::RecordTargetToggle),
                        RecordTarget::Hold => self.t(Key::RecordTargetHold),
                    }
                    .to_string()],
                ),
            )
        } else if let Some(error) = &self.daemon_hotkey_error {
            (theme::danger(), error.clone())
        } else if self.daemon_hotkey_paused {
            (theme::muted(), self.t(Key::HotkeyPaused).to_string())
        } else {
            (theme::dim(), self.t(Key::HotkeyHint).to_string())
        };
        ui.add_space(6.0);
        ui.add(egui::Label::new(RichText::new(hint.1).font(theme::sans(9.5)).color(hint.0)).wrap());

        if let Some(target) = start {
            if self.recording == Some(target) {
                self.stop_recording(true);
            } else {
                ui.ctx().memory_mut(|memory| memory.stop_text_input());
                self.start_recording(target);
            }
        }
    }

    #[cfg(not(windows))]
    fn draw_hotkeys_tab(&mut self, ui: &mut Ui) {
        let bind = self.toggle_bind();

        ui.horizontal(|ui| {
            let _ = row_label(ui, self.t(Key::RowGlobal), self.t(Key::HintGlobal), 20.0);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let response = ui.add(
                    egui::Label::new(
                        RichText::new("MOD+ALT+C")
                            .font(theme::mono_medium(10.0))
                            .color(theme::text()),
                    )
                    .sense(Sense::click()),
                );
                let response = response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(format!("{}\n{bind}", self.t(Key::CopyBind)));
                if response.clicked() {
                    ui.ctx().copy_text(bind.clone());
                    self.notice = Some(self.t(Key::BindCopied).to_string());
                }
            });
        });

        theme::rule(ui);

        ui.add_space(6.0);
        ui.add(
            egui::Label::new(
                RichText::new(self.t(Key::BindHint))
                    .font(theme::sans(9.5))
                    .color(theme::dim()),
            )
            .wrap(),
        );
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(
                RichText::new(self.t(Key::HoldWindowsOnly))
                    .font(theme::sans(9.5))
                    .color(theme::dim()),
            )
            .wrap(),
        );

        let _ = &self.hold_hotkey;
    }

    #[cfg(not(windows))]
    fn toggle_bind(&self) -> String {
        let executable = std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "savv-autoclicker".to_string());
        let executable = executable.replace('"', "\\\"");
        format!("Mod+Alt+C {{ spawn \"{executable}\" \"toggle\"; }}")
    }

    #[cfg(not(windows))]
    fn draw_footer(&mut self, ui: &mut Ui) {
        let mut quit_clicked = false;

        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
            ui.painter().circle_filled(
                dot.center(),
                2.5,
                if self.online {
                    theme::accent()
                } else {
                    theme::dim()
                },
            );

            let response = ui.label(
                RichText::new(self.t(if self.online {
                    Key::DaemonRunning
                } else {
                    Key::DaemonStopped
                }))
                .font(theme::mono(8.5))
                .color(theme::muted()),
            );
            if let Some(reason) = &self.offline_reason {
                let _ = response.on_hover_text(reason);
            }

            ui.add_space(3.0);
            ui.label(
                RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                    .font(theme::mono(8.0))
                    .color(theme::dim()),
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(self.online, |ui| {
                    if widgets::text_button(
                        ui,
                        self.t(Key::QuitDaemon),
                        theme::muted(),
                        theme::danger(),
                    )
                    .clicked()
                    {
                        quit_clicked = true;
                    }
                });
            });
        });

        if quit_clicked {
            self.confirm_quit = true;
        }
    }

    #[cfg(not(windows))]
    fn draw_quit_confirmation(&mut self, ctx: &egui::Context) {
        if !self.confirm_quit {
            return;
        }

        let mut close = false;
        let mut quit = false;

        let modal = egui::Modal::new(egui::Id::new("quit_daemon"))
            .backdrop_color(Color32::from_black_alpha(120))
            .frame(
                Frame::new()
                    .fill(theme::panel())
                    .stroke(Stroke::new(1.0_f32, theme::line()))
                    .corner_radius(CornerRadius::same(16))
                    .inner_margin(Margin::same(24))
                    .shadow(theme::shadow()),
            )
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(
                    RichText::new(self.t(Key::QuitTitle))
                        .font(theme::mono_medium(8.5))
                        .color(theme::dim()),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(self.t(Key::QuitQuestion))
                        .font(theme::sans_semibold(22.0))
                        .color(theme::text()),
                );
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(self.t(Key::QuitText))
                            .font(theme::sans(12.0))
                            .color(theme::muted()),
                    )
                    .wrap(),
                );
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::solid_button(
                            ui,
                            self.t(Key::QuitConfirm),
                            theme::danger(),
                            theme::panel(),
                        )
                        .clicked()
                        {
                            quit = true;
                        }
                        if widgets::text_button(
                            ui,
                            self.t(Key::Cancel),
                            theme::muted(),
                            theme::text(),
                        )
                        .clicked()
                        {
                            close = true;
                        }
                    });
                });
            });

        if modal.should_close() {
            close = true;
        }

        if quit {
            self.confirm_quit = false;
            self.request_quit();
        } else if close {
            self.confirm_quit = false;
        }
    }
}

impl egui_software_backend::App for GuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _backend: &mut SoftwareBackend) {
        if ui.ctx().input(|input| input.viewport().close_requested()) {
            // Persist the draft even if the window is closed before the next
            // debounce tick has a chance to save it.
            self.save_config();
        }

        self.apply_ui_scale(ui.ctx());
        #[cfg(windows)]
        self.sync_tray(ui.ctx());
        #[cfg(windows)]
        if self.start_hidden {
            // If the tray icon could not be created, never leave the app
            // running without any window to interact with.
            self.start_hidden = false;
            if self.tray.is_none() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Visible(true));
            }
        }
        self.poll_pipette(ui.ctx());
        #[cfg(windows)]
        self.handle_minimize(ui.ctx());

        self.capture_hotkey(ui.ctx());
        self.drain_events(ui.ctx());
        self.maybe_apply_settings();

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(theme::bg())
                    .inner_margin(Margin::symmetric(16, 10)),
            )
            .show_inside(ui, |ui| {
                #[cfg(not(windows))]
                egui::Panel::bottom("daemon_footer")
                    .frame(
                        Frame::new()
                            .fill(theme::bg())
                            .inner_margin(Margin::symmetric(0, 4)),
                    )
                    .show_separator_line(false)
                    .show_inside(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.set_max_width(ui.available_width().min(CONTENT_WIDTH));
                            theme::rule(ui);
                            ui.add_space(3.0);
                            self.draw_footer(ui);
                        });
                    });

                ui.vertical_centered(|ui| {
                    ui.set_max_width(ui.available_width().min(CONTENT_WIDTH));
                    self.draw_topbar(ui);
                    ui.add_space(4.0);
                    theme::rule(ui);
                    self.draw_hero(ui);
                    self.draw_metrics(ui);
                    ui.add_space(2.0);
                    self.draw_notice(ui);
                    ui.add_space(3.0);
                    self.draw_tabs(ui);
                    ui.add_space(3.0);

                    egui::ScrollArea::vertical()
                        .id_salt(("tab_scroll", self.tab))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.allocate_ui_with_layout(
                                Vec2::new(ui.available_width(), ui.available_height()),
                                Layout::top_down(Align::LEFT),
                                |ui| match self.tab {
                                    Tab::Clicks => self.draw_clicks_tab(ui),
                                    Tab::Mode => self.draw_mode_tab(ui),
                                    Tab::Hotkeys => self.draw_hotkeys_tab(ui),
                                    Tab::System => self.draw_system_tab(ui),
                                },
                            );
                        });
                });
            });

        #[cfg(not(windows))]
        self.draw_quit_confirmation(ui.ctx());
        self.draw_defaults_confirmation(ui.ctx());

        // The debounce needs its own wake-up: the software backend waits for
        // events, so an idle window would otherwise not apply or save the
        // pending draft until the user interacts again.
        if let Some(changed_at) = self.draft_changed_at {
            ui.ctx()
                .request_repaint_after(APPLY_DEBOUNCE.saturating_sub(changed_at.elapsed()));
        }
    }
}

impl Drop for GuiApp {
    fn drop(&mut self) {
        // The worker owns the daemon cleanup (resume, OFF and, on Windows,
        // QUIT). Join it so the process does not exit before that best-effort
        // IPC completes.
        let _ = self.commands.send(Command::Shutdown);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        self.save_config();
    }
}

pub fn run(start_hidden: bool) -> Result<(), Box<dyn std::error::Error>> {
    // A hidden start only makes sense together with a tray icon; the config is
    // read again by the app, but the visibility has to be known before the
    // window is created.
    let start_hidden = start_hidden && cfg!(windows) && config::load().config.minimize_to_tray;

    let mut viewport = egui::ViewportBuilder::default()
        .with_app_id("savv-autoclicker")
        .with_inner_size(WINDOW_SIZE)
        .with_min_inner_size(MIN_WINDOW_SIZE)
        .with_title("SAVV-AutoClicker")
        .with_visible(!start_hidden);

    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }

    let config = SoftwareBackendAppConfiguration::new().viewport_builder(viewport);

    egui_software_backend::run_app_with_software_backend(config, move |ctx| {
        GuiApp::new(ctx, start_hidden)
    })
    .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
}

fn load_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../assets/icon/savv-autoclicker-256.png");
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    buffer.truncate(info.buffer_size());

    if info.color_type != png::ColorType::Rgba {
        return None;
    }

    Some(egui::IconData {
        rgba: buffer,
        width: info.width,
        height: info.height,
    })
}

fn row_label(ui: &mut Ui, title: &str, hint: &str, height: f32) -> egui::Response {
    let response = ui
        .allocate_ui_with_layout(
            Vec2::new(LABEL_WIDTH, height),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .font(theme::sans_medium(12.0))
                            .color(theme::text()),
                    )
                    .truncate(),
                )
            },
        )
        .inner;
    response.on_hover_text(hint)
}

fn compact_slider_row(
    ui: &mut Ui,
    title: &str,
    hint: &str,
    value: &mut f64,
    spec: SliderSpec<'_>,
) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let _ = row_label(ui, title, hint, 18.0);

        let field_width = widgets::numeric_row_width(ui, &spec.range, spec.decimals, spec.suffix);
        let slider_width = (ui.available_width() - field_width).max(60.0);
        ui.allocate_ui_with_layout(
            Vec2::new(slider_width, 18.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().slider_width = slider_width;
                changed |= widgets::parameter_slider(
                    ui,
                    value,
                    spec.range.clone(),
                    spec.logarithmic,
                    spec.step,
                )
                .changed();
            },
        );

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            changed |= widgets::numeric_field(
                ui,
                value,
                spec.range,
                spec.step,
                spec.decimals,
                spec.suffix,
            );
        });
    });

    changed
}

fn compact_integer_row(
    ui: &mut Ui,
    title: &str,
    hint: &str,
    value: &mut u64,
    range: std::ops::RangeInclusive<u64>,
    suffix: &str,
) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let _ = row_label(ui, title, hint, 18.0);

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            changed |= widgets::integer_field(ui, value, range, 1.0, suffix);
        });
    });

    changed
}

fn compact_choice_row(
    ui: &mut Ui,
    title: &str,
    hint: &str,
    add: impl FnOnce(&mut Ui) -> bool,
) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let _ = row_label(ui, title, hint, 22.0);

        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), 22.0),
            Layout::left_to_right(Align::Center),
            |ui| changed = add(ui),
        );
    });

    changed
}

#[cfg(windows)]
fn hotkey_field(
    ui: &mut Ui,
    combo: &str,
    recording: bool,
    waiting: &str,
    empty: &str,
) -> egui::Response {
    let width = ui.available_width().clamp(120.0, 190.0);
    let label = if recording {
        waiting.to_string()
    } else if combo.is_empty() {
        empty.to_string()
    } else {
        hotkey_label(combo)
    };
    let text_color = if recording {
        theme::accent()
    } else if combo.is_empty() {
        theme::dim()
    } else {
        theme::text()
    };
    let button = egui::Button::new(
        RichText::new(label)
            .font(theme::mono_medium(9.0))
            .color(text_color),
    )
    .fill(if recording {
        theme::alpha(theme::accent(), 10)
    } else {
        theme::panel()
    })
    .stroke(Stroke::new(
        1.0_f32,
        if recording {
            theme::accent()
        } else {
            theme::line()
        },
    ))
    .corner_radius(CornerRadius::same(7))
    .min_size(Vec2::new(width, 24.0));

    ui.add(button)
}

#[cfg(windows)]
fn hotkey_label(combo: &str) -> String {
    crate::hotkey::Hotkey::parse(combo)
        .map(|hotkey| hotkey.label())
        .unwrap_or_else(|_| combo.to_ascii_uppercase())
}

fn metric(ui: &mut Ui, label: &str, value: &str, unit: &str) {
    ui.label(
        RichText::new(label)
            .font(theme::mono_medium(7.0))
            .color(theme::muted()),
    );
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(
            RichText::new(value)
                .font(theme::mono_medium(16.0))
                .color(theme::text()),
        );
        ui.label(
            RichText::new(unit)
                .font(theme::mono(7.0))
                .color(theme::dim()),
        );
    });
}

fn trim_number(value: f64) -> String {
    let mut text = format!("{value:.2}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn group_digits(value: u64) -> String {
    let text = value.to_string();
    let mut grouped = String::with_capacity(text.len() + text.len() / 3);

    for (index, character) in text.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push('\u{00A0}');
        }
        grouped.push(character);
    }

    grouped.chars().rev().collect()
}

fn settings_equivalent(left: &Settings, right: &Settings) -> bool {
    left.button == right.button
        && left.limit == right.limit
        && left.multi_click == right.multi_click
        && left.mode == right.mode
        && left.target == right.target
        && left.points == right.points
        && left.point_interval == right.point_interval
        && (left.cps - right.cps).abs() < 0.001
        && (left.hold_ms() - right.hold_ms()).abs() < 0.001
        && (left.jitter_pct - right.jitter_pct).abs() < 0.001
        && (left.delay_ms() - right.delay_ms()).abs() < 0.001
}
