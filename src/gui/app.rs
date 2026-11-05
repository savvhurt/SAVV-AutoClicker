use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use egui::{
    self, Align, Color32, CornerRadius, Frame, Id, Layout, Margin, RichText, ScrollArea, Sense,
    Stroke, Ui, Vec2,
};
use egui_software_backend::{SoftwareBackend, SoftwareBackendAppConfiguration};

use super::worker::{Command, Event, Worker};
use super::{config, theme, widgets};

use crate::ipc;
use crate::state::{Button, Settings, Status};

const APPLY_DEBOUNCE: Duration = Duration::from_millis(250);
const SYNC_GRACE: Duration = Duration::from_millis(800);
const TOGGLE_TIMEOUT: Duration = Duration::from_secs(3);
const CONTENT_WIDTH: f32 = 424.0;
const WINDOW_SIZE: [f32; 2] = [460.0, 560.0];
const MIN_WINDOW_SIZE: [f32; 2] = [420.0, 520.0];

struct SliderSpec<'a> {
    title: &'a str,
    hint: &'a str,
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
}

impl From<Settings> for DraftSettings {
    fn from(settings: Settings) -> Self {
        Self {
            cps: settings.cps,
            button: settings.button,
            hold_ms: settings.hold_ms(),
            jitter_pct: settings.jitter_pct,
        }
    }
}

impl DraftSettings {
    fn to_settings(self) -> Result<Settings, String> {
        Settings::new(self.cps, self.button, self.hold_ms, self.jitter_pct)
            .map_err(|error| format!("{error:#}"))
    }
}

pub struct GuiApp {
    settings: DraftSettings,
    hotkey: String,
    recording_hotkey: bool,
    online: bool,
    enabled: bool,
    actual_cps: f64,
    clicks: u64,
    daemon_error: Option<String>,
    daemon_hotkey_error: Option<String>,
    daemon_settings: Option<Settings>,
    notice: Option<String>,
    offline_reason: Option<String>,
    initialized: bool,
    draft_changed_at: Option<Instant>,
    last_apply_sent: Option<Instant>,
    pending_toggle: Option<(bool, Instant)>,
    confirm_quit: bool,
    commands: Sender<Command>,
    events: Receiver<Event>,
}

impl GuiApp {
    fn new(ctx: egui::Context) -> Self {
        theme::install(&ctx);
        let loaded = config::load();
        let worker = Worker::spawn(ctx, loaded.hotkey.clone())
            .expect("failed to start the autoclicker GUI IPC worker");

        Self {
            settings: loaded.settings.into(),
            hotkey: loaded.hotkey,
            recording_hotkey: false,
            online: false,
            enabled: false,
            actual_cps: 0.0,
            clicks: 0,
            daemon_error: None,
            daemon_hotkey_error: None,
            daemon_settings: None,
            notice: loaded.warning,
            offline_reason: None,
            initialized: false,
            draft_changed_at: None,
            last_apply_sent: None,
            pending_toggle: None,
            confirm_quit: false,
            commands: worker.commands,
            events: worker.events,
        }
    }

    fn drain_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Status(status) => self.apply_status(status),
                Event::Offline(reason) => {
                    self.online = false;
                    self.enabled = false;
                    self.actual_cps = 0.0;
                    self.daemon_settings = None;
                    self.daemon_hotkey_error = None;
                    self.offline_reason = Some(reason);
                    self.initialized = false;
                    self.last_apply_sent = None;
                }
                Event::Error(error) => {
                    self.notice = Some(error);
                    self.pending_toggle = None;
                }
                Event::Stopped => {
                    self.online = false;
                    self.enabled = false;
                    self.actual_cps = 0.0;
                    self.daemon_settings = None;
                    self.daemon_hotkey_error = None;
                    self.offline_reason = None;
                    self.initialized = false;
                    self.last_apply_sent = None;
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

        if self.pending_toggle.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn apply_status(&mut self, status: Status) {
        let first_status = !self.initialized;

        self.online = true;
        self.initialized = true;
        self.offline_reason = None;
        self.enabled = status.enabled;
        self.actual_cps = status.actual_cps;
        self.clicks = status.clicks;
        self.daemon_error = status.last_error.clone();
        self.daemon_hotkey_error = status.hotkey_error.clone();
        self.daemon_settings = Some(status.settings);

        #[cfg(windows)]
        if !status.hotkey.is_empty() && status.hotkey != self.hotkey {
            let _ = self.commands.send(Command::SetHotkey(self.hotkey.clone()));
        }

        if first_status {
            if status.enabled {
                self.settings = status.settings.into();
                self.draft_changed_at = None;
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
                self.draft_changed_at = None;
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

        if let Err(error) = config::save(&settings, &self.hotkey) {
            self.notice = Some(format!("Не удалось сохранить настройки: {error:#}"));
        }
    }

    fn set_hotkey(&mut self, combo: &str) {
        let hotkey = match crate::hotkey::Hotkey::parse(combo) {
            Ok(hotkey) => hotkey,
            Err(error) => {
                self.notice = Some(format!("Некорректная горячая клавиша: {error:#}"));
                return;
            }
        };
        let combo = hotkey.to_string();
        if combo == self.hotkey && self.daemon_hotkey_error.is_none() {
            self.recording_hotkey = false;
            return;
        }

        self.hotkey = combo.clone();
        self.recording_hotkey = false;
        self.daemon_hotkey_error = None;
        self.notice = None;
        let _ = self.commands.send(Command::SetHotkey(combo));

        if let Ok(settings) = self.settings.to_settings() {
            if let Err(error) = config::save(&settings, &self.hotkey) {
                self.notice = Some(format!("Не удалось сохранить настройки: {error:#}"));
            }
        }
    }

    fn capture_hotkey(&mut self, ctx: &egui::Context) {
        if !self.recording_hotkey {
            return;
        }

        let mut pressed = None;
        ctx.input(|input| {
            for event in &input.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } = event
                {
                    pressed = Some((*key, *modifiers));
                    break;
                }
            }
        });

        let Some((key, modifiers)) = pressed else {
            return;
        };

        if key == egui::Key::Escape && !modifiers.ctrl && !modifiers.alt && !modifiers.shift {
            self.recording_hotkey = false;
            self.notice = None;
            return;
        }

        let mut combo = String::new();
        for (enabled, name) in [
            (modifiers.ctrl, "ctrl"),
            (modifiers.alt, "alt"),
            (modifiers.shift, "shift"),
        ] {
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

        match crate::hotkey::Hotkey::parse(&combo) {
            Ok(hotkey) => self.set_hotkey(&hotkey.to_string()),
            Err(error) => {
                self.notice = Some(format!(
                    "{error:#}. Поддерживаются Ctrl, Alt, Shift и клавиши A-Z, 0-9, F1-F24, Space, Tab, Enter, Esc, Backspace."
                ));
            }
        }
    }

    fn mark_dirty(&mut self) {
        self.draft_changed_at = Some(Instant::now());
        self.notice = None;
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
                theme::ACCENT,
                true,
            );
        }
        if !self.online {
            ("НЕТ СВЯЗИ", theme::DIM, false)
        } else if self.enabled {
            ("АКТИВЕН", theme::ACCENT, true)
        } else {
            ("ОЖИДАНИЕ", theme::WARNING, false)
        }
    }

    fn draw_topbar(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let (mark, _) = ui.allocate_exact_size(Vec2::splat(7.0), Sense::hover());
            ui.painter()
                .rect_filled(mark, CornerRadius::same(2), theme::ACCENT);

            ui.label(
                RichText::new("Autoclicker")
                    .font(theme::sans_semibold(16.0))
                    .color(theme::TEXT),
            );

            let (label, color, pulse) = self.status_palette();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let response = widgets::status_badge(ui, label, color, pulse);
                if let Some(reason) = &self.offline_reason {
                    let _ = response.on_hover_text(reason);
                }
            });
        });
    }

    fn draw_hero(&mut self, ui: &mut Ui) {
        let active = self.online && self.enabled;
        let pending = self.pending_toggle.map(|(start, _)| start);

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
                    let (title, title_color) = match pending {
                        Some(true) => ("Запуск…", theme::MUTED),
                        Some(false) => ("Остановка…", theme::MUTED),
                        None if active => ("Клики идут", theme::TEXT),
                        None => ("Клики остановлены", theme::TEXT),
                    };
                    ui.label(
                        RichText::new(title)
                            .font(theme::sans_semibold(20.0))
                            .color(title_color),
                    );

                    ui.add_space(1.0);
                    let description = match pending {
                        Some(_) => "Ждём подтверждения от демона.",
                        None if active => "Импульсы уходят в текущую позицию курсора.",
                        None if !self.online => "Демон запустится автоматически.",
                        None => "Нажмите кнопку, чтобы начать.",
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(description)
                                .font(theme::sans(10.5))
                                .color(theme::MUTED),
                        )
                        .wrap(),
                    );
                },
            );
        });
        ui.add_space(2.0);
    }

    fn draw_metrics(&self, ui: &mut Ui) {
        let period = 1000.0 / self.settings.cps.max(0.2);

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
            metric(&mut columns[2], "ЦИКЛ", &trim_number(period), "МС");
        });
        ui.add_space(2.0);
        theme::rule(ui);
    }

    fn draw_settings(&mut self, ui: &mut Ui) {
        let mut draft = self.settings;
        let mut changed = false;

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Параметры")
                    .font(theme::sans_semibold(12.5))
                    .color(theme::TEXT),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new("АВТОСОХРАНЕНИЕ")
                        .font(theme::mono(7.5))
                        .color(theme::DIM),
                );
            });
        });
        ui.add_space(2.0);

        changed |= slider_setting(
            ui,
            &mut draft.cps,
            SliderSpec {
                title: "Скорость",
                hint: "Частота кликов в секунду",
                range: 0.2..=1000.0,
                logarithmic: true,
                step: 0.1,
                decimals: 1,
                suffix: "CPS",
            },
        );

        settings_divider(ui);

        changed |= choice_setting(
            ui,
            "Кнопка мыши",
            "Какую кнопку нажимать",
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

        settings_divider(ui);

        changed |= slider_setting(
            ui,
            &mut draft.hold_ms,
            SliderSpec {
                title: "Удержание",
                hint: "Длительность нажатия",
                range: 0.1..=5000.0,
                logarithmic: false,
                step: 0.1,
                decimals: 2,
                suffix: "МС",
            },
        );

        let period_ms = 1000.0 / draft.cps.max(0.2);
        let effective_hold = draft.hold_ms.min((period_ms - 0.2).max(0.1));
        if draft.hold_ms > effective_hold + 0.001 {
            ui.add_space(3.0);
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "Фактическое удержание: {} мс — частота не даёт удержать дольше.",
                        trim_number(effective_hold)
                    ))
                    .font(theme::sans(9.5))
                    .color(theme::WARNING),
                )
                .wrap(),
            );
        }

        settings_divider(ui);

        changed |= slider_setting(
            ui,
            &mut draft.jitter_pct,
            SliderSpec {
                title: "Джиттер",
                hint: "Случайный разброс интервала",
                range: 0.0..=100.0,
                logarithmic: false,
                step: 0.5,
                decimals: 2,
                suffix: "%",
            },
        );

        settings_divider(ui);
        self.draw_hotkey(ui);

        if changed {
            self.settings = draft;
            self.mark_dirty();
        }
    }

    #[cfg(windows)]
    fn draw_hotkey(&mut self, ui: &mut Ui) {
        let mut toggle_recording = false;

        ui.horizontal(|ui| {
            let response = ui.label(
                RichText::new("Горячая клавиша")
                    .font(theme::sans_medium(12.5))
                    .color(theme::TEXT),
            );
            let _ = response.on_hover_text("Глобальное переключение кликов в Windows");

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let recording = self.recording_hotkey;
                let label = if recording {
                    "НАЖМИТЕ КОМБИНАЦИЮ".to_string()
                } else {
                    hotkey_label(&self.hotkey)
                };
                let text_color = if recording {
                    theme::ACCENT
                } else {
                    theme::TEXT
                };
                let button = egui::Button::new(
                    RichText::new(label)
                        .font(theme::mono_medium(9.0))
                        .color(text_color),
                )
                .fill(if recording {
                    theme::alpha(theme::ACCENT, 10)
                } else {
                    theme::PANEL
                })
                .stroke(Stroke::new(
                    1.0_f32,
                    if recording {
                        theme::ACCENT
                    } else {
                        theme::LINE
                    },
                ))
                .corner_radius(CornerRadius::same(7))
                .min_size(Vec2::new(0.0, 23.0));

                if ui.add(button).clicked() {
                    toggle_recording = true;
                }
            });
        });

        if toggle_recording {
            self.recording_hotkey = !self.recording_hotkey;
            if self.recording_hotkey {
                ui.ctx().memory_mut(|memory| memory.stop_text_input());
                self.notice = None;
            }
        }

        if self.recording_hotkey {
            ui.add_space(2.0);
            ui.label(
                RichText::new("Нажмите Ctrl/Alt/Shift + клавишу. Esc — отмена.")
                    .font(theme::sans(9.5))
                    .color(theme::ACCENT),
            );
        } else if let Some(error) = &self.daemon_hotkey_error {
            ui.add_space(2.0);
            ui.add(
                egui::Label::new(
                    RichText::new(error)
                        .font(theme::sans(9.5))
                        .color(theme::DANGER),
                )
                .wrap(),
            );
        }
    }

    #[cfg(not(windows))]
    fn draw_hotkey(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Горячая клавиша")
                    .font(theme::sans_medium(12.5))
                    .color(theme::TEXT),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new("MOD+SHIFT+C → TOGGLE")
                        .font(theme::mono(8.0))
                        .color(theme::MUTED),
                );
            });
        });
    }

    fn draw_notices(&self, ui: &mut Ui) {
        let Some(first) = self.daemon_error.as_deref().or(self.notice.as_deref()) else {
            return;
        };

        ui.add_space(8.0);
        if let Some(error) = &self.daemon_error {
            widgets::banner(ui, theme::DANGER, "ОШИБКА ДЕМОНА", error);
        } else {
            widgets::banner(ui, theme::WARNING, "СИСТЕМА", first);
        }

        if self.daemon_error.is_some() {
            if let Some(notice) = &self.notice {
                ui.add_space(8.0);
                widgets::banner(ui, theme::WARNING, "СИСТЕМА", notice);
            }
        }
    }

    fn draw_footer(&mut self, ui: &mut Ui) {
        let mut quit_clicked = false;

        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
            ui.painter().circle_filled(
                dot.center(),
                2.5,
                if self.online {
                    theme::ACCENT
                } else {
                    theme::DIM
                },
            );

            let response = ui.label(
                RichText::new(if self.online {
                    "ДЕМОН РАБОТАЕТ"
                } else {
                    "ДЕМОН НЕ ЗАПУЩЕН"
                })
                .font(theme::mono(8.5))
                .color(theme::MUTED),
            );
            if let Some(reason) = &self.offline_reason {
                let _ = response.on_hover_text(reason);
            }

            ui.add_space(3.0);
            ui.label(
                RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                    .font(theme::mono(8.0))
                    .color(theme::DIM),
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(self.online, |ui| {
                    if widgets::text_button(ui, "ЗАВЕРШИТЬ ДЕМОН", theme::MUTED, theme::DANGER)
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

    fn draw_quit_confirmation(&mut self, ctx: &egui::Context) {
        if !self.confirm_quit {
            return;
        }

        let mut close = false;
        let mut quit = false;

        let modal = egui::Modal::new(Id::new("quit_daemon"))
            .backdrop_color(Color32::from_black_alpha(120))
            .frame(
                Frame::new()
                    .fill(theme::PANEL)
                    .stroke(Stroke::new(1.0_f32, theme::LINE))
                    .corner_radius(CornerRadius::same(16))
                    .inner_margin(Margin::same(24))
                    .shadow(theme::shadow()),
            )
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(
                    RichText::new("ЗАВЕРШЕНИЕ РАБОТЫ")
                        .font(theme::mono_medium(8.5))
                        .color(theme::DIM),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Завершить демон?")
                        .font(theme::sans_semibold(22.0))
                        .color(theme::TEXT),
                );
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(
                        RichText::new("Клики будут остановлены, демон завершит работу.")
                            .font(theme::sans(12.0))
                            .color(theme::MUTED),
                    )
                    .wrap(),
                );
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::solid_button(ui, "ЗАВЕРШИТЬ", theme::DANGER, theme::PANEL)
                            .clicked()
                        {
                            quit = true;
                        }
                        if widgets::text_button(ui, "ОТМЕНА", theme::MUTED, theme::TEXT).clicked()
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
                    .fill(theme::BG)
                    .inner_margin(Margin::symmetric(16, 10)),
            )
            .show_inside(ui, |ui| {
                egui::Panel::bottom("daemon_footer")
                    .frame(
                        Frame::new()
                            .fill(theme::BG)
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

                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.set_max_width(ui.available_width().min(CONTENT_WIDTH));
                            self.draw_topbar(ui);
                            ui.add_space(4.0);
                            theme::rule(ui);
                            self.draw_hero(ui);
                            self.draw_metrics(ui);
                            self.draw_notices(ui);
                            self.draw_settings(ui);
                        });
                    });
            });

        self.draw_quit_confirmation(ui.ctx());
    }
}

impl Drop for GuiApp {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        let _ = ipc::send("OFF");
        if let Ok(settings) = self.settings.to_settings() {
            let _ = config::save(&settings, &self.hotkey);
        }
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

fn slider_setting(ui: &mut Ui, value: &mut f64, spec: SliderSpec<'_>) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        let response = ui.label(
            RichText::new(spec.title)
                .font(theme::sans_medium(12.5))
                .color(theme::TEXT),
        );
        let _ = response.on_hover_text(spec.hint);

        let control_width = ui.available_width().max(100.0);
        ui.allocate_ui_with_layout(
            Vec2::new(control_width, 20.0),
            Layout::right_to_left(Align::Center),
            |ui| {
                changed |= widgets::numeric_field(
                    ui,
                    value,
                    spec.range.clone(),
                    spec.step,
                    spec.decimals,
                    spec.suffix,
                );
            },
        );
    });

    changed |=
        widgets::parameter_slider(ui, value, spec.range, spec.logarithmic, spec.step).changed();

    changed
}

fn choice_setting(ui: &mut Ui, title: &str, hint: &str, add: impl FnOnce(&mut Ui) -> bool) -> bool {
    ui.horizontal(|ui| {
        let response = ui.label(
            RichText::new(title)
                .font(theme::sans_medium(12.5))
                .color(theme::TEXT),
        );
        let _ = response.on_hover_text(hint);
    });
    add(ui)
}

fn settings_divider(ui: &mut Ui) {
    theme::rule(ui);
}

fn metric(ui: &mut Ui, label: &str, value: &str, unit: &str) {
    ui.label(
        RichText::new(label)
            .font(theme::mono_medium(7.0))
            .color(theme::MUTED),
    );
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(
            RichText::new(value)
                .font(theme::mono_medium(16.0))
                .color(theme::TEXT),
        );
        ui.label(RichText::new(unit).font(theme::mono(7.0)).color(theme::DIM));
    });
}

#[cfg(windows)]
fn hotkey_label(combo: &str) -> String {
    crate::hotkey::Hotkey::parse(combo)
        .map(|hotkey| hotkey.label())
        .unwrap_or_else(|_| combo.to_ascii_uppercase())
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
        && (left.cps - right.cps).abs() < 0.001
        && (left.hold_ms() - right.hold_ms()).abs() < 0.001
        && (left.jitter_pct - right.jitter_pct).abs() < 0.001
}
