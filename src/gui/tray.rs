//! Native Windows tray icon. The GUI cannot intercept close requests with the
//! software backend, so "minimize to tray" hides the window when Windows
//! reports it as minimized and restores it from this icon.

use std::mem::size_of;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::Duration;

use anyhow::{Result, bail};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, FindWindowW, GetCursorPos, GetMessageW, HWND_MESSAGE, IDI_APPLICATION,
    LoadIconW, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    SW_RESTORE, SetForegroundWindow, ShowWindow, TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD,
    TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONDBLCLK, WM_RBUTTONUP, WNDCLASS_STYLES, WNDCLASSW,
    WS_POPUP,
};
use windows::core::PCWSTR;

use crate::i18n::{Key, Lang};

const WINDOW_TITLE: &str = "SAVV-AutoClicker";
const TRAY_ID: u32 = 1;
const TRAY_CALLBACK: u32 = WM_APP + 1;
const MENU_SHOW: usize = 1;
const MENU_EXIT: usize = 2;

struct TrayState {
    id: u64,
    ctx: egui::Context,
    lang: Lang,
}

static STATE: Mutex<Option<TrayState>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub struct Tray {
    hwnd: isize,
    id: u64,
}

impl Tray {
    pub fn start(ctx: egui::Context, lang: Lang) -> Result<Self> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        {
            let mut state = STATE.lock().unwrap();
            if state.is_some() {
                bail!("the tray icon is already running");
            }
            *state = Some(TrayState { id, ctx, lang });
        }

        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("savv-autoclicker-tray".to_string())
            .spawn(move || {
                if let Err(error) = run(sender, id) {
                    eprintln!("savv-autoclicker: tray error: {error:#}");
                }
            })?;

        match receiver.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(hwnd)) => Ok(Self { hwnd, id }),
            Ok(Err(error)) => {
                clear_state(id);
                Err(error)
            }
            Err(_) => {
                clear_state(id);
                bail!("the tray icon did not start")
            }
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.hwnd as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
        clear_state(self.id);
    }
}

/// Only the owning tray thread may clear the global state: a quick
/// disable/enable sequence can start a new tray before the old thread exits.
fn clear_state(id: u64) {
    let mut state = STATE.lock().unwrap();
    if state.as_ref().is_some_and(|state| state.id == id) {
        *state = None;
    }
}

pub fn set_language(lang: Lang) {
    if let Some(state) = STATE.lock().unwrap().as_mut() {
        state.lang = lang;
    }
}

type StartResult = std::result::Result<isize, anyhow::Error>;

fn run(sender: mpsc::Sender<StartResult>, id: u64) -> Result<()> {
    let class_name = wide("SAVV-AutoClickerTray");
    let module = unsafe { GetModuleHandleW(None) }?;

    let class = WNDCLASSW {
        style: WNDCLASS_STYLES(0),
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: HINSTANCE(module.0),
        hIcon: unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() },
        hCursor: Default::default(),
        hbrBackground: HBRUSH::default(),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: PCWSTR(class_name.as_ptr()),
    };

    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        clear_state(id);
        let error = std::io::Error::last_os_error();
        let _ = sender.send(Err(anyhow::anyhow!("RegisterClassW failed: {error}")));
        return Ok(());
    }

    let created = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(class_name.as_ptr()),
            WS_POPUP,
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(HINSTANCE(module.0)),
            None,
        )
    };

    let hwnd = match created {
        Ok(hwnd) => hwnd,
        Err(error) => {
            clear_state(id);
            let _ = sender.send(Err(anyhow::anyhow!("CreateWindowExW failed: {error}")));
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::UnregisterClassW(
                    PCWSTR(class_name.as_ptr()),
                    Some(HINSTANCE(module.0)),
                );
            }
            return Ok(());
        }
    };

    let data = notify_data(hwnd);
    if unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.0 == 0 {
        let error = std::io::Error::last_os_error();
        unsafe {
            let _ = DestroyWindow(hwnd);
            let _ = windows::Win32::UI::WindowsAndMessaging::UnregisterClassW(
                PCWSTR(class_name.as_ptr()),
                Some(HINSTANCE(module.0)),
            );
        }
        clear_state(id);
        let _ = sender.send(Err(anyhow::anyhow!("Shell_NotifyIconW failed: {error}")));
        return Ok(());
    }

    let _ = sender.send(Ok(hwnd.0 as isize));

    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    let data = notify_data(hwnd);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        let _ = windows::Win32::UI::WindowsAndMessaging::UnregisterClassW(
            PCWSTR(class_name.as_ptr()),
            Some(HINSTANCE(module.0)),
        );
    }
    clear_state(id);
    Ok(())
}

fn notify_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    };
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = TRAY_CALLBACK;
    data.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() };

    let tip: Vec<u16> = WINDOW_TITLE.encode_utf16().collect();
    let length = tip.len().min(data.szTip.len() - 1);
    data.szTip[..length].copy_from_slice(&tip[..length]);
    data
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        TRAY_CALLBACK => {
            match lparam.0 as u32 & 0xffff {
                WM_LBUTTONDBLCLK => show_window(),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            match wparam.0 & 0xffff {
                MENU_SHOW => show_window(),
                MENU_EXIT => exit_window(),
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let data = notify_data(hwnd);
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &data);
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn runtime_lang() -> Lang {
    STATE
        .lock()
        .unwrap()
        .as_ref()
        .map(|state| state.lang)
        .unwrap_or(Lang::En)
}

fn show_menu(hwnd: HWND) {
    let lang = runtime_lang();
    let show = wide(crate::i18n::tr(lang, Key::TrayShow));
    let exit = wide(crate::i18n::tr(lang, Key::TrayExit));

    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let _ = AppendMenuW(menu, MF_STRING, MENU_SHOW, PCWSTR(show.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, MENU_EXIT, PCWSTR(exit.as_ptr()));

        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        // Without foreground ownership the menu does not close when the user
        // clicks elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_LEFTALIGN,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);

        match command.0 as usize {
            MENU_SHOW => show_window(),
            MENU_EXIT => exit_window(),
            _ => {}
        }
    }
}

fn show_window() {
    let title = wide(WINDOW_TITLE);
    if let Ok(hwnd) = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) } {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }

    if let Some(state) = STATE.lock().unwrap().as_ref() {
        state
            .ctx
            .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        state
            .ctx
            .send_viewport_cmd(egui::ViewportCommand::Visible(true));
        state.ctx.request_repaint();
    }
}

fn exit_window() {
    let title = wide(WINDOW_TITLE);
    if let Ok(hwnd) = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) } {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
