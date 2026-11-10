use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use egui;

use crate::i18n::{self, Key, Lang, Language};
use crate::ipc;
use crate::state::{Settings, Status};

const POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub enum Command {
    Start(Settings),
    Stop,
    Apply(Settings),
    SetHotkey(String),
    SetHoldHotkey(String),
    SetLanguage(Language),
    #[cfg(windows)]
    PauseHotkey,
    #[cfg(windows)]
    ResumeHotkey,
    RestartDaemon {
        hotkey: String,
        hold_hotkey: String,
        resume: Option<Settings>,
    },
    #[cfg(not(windows))]
    QuitDaemon,
    Shutdown,
}

#[derive(Debug)]
pub enum Event {
    Status(Box<Status>),
    Offline(String),
    Error(String),
    #[cfg(not(windows))]
    Stopped,
}

pub struct Worker {
    pub commands: Sender<Command>,
    pub events: Receiver<Event>,
}

struct Session {
    online: bool,
    last_offline: Option<String>,
    last_status: Option<Status>,
    hotkey: String,
    hold_hotkey: String,
    lang: Lang,
}

impl Worker {
    pub fn spawn(
        ctx: egui::Context,
        hotkey: String,
        hold_hotkey: String,
        language: Language,
    ) -> std::io::Result<Self> {
        let (command_tx, command_rx) = channel();
        let (event_tx, event_rx) = channel();
        let lang = language.resolve();

        thread::Builder::new()
            .name("autoclicker-gui-ipc".to_string())
            .spawn(move || run(command_rx, event_tx, ctx, hotkey, hold_hotkey, lang))?;

        Ok(Self {
            commands: command_tx,
            events: event_rx,
        })
    }
}

fn run(
    commands: Receiver<Command>,
    events: Sender<Event>,
    ctx: egui::Context,
    hotkey: String,
    hold_hotkey: String,
    lang: Lang,
) {
    let mut session = Session {
        online: false,
        last_offline: None,
        last_status: None,
        hotkey,
        hold_hotkey,
        lang,
    };
    let mut next_poll = Instant::now();

    // On Windows the GUI owns the daemon: bring it up as soon as the window
    // opens so the global hotkey is usable without pressing any button.
    #[cfg(windows)]
    ensure_daemon(&events, &ctx, &mut session);

    loop {
        let timeout = next_poll.saturating_duration_since(Instant::now());
        match commands.recv_timeout(timeout) {
            Ok(Command::Shutdown) => {
                let _ = ipc::send("HOTKEY_RESUME");
                let _ = ipc::send("OFF");
                #[cfg(windows)]
                let _ = ipc::send("QUIT");
                break;
            }
            Ok(command) => {
                handle_command(command, &events, &ctx, &mut session);
                next_poll = Instant::now() + POLL_INTERVAL;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if Instant::now() >= next_poll {
            poll(&events, &ctx, &mut session);
            next_poll = Instant::now() + POLL_INTERVAL;
        }
    }
}

fn handle_command(
    command: Command,
    events: &Sender<Event>,
    ctx: &egui::Context,
    session: &mut Session,
) {
    #[cfg(not(windows))]
    let quit_daemon = matches!(command, Command::QuitDaemon);
    #[cfg(windows)]
    let quit_daemon = false;

    match command {
        Command::Start(settings) => {
            let command = settings.to_wire_command("ON");
            match ipc::send_or_spawn_with_hotkeys(
                &command,
                &session.hotkey,
                Some(session.hold_hotkey.as_str()),
            ) {
                Ok(response) => {
                    if let Err(error) = ipc::ensure_ok(&response) {
                        send_error(
                            events,
                            ctx,
                            message(session.lang, Key::WorkerStartClicks, format!("{error:#}")),
                        );
                    } else {
                        session.online = true;
                        session.last_offline = None;
                    }
                }
                Err(error) => send_error(
                    events,
                    ctx,
                    message(session.lang, Key::WorkerStartDaemon, format!("{error:#}")),
                ),
            }
        }
        Command::Stop => {
            match ipc::send("OFF").and_then(|response| {
                ipc::ensure_ok(&response)?;
                Ok(())
            }) {
                Ok(()) => {
                    session.online = true;
                    session.last_offline = None;
                }
                Err(error) => {
                    send_error(
                        events,
                        ctx,
                        message(session.lang, Key::WorkerStopClicks, format!("{error:#}")),
                    );
                }
            }
        }
        Command::Apply(settings) => {
            if session.online {
                let command = settings.to_wire_command("SET");
                match ipc::send(&command).and_then(|response| {
                    ipc::ensure_ok(&response)?;
                    Ok(())
                }) {
                    Ok(()) => {
                        session.last_offline = None;
                    }
                    Err(error) => {
                        send_error(
                            events,
                            ctx,
                            message(session.lang, Key::WorkerApply, format!("{error:#}")),
                        );
                    }
                }
            }
        }
        Command::SetHotkey(next) => {
            session.hotkey = next;
            if session.online {
                let command = format!("HOTKEY {}", session.hotkey);
                match ipc::send(&command).and_then(|response| {
                    ipc::ensure_ok(&response)?;
                    Ok(())
                }) {
                    Ok(()) => {
                        session.last_offline = None;
                    }
                    Err(error) => {
                        send_error(
                            events,
                            ctx,
                            message(session.lang, Key::WorkerHotkey, format!("{error:#}")),
                        );
                    }
                }
            }
        }
        Command::SetHoldHotkey(next) => {
            session.hold_hotkey = next;
            if session.online {
                let command = if session.hold_hotkey.is_empty() {
                    String::from("HOLD_HOTKEY none")
                } else {
                    format!("HOLD_HOTKEY {}", session.hold_hotkey)
                };
                match ipc::send(&command).and_then(|response| {
                    ipc::ensure_ok(&response)?;
                    Ok(())
                }) {
                    Ok(()) => {
                        session.last_offline = None;
                    }
                    Err(error) => {
                        send_error(
                            events,
                            ctx,
                            message(session.lang, Key::WorkerHoldHotkey, format!("{error:#}")),
                        );
                    }
                }
            }
        }
        Command::SetLanguage(language) => {
            session.lang = language.resolve();
        }
        #[cfg(windows)]
        Command::PauseHotkey => {
            if session.online {
                match ipc::send("HOTKEY_PAUSE").and_then(|response| {
                    ipc::ensure_ok(&response)?;
                    Ok(())
                }) {
                    Ok(()) => {}
                    Err(error) => send_error(
                        events,
                        ctx,
                        message(session.lang, Key::WorkerPauseHotkey, format!("{error:#}")),
                    ),
                }
            }
        }
        #[cfg(windows)]
        Command::ResumeHotkey => {
            if session.online {
                match ipc::send("HOTKEY_RESUME").and_then(|response| {
                    ipc::ensure_ok(&response)?;
                    Ok(())
                }) {
                    Ok(()) => {}
                    Err(error) => send_error(
                        events,
                        ctx,
                        message(session.lang, Key::WorkerResumeHotkey, format!("{error:#}")),
                    ),
                }
            }
        }
        Command::RestartDaemon {
            hotkey: next_hotkey,
            hold_hotkey: next_hold_hotkey,
            resume,
        } => {
            session.hotkey = next_hotkey;
            session.hold_hotkey = next_hold_hotkey;
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
                    i18n::tr(session.lang, Key::WorkerStopOldDaemon).to_string(),
                );
                return;
            }

            match ipc::send_or_spawn_with_hotkeys(
                "STATUS",
                &session.hotkey,
                Some(session.hold_hotkey.as_str()),
            ) {
                Ok(response) => {
                    if let Err(error) = ipc::ensure_ok(&response) {
                        send_error(
                            events,
                            ctx,
                            message(session.lang, Key::WorkerRestartDaemon, format!("{error:#}")),
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
                                message(
                                    session.lang,
                                    Key::WorkerResumeFailed,
                                    format!("{error:#}"),
                                ),
                            );
                        }
                    }
                    session.online = true;
                    session.last_offline = None;
                    session.last_status = None;
                }
                Err(error) => send_error(
                    events,
                    ctx,
                    message(session.lang, Key::WorkerRestartDaemon, format!("{error:#}")),
                ),
            }
        }
        #[cfg(not(windows))]
        Command::QuitDaemon => {
            let _ = ipc::send("OFF");
            match ipc::send("QUIT").and_then(|response| {
                ipc::ensure_ok(&response)?;
                Ok(())
            }) {
                Ok(()) => {
                    session.online = false;
                    session.last_offline = None;
                    let _ = events.send(Event::Stopped);
                }
                Err(error) => {
                    send_error(
                        events,
                        ctx,
                        message(session.lang, Key::WorkerQuitFailed, format!("{error:#}")),
                    );
                }
            }
        }
        Command::Shutdown => {}
    }

    if !quit_daemon {
        poll(events, ctx, session);
    }
    ctx.request_repaint();
}

fn poll(events: &Sender<Event>, ctx: &egui::Context, session: &mut Session) {
    match ipc::send("STATUS") {
        Ok(response) => match Status::from_wire(&response) {
            Ok(status) => {
                session.online = true;
                session.last_offline = None;
                if session.last_status.as_ref() != Some(&status) {
                    session.last_status = Some(status.clone());
                    let _ = events.send(Event::Status(Box::new(status)));
                    ctx.request_repaint();
                }
            }
            Err(error) => {
                send_error(
                    events,
                    ctx,
                    message(session.lang, Key::WorkerBadResponse, format!("{error:#}")),
                );
            }
        },
        Err(error) => {
            session.online = false;
            session.last_status = None;
            let message = format!("{error:#}");
            if session.last_offline.as_deref() != Some(message.as_str()) {
                session.last_offline = Some(message.clone());
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

fn message(lang: Lang, key: Key, detail: impl std::fmt::Display) -> String {
    let detail = detail.to_string();
    i18n::tr(lang, key)
        .replace("{:#}", &detail)
        .replace("{}", &detail)
}

#[cfg(windows)]
fn ensure_daemon(events: &Sender<Event>, ctx: &egui::Context, session: &mut Session) {
    match ipc::send_or_spawn_with_hotkeys(
        "STATUS",
        &session.hotkey,
        Some(session.hold_hotkey.as_str()),
    ) {
        Ok(response) => match ipc::ensure_ok(&response) {
            Ok(()) => {
                session.online = true;
                session.last_offline = None;
            }
            Err(error) => send_error(
                events,
                ctx,
                message(session.lang, Key::WorkerStartDaemon, format!("{error:#}")),
            ),
        },
        Err(error) => send_error(
            events,
            ctx,
            message(session.lang, Key::WorkerStartDaemon, format!("{error:#}")),
        ),
    }
}
