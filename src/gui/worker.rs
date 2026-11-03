use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::ipc;
use crate::state::{Settings, Status};

const POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub enum Command {
    Start(Settings),
    Stop,
    Apply(Settings),
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
    pub fn spawn(ctx: egui::Context) -> std::io::Result<Self> {
        let (command_tx, command_rx) = channel();
        let (event_tx, event_rx) = channel();

        thread::Builder::new()
            .name("autoclicker-gui-ipc".to_string())
            .spawn(move || run(command_rx, event_tx, ctx))?;

        Ok(Self {
            commands: command_tx,
            events: event_rx,
        })
    }
}

fn run(commands: Receiver<Command>, events: Sender<Event>, ctx: egui::Context) {
    let mut online = false;
    let mut last_offline = None;
    let mut next_poll = Instant::now();

    loop {
        let timeout = next_poll.saturating_duration_since(Instant::now());
        match commands.recv_timeout(timeout) {
            Ok(Command::Shutdown) => {
                let _ = ipc::send("OFF");
                break;
            }
            Ok(command) => {
                handle_command(command, &events, &ctx, &mut online, &mut last_offline);
                next_poll = Instant::now() + POLL_INTERVAL;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if Instant::now() >= next_poll {
            poll(&events, &ctx, &mut online, &mut last_offline);
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
) {
    let quit_daemon = matches!(command, Command::QuitDaemon);

    match command {
        Command::Start(settings) => {
            let command = settings.to_wire_command("ON");
            match ipc::send_or_spawn(&command) {
                Ok(response) => {
                    if let Err(error) = ipc::ensure_ok(&response) {
                        send_error(events, format!("Не удалось запустить клики: {error:#}"));
                    } else {
                        *online = true;
                        *last_offline = None;
                    }
                }
                Err(error) => send_error(events, format!("Не удалось запустить демон: {error:#}")),
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
                    send_error(events, format!("Не удалось остановить клики: {error:#}"));
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
                        send_error(events, format!("Не удалось применить настройки: {error:#}"));
                    }
                }
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
                    send_error(events, format!("Не удалось завершить демон: {error:#}"));
                }
            }
        }
        Command::Shutdown => {}
    }

    if !quit_daemon {
        poll(events, ctx, online, last_offline);
    }
    ctx.request_repaint();
}

fn poll(
    events: &Sender<Event>,
    ctx: &egui::Context,
    online: &mut bool,
    last_offline: &mut Option<String>,
) {
    match ipc::send("STATUS") {
        Ok(response) => match Status::from_wire(&response) {
            Ok(status) => {
                *online = true;
                *last_offline = None;
                let _ = events.send(Event::Status(status));
            }
            Err(error) => {
                send_error(events, format!("Некорректный ответ демона: {error:#}"));
            }
        },
        Err(error) => {
            *online = false;
            let message = format!("{error:#}");
            if last_offline.as_deref() != Some(message.as_str()) {
                *last_offline = Some(message.clone());
                let _ = events.send(Event::Offline(message));
            }
        }
    }

    ctx.request_repaint();
}

fn send_error(events: &Sender<Event>, message: String) {
    let _ = events.send(Event::Error(message));
}
