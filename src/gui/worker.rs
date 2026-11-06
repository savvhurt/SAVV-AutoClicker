use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use egui;

use crate::ipc;
use crate::state::{Settings, Status};

const POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub enum Command {
    Start(Settings),
    Stop,
    Apply(Settings),
    SetHotkey(String),
    #[cfg(windows)]
    PauseHotkey,
    #[cfg(windows)]
    ResumeHotkey,
    #[cfg(windows)]
    RestartDaemon {
        hotkey: String,
        resume: Option<Settings>,
    },
    QuitDaemon,
    Shutdown,
}

#[derive(Debug)]
pub enum Event {
    Status(Status),
    Offline(String),
    Error(String),
    Stopped,
}

pub struct Worker {
    pub commands: Sender<Command>,
    pub events: Receiver<Event>,
}

impl Worker {
    pub fn spawn(ctx: egui::Context, hotkey: String) -> std::io::Result<Self> {
        let (command_tx, command_rx) = channel();
        let (event_tx, event_rx) = channel();

        thread::Builder::new()
            .name("autoclicker-gui-ipc".to_string())
            .spawn(move || run(command_rx, event_tx, ctx, hotkey))?;

        Ok(Self {
            commands: command_tx,
            events: event_rx,
        })
    }
}

fn run(commands: Receiver<Command>, events: Sender<Event>, ctx: egui::Context, mut hotkey: String) {
    let mut online = false;
    let mut last_offline = None;
    let mut last_status: Option<Status> = None;
    let mut next_poll = Instant::now();

    loop {
        let timeout = next_poll.saturating_duration_since(Instant::now());
        match commands.recv_timeout(timeout) {
            Ok(Command::Shutdown) => {
                let _ = ipc::send("HOTKEY_RESUME");
                let _ = ipc::send("OFF");
                break;
            }
            Ok(command) => {
                handle_command(
                    command,
                    &events,
                    &ctx,
                    &mut online,
                    &mut last_offline,
                    &mut last_status,
                    &mut hotkey,
                );
                next_poll = Instant::now() + POLL_INTERVAL;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if Instant::now() >= next_poll {
            poll(
                &events,
                &ctx,
                &mut online,
                &mut last_offline,
                &mut last_status,
            );
            next_poll = Instant::now() + POLL_INTERVAL;
        }
    }
}

fn handle_command(
    command: Command,
    events: &Sender<Event>,
    ctx: &egui::Context,
    online: &mut bool,
    last_offline: &mut Option<String>,
    last_status: &mut Option<Status>,
    hotkey: &mut String,
) {
    let quit_daemon = matches!(command, Command::QuitDaemon);

    match command {
        Command::Start(settings) => {
            let command = settings.to_wire_command("ON");
            match ipc::send_or_spawn_with_hotkey(&command, hotkey) {
                Ok(response) => {
                    if let Err(error) = ipc::ensure_ok(&response) {
                        send_error(
                            events,
                            ctx,
                            format!("Не удалось запустить клики: {error:#}"),
                        );
                    } else {
                        *online = true;
                        *last_offline = None;
                    }
                }
                Err(error) => send_error(
                    events,
                    ctx,
                    format!("Не удалось запустить демон: {error:#}"),
                ),
            }
        }
        Command::Stop => {
            match ipc::send("OFF").and_then(|response| {
                ipc::ensure_ok(&response)?;
                Ok(())
            }) {
                Ok(()) => {
                    *online = true;
                    *last_offline = None;
                }
                Err(error) => {
                    send_error(
                        events,
                        ctx,
                        format!("Не удалось остановить клики: {error:#}"),
                    );
                }
            }
        }
        Command::Apply(settings) => {
            if *online {
                let command = settings.to_wire_command("SET");
                match ipc::send(&command).and_then(|response| {
                    ipc::ensure_ok(&response)?;
                    Ok(())
                }) {
                    Ok(()) => {
                        *last_offline = None;
                    }
                    Err(error) => {
                        send_error(
                            events,
                            ctx,
                            format!("Не удалось применить настройки: {error:#}"),
                        );
                    }
                }
            }
        }
        Command::SetHotkey(next) => {
            *hotkey = next;
            if *online {
                let command = format!("HOTKEY {hotkey}");
                match ipc::send(&command).and_then(|response| {
                    ipc::ensure_ok(&response)?;
                    Ok(())
                }) {
                    Ok(()) => {
                        *last_offline = None;
                    }
                    Err(error) => {
                        send_error(
                            events,
                            ctx,
                            format!("Не удалось применить горячую клавишу: {error:#}"),
                        );
                    }
                }
            }
        }
        #[cfg(windows)]
        Command::PauseHotkey => {
            if *online {
                match ipc::send("HOTKEY_PAUSE").and_then(|response| {
                    ipc::ensure_ok(&response)?;
                    Ok(())
                }) {
                    Ok(()) => {}
                    Err(error) => send_error(
                        events,
                        ctx,
                        format!("Не удалось приостановить горячую клавишу: {error:#}"),
                    ),
                }
            }
        }
        #[cfg(windows)]
        Command::ResumeHotkey => {
            if *online {
                match ipc::send("HOTKEY_RESUME").and_then(|response| {
                    ipc::ensure_ok(&response)?;
                    Ok(())
                }) {
                    Ok(()) => {}
                    Err(error) => send_error(
                        events,
                        ctx,
                        format!("Не удалось восстановить горячую клавишу: {error:#}"),
                    ),
                }
            }
        }
        #[cfg(windows)]
        Command::RestartDaemon {
            hotkey: next,
            resume,
        } => {
            *hotkey = next;
            let _ = ipc::send("OFF");
            let _ = ipc::send("QUIT");

            let deadline = Instant::now() + Duration::from_secs(3);
            let mut stopped = false;
            while Instant::now() < deadline {
                if ipc::send("STATUS").is_err() {
                    stopped = true;
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }

            if !stopped {
                send_error(
                    events,
                    ctx,
                    "Не удалось остановить старый демон для обновления горячей клавиши."
                        .to_string(),
                );
                return;
            }

            match ipc::send_or_spawn_with_hotkey("STATUS", hotkey) {
                Ok(response) => {
                    if let Err(error) = ipc::ensure_ok(&response) {
                        send_error(
                            events,
                            ctx,
                            format!("Не удалось перезапустить демон: {error:#}"),
                        );
                        return;
                    }
                    if let Some(settings) = resume {
                        let command = settings.to_wire_command("ON");
                        if let Err(error) = ipc::send(&command).and_then(|response| {
                            ipc::ensure_ok(&response)?;
                            Ok(())
                        }) {
                            send_error(
                                events,
                                ctx,
                                format!("Демон перезапущен, но клики не возобновлены: {error:#}"),
                            );
                        }
                    }
                    *online = true;
                    *last_offline = None;
                    *last_status = None;
                }
                Err(error) => send_error(
                    events,
                    ctx,
                    format!("Не удалось перезапустить демон: {error:#}"),
                ),
            }
        }
        Command::QuitDaemon => {
            let _ = ipc::send("OFF");
            match ipc::send("QUIT").and_then(|response| {
                ipc::ensure_ok(&response)?;
                Ok(())
            }) {
                Ok(()) => {
                    *online = false;
                    *last_offline = None;
                    let _ = events.send(Event::Stopped);
                }
                Err(error) => {
                    send_error(
                        events,
                        ctx,
                        format!("Не удалось завершить демон: {error:#}"),
                    );
                }
            }
        }
        Command::Shutdown => {}
    }

    if !quit_daemon {
        poll(events, ctx, online, last_offline, last_status);
    }
    ctx.request_repaint();
}

fn poll(
    events: &Sender<Event>,
    ctx: &egui::Context,
    online: &mut bool,
    last_offline: &mut Option<String>,
    last_status: &mut Option<Status>,
) {
    match ipc::send("STATUS") {
        Ok(response) => match Status::from_wire(&response) {
            Ok(status) => {
                *online = true;
                *last_offline = None;
                if last_status.as_ref() != Some(&status) {
                    *last_status = Some(status.clone());
                    let _ = events.send(Event::Status(status));
                    ctx.request_repaint();
                }
            }
            Err(error) => {
                send_error(events, ctx, format!("Некорректный ответ демона: {error:#}"));
            }
        },
        Err(error) => {
            *online = false;
            *last_status = None;
            let message = format!("{error:#}");
            if last_offline.as_deref() != Some(message.as_str()) {
                *last_offline = Some(message.clone());
                let _ = events.send(Event::Offline(message));
                ctx.request_repaint();
            }
        }
    }
}

fn send_error(events: &Sender<Event>, ctx: &egui::Context, message: String) {
    let _ = events.send(Event::Error(message));
    ctx.request_repaint();
}
