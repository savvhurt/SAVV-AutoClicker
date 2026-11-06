use std::mem::size_of;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, bail};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOD_NOREPEAT, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEINPUT, RegisterHotKey, SendInput, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, KillTimer, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetTimer,
    WM_HOTKEY, WM_TIMER,
};

use crate::hotkey::Hotkey;
use crate::state::{Button, SharedState};

pub struct WindowsBackend;

impl super::Backend for WindowsBackend {
    fn press(&mut self, button: Button) -> Result<()> {
        send(button, true)
    }

    fn release(&mut self, button: Button) -> Result<()> {
        send(button, false)
    }
}

fn send(button: Button, down: bool) -> Result<()> {
    let flags = match (button, down) {
        (Button::Left, true) => MOUSEEVENTF_LEFTDOWN,
        (Button::Left, false) => MOUSEEVENTF_LEFTUP,
        (Button::Right, true) => MOUSEEVENTF_RIGHTDOWN,
        (Button::Right, false) => MOUSEEVENTF_RIGHTUP,
        (Button::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
        (Button::Middle, false) => MOUSEEVENTF_MIDDLEUP,
    };

    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    // SendInput can be blocked by UIPI when the target window runs elevated.
    let sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
    if sent == 0 {
        bail!("SendInput failed: {}", std::io::Error::last_os_error());
    }

    Ok(())
}

const HOTKEY_ID_A: i32 = 1;
const HOTKEY_ID_B: i32 = 2;
const WM_RELOAD_HOTKEY: u32 = 0x8000 + 1;
const WM_PAUSE_HOTKEY: u32 = 0x8000 + 2;
const WM_RESUME_HOTKEY: u32 = 0x8000 + 3;
const TIMER_RESUME: usize = 1;
const PAUSE_TIMEOUT_MS: u32 = 30_000;

static HOTKEY_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static HOTKEY_REQUEST_LOCK: Mutex<()> = Mutex::new(());
static HOTKEY_RESPONSE: Mutex<Option<Sender<std::result::Result<(), String>>>> = Mutex::new(None);

pub(crate) fn spawn_hotkey(shared: Arc<SharedState>, hotkey: Hotkey) -> Result<()> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();

    std::thread::Builder::new()
        .name("hotkey".into())
        .spawn(move || {
            // Make sure the thread message queue exists before publishing the id.
            let mut queued = MSG::default();
            let _ = unsafe { PeekMessageW(&mut queued, None, 0, 0, PM_NOREMOVE) };
            let thread_id = unsafe { GetCurrentThreadId() };
            HOTKEY_THREAD_ID.store(thread_id, Ordering::Release);

            let mut next_id = HOTKEY_ID_A;
            let mut active = None;
            let mut paused = None;
            match register_hotkey(next_id, &hotkey) {
                Ok(()) => {
                    shared.set_hotkey_active(hotkey.to_string());
                    active = Some((hotkey, next_id));
                    let _ = ready_tx.send(Ok(()));
                }
                Err(error) => {
                    shared.clear_hotkey_active();
                    shared.set_hotkey_error(format!("could not register global hotkey: {error}"));
                    let _ = ready_tx.send(Err(error));
                }
            }
            next_id = other_id(next_id);

            let mut message = MSG::default();
            loop {
                let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
                if result.0 == 0 || result.0 == -1 {
                    break;
                }

                match message.message {
                    WM_HOTKEY => {
                        shared.toggle();
                    }
                    WM_RELOAD_HOTKEY => {
                        handle_reload_hotkey(&shared, &mut active, &mut paused, &mut next_id);
                    }
                    WM_PAUSE_HOTKEY => {
                        handle_pause_hotkey(&shared, &mut active, &mut paused);
                    }
                    WM_RESUME_HOTKEY => {
                        handle_resume_hotkey(&shared, &mut active, &mut paused, &mut next_id, true);
                    }
                    WM_TIMER if message.wParam.0 == TIMER_RESUME => {
                        handle_resume_hotkey(
                            &shared,
                            &mut active,
                            &mut paused,
                            &mut next_id,
                            false,
                        );
                    }
                    _ => {}
                }
            }
        })?;

    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => bail!("could not register global hotkey: {error}"),
        Err(_) => bail!("hotkey thread did not report readiness"),
    }
}

pub(crate) fn request_hotkey(shared: &SharedState, combo: &str) -> Result<()> {
    let hotkey = Hotkey::parse(combo)?;
    let canonical = hotkey.to_string();
    shared.request_hotkey(canonical);
    send_hotkey_message(WM_RELOAD_HOTKEY)
}

pub(crate) fn pause_registration(_shared: &SharedState) -> Result<()> {
    send_hotkey_message(WM_PAUSE_HOTKEY)
}

pub(crate) fn resume_registration(_shared: &SharedState) -> Result<()> {
    send_hotkey_message(WM_RESUME_HOTKEY)
}

fn send_hotkey_message(message: u32) -> Result<()> {
    let _serial = HOTKEY_REQUEST_LOCK.lock().unwrap();

    let thread_id = HOTKEY_THREAD_ID.load(Ordering::Acquire);
    if thread_id == 0 {
        bail!("the global hotkey thread is not running");
    }

    let (response_tx, response_rx) = channel();
    *HOTKEY_RESPONSE.lock().unwrap() = Some(response_tx);

    if let Err(error) = unsafe { PostThreadMessageW(thread_id, message, WPARAM(0), LPARAM(0)) } {
        let _ = HOTKEY_RESPONSE.lock().unwrap().take();
        bail!("failed to wake the global hotkey thread: {error}");
    }

    match response_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => bail!("{error}"),
        Err(_) => {
            let _ = HOTKEY_RESPONSE.lock().unwrap().take();
            bail!("the global hotkey thread did not respond")
        }
    }
}

fn respond(result: std::result::Result<(), String>) {
    if let Some(sender) = HOTKEY_RESPONSE.lock().unwrap().take() {
        let _ = sender.send(result);
    }
}

fn register_hotkey(id: i32, hotkey: &Hotkey) -> std::result::Result<(), String> {
    unsafe { RegisterHotKey(None, id, hotkey.modifiers() | MOD_NOREPEAT, hotkey.vk()) }
        .map_err(|error| error.to_string())
}

fn handle_reload_hotkey(
    shared: &SharedState,
    active: &mut Option<(Hotkey, i32)>,
    paused: &mut Option<(Hotkey, i32)>,
    next_id: &mut i32,
) {
    let Some(combo) = shared.take_hotkey_request() else {
        respond(Ok(()));
        return;
    };

    let hotkey = match Hotkey::parse(&combo) {
        Ok(hotkey) => hotkey,
        Err(error) => {
            let message = format!("invalid hotkey: {error:#}");
            shared.set_hotkey_error(message.clone());
            respond(Err(message));
            return;
        }
    };

    let id = *next_id;
    match register_hotkey(id, &hotkey) {
        Ok(()) => {
            if let Some((_, old_id)) = active.replace((hotkey.clone(), id)) {
                let _ = unsafe { UnregisterHotKey(None, old_id) };
            }
            *paused = None;
            *next_id = other_id(id);
            stop_resume_timer();
            shared.set_hotkey_active(hotkey.to_string());
            respond(Ok(()));
        }
        Err(error) => {
            let (message, restored) = match active.as_ref() {
                Some((current, _)) => (
                    format!(
                        "could not register global hotkey: {error}; {} is still active",
                        current.label()
                    ),
                    true,
                ),
                None => match paused.take() {
                    Some((previous, previous_id)) => {
                        match register_hotkey(previous_id, &previous) {
                            Ok(()) => {
                                let canonical = previous.to_string();
                                *active = Some((previous, previous_id));
                                (
                                    format!(
                                        "could not register global hotkey: {error}; {} is still active",
                                        canonical
                                    ),
                                    true,
                                )
                            }
                            Err(restore_error) => (
                                format!(
                                    "could not register global hotkey: {error}; previous hotkey could not be restored: {restore_error}"
                                ),
                                false,
                            ),
                        }
                    }
                    None => (format!("could not register global hotkey: {error}"), false),
                },
            };

            if restored {
                if let Some((current, _)) = active.as_ref() {
                    shared.set_hotkey_active(current.to_string());
                }
            } else {
                shared.clear_hotkey_active();
            }
            stop_resume_timer();
            shared.set_hotkey_paused(false);
            shared.set_hotkey_error(message.clone());
            respond(Err(message));
        }
    }
}

fn handle_pause_hotkey(
    shared: &SharedState,
    active: &mut Option<(Hotkey, i32)>,
    paused: &mut Option<(Hotkey, i32)>,
) {
    if let Some((hotkey, id)) = active.take() {
        let _ = unsafe { UnregisterHotKey(None, id) };
        *paused = Some((hotkey, id));
    }

    shared.clear_hotkey_active();
    shared.set_hotkey_paused(true);
    schedule_resume_timer();
    respond(Ok(()));
}

fn handle_resume_hotkey(
    shared: &SharedState,
    active: &mut Option<(Hotkey, i32)>,
    paused: &mut Option<(Hotkey, i32)>,
    next_id: &mut i32,
    reply: bool,
) {
    stop_resume_timer();
    shared.set_hotkey_paused(false);

    if active.is_some() {
        if reply {
            respond(Ok(()));
        }
        return;
    }

    let Some((hotkey, _)) = paused.take() else {
        shared.clear_hotkey_active();
        if reply {
            respond(Ok(()));
        }
        return;
    };

    let id = *next_id;
    match register_hotkey(id, &hotkey) {
        Ok(()) => {
            *next_id = other_id(id);
            shared.set_hotkey_active(hotkey.to_string());
            *active = Some((hotkey, id));
            if reply {
                respond(Ok(()));
            }
        }
        Err(error) => {
            let message = format!("could not restore global hotkey: {error}");
            shared.clear_hotkey_active();
            shared.set_hotkey_error(message.clone());
            *paused = Some((hotkey, id));
            if reply {
                respond(Err(message));
            }
        }
    }
}

fn schedule_resume_timer() {
    let _ = unsafe { SetTimer(None, TIMER_RESUME, PAUSE_TIMEOUT_MS, None) };
}

fn stop_resume_timer() {
    let _ = unsafe { KillTimer(None, TIMER_RESUME) };
}

fn other_id(id: i32) -> i32 {
    if id == HOTKEY_ID_A {
        HOTKEY_ID_B
    } else {
        HOTKEY_ID_A
    }
}
