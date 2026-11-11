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
        if Duration::try_from_secs_f64(seconds).is_err() {
            bail!("--duration-secs is too large");
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
    #[cfg(windows)]
    let hold_hotkey = args
        .hold_hotkey
        .as_deref()
        .filter(|combo| !combo.trim().is_empty())
        .map(crate::hotkey::Hotkey::parse)
        .transpose()?;
    #[cfg(windows)]
    let hotkey_combo = hotkey.to_string();
    #[cfg(windows)]
    let hold_combo = hold_hotkey
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    #[cfg(not(windows))]
    let hotkey_combo = args.hotkey.clone();
    #[cfg(not(windows))]
    let hold_combo = args.hold_hotkey.clone().unwrap_or_default();

    if args.detach {
        if ipc::send("STATUS").is_ok() {
            #[cfg(windows)]
            apply_hotkey_to_running_daemon(&hotkey_combo, &hold_combo)?;
            println!("savv-autoclicker: daemon is running");
            return Ok(());
        }

        let mut child_args = vec![
            String::from("daemon"),
            String::from("--hotkey"),
            hotkey_combo.clone(),
        ];
        if !hold_combo.is_empty() {
            child_args.push(String::from("--hold-hotkey"));
            child_args.push(hold_combo.clone());
        }
        platform::spawn_daemon(&child_args)?;

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if ipc::send("STATUS").is_ok() {
                println!("savv-autoclicker: daemon is running");
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }

        bail!(
            "the daemon did not start; run `savv-autoclicker daemon` in a terminal to see the error"
        );
    }

    let Some(_lock) = platform::InstanceLock::acquire()? else {
        #[cfg(windows)]
        {
            apply_hotkey_to_running_daemon(&hotkey_combo, &hold_combo)?;
        }
        #[cfg(not(windows))]
        eprintln!("savv-autoclicker: another daemon is already running");
        return Ok(());
    };

    let _timer = platform::timer_resolution();
    let backend = backend::create()?;
    let shared = Arc::new(SharedState::new(Settings::default(), false));
    shared.set_hotkey_requested(hotkey_combo.clone());
    if !hold_combo.is_empty() {
        shared.set_hold_hotkey_requested(hold_combo.clone());
    }
    let listener = ipc::create_listener().context("failed to create the control socket")?;

    #[cfg(windows)]
    {
        match backend::win::spawn_hotkey(shared.clone(), hotkey, hold_hotkey) {
            Ok(()) => {
                if hold_combo.is_empty() {
                    eprintln!("savv-autoclicker: daemon ready, global hotkey: {hotkey_combo}");
                } else {
                    eprintln!(
                        "savv-autoclicker: daemon ready, global hotkeys: {hotkey_combo} / hold {hold_combo}"
                    );
                }
            }
            Err(error) => eprintln!("savv-autoclicker: warning: {error:#}"),
        }
    }

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

    #[cfg(not(windows))]
    {
        eprintln!(
            "savv-autoclicker: daemon ready; bind `savv-autoclicker toggle` to a compositor shortcut (niri: Mod+Alt+C). --hotkey is ignored on Linux"
        );
    }

    shared.wait_for_quit();

    if let Err(error) = scheduler_thread.join() {
        eprintln!("savv-autoclicker: clicker thread panicked: {error:?}");
    }
    let _ = server_thread.join();

    Ok(())
}

#[cfg(windows)]
fn apply_hotkey_to_running_daemon(combo: &str, hold_combo: &str) -> Result<()> {
    let response = ipc::send(&format!("HOTKEY {combo}"))
        .context("another daemon is running, but it did not accept the hotkey change")?;
    ipc::ensure_ok(&response)?;
    println!("savv-autoclicker: hotkey set to {combo}");

    let response = ipc::send(&format!(
        "HOLD_HOTKEY {}",
        if hold_combo.is_empty() {
            "none"
        } else {
            hold_combo
        }
    ))
    .context("another daemon is running, but it did not accept the hold hotkey change")?;
    ipc::ensure_ok(&response)?;
    if !hold_combo.is_empty() {
        println!("savv-autoclicker: hold hotkey set to {hold_combo}");
    }
    Ok(())
}

fn install_ctrlc(shared: Arc<SharedState>, required: bool) -> Result<()> {
    if let Err(error) = ctrlc::set_handler(move || shared.quit()) {
        if required {
            bail!("failed to install the Ctrl+C handler: {error}");
        }
        eprintln!("savv-autoclicker: warning: Ctrl+C handler not installed: {error}");
    }
    Ok(())
}
