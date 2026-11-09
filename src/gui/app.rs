use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2,
};
use egui_software_backend::{SoftwareBackend, SoftwareBackendAppConfiguration};

use super::config::{self, ThemeMode};
use super::worker::{Command, Event, Worker};
use super::{theme, widgets};

use crate::ipc;
use crate::state::{Button, MAX_DELAY_MS, MAX_LIMIT, Settings, Status, StopReason};

const APPLY_DEBOUNCE: Duration = Duration::from_millis(250);
const SYNC_GRACE: Duration = Duration::from_millis(800);
const TOGGLE_TIMEOUT: Duration = Duration::from_secs(3);
const CONTENT_WIDTH: f32 = 424.0;
const WINDOW_SIZE: [f32; 2] = [460.0, 560.0];
const MIN_WINDOW_SIZE: [f32; 2] = [420.0, 520.0];
const LABEL_WIDTH: f32 = 90.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Clicks,
    Hotkeys,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
enum RecordTarget {
    Toggle,
    Hold,
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
        }
    }
}

impl DraftSettings {
    fn to_settings(self) -> Result<Settings, String> {
        Settings::new(self.cps, self.button, self.hold_ms, self.jitter_pct)
            .and_then(|settings| settings.with_limit(self.limit))
            .and_then(|settings| settings.with_delay_ms(self.delay_ms))
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
}

impl GuiApp {
    fn new(ctx: egui::Context) -> Self {
        let loaded = config::load();
        let config = loaded.config;
        theme::install(&ctx, config.theme);
        let worker = Worker::spawn(ctx, config.hotkey.clone(), config.hold_hotkey.clone())
            .expect("failed to start the autoclicker GUI IPC worker");

        Self {
            settings: config.settings.into(),
            tab: Tab::Clicks,
            hotkey: config.hotkey,
            hold_hotkey: config.hold_hotkey,
            recording: None,
            theme_mode: config.theme,
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
        }
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
                    self.notice = Some("Демон остановлен.".to_string());
                }
            }
        }

        if self
            .pending_toggle
            .is_some_and(|(_, sent_at)| sent_at.elapsed() > TOGGLE_TIMEOUT)
        {
            self.pending_toggle = None;
            self.notice = Some("Демон не ответил на команду. Проверь его состояние.".to_string());
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
            self.notice =
                Some("Демон старой версии: перезапускаю его для новых настроек…".to_string());
            let _ = self.commands.send(Command::RestartDaemon {
                hotkey: self.hotkey.clone(),
                hold_hotkey: self.hold_hotkey.clone(),
                resume,
            });
            return;
        }

        if self.legacy_restart_pending && status.protocol >= crate::state::PROTOCOL {
            self.legacy_restart_pending = false;
            if self
                .notice
                .as_deref()
                .is_some_and(|text| text.starts_with("Демон старой версии"))
            {
                self.notice = None;
            }
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
            })
    }

    fn save_config(&mut self) {
        let Some(config) = self.config_snapshot() else {
            return;
        };
        if let Err(error) = config::save(&config) {
            self.notice = Some(format!("Не удалось сохранить настройки: {error:#}"));
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
        self.notice = Some(format!(
            "Тема: {}.",
            match self.theme_mode {
                ThemeMode::Light => "светлая",
                ThemeMode::Dark => "тёмная",
            }
        ));
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
                self.notice = Some(format!(
                    "Не удалось назначить клавишу: {error:#}. Поддерживаются A-Z, 0-9, F1-F24, Space, Tab, Enter, Esc, стрелки и знаки."
                ));
                return;
            }
        };
        let combo = hotkey.to_string();

        match target {
            RecordTarget::Toggle => {
                if !self.hold_hotkey.is_empty() && combo == self.hold_hotkey {
                    self.stop_recording(true);
                    self.notice = Some("Эта клавиша уже назначена для удержания.".to_string());
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
                    self.notice = Some("Эта клавиша уже назначена для переключения.".to_string());
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
        self.notice = Some(format!(
            "{}: {}.",
            match target {
                RecordTarget::Toggle => "Переключение",
                RecordTarget::Hold => "Удержание",
            },
            label
        ));
        self.save_config();
    }

    #[cfg(windows)]
    fn clear_hold_hotkey(&mut self) {
        self.hold_hotkey.clear();
        self.daemon_hotkey_error = None;
        self.last_hold_hotkey_sync = Some(Instant::now());
        let _ = self.commands.send(Command::SetHoldHotkey(String::new()));
        self.notice = Some("Клавиша удержания отключена.".to_string());
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
        self.notice = Some("Завершаю демон…".to_string());
        let _ = self.commands.send(Command::QuitDaemon);
    }

    fn status_palette(&self) -> (&'static str, Color32, bool) {
        if let Some((start, _)) = self.pending_toggle {
            return (
                if start {
                    "ЗАПУСК"
                } else {
                    "ОСТАНОВКА"
                },
                theme::accent(),
                true,
            );
        }
        if !self.online {
            ("НЕТ СВЯЗИ", theme::dim(), false)
        } else if self.starting {
            ("ЗАДЕРЖКА", theme::warning(), true)
        } else if self.enabled {
            ("АКТИВЕН", theme::accent(), true)
        } else {
            ("ОЖИДАНИЕ", theme::warning(), false)
        }
    }

    fn draw_topbar(&mut self, ui: &mut Ui) {
        let mut toggle_theme = false;

        ui.horizontal(|ui| {
            let (mark, _) = ui.allocate_exact_size(Vec2::splat(7.0), Sense::hover());
            ui.painter()
                .rect_filled(mark, CornerRadius::same(2), theme::accent());

            ui.label(
                RichText::new("Autoclicker")
                    .font(theme::sans_semibold(16.0))
                    .color(theme::text()),
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (label, color, pulse) = self.status_palette();
                let response = widgets::status_badge(ui, label, color, pulse);
                if let Some(reason) = &self.offline_reason {
                    let _ = response.on_hover_text(reason);
                }

                if widgets::theme_toggle(ui, self.theme_mode == ThemeMode::Dark)
                    .on_hover_text("Переключить светлую и тёмную тему")
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
                    "Запуск…"
                } else {
                    "Остановка…"
                }
                .to_string(),
                "Ждём подтверждения от демона.".to_string(),
            )
        } else if self.starting {
            (
                format!("Старт через {:.1} с", self.delay_left_ms as f64 / 1000.0),
                "Клики начнутся после задержки. Выключение отменит запуск.".to_string(),
            )
        } else if self.hold_active {
            (
                "Удержание".to_string(),
                "Клики идут, пока горячая клавиша зажата.".to_string(),
            )
        } else if active {
            let description = if self.settings.limit > 0 {
                format!(
                    "{} из {} кликов сессии.",
                    self.session_clicks, self.settings.limit
                )
            } else {
                "Импульсы уходят в текущую позицию курсора.".to_string()
            };
            ("Клики идут".to_string(), description)
        } else if self.stop_reason == Some(StopReason::Limit) {
            (
                "Лимит выполнен".to_string(),
                format!(
                    "{} кликов сессии. Запустите снова или измените лимит.",
                    self.session_clicks
                ),
            )
        } else {
            (
                "Клики остановлены".to_string(),
                if self.online {
                    "Нажмите кнопку, чтобы начать.".to_string()
                } else {
                    "Демон запустится автоматически.".to_string()
                },
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
                "ФАКТ. СКОРОСТЬ",
                &format!("{:.1}", self.actual_cps),
                "CPS",
            );
            metric(&mut columns[1], "КЛИКОВ", &group_digits(self.clicks), "ШТ");
            let session = group_digits(self.session_clicks);
            let limit = if self.settings.limit > 0 {
                format!("/ {}", group_digits(self.settings.limit))
            } else {
                "ШТ".to_string()
            };
            metric(&mut columns[2], "СЕССИЯ", &session, &limit);
        });
        ui.add_space(2.0);
        theme::rule(ui);
    }

    fn notice_data(&self) -> (Color32, &'static str, String) {
        if let Some(error) = &self.daemon_error {
            return (theme::danger(), "ОШИБКА", error.clone());
        }
        if let Some(notice) = &self.notice {
            return (theme::warning(), "СИСТЕМА", notice.clone());
        }
        if let Some(error) = &self.daemon_hotkey_error {
            return (theme::danger(), "ХОТКЕЙ", error.clone());
        }
        if self.starting {
            return (
                theme::warning(),
                "СТАРТ",
                format!(
                    "Клики начнутся через {:.1} с.",
                    self.delay_left_ms as f64 / 1000.0
                ),
            );
        }
        if self.stop_reason == Some(StopReason::Limit) && !self.enabled {
            return (
                theme::warning(),
                "ЛИМИТ",
                format!("Выполнено {} кликов сессии.", self.session_clicks),
            );
        }
        if !self.online {
            return (
                theme::dim(),
                "ДЕМОН",
                "Не запущен — стартует вместе с кликами.".to_string(),
            );
        }
        (
            theme::dim(),
            "ГОТОВО",
            "Изменения применяются автоматически.".to_string(),
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
            &[(Tab::Clicks, "КЛИКИ"), (Tab::Hotkeys, "ГОРЯЧИЕ КЛАВИШИ")],
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

        changed |= compact_slider_row(
            ui,
            "Скорость",
            "Частота кликов в секунду",
            &mut draft.cps,
            SliderSpec {
                range: 0.2..=1000.0,
                logarithmic: true,
                step: 0.1,
                decimals: 1,
                suffix: "CPS",
            },
        );

        theme::rule(ui);

        changed |= compact_choice_row(
            ui,
            "Кнопка",
            "Какую кнопку мыши нажимать",
            |ui| {
                widgets::segmented(
                    ui,
                    "mouse_button",
                    &mut draft.button,
                    &[
                        (Button::Left, "ЛЕВАЯ"),
                        (Button::Right, "ПРАВАЯ"),
                        (Button::Middle, "СРЕДНЯЯ"),
                    ],
                )
            },
        );

        theme::rule(ui);

        changed |= compact_slider_row(
            ui,
            "Удержание",
            "Длительность нажатия; ограничивается периодом",
            &mut draft.hold_ms,
            SliderSpec {
                range: 0.1..=5000.0,
                logarithmic: false,
                step: 0.1,
                decimals: 2,
                suffix: "МС",
            },
        );

        theme::rule(ui);

        changed |= compact_slider_row(
            ui,
            "Джиттер",
            "Случайный разброс интервала",
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

        changed |= compact_integer_row(
            ui,
            "Лимит",
            "Сколько кликов сделать за сессию. 0 — без ограничения",
            &mut draft.limit,
            0..=MAX_LIMIT,
            "ШТ",
        );

        theme::rule(ui);

        changed |= compact_slider_row(
            ui,
            "Задержка",
            "Пауза перед началом новой сессии, 0 - 10000 мс",
            &mut draft.delay_ms,
            SliderSpec {
                range: 0.0..=MAX_DELAY_MS,
                logarithmic: false,
                step: 10.0,
                decimals: 0,
                suffix: "МС",
            },
        );

        let period_ms = 1000.0 / draft.cps.max(0.2);
        let effective_hold = draft.hold_ms.min((period_ms - 0.2).max(0.1));
        let hint = if draft.hold_ms > effective_hold + 0.001 {
            (
                theme::warning(),
                format!(
                    "Фактическое удержание {} мс: частота не даёт удержать дольше.",
                    trim_number(effective_hold)
                ),
            )
        } else if draft.limit > 0 {
            (
                theme::muted(),
                format!(
                    "Лимит {} кликов за сессию; 0 отключает лимит.",
                    group_digits(draft.limit)
                ),
            )
        } else {
            (
                theme::dim(),
                "Лимит 0 — без ограничения. Задержка действует только на старте.".to_string(),
            )
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

    #[cfg(windows)]
    fn draw_hotkeys_tab(&mut self, ui: &mut Ui) {
        let mut start = None;

        ui.horizontal(|ui| {
            let _ = row_label(
                ui,
                "Переключение",
                "Глобальное включение и выключение кликов",
                20.0,
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let recording = self.recording == Some(RecordTarget::Toggle);
                let response = hotkey_field(ui, &self.hotkey, recording);
                if response.clicked() {
                    start = Some(RecordTarget::Toggle);
                }
                let response = if self.daemon_hotkey_active.is_empty() {
                    response
                } else {
                    response.on_hover_text(format!(
                        "Сейчас зарегистрирована: {}",
                        hotkey_label(&self.daemon_hotkey_active)
                    ))
                };
                let _ = response;
            });
        });

        theme::rule(ui);

        ui.horizontal(|ui| {
            let _ = row_label(ui, "Удержание", "Клики идут, пока клавиша зажата", 20.0);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let recording = self.recording == Some(RecordTarget::Hold);
                let response = hotkey_field(ui, &self.hold_hotkey, recording);
                if response.clicked() {
                    start = Some(RecordTarget::Hold);
                }
                if !self.hold_hotkey.is_empty()
                    && widgets::text_button(ui, "ОЧИСТИТЬ", theme::dim(), theme::danger()).clicked()
                {
                    self.clear_hold_hotkey();
                }
                if !self.daemon_hold_hotkey_active.is_empty() {
                    let _ = response.on_hover_text(format!(
                        "Сейчас зарегистрирована: {}",
                        hotkey_label(&self.daemon_hold_hotkey_active)
                    ));
                }
            });
        });

        let hint = if let Some(target) = self.recording {
            (
                theme::accent(),
                format!(
                    "Нажмите любую клавишу для «{}». Повторный клик по полю — отмена.",
                    match target {
                        RecordTarget::Toggle => "переключения",
                        RecordTarget::Hold => "удержания",
                    }
                ),
            )
        } else if let Some(error) = &self.daemon_hotkey_error {
            (theme::danger(), error.clone())
        } else if self.daemon_hotkey_paused {
            (
                theme::muted(),
                "Регистрация горячих клавиш приостановлена.".to_string(),
            )
        } else {
            (
                theme::dim(),
                "Клавиша без модификаторов тоже работает; F12 может быть занята Windows."
                    .to_string(),
            )
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
            let _ = row_label(ui, "Глобальная", "Комбинацию задаёт композитор", 20.0);

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
                    .on_hover_text(format!("Нажмите, чтобы скопировать bind\n{bind}"));
                if response.clicked() {
                    ui.ctx().copy_text(bind.clone());
                    self.notice = Some("Bind скопирован в буфер обмена.".to_string());
                }
            });
        });

        theme::rule(ui);

        ui.add_space(6.0);
        ui.add(
            egui::Label::new(
                RichText::new(
                    "Глобальную комбинацию задаёт композитор. Нажмите MOD+ALT+C, чтобы скопировать готовый bind для niri.",
                )
                .font(theme::sans(9.5))
                .color(theme::dim()),
            )
            .wrap(),
        );
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(
                RichText::new("Удержание доступно только в Windows.")
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
            .unwrap_or_else(|_| "autoclicker".to_string());
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
                RichText::new(if self.online {
                    "ДЕМОН РАБОТАЕТ"
                } else {
                    "ДЕМОН НЕ ЗАПУЩЕН"
                })
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
                    if widgets::text_button(ui, "ЗАВЕРШИТЬ ДЕМОН", theme::muted(), theme::danger())
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
                    RichText::new("ЗАВЕРШЕНИЕ РАБОТЫ")
                        .font(theme::mono_medium(8.5))
                        .color(theme::dim()),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Завершить демон?")
                        .font(theme::sans_semibold(22.0))
                        .color(theme::text()),
                );
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(
                        RichText::new("Клики будут остановлены, демон завершит работу.")
                            .font(theme::sans(12.0))
                            .color(theme::muted()),
                    )
                    .wrap(),
                );
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::solid_button(ui, "ЗАВЕРШИТЬ", theme::danger(), theme::panel())
                            .clicked()
                        {
                            quit = true;
                        }
                        if widgets::text_button(ui, "ОТМЕНА", theme::muted(), theme::text())
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

                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), ui.available_height()),
                        Layout::top_down(Align::LEFT),
                        |ui| match self.tab {
                            Tab::Clicks => self.draw_clicks_tab(ui),
                            Tab::Hotkeys => self.draw_hotkeys_tab(ui),
                        },
                    );
                });
            });

        #[cfg(not(windows))]
        self.draw_quit_confirmation(ui.ctx());

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
        let _ = ipc::send("HOTKEY_RESUME");
        let _ = ipc::send("OFF");
        #[cfg(windows)]
        let _ = ipc::send("QUIT");
        let _ = self.commands.send(Command::Shutdown);
        self.save_config();
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = SoftwareBackendAppConfiguration::new().viewport_builder(
        egui::ViewportBuilder::default()
            .with_app_id("autoclicker")
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size(MIN_WINDOW_SIZE)
            .with_title("Autoclicker"),
    );

    egui_software_backend::run_app_with_software_backend(config, GuiApp::new)
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
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

        let field_width = 82.0;
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
fn hotkey_field(ui: &mut Ui, combo: &str, recording: bool) -> egui::Response {
    let width = ui.available_width().clamp(120.0, 190.0);
    let label = if recording {
        "ЖДЁМ КЛАВИШУ".to_string()
    } else if combo.is_empty() {
        "НЕ ЗАДАНА".to_string()
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
        && (left.cps - right.cps).abs() < 0.001
        && (left.hold_ms() - right.hold_ms()).abs() < 0.001
        && (left.jitter_pct - right.jitter_pct).abs() < 0.001
        && (left.delay_ms() - right.delay_ms()).abs() < 0.001
}
