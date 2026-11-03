use std::mem::size_of;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOD_NOREPEAT, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEINPUT, RegisterHotKey, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

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

pub(crate) fn spawn_hotkey(shared: Arc<SharedState>, hotkey: Hotkey) -> Result<()> {
    const HOTKEY_ID: i32 = 1;

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();

    std::thread::Builder::new()
        .name("hotkey".into())
        .spawn(move || unsafe {
            match RegisterHotKey(
                None,
                HOTKEY_ID,
                hotkey.modifiers() | MOD_NOREPEAT,
                hotkey.vk(),
            ) {
                Ok(()) => {
                    let _ = ready_tx.send(Ok(()));
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(format!("{error}")));
                    return;
                }
            }

            let mut message = MSG::default();
            loop {
                let result = GetMessageW(&mut message, None, 0, 0);
                if result.0 == 0 || result.0 == -1 {
                    break;
                }
                if message.message == WM_HOTKEY {
                    shared.toggle();
                }
            }
        })?;

    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => bail!("could not register global hotkey: {error}"),
        Err(_) => bail!("hotkey thread did not report readiness"),
    }
}
