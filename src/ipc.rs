use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{
    Listener, ListenerNonblockingMode, ListenerOptions, Stream, prelude::*,
};

use crate::platform;
use crate::state::{Button, Settings, SharedState};

#[cfg(unix)]
fn socket_name() -> io::Result<interprocess::local_socket::Name<'static>> {
    platform::socket_path()?.to_fs_name::<GenericFilePath>()
}

#[cfg(windows)]
fn socket_name() -> io::Result<interprocess::local_socket::Name<'static>> {
    platform::pipe_name().to_ns_name::<GenericNamespaced>()
}

pub fn create_listener() -> io::Result<Listener> {
    #[cfg(unix)]
    {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;

        ListenerOptions::new()
            .name(socket_name()?)
            .mode(0o600)
            .try_overwrite(true)
            .nonblocking(ListenerNonblockingMode::Accept)
            .create_sync()
    }

    #[cfg(windows)]
    {
        ListenerOptions::new()
            .name(socket_name()?)
            .nonblocking(ListenerNonblockingMode::Accept)
            .create_sync()
    }
}

pub fn connect() -> io::Result<Stream> {
    Stream::connect(socket_name()?)
}

pub fn send(command: &str) -> Result<String> {
    let mut stream = connect().context("failed to connect to the autoclicker daemon")?;
    writeln!(stream, "{command}").context("failed to send a command to the daemon")?;
    stream
        .flush()
        .context("failed to send a command to the daemon")?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .context("failed to read the daemon response")?;
    Ok(response)
}

pub fn ensure_ok(response: &str) -> Result<()> {
    if let Some(error) = response.strip_prefix("ERR ") {
        bail!("{}", error.trim());
    }
    if !response.starts_with("OK") {
        bail!("unexpected daemon response: {}", response.trim());
    }
    Ok(())
}

pub fn send_or_spawn(command: &str) -> Result<String> {
    if let Ok(response) = send(command) {
        return Ok(response);
    }

    platform::spawn_daemon(&[String::from("daemon")])
        .context("failed to start the autoclicker daemon")?;

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_error = None;

    while Instant::now() < deadline {
        match send("STATUS") {
            Ok(_) => return send(command),
            Err(error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    let detail = last_error
        .map(|error| format!("{error:#}"))
        .unwrap_or_else(|| "timed out".to_string());
    bail!(
        "the daemon did not become ready ({detail}); run `autoclicker daemon` in a terminal to see the error"
    )
}

pub fn serve(listener: Listener, shared: Arc<SharedState>) {
    loop {
        if shared.is_quit() {
            break;
        }

        match listener.accept() {
            Ok(stream) => {
                let shared = shared.clone();
                thread::spawn(move || {
                    if let Err(error) = handle(stream, &shared) {
                        eprintln!("autoclicker: ipc error: {error:#}");
                    }
                });
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => {
                eprintln!("autoclicker: ipc accept error: {error}");
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn handle(mut stream: Stream, shared: &SharedState) -> Result<()> {
    let mut line = String::new();
    {
        let mut reader = BufReader::new(&mut stream);
        reader.read_line(&mut line)?;
    }

    if line.len() > 4096 {
        stream.write_all(b"ERR command is too long\n")?;
        stream.flush()?;
        return Ok(());
    }

    let is_quit = line.trim().eq_ignore_ascii_case("QUIT");
    let response = process(&line, shared);

    stream.write_all(response.as_bytes())?;
    stream.flush()?;

    // Let the client receive the acknowledgement before the process starts shutting down.
    if is_quit {
        shared.quit();
    }

    Ok(())
}

fn process(line: &str, shared: &SharedState) -> String {
    let mut parts = line.split_whitespace();
    let Some(command) = parts.next() else {
        return "ERR empty command\n".to_string();
    };

    match command.to_ascii_uppercase().as_str() {
        "TOGGLE" => format!("OK enabled={}\n", shared.toggle()),
        "ON" => handle_on(parts, shared),
        "SET" => handle_set(parts, shared),
        "OFF" => {
            shared.off();
            "OK\n".to_string()
        }
        "STATUS" => shared.status().to_wire(),
        "QUIT" => "OK\n".to_string(),
        other => format!("ERR unknown command `{other}`\n"),
    }
}

fn handle_on(parts: std::str::SplitWhitespace<'_>, shared: &SharedState) -> String {
    match parse_settings(parts, "ON") {
        Ok(settings) => {
            shared.on(settings);
            "OK\n".to_string()
        }
        Err(error) => format!("ERR {error}\n"),
    }
}

fn handle_set(parts: std::str::SplitWhitespace<'_>, shared: &SharedState) -> String {
    match parse_settings(parts, "SET") {
        Ok(settings) => {
            shared.set_settings(settings);
            "OK\n".to_string()
        }
        Err(error) => format!("ERR {error}\n"),
    }
}

fn parse_settings(
    mut parts: std::str::SplitWhitespace<'_>,
    command: &str,
) -> std::result::Result<Settings, String> {
    let required = || format!("ERR {command} requires: cps button hold_ms jitter_pct\n");

    let Some(cps) = parts.next() else {
        return Err(required());
    };
    let cps = cps
        .parse::<f64>()
        .map_err(|_| "ERR invalid cps\n".to_string())?;

    let Some(button) = parts.next() else {
        return Err(required());
    };
    let button = button
        .parse::<Button>()
        .map_err(|error| format!("ERR {error}\n"))?;

    let Some(hold_ms) = parts.next() else {
        return Err(required());
    };
    let hold_ms = hold_ms
        .parse::<f64>()
        .map_err(|_| "ERR invalid hold_ms\n".to_string())?;

    let Some(jitter_pct) = parts.next() else {
        return Err(required());
    };
    let jitter_pct = jitter_pct
        .parse::<f64>()
        .map_err(|_| "ERR invalid jitter_pct\n".to_string())?;

    if parts.next().is_some() {
        return Err(format!("ERR too many arguments for {command}\n"));
    }

    Settings::new(cps, button, hold_ms, jitter_pct).map_err(|error| format!("ERR {error}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_command_changes_state() {
        let shared = SharedState::new(Settings::default(), false);

        let response = process("TOGGLE", &shared);
        assert_eq!(response, "OK enabled=true\n");
        assert!(shared.status().enabled);

        let response = process("TOGGLE", &shared);
        assert_eq!(response, "OK enabled=false\n");
        assert!(!shared.status().enabled);
    }

    #[test]
    fn on_command_applies_settings() {
        let shared = SharedState::new(Settings::default(), false);

        let response = process("ON 125 right 5 10", &shared);
        assert_eq!(response, "OK\n");

        let status = shared.status();
        assert!(status.enabled);
        assert_eq!(status.settings.cps, 125.0);
        assert_eq!(status.settings.button, Button::Right);
        assert!((status.settings.hold_ms() - 5.0).abs() < 1e-9);
        assert_eq!(status.settings.jitter_pct, 10.0);
    }

    #[test]
    fn set_command_updates_settings_without_enabling() {
        let shared = SharedState::new(Settings::default(), false);

        let response = process("SET 125 right 5 10", &shared);
        assert_eq!(response, "OK\n");

        let status = shared.status();
        assert!(!status.enabled);
        assert_eq!(status.settings.cps, 125.0);
        assert_eq!(status.settings.button, Button::Right);
        assert!((status.settings.hold_ms() - 5.0).abs() < 1e-9);
        assert_eq!(status.settings.jitter_pct, 10.0);
    }

    #[test]
    fn set_command_keeps_enabled_state() {
        let shared = SharedState::new(Settings::default(), true);

        assert_eq!(process("SET 200 middle 2 0", &shared), "OK\n");

        let status = shared.status();
        assert!(status.enabled);
        assert_eq!(status.settings.cps, 200.0);
        assert_eq!(status.settings.button, Button::Middle);
    }

    #[test]
    fn invalid_on_command_is_rejected() {
        let shared = SharedState::new(Settings::default(), false);

        assert!(process("ON 0 left 10 0", &shared).starts_with("ERR "));
        assert!(process("ON 50 left 10", &shared).starts_with("ERR "));
        assert!(process("ON 50 side 10 0", &shared).starts_with("ERR "));
        assert!(!shared.status().enabled);
    }

    #[test]
    fn invalid_set_command_is_rejected() {
        let shared = SharedState::new(Settings::default(), false);

        assert!(process("SET 0 left 10 0", &shared).starts_with("ERR "));
        assert!(process("SET 50 left 10", &shared).starts_with("ERR "));
        assert!(process("SET 50 side 10 0", &shared).starts_with("ERR "));
        assert!(!shared.status().enabled);
    }

    #[test]
    fn responses_can_be_checked() {
        assert!(ensure_ok("OK\n").is_ok());
        assert!(ensure_ok("OK enabled=true\n").is_ok());
        assert!(ensure_ok("ERR bad settings\n").is_err());
        assert!(ensure_ok("garbage\n").is_err());
    }

    #[test]
    fn quit_is_case_insensitive() {
        let shared = SharedState::new(Settings::default(), true);

        assert_eq!(process("quit", &shared), "OK\n");
        // `process` deliberately does not quit; the connection handler does it
        // after sending the acknowledgement.
        assert!(!shared.is_quit());
    }
}
