//! Native Windows tray icon.
//!
//! Closing the window is intercepted with a window subclass on the GUI thread,
//! because the software backend gives the application no way to see a close
//! request. When "close to tray" is enabled, `WM_CLOSE` hides the window and
//! keeps the process running; the tray icon restores it. The tray menu's Exit
//! command bypasses the guard so it always ends the process.
//!
//! Minimizing is intentionally left alone: it keeps the normal taskbar button.

use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::Write;
use std::mem::size_of;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    DefSubclassProc, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    SetWindowSubclass, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, FindWindowW, GetCursorPos, GetMessageW, HICON, HWND_MESSAGE, IDI_APPLICATION,
    IsWindow, LoadIconW, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage,
    RegisterClassW, SW_HIDE, SW_RESTORE, SetForegroundWindow, ShowWindow, TPM_LEFTALIGN,
    TPM_NONOTIFY, TPM_RETURNCMD, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WM_APP,
    WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONUP,
    WNDCLASS_STYLES, WNDCLASSW, WS_OVERLAPPED,
};
use windows::core::PCWSTR;

use crate::i18n::{Key, Lang};

const WINDOW_TITLE: &str = "V-AutoClicker";
const TRAY_CLASS: &str = "V-AutoClickerTray";
const TRAY_ID: u32 = 1;
const TRAY_CALLBACK: u32 = WM_APP + 1;
const CLOSE_SUBCLASS_ID: usize = 1;
const MENU_SHOW: usize = 1;
const MENU_EXIT: usize = 2;

struct TrayState {
    id: u64,
    ctx: egui::Context,
    lang: Lang,
}

#[derive(Default)]
struct TrayRuntime {
    gui: Option<HWND>,
}

thread_local! {
    static RUNTIME: RefCell<TrayRuntime> = RefCell::new(TrayRuntime::default());
}

static STATE: Mutex<Option<TrayState>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Whether `WM_CLOSE` on the GUI window should hide it instead of exiting.
static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(false);
/// Set by the tray Exit command so the close guard lets the window die.
static FORCE_EXIT: AtomicBool = AtomicBool::new(false);
/// HWND that already has [`close_subclass`] installed, or 0.
static CLOSE_HOOK: AtomicIsize = AtomicIsize::new(0);

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
        let spawned = thread::Builder::new()
            .name("v-autoclicker-tray".to_string())
            .spawn(move || {
                if let Err(error) = run(sender, id) {
                    log_error(format!("tray thread: {error:#}"));
                }
            });
        if let Err(error) = spawned {
            clear_state(id);
            log_error(format!("could not start the tray thread: {error}"));
            return Err(error.into());
        }

        match receiver.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(hwnd)) => Ok(Self { hwnd, id }),
            Ok(Err(error)) => {
                clear_state(id);
                log_error(format!("tray start: {error:#}"));
                Err(error)
            }
            Err(error) => {
                clear_state(id);
                log_error(format!("tray start timed out: {error}"));
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

/// Enable or disable the close-to-tray guard. Called from the GUI thread so the
/// window subclass is installed by the thread that owns the window.
pub fn set_close_to_tray(enabled: bool) {
    CLOSE_TO_TRAY.store(enabled, Ordering::Relaxed);
    if enabled {
        install_close_hook();
    }
}

fn install_close_hook() {
    if CLOSE_HOOK.load(Ordering::Relaxed) != 0 {
        return;
    }
    let Some(hwnd) = find_gui_window() else {
        return;
    };
    let installed = unsafe { SetWindowSubclass(hwnd, Some(close_subclass), CLOSE_SUBCLASS_ID, 0) };
    if installed.as_bool() {
        CLOSE_HOOK.store(hwnd.0 as isize, Ordering::Relaxed);
    } else {
        log_error(format!(
            "SetWindowSubclass failed: {}",
            std::io::Error::last_os_error()
        ));
    }
}

/// Close requests are hidden instead of delivered while close-to-tray is on.
/// The tray Exit command raises [`FORCE_EXIT`] first, so it still quits.
unsafe extern "system" fn close_subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_CLOSE
        && CLOSE_TO_TRAY.load(Ordering::Relaxed)
        && !FORCE_EXIT.load(Ordering::Relaxed)
    {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        return LRESULT(0);
    }

    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

type StartResult = std::result::Result<isize, anyhow::Error>;

fn run(sender: mpsc::Sender<StartResult>, id: u64) -> Result<()> {
    let class_name = wide(TRAY_CLASS);
    let module = unsafe { GetModuleHandleW(None) }?;

    let class = WNDCLASSW {
        style: WNDCLASS_STYLES(0),
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: HINSTANCE(module.0),
        hIcon: load_app_icon(),
        hCursor: Default::default(),
        hbrBackground: HBRUSH::default(),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: PCWSTR(class_name.as_ptr()),
    };

    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        let error = std::io::Error::last_os_error();
        let already_exists = error.raw_os_error() == Some(ERROR_CLASS_ALREADY_EXISTS.0 as i32);
        if !already_exists {
            clear_state(id);
            let message = format!("RegisterClassW failed: {error}");
            log_error(&message);
            let _ = sender.send(Err(anyhow::anyhow!(message)));
            return Ok(());
        }
    }

    let created = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(class_name.as_ptr()),
            WS_OVERLAPPED,
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
            let message = format!("CreateWindowExW failed: {error}");
            log_error(&message);
            let _ = sender.send(Err(anyhow::anyhow!(message)));
            return Ok(());
        }
    };

    let data = notify_data(hwnd);
    if unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.0 == 0 {
        let error = std::io::Error::last_os_error();
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        clear_state(id);
        let message = format!("Shell_NotifyIconW failed: {error}");
        log_error(&message);
        let _ = sender.send(Err(anyhow::anyhow!(message)));
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
    }
    clear_state(id);
    Ok(())
}

fn load_app_icon() -> HICON {
    if let Ok(module) = unsafe { GetModuleHandleW(None) } {
        // winresource embeds the application icon as resource id 1.
        if let Ok(icon) = unsafe {
            LoadIconW(
                Some(HINSTANCE(module.0)),
                PCWSTR(std::ptr::without_provenance(1)),
            )
        } {
            if !icon.is_invalid() {
                return icon;
            }
        }
    }

    unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() }
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
    data.hIcon = load_app_icon();

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
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => show_window(),
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
            unsafe {
                let data = notify_data(hwnd);
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

fn find_gui_window() -> Option<HWND> {
    let title = wide(WINDOW_TITLE);
    unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())).ok() }
}

fn cached_gui_window() -> Option<HWND> {
    let cached = RUNTIME.with(|runtime| runtime.borrow().gui);
    if let Some(hwnd) = cached {
        if unsafe { IsWindow(Some(hwnd)).as_bool() } {
            return Some(hwnd);
        }
    }

    let found = find_gui_window();
    RUNTIME.with(|runtime| runtime.borrow_mut().gui = found);
    found
}

fn show_window() {
    let hwnd = cached_gui_window().or_else(find_gui_window);
    if let Some(hwnd) = hwnd {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }

    RUNTIME.with(|runtime| runtime.borrow_mut().gui = hwnd);

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
    // A real exit must work even when the close guard is enabled.
    FORCE_EXIT.store(true, Ordering::Relaxed);
    if let Some(hwnd) = cached_gui_window().or_else(find_gui_window) {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

fn log_error(message: impl AsRef<str>) {
    let Some(appdata) = std::env::var_os("APPDATA") else {
        return;
    };
    let directory = PathBuf::from(appdata).join("V-AutoClicker");
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }

    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("tray.log"))
    else {
        return;
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let _ = writeln!(file, "[{timestamp}] {}", message.as_ref());
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
