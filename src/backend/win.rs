use std::mem::size_of;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
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
    GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_HOTKEY,
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

static HOTKEY_THREAD_ID: AtomicU32 = AtomicU32::new(0);

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
            match register_hotkey(next_id, &hotkey) {
                Ok(()) => {
                    active = Some((hotkey, next_id));
                    shared.clear_hotkey_error();
                    let _ = ready_tx.send(Ok(()));
                }
                Err(error) => {
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
                        reload_hotkey(&shared, &mut active, &mut next_id);
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
    let thread_id = HOTKEY_THREAD_ID.load(Ordering::Acquire);
    if thread_id == 0 {
        bail!("the global hotkey thread is not running");
    }

    shared.request_hotkey(combo);
    if let Err(error) =
        unsafe { PostThreadMessageW(thread_id, WM_RELOAD_HOTKEY, WPARAM(0), LPARAM(0)) }
    {
        let message = format!("failed to wake the global hotkey thread: {error}");
        shared.set_hotkey_error(message.clone());
        bail!("{message}");
    }
    Ok(())
}

fn register_hotkey(id: i32, hotkey: &Hotkey) -> std::result::Result<(), String> {
    unsafe { RegisterHotKey(None, id, hotkey.modifiers() | MOD_NOREPEAT, hotkey.vk()) }
        .map_err(|error| error.to_string())
}

fn reload_hotkey(shared: &SharedState, active: &mut Option<(Hotkey, i32)>, next_id: &mut i32) {
    let Some(combo) = shared.take_hotkey_request() else {
        return;
    };

    let hotkey = match Hotkey::parse(&combo) {
        Ok(hotkey) => hotkey,
        Err(error) => {
            shared.set_hotkey_error(format!("invalid hotkey: {error:#}"));
            return;
        }
    };

    let id = *next_id;
    match register_hotkey(id, &hotkey) {
        Ok(()) => {
            if let Some((_, old_id)) = active.replace((hotkey, id)) {
                let _ = unsafe { UnregisterHotKey(None, old_id) };
            }
            *next_id = other_id(id);
            shared.clear_hotkey_error();
        }
        Err(error) => {
            let message = match active {
                Some((current, _)) => format!(
                    "could not register global hotkey: {error}; {} is still active",
                    current.label()
                ),
                None => format!("could not register global hotkey: {error}"),
            };
            shared.set_hotkey_error(message);
        }
    }
}

fn other_id(id: i32) -> i32 {
    if id == HOTKEY_ID_A {
        HOTKEY_ID_B
    } else {
        HOTKEY_ID_A
    }
}
