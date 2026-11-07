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

const HOTKEY_ID_TOGGLE_A: i32 = 1;
const HOTKEY_ID_TOGGLE_B: i32 = 2;
const HOTKEY_ID_HOLD_A: i32 = 3;
const HOTKEY_ID_HOLD_B: i32 = 4;

const WM_RELOAD_TOGGLE: u32 = 0x8000 + 1;
const WM_RELOAD_HOLD: u32 = 0x8000 + 2;
const WM_PAUSE_HOTKEY: u32 = 0x8000 + 3;
const WM_RESUME_HOTKEY: u32 = 0x8000 + 4;
const TIMER_RESUME: usize = 1;
const TIMER_HOLD_RELEASE: usize = 2;
const PAUSE_TIMEOUT_MS: u32 = 30_000;
const HOLD_POLL_MS: u32 = 15;

static HOTKEY_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static HOTKEY_REQUEST_LOCK: Mutex<()> = Mutex::new(());
static HOTKEY_RESPONSE: Mutex<Option<Sender<std::result::Result<(), String>>>> = Mutex::new(None);

#[derive(Clone)]
struct Registration {
    hotkey: Hotkey,
    id: i32,
}

struct Registry {
    toggle: Option<Registration>,
    hold: Option<Registration>,
    paused_toggle: Option<Registration>,
    paused_hold: Option<Registration>,
    next_toggle_id: i32,
    next_hold_id: i32,
}

pub(crate) fn spawn_hotkey(
    shared: Arc<SharedState>,
    toggle: Hotkey,
    hold: Option<Hotkey>,
) -> Result<()> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();

    std::thread::Builder::new()
        .name("hotkey".into())
        .spawn(move || {
            // Make sure the thread message queue exists before publishing the id.
            let mut queued = MSG::default();
            let _ = unsafe { PeekMessageW(&mut queued, None, 0, 0, PM_NOREMOVE) };
            let thread_id = unsafe { GetCurrentThreadId() };
            HOTKEY_THREAD_ID.store(thread_id, Ordering::Release);

            let mut registry = Registry {
                toggle: None,
                hold: None,
                paused_toggle: None,
                paused_hold: None,
                next_toggle_id: HOTKEY_ID_TOGGLE_A,
                next_hold_id: HOTKEY_ID_HOLD_A,
            };

            let toggle_result = start_toggle(&shared, &mut registry, &toggle);
            let hold_result = match &hold {
                Some(hold) if hold.to_string() == toggle.to_string() => {
                    Err("the hold hotkey must differ from the toggle hotkey".to_string())
                }
                Some(hold) => start_hold(&shared, &mut registry, hold),
                None => Ok(()),
            };

            if let Err(error) = &hold_result {
                shared.clear_hold_hotkey_active();
                shared.set_hotkey_error(error.clone());
            }

            let ready = match toggle_result {
                Ok(()) => Ok(()),
                Err(error) => {
                    shared.clear_hotkey_active();
                    shared.set_hotkey_error(error.clone());
                    Err(error)
                }
            };
            let _ = ready_tx.send(ready);

            let mut message = MSG::default();
            loop {
                let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
                if result.0 == 0 || result.0 == -1 {
                    break;
                }

                match message.message {
                    WM_HOTKEY => handle_hotkey_press(&shared, &registry, message.wParam.0 as i32),
                    WM_RELOAD_TOGGLE => handle_reload_toggle(&shared, &mut registry),
                    WM_RELOAD_HOLD => handle_reload_hold(&shared, &mut registry),
                    WM_PAUSE_HOTKEY => handle_pause(&shared, &mut registry),
                    WM_RESUME_HOTKEY => {
                        handle_resume(&shared, &mut registry, true);
                    }
                    WM_TIMER if message.wParam.0 == TIMER_RESUME => {
                        handle_resume(&shared, &mut registry, false);
                    }
                    WM_TIMER if message.wParam.0 == TIMER_HOLD_RELEASE => {
                        handle_hold_release(&shared, &registry);
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

pub(crate) fn request_hotkey(_shared: &SharedState, combo: &str) -> Result<()> {
    let hotkey = Hotkey::parse(combo)?;
    _shared.request_hotkey(hotkey.to_string());
    send_hotkey_message(WM_RELOAD_TOGGLE)
}

pub(crate) fn request_hold_hotkey(_shared: &SharedState, combo: Option<&str>) -> Result<()> {
    let canonical = match combo {
        Some(combo) => Hotkey::parse(combo)?.to_string(),
        None => String::new(),
    };
    _shared.request_hold_hotkey(canonical);
    send_hotkey_message(WM_RELOAD_HOLD)
}

pub(crate) fn pause_registration(_shared: &SharedState) -> Result<()> {
    send_hotkey_message(WM_PAUSE_HOTKEY)
}

pub(crate) fn resume_registration(_shared: &SharedState) -> Result<()> {
    send_hotkey_message(WM_RESUME_HOTKEY)
}

fn start_toggle(
    shared: &SharedState,
    registry: &mut Registry,
    hotkey: &Hotkey,
) -> std::result::Result<(), String> {
    let id = registry.next_toggle_id;
    match register_hotkey(id, hotkey) {
        Ok(()) => {
            registry.next_toggle_id = other_toggle_id(id);
            registry.toggle = Some(Registration {
                hotkey: hotkey.clone(),
                id,
            });
            shared.set_hotkey_active(hotkey.to_string());
            Ok(())
        }
        Err(error) => Err(format!(
            "could not register global hotkey {}: {error}",
            hotkey.label()
        )),
    }
}

fn start_hold(
    shared: &SharedState,
    registry: &mut Registry,
    hotkey: &Hotkey,
) -> std::result::Result<(), String> {
    let id = registry.next_hold_id;
    match register_hotkey(id, hotkey) {
        Ok(()) => {
            registry.next_hold_id = other_hold_id(id);
            registry.hold = Some(Registration {
                hotkey: hotkey.clone(),
                id,
            });
            shared.set_hold_hotkey_active(hotkey.to_string());
            Ok(())
        }
        Err(error) => Err(format!(
            "could not register hold hotkey {}: {error}",
            hotkey.label()
        )),
    }
}

fn handle_hotkey_press(shared: &SharedState, registry: &Registry, id: i32) {
    if registry.toggle.as_ref().is_some_and(|item| item.id == id) {
        shared.toggle();
    } else if registry.hold.as_ref().is_some_and(|item| item.id == id) && shared.hold_press() {
        start_hold_timer();
    }
}

fn handle_hold_release(shared: &SharedState, registry: &Registry) {
    if !shared.status().hold_active {
        stop_hold_timer();
        return;
    }

    let still_down = registry
        .hold
        .as_ref()
        .is_some_and(|item| item.hotkey.is_down());
    if !still_down {
        shared.hold_release();
        stop_hold_timer();
    }
}

fn handle_reload_toggle(shared: &SharedState, registry: &mut Registry) {
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
    let canonical = hotkey.to_string();

    if shared.status().hold_hotkey.eq_ignore_ascii_case(&canonical) {
        let message = "the toggle and hold hotkeys must differ".to_string();
        shared.set_hotkey_error(message.clone());
        respond(Err(message));
        return;
    }

    if let Some(current) = &registry.toggle {
        if current.hotkey == hotkey {
            shared.set_hotkey_active(canonical);
            respond(Ok(()));
            return;
        }
    }

    stop_hold_session(shared);

    if let Some(paused) = &mut registry.paused_toggle {
        paused.hotkey = hotkey;
        shared.clear_hotkey_active();
        respond(Ok(()));
        return;
    }

    let id = registry.next_toggle_id;
    match register_hotkey(id, &hotkey) {
        Ok(()) => {
            if let Some(old) = registry.toggle.replace(Registration {
                hotkey: hotkey.clone(),
                id,
            }) {
                let _ = unsafe { UnregisterHotKey(None, old.id) };
            }
            registry.next_toggle_id = other_toggle_id(id);
            shared.set_hotkey_active(canonical);
            respond(Ok(()));
        }
        Err(error) => {
            let message = match &registry.toggle {
                Some(current) => format!(
                    "could not register global hotkey {}: {error}; {} is still active",
                    hotkey.label(),
                    current.hotkey.label()
                ),
                None => format!(
                    "could not register global hotkey {}: {error}",
                    hotkey.label()
                ),
            };
            shared.set_hotkey_error(message.clone());
            respond(Err(message));
        }
    }
}

fn handle_reload_hold(shared: &SharedState, registry: &mut Registry) {
    let Some(combo) = shared.take_hold_hotkey_request() else {
        respond(Ok(()));
        return;
    };

    if combo.trim().is_empty() {
        stop_hold_session(shared);
        if let Some(current) = registry.hold.take() {
            let _ = unsafe { UnregisterHotKey(None, current.id) };
        }
        registry.paused_hold = None;
        shared.clear_hold_hotkey_active();
        respond(Ok(()));
        return;
    }

    let hotkey = match Hotkey::parse(&combo) {
        Ok(hotkey) => hotkey,
        Err(error) => {
            let message = format!("invalid hold hotkey: {error:#}");
            shared.set_hotkey_error(message.clone());
            respond(Err(message));
            return;
        }
    };
    let canonical = hotkey.to_string();

    if shared.hotkey_requested().eq_ignore_ascii_case(&canonical) {
        let message = "the toggle and hold hotkeys must differ".to_string();
        shared.set_hotkey_error(message.clone());
        respond(Err(message));
        return;
    }

    if let Some(current) = &registry.hold {
        if current.hotkey == hotkey {
            shared.set_hold_hotkey_active(canonical);
            respond(Ok(()));
            return;
        }
    }

    stop_hold_session(shared);

    if let Some(paused) = &mut registry.paused_hold {
        paused.hotkey = hotkey;
        shared.clear_hold_hotkey_active();
        respond(Ok(()));
        return;
    }

    let id = registry.next_hold_id;
    match register_hotkey(id, &hotkey) {
        Ok(()) => {
            if let Some(old) = registry.hold.replace(Registration {
                hotkey: hotkey.clone(),
                id,
            }) {
                let _ = unsafe { UnregisterHotKey(None, old.id) };
            }
            registry.next_hold_id = other_hold_id(id);
            shared.set_hold_hotkey_active(canonical);
            respond(Ok(()));
        }
        Err(error) => {
            let message = match &registry.hold {
                Some(current) => format!(
                    "could not register hold hotkey {}: {error}; {} is still active",
                    hotkey.label(),
                    current.hotkey.label()
                ),
                None => format!("could not register hold hotkey {}: {error}", hotkey.label()),
            };
            shared.set_hotkey_error(message.clone());
            respond(Err(message));
        }
    }
}

fn handle_pause(shared: &SharedState, registry: &mut Registry) {
    stop_hold_session(shared);

    if let Some(current) = registry.toggle.take() {
        let _ = unsafe { UnregisterHotKey(None, current.id) };
        registry.paused_toggle = Some(current);
    } else if registry.paused_toggle.is_none() {
        if let Ok(hotkey) = Hotkey::parse(&shared.hotkey_requested()) {
            let id = registry.next_toggle_id;
            registry.next_toggle_id = other_toggle_id(id);
            registry.paused_toggle = Some(Registration { hotkey, id });
        }
    }

    if let Some(current) = registry.hold.take() {
        let _ = unsafe { UnregisterHotKey(None, current.id) };
        registry.paused_hold = Some(current);
    } else if registry.paused_hold.is_none() {
        let requested = shared.hold_hotkey_requested();
        if !requested.is_empty() {
            if let Ok(hotkey) = Hotkey::parse(&requested) {
                let id = registry.next_hold_id;
                registry.next_hold_id = other_hold_id(id);
                registry.paused_hold = Some(Registration { hotkey, id });
            }
        }
    }

    shared.clear_hotkey_active();
    shared.clear_hold_hotkey_active();
    shared.set_hotkey_paused(true);
    schedule_resume_timer();
    respond(Ok(()));
}

fn handle_resume(shared: &SharedState, registry: &mut Registry, reply: bool) {
    stop_resume_timer();
    shared.set_hotkey_paused(false);

    let mut first_error = None;

    if registry.toggle.is_none() {
        if let Some(paused) = registry.paused_toggle.take() {
            match register_hotkey(paused.id, &paused.hotkey) {
                Ok(()) => {
                    registry.next_toggle_id = other_toggle_id(paused.id);
                    shared.set_hotkey_active(paused.hotkey.to_string());
                    registry.toggle = Some(paused);
                }
                Err(error) => {
                    let message = format!(
                        "could not restore global hotkey {}: {error}",
                        paused.hotkey.label()
                    );
                    registry.paused_toggle = Some(paused);
                    shared.clear_hotkey_active();
                    shared.set_hotkey_error(message.clone());
                    first_error.get_or_insert(message);
                }
            }
        }
    }

    if registry.hold.is_none() {
        if let Some(paused) = registry.paused_hold.take() {
            match register_hotkey(paused.id, &paused.hotkey) {
                Ok(()) => {
                    registry.next_hold_id = other_hold_id(paused.id);
                    shared.set_hold_hotkey_active(paused.hotkey.to_string());
                    registry.hold = Some(paused);
                }
                Err(error) => {
                    let message = format!(
                        "could not restore hold hotkey {}: {error}",
                        paused.hotkey.label()
                    );
                    registry.paused_hold = Some(paused);
                    shared.clear_hold_hotkey_active();
                    if first_error.is_none() {
                        shared.set_hotkey_error(message.clone());
                    }
                    first_error.get_or_insert(message);
                }
            }
        }
    }

    if reply {
        match first_error {
            Some(error) => respond(Err(error)),
            None => respond(Ok(())),
        }
    }
}

fn stop_hold_session(shared: &SharedState) {
    if shared.status().hold_active {
        shared.hold_release();
    }
    stop_hold_timer();
}

fn register_hotkey(id: i32, hotkey: &Hotkey) -> std::result::Result<(), String> {
    unsafe { RegisterHotKey(None, id, hotkey.modifiers() | MOD_NOREPEAT, hotkey.vk()) }
        .map_err(|error| error.to_string())
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

fn start_hold_timer() {
    let _ = unsafe { SetTimer(None, TIMER_HOLD_RELEASE, HOLD_POLL_MS, None) };
}

fn stop_hold_timer() {
    let _ = unsafe { KillTimer(None, TIMER_HOLD_RELEASE) };
}

fn schedule_resume_timer() {
    let _ = unsafe { SetTimer(None, TIMER_RESUME, PAUSE_TIMEOUT_MS, None) };
}

fn stop_resume_timer() {
    let _ = unsafe { KillTimer(None, TIMER_RESUME) };
}

fn other_toggle_id(id: i32) -> i32 {
    if id == HOTKEY_ID_TOGGLE_A {
        HOTKEY_ID_TOGGLE_B
    } else {
        HOTKEY_ID_TOGGLE_A
    }
}

fn other_hold_id(id: i32) -> i32 {
    if id == HOTKEY_ID_HOLD_A {
        HOTKEY_ID_HOLD_B
    } else {
        HOTKEY_ID_HOLD_A
    }
}
