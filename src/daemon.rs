use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::backend;
use crate::cli::{DaemonArgs, RunArgs};
use crate::ipc;
use crate::platform;
use crate::scheduler;
use crate::state::{Settings, SharedState};

pub fn run_foreground(args: RunArgs) -> Result<()> {
    let settings = args.click.to_settings()?;
    if let Some(seconds) = args.duration_secs {
        if !seconds.is_finite() || seconds <= 0.0 {
            bail!("--duration-secs must be a positive number");
        }
    }

    let _timer = platform::timer_resolution();
    let backend = backend::create()?;
    let shared = Arc::new(SharedState::new(settings, true));

    install_ctrlc(shared.clone(), true)?;

    if let Some(seconds) = args.duration_secs {
        let shared = shared.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs_f64(seconds));
            shared.quit();
        });
    }

    let started = Instant::now();
    scheduler::run(shared.clone(), backend);
    let elapsed = started.elapsed().max(Duration::from_millis(1));
    let clicks = shared.total_clicks();

    println!(
        "{clicks} clicks in {:.2}s ({:.1} clicks/s average)",
        elapsed.as_secs_f64(),
        clicks as f64 / elapsed.as_secs_f64()
    );

    Ok(())
}

pub fn run_daemon(args: DaemonArgs) -> Result<()> {
    #[cfg(windows)]
    let hotkey = crate::hotkey::Hotkey::parse(&args.hotkey)?;

    if args.detach {
        let child_args = vec![
            String::from("daemon"),
            String::from("--hotkey"),
            args.hotkey.clone(),
        ];
        platform::spawn_daemon(&child_args)?;

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if ipc::send("STATUS").is_ok() {
                println!("autoclicker: daemon is running");
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }

        bail!("the daemon did not start; run `autoclicker daemon` in a terminal to see the error");
    }

    let Some(_lock) = platform::InstanceLock::acquire()? else {
        eprintln!("autoclicker: another daemon is already running");
        return Ok(());
    };

    let _timer = platform::timer_resolution();
    let backend = backend::create()?;
    let shared = Arc::new(SharedState::new(Settings::default(), false));
    let listener = ipc::create_listener().context("failed to create the control socket")?;

    let scheduler_shared = shared.clone();
    let scheduler_thread = thread::Builder::new()
        .name("clicker".to_string())
        .spawn(move || scheduler::run(scheduler_shared, backend))
        .context("failed to spawn the clicker thread")?;

    let server_shared = shared.clone();
    let server_thread = thread::Builder::new()
        .name("ipc".to_string())
        .spawn(move || ipc::serve(listener, server_shared))
        .context("failed to spawn the IPC thread")?;

    install_ctrlc(shared.clone(), false)?;

    #[cfg(windows)]
    {
        match backend::win::spawn_hotkey(shared.clone(), hotkey) {
            Ok(()) => eprintln!("autoclicker: daemon ready, global hotkey: {}", args.hotkey),
            Err(error) => eprintln!("autoclicker: warning: {error:#}"),
        }
    }

    #[cfg(not(windows))]
    {
        let _ = &args.hotkey;
        eprintln!(
            "autoclicker: daemon ready; bind `autoclicker toggle` to a compositor shortcut (niri: Mod+Shift+C)"
        );
    }

    shared.wait_for_quit();

    if let Err(error) = scheduler_thread.join() {
        eprintln!("autoclicker: clicker thread panicked: {error:?}");
    }
    let _ = server_thread.join();

    Ok(())
}

fn install_ctrlc(shared: Arc<SharedState>, required: bool) -> Result<()> {
    if let Err(error) = ctrlc::set_handler(move || shared.quit()) {
        if required {
            bail!("failed to install the Ctrl+C handler: {error}");
        }
        eprintln!("autoclicker: warning: Ctrl+C handler not installed: {error}");
    }
    Ok(())
}
