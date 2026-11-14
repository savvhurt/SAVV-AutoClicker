#![cfg_attr(
    all(windows, feature = "gui", not(test)),
    windows_subsystem = "windows"
)]

use anyhow::Result;
use clap::Parser;

use v_autoclicker::cli::{Cli, Command};
use v_autoclicker::{daemon, ipc};

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let start_hidden = args.iter().skip(1).any(|argument| argument == "--hidden");
    let gui_launch = args.len() == 1 || (args.len() == 2 && start_hidden);

    #[cfg(all(windows, feature = "gui"))]
    if !gui_launch {
        windows_console::attach_for_cli();
    }

    // Stop a daemon left behind by the pre-rename `autoclicker` binary. The
    // new socket/pipe lives in a separate namespace, so without this both
    // daemons could press the mouse at the same time.
    ipc::quit_legacy_daemon();

    #[cfg(feature = "gui")]
    if gui_launch {
        run_gui(start_hidden);
        return;
    }

    #[cfg(not(feature = "gui"))]
    let _ = (gui_launch, start_hidden);

    if let Err(error) = run() {
        eprintln!("v-autoclicker: error: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(feature = "gui")]
fn run_gui(start_hidden: bool) {
    if let Err(error) = v_autoclicker::gui::run(start_hidden) {
        report_error(&format!("{error:#}"));
        std::process::exit(1);
    }
}

#[cfg(feature = "gui")]
#[cfg(windows)]
fn report_error(message: &str) {
    use std::io::Write;

    if let Some(appdata) = std::env::var_os("APPDATA") {
        let directory = std::path::PathBuf::from(appdata).join("V-AutoClicker");
        if std::fs::create_dir_all(&directory).is_ok() {
            if let Ok(mut file) = std::fs::File::create(directory.join("gui-error.log")) {
                let _ = writeln!(file, "{message}");
            }
        }
    }

    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::HSTRING;

    let text = HSTRING::from(message);
    let title = HSTRING::from("V-AutoClicker");
    unsafe {
        MessageBoxW(None, &text, &title, MB_OK | MB_ICONERROR);
    }
}

#[cfg(feature = "gui")]
#[cfg(not(windows))]
fn report_error(message: &str) {
    eprintln!("v-autoclicker: gui error: {message}");
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Run(args) => daemon::run_foreground(args),

        Command::Daemon(args) => daemon::run_daemon(args),

        Command::Toggle => {
            let response = ipc::send_or_spawn("TOGGLE")?;
            ipc::ensure_ok(&response)?;
            println!(
                "v-autoclicker: {}",
                if response.contains("enabled=true") {
                    "on"
                } else {
                    "off"
                }
            );
            Ok(())
        }

        Command::On(args) => {
            let command = args.click.to_wire_command()?;
            let response = ipc::send_or_spawn(&command)?;
            ipc::ensure_ok(&response)?;
            println!("v-autoclicker: on");
            Ok(())
        }

        Command::Off => {
            match ipc::send("OFF") {
                Ok(response) => {
                    ipc::ensure_ok(&response)?;
                    println!("v-autoclicker: off");
                }
                Err(_) => println!("v-autoclicker: not running"),
            }
            Ok(())
        }

        Command::Status => {
            match ipc::send("STATUS") {
                Ok(response) => {
                    ipc::ensure_ok(&response)?;
                    let body = response.strip_prefix("OK\n").unwrap_or(&response);
                    print!("{body}");
                }
                Err(_) => println!("v-autoclicker: not running"),
            }
            Ok(())
        }

        Command::Quit => {
            match ipc::send("QUIT") {
                Ok(response) => {
                    ipc::ensure_ok(&response)?;
                    println!("v-autoclicker: stopped");
                }
                Err(_) => println!("v-autoclicker: not running"),
            }
            Ok(())
        }
    }
}

/// A GUI-subsystem executable does not inherit its parent's console. When the
/// same executable is used from a terminal, attach to that console so CLI
/// output (including `--help` and clap errors) stays visible.
#[cfg(all(windows, feature = "gui"))]
mod windows_console {
    use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole, GetStdHandle, STD_ERROR_HANDLE,
        STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
    };
    use windows::core::HSTRING;

    pub fn attach_for_cli() {
        // Keep explicit redirection (`v-autoclicker status > file.txt`, pipes,
        // etc.) intact. Only fill in handles the process did not inherit.
        let stdin_valid = handle_is_valid(STD_INPUT_HANDLE);
        let stdout_valid = handle_is_valid(STD_OUTPUT_HANDLE);
        let stderr_valid = handle_is_valid(STD_ERROR_HANDLE);

        if stdin_valid && stdout_valid && stderr_valid {
            return;
        }

        if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_err() {
            // Either we are already attached to a console, or the parent has
            // none. AllocConsole fails in the former case and creates a new
            // console (useful when launching a subcommand from Explorer) in
            // the latter.
            let _ = unsafe { AllocConsole() };
        }

        if !stdin_valid {
            redirect_handle("CONIN$", STD_INPUT_HANDLE);
        }
        if !stdout_valid {
            redirect_handle("CONOUT$", STD_OUTPUT_HANDLE);
        }
        if !stderr_valid {
            redirect_handle("CONOUT$", STD_ERROR_HANDLE);
        }
    }

    fn handle_is_valid(which: STD_HANDLE) -> bool {
        match unsafe { GetStdHandle(which) } {
            Ok(handle) => !handle.is_invalid(),
            Err(_) => false,
        }
    }

    fn redirect_handle(name: &str, which: STD_HANDLE) {
        let name = HSTRING::from(name);
        let access = GENERIC_READ.0 | GENERIC_WRITE.0;
        let handle: HANDLE = match unsafe {
            CreateFileW(
                &name,
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        } {
            Ok(handle) => handle,
            Err(_) => return,
        };

        let _ = unsafe { SetStdHandle(which, handle) };
    }
}
