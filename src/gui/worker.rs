use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use egui;

use crate::i18n::{self, Key, Lang, Language};
use crate::ipc;
use crate::state::{Settings, Status};

/// Clicking changes its counters many times per second, so poll quickly while
/// a session runs and slowly when the window is only showing an idle daemon.
const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(1000);
/// Re-send an unchanged status once a second. This doubles as a repaint
/// heartbeat: if the software backend misses a paint request, the window still
/// refreshes and never shows stale "clicking stopped" data.
const STATUS_HEARTBEAT: Duration = Duration::from_millis(1000);
/// A bounded event queue keeps memory flat when the window is hidden in the
/// tray and the GUI cannot drain status updates.
const EVENT_QUEUE: usize = 32;

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
    pub handle: Option<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

impl Worker {
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }
}

struct Session {
    online: bool,
    last_offline: Option<String>,
    last_status: Option<Status>,
    last_status_sent: Instant,
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
        let (event_tx, event_rx) = sync_channel(EVENT_QUEUE);
        let lang = language.resolve();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();

        let handle = thread::Builder::new()
            .name("savv-autoclicker-gui-ipc".to_string())
            .spawn(move || {
                run(
                    command_rx,
                    event_tx,
                    ctx,
                    hotkey,
                    hold_hotkey,
                    lang,
                    thread_stop,
                )
            })?;

        Ok(Self {
            commands: command_tx,
            events: event_rx,
            handle: Some(handle),
            stop,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    commands: Receiver<Command>,
    events: SyncSender<Event>,
    ctx: egui::Context,
    hotkey: String,
    hold_hotkey: String,
    lang: Lang,
    stop: Arc<AtomicBool>,
) {
    let mut session = Session {
        online: false,
        last_offline: None,
        last_status: None,
        last_status_sent: Instant::now(),
        hotkey,
        hold_hotkey,
        lang,
    };
    let mut next_poll = Instant::now();

    // Bring the daemon up as soon as the window opens so clicking and the
    // global hotkey are usable without pressing any button. On Windows the
    // GUI owns the daemon; on Linux the daemon runs independently.
    ensure_daemon(&events, &ctx, &mut session);

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }

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
                next_poll = Instant::now() + poll_interval(&session);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if Instant::now() >= next_poll {
            poll(&events, &ctx, &mut session);
            next_poll = Instant::now() + poll_interval(&session);
        }
    }
}

fn poll_interval(session: &Session) -> Duration {
    let busy = session
        .last_status
        .as_ref()
        .is_some_and(|status| status.enabled || status.starting);
    if busy {
        ACTIVE_POLL_INTERVAL
    } else {
        IDLE_POLL_INTERVAL
    }
}

fn handle_command(
    command: Command,
    events: &SyncSender<Event>,
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
                    let _ = events.try_send(Event::Stopped);
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

fn poll(events: &SyncSender<Event>, ctx: &egui::Context, session: &mut Session) {
    match ipc::send("STATUS") {
        Ok(response) => match Status::from_wire(&response) {
            Ok(status) => {
                session.online = true;
                session.last_offline = None;
                let changed = session.last_status.as_ref() != Some(&status);
                let heartbeat = session.last_status_sent.elapsed() >= STATUS_HEARTBEAT;

                if changed || heartbeat {
                    if changed {
                        session.last_status = Some(status.clone());
                    }
                    // Dropping a status when the queue is full is safe: the
                    // next heartbeat carries an even newer one.
                    let _ = events.try_send(Event::Status(Box::new(status)));
                    session.last_status_sent = Instant::now();
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
                let _ = events.try_send(Event::Offline(message));
                ctx.request_repaint();
            }
        }
    }
}

fn send_error(events: &SyncSender<Event>, ctx: &egui::Context, message: String) {
    let _ = events.try_send(Event::Error(message));
    ctx.request_repaint();
}

fn message(lang: Lang, key: Key, detail: impl std::fmt::Display) -> String {
    let detail = detail.to_string();
    i18n::tr(lang, key)
        .replace("{:#}", &detail)
        .replace("{}", &detail)
}

fn ensure_daemon(events: &SyncSender<Event>, ctx: &egui::Context, session: &mut Session) {
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
