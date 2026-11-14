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
use crate::state::{Button, ClickMode, Point, Settings, SharedState};

const IO_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_COMMAND_BYTES: u64 = 4096;

#[cfg(unix)]
fn socket_name() -> io::Result<interprocess::local_socket::Name<'static>> {
    platform::socket_path()?.to_fs_name::<GenericFilePath>()
}

#[cfg(windows)]
fn socket_name() -> io::Result<interprocess::local_socket::Name<'static>> {
    platform::pipe_name().to_ns_name::<GenericNamespaced>()
}

#[cfg(unix)]
fn legacy_socket_names() -> Vec<interprocess::local_socket::Name<'static>> {
    platform::legacy_socket_paths()
        .into_iter()
        .filter_map(|path| path.to_fs_name::<GenericFilePath>().ok())
        .collect()
}

#[cfg(windows)]
fn legacy_socket_names() -> Vec<interprocess::local_socket::Name<'static>> {
    platform::legacy_pipe_names()
        .into_iter()
        .filter_map(|name| name.to_ns_name::<GenericNamespaced>().ok())
        .collect()
}

/// Ask a daemon from a pre-rename version to quit. The old and new versions
/// use different sockets, so without this both could press the mouse.
pub fn quit_legacy_daemon() {
    for name in legacy_socket_names() {
        let Ok(mut stream) = Stream::connect(name) else {
            continue;
        };
        let _ = stream.set_recv_timeout(Some(Duration::from_millis(500)));
        let _ = stream.set_send_timeout(Some(Duration::from_millis(500)));
        let _ = writeln!(stream, "QUIT");
        let _ = stream.flush();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
    }
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
    let mut stream = connect().context("failed to connect to the V-AutoClicker daemon")?;
    // Unix-domain sockets support timeouts; named pipes currently ignore
    // them, but keeping the call best-effort still protects the common path
    // from a wedged daemon.
    let _ = stream.set_recv_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_send_timeout(Some(IO_TIMEOUT));
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
    send_or_spawn_with_hotkeys(command, crate::hotkey::DEFAULT, None)
}

pub fn send_or_spawn_with_hotkey(command: &str, hotkey: &str) -> Result<String> {
    send_or_spawn_with_hotkeys(command, hotkey, None)
}

pub fn send_or_spawn_with_hotkeys(
    command: &str,
    hotkey: &str,
    hold_hotkey: Option<&str>,
) -> Result<String> {
    if let Ok(response) = send(command) {
        return Ok(response);
    }

    let mut args = vec![
        String::from("daemon"),
        String::from("--hotkey"),
        hotkey.to_string(),
    ];
    if let Some(hold_hotkey) = hold_hotkey.filter(|combo| !combo.trim().is_empty()) {
        args.push(String::from("--hold-hotkey"));
        args.push(hold_hotkey.to_string());
    }
    platform::spawn_daemon(&args).context("failed to start the V-AutoClicker daemon")?;

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
        "the daemon did not become ready ({detail}); run `v-autoclicker daemon` in a terminal to see the error"
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
                        eprintln!("v-autoclicker: ipc error: {error:#}");
                    }
                });
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => {
                eprintln!("v-autoclicker: ipc accept error: {error}");
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn handle(mut stream: Stream, shared: &SharedState) -> Result<()> {
    let _ = stream.set_recv_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_send_timeout(Some(IO_TIMEOUT));

    let mut line = String::new();
    {
        let mut reader = BufReader::new(&mut stream).take(MAX_COMMAND_BYTES + 1);
        reader.read_line(&mut line)?;
    }

    if line.len() as u64 > MAX_COMMAND_BYTES {
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
        "HOTKEY" => handle_hotkey(parts, shared),
        "HOLD_HOTKEY" => handle_hold_hotkey(parts, shared),
        "HOTKEY_PAUSE" => handle_hotkey_pause(shared),
        "HOTKEY_RESUME" => handle_hotkey_resume(shared),
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
        Ok(settings) => match settings.readiness() {
            Ok(()) => {
                shared.on(settings);
                "OK\n".to_string()
            }
            Err(error) => format!("ERR {error}\n"),
        },
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

fn handle_hotkey(parts: std::str::SplitWhitespace<'_>, shared: &SharedState) -> String {
    let combo = parts.collect::<Vec<_>>().join(" ");
    if combo.is_empty() {
        return "ERR HOTKEY requires a key combination\n".to_string();
    }

    let hotkey = match crate::hotkey::Hotkey::parse(&combo) {
        Ok(hotkey) => hotkey,
        Err(error) => return format!("ERR {error}\n"),
    };
    let canonical = hotkey.to_string();

    #[cfg(windows)]
    {
        match crate::backend::win::request_hotkey(shared, &canonical) {
            Ok(()) => "OK\n".to_string(),
            Err(error) => format!("ERR {error:#}\n"),
        }
    }

    #[cfg(not(windows))]
    {
        let _ = hotkey;
        shared.set_hotkey_requested(canonical);
        "OK\n".to_string()
    }
}

fn handle_hold_hotkey(parts: std::str::SplitWhitespace<'_>, shared: &SharedState) -> String {
    let combo = parts.collect::<Vec<_>>().join(" ");
    let disabled = combo.is_empty() || combo.eq_ignore_ascii_case("none");
    let canonical = if disabled {
        String::new()
    } else {
        match crate::hotkey::Hotkey::parse(&combo) {
            Ok(hotkey) => hotkey.to_string(),
            Err(error) => return format!("ERR {error}\n"),
        }
    };

    #[cfg(windows)]
    {
        let value = (!canonical.is_empty()).then_some(canonical.as_str());
        match crate::backend::win::request_hold_hotkey(shared, value) {
            Ok(()) => "OK\n".to_string(),
            Err(error) => format!("ERR {error:#}\n"),
        }
    }

    #[cfg(not(windows))]
    {
        shared.set_hold_hotkey_requested(canonical);
        "OK\n".to_string()
    }
}

#[cfg(windows)]
fn handle_hotkey_pause(shared: &SharedState) -> String {
    match crate::backend::win::pause_registration(shared) {
        Ok(()) => "OK\n".to_string(),
        Err(error) => format!("ERR {error:#}\n"),
    }
}

#[cfg(not(windows))]
fn handle_hotkey_pause(_shared: &SharedState) -> String {
    "OK\n".to_string()
}

#[cfg(windows)]
fn handle_hotkey_resume(shared: &SharedState) -> String {
    match crate::backend::win::resume_registration(shared) {
        Ok(()) => "OK\n".to_string(),
        Err(error) => format!("ERR {error:#}\n"),
    }
}

#[cfg(not(windows))]
fn handle_hotkey_resume(_shared: &SharedState) -> String {
    "OK\n".to_string()
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

    let limit = match parts.next() {
        Some(value) => value
            .parse::<u64>()
            .map_err(|_| "ERR invalid limit\n".to_string())?,
        None => 0,
    };
    let delay_ms = match parts.next() {
        Some(value) => value
            .parse::<f64>()
            .map_err(|_| "ERR invalid delay_ms\n".to_string())?,
        None => 0.0,
    };
    let multi_click = match parts.next() {
        Some(value) => value
            .parse::<u8>()
            .map_err(|_| "ERR invalid multi_click\n".to_string())?,
        None => 1,
    };

    let mut mode = None;
    let mut target = None;
    let mut points = Vec::new();
    let mut interval_ms = crate::state::DEFAULT_POINT_INTERVAL_MS;

    if let Some(value) = parts.next() {
        if let Some(parsed) = ClickMode::parse(value) {
            mode = Some(parsed);

            match parsed {
                ClickMode::Cursor => {}
                ClickMode::Point => {
                    let (x, y) = read_point(&mut parts)?;
                    target = Some(Point::new(x, y));
                }
                ClickMode::Sequence => {
                    interval_ms = parts
                        .next()
                        .ok_or_else(|| "ERR sequence requires interval_ms\n".to_string())?
                        .parse::<u32>()
                        .map_err(|_| "ERR invalid interval_ms\n".to_string())?;
                    let count = parts
                        .next()
                        .ok_or_else(|| "ERR sequence requires a point count\n".to_string())?
                        .parse::<usize>()
                        .map_err(|_| "ERR invalid point count\n".to_string())?;
                    if count > crate::state::MAX_POINTS {
                        return Err(format!(
                            "ERR at most {} points are supported\n",
                            crate::state::MAX_POINTS
                        ));
                    }
                    for _ in 0..count {
                        let (x, y) = read_point(&mut parts)?;
                        points.push(Point::new(x, y));
                    }
                }
            }
        } else {
            // Protocol 1-3 commands append the fixed point directly after
            // `multi_click`, without an explicit mode.
            let x = value
                .parse::<i32>()
                .map_err(|_| "ERR invalid mode or target x\n".to_string())?;
            let y = parts
                .next()
                .ok_or_else(|| "ERR target requires both x and y\n".to_string())?
                .parse::<i32>()
                .map_err(|_| "ERR invalid target y\n".to_string())?;
            target = Some(Point::new(x, y));
            mode = Some(ClickMode::Point);
        }
    }

    if parts.next().is_some() {
        return Err(format!("ERR too many arguments for {command}\n"));
    }

    let mode = mode.unwrap_or(if target.is_some() {
        ClickMode::Point
    } else {
        ClickMode::Cursor
    });

    Settings::new(cps, button, hold_ms, jitter_pct)
        .and_then(|settings| settings.with_limit(limit))
        .and_then(|settings| settings.with_delay_ms(delay_ms))
        .and_then(|settings| settings.with_multi_click(multi_click))
        .map(|settings| settings.with_mode(mode))
        .and_then(|settings| settings.with_target(target))
        .and_then(|settings| settings.with_points(&points))
        .and_then(|settings| settings.with_point_interval_ms(interval_ms))
        .map_err(|error| format!("ERR {error}\n"))
}

fn read_point(
    parts: &mut std::str::SplitWhitespace<'_>,
) -> std::result::Result<(i32, i32), String> {
    let x = parts
        .next()
        .ok_or_else(|| "ERR point requires both x and y\n".to_string())?
        .parse::<i32>()
        .map_err(|_| "ERR invalid point x\n".to_string())?;
    let y = parts
        .next()
        .ok_or_else(|| "ERR point requires both x and y\n".to_string())?
        .parse::<i32>()
        .map_err(|_| "ERR invalid point y\n".to_string())?;
    Ok((x, y))
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
    fn set_command_accepts_optional_limit_and_delay() {
        let shared = SharedState::new(Settings::default(), false);

        assert_eq!(process("SET 125 right 5 10 42 300", &shared), "OK\n");

        let status = shared.status();
        assert_eq!(status.settings.limit, 42);
        assert!((status.settings.delay_ms() - 300.0).abs() < 1e-9);
    }

    #[test]
    fn set_command_accepts_multi_click_and_target() {
        let shared = SharedState::new(Settings::default(), false);

        assert_eq!(
            process("SET 125 right 5 10 0 0 3 -1920 1080", &shared),
            "OK\n"
        );

        let status = shared.status();
        assert_eq!(status.settings.multi_click, 3);
        assert_eq!(status.settings.target, Some(Point::new(-1920, 1080)));
    }

    #[test]
    fn set_command_accepts_sequence_mode() {
        let shared = SharedState::new(Settings::default(), false);

        assert_eq!(
            process(
                "SET 50 left 10 0 0 0 1 sequence 250 3 -10 20 30 -40 500 600",
                &shared
            ),
            "OK\n"
        );

        let status = shared.status();
        assert_eq!(status.settings.mode, ClickMode::Sequence);
        assert_eq!(status.settings.point_count(), 3);
        assert_eq!(status.settings.point_at(0), Some(Point::new(-10, 20)));
        assert_eq!(status.settings.point_at(2), Some(Point::new(500, 600)));
        assert!((status.settings.point_interval_ms() - 250.0).abs() < 1e-9);
    }

    #[test]
    fn on_command_rejects_an_incomplete_sequence() {
        let shared = SharedState::new(Settings::default(), false);

        assert!(process("ON 50 left 10 0 0 0 1 sequence 100 1 10 20", &shared).starts_with("ERR "));
        assert!(!shared.status().enabled);

        assert_eq!(
            process("ON 50 left 10 0 0 0 1 sequence 100 2 10 20 30 40", &shared),
            "OK\n"
        );
        assert!(shared.status().enabled);
    }

    #[test]
    fn sequence_rejects_more_than_the_point_limit() {
        let shared = SharedState::new(Settings::default(), false);
        let mut command = String::from("SET 50 left 10 0 0 0 1 sequence 100 17");
        for index in 0..17 {
            command.push_str(&format!(" {index} {index}"));
        }

        assert!(process(&command, &shared).starts_with("ERR "));
        assert_eq!(shared.status().settings.point_count(), 0);
    }

    #[test]
    fn target_requires_both_coordinates() {
        let shared = SharedState::new(Settings::default(), false);

        assert!(process("SET 50 left 10 0 0 0 1 100", &shared).starts_with("ERR "));
        assert!(process("SET 50 left 10 0 0 0 1 100 200 300", &shared).starts_with("ERR "));
        assert_eq!(shared.status().settings.target, None);
    }

    #[test]
    fn invalid_on_command_is_rejected() {
        let shared = SharedState::new(Settings::default(), false);

        assert!(process("ON 0 left 10 0", &shared).starts_with("ERR "));
        assert!(process("ON 50 left 10", &shared).starts_with("ERR "));
        assert!(process("ON 50 side 10 0", &shared).starts_with("ERR "));
        assert!(process("ON 50 left 10 0 1 2 3 4", &shared).starts_with("ERR "));
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

    #[cfg(not(windows))]
    #[test]
    fn hotkey_command_canonicalizes_and_updates_state() {
        let shared = SharedState::new(Settings::default(), false);

        assert_eq!(process("HOTKEY alt+control+space", &shared), "OK\n");
        assert_eq!(shared.status().hotkey, "ctrl+alt+space");
        assert_eq!(shared.status().hotkey_error, None);

        assert_eq!(process("hotkey ctrl+shift+f8", &shared), "OK\n");
        assert_eq!(shared.status().hotkey, "ctrl+shift+f8");
        assert_eq!(shared.status().hotkey_active, "");
    }

    #[test]
    fn invalid_hotkey_is_rejected_without_changing_state() {
        let shared = SharedState::new(Settings::default(), false);

        let response = process("HOTKEY ctrl", &shared);
        assert!(response.starts_with("ERR "));
        assert_eq!(shared.status().hotkey, crate::hotkey::DEFAULT);

        let response = process("HOTKEY", &shared);
        assert!(response.starts_with("ERR "));
        assert_eq!(shared.status().hotkey, crate::hotkey::DEFAULT);
    }

    #[cfg(not(windows))]
    #[test]
    fn single_key_hotkey_is_accepted() {
        let shared = SharedState::new(Settings::default(), false);

        assert_eq!(process("HOTKEY f8", &shared), "OK\n");
        assert_eq!(shared.status().hotkey, "f8");
    }

    #[cfg(not(windows))]
    #[test]
    fn hold_hotkey_can_be_set_and_cleared() {
        let shared = SharedState::new(Settings::default(), false);

        assert_eq!(process("HOLD_HOTKEY alt+space", &shared), "OK\n");
        assert_eq!(shared.status().hold_hotkey, "alt+space");

        assert_eq!(process("HOLD_HOTKEY none", &shared), "OK\n");
        assert_eq!(shared.status().hold_hotkey, "");
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
