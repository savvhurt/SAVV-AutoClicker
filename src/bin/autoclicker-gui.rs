#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    if let Err(error) = autoclicker::gui::run() {
        report_error(&format!("{error:#}"));
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn report_error(message: &str) {
    use std::io::Write;

    if let Some(appdata) = std::env::var_os("APPDATA") {
        let directory = std::path::PathBuf::from(appdata).join("autoclicker");
        if std::fs::create_dir_all(&directory).is_ok() {
            if let Ok(mut file) = std::fs::File::create(directory.join("gui-error.log")) {
                let _ = writeln!(file, "{message}");
            }
        }
    }

    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::HSTRING;

    let text = HSTRING::from(message);
    let title = HSTRING::from("Autoclicker");
    unsafe {
        MessageBoxW(None, &text, &title, MB_OK | MB_ICONERROR);
    }
}

#[cfg(not(windows))]
fn report_error(message: &str) {
    eprintln!("autoclicker-gui: error: {message}");
}
