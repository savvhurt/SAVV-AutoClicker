<p align="center">
  <a href="https://github.com/savvhurt/SAVV-AutoClicker/releases">
    <img src="https://img.shields.io/badge/DOWNLOADS-0-brightgreen?style=for-the-badge" alt="Downloads">
  </a>
  <a href="https://github.com/savvhurt/SAVV-AutoClicker/releases">
    <img src="https://img.shields.io/badge/VERSION-V0.4.1-blue?style=for-the-badge" alt="Version">
  </a>
  <a href="https://github.com/savvhurt/SAVV-AutoClicker/blob/main/LICENSE">
    <img src="https://img.shields.io/badge/LICENSE-GPL--3.0-orange?style=for-the-badge" alt="License">
  </a>
  <a href="https://github.com/savvhurt/SAVV-AutoClicker/stargazers">
    <img src="https://img.shields.io/github/stars/savvhurt/SAVV-AutoClicker?style=for-the-badge&color=blue" alt="Stars">
</p>

# V-AutoClicker

A cross-platform autoclicker written in Rust for **Linux** (Wayland and X11) and
**Windows**. Clicks are injected at the operating-system level:

- **Linux** - a virtual mouse through `/dev/uinput` (works on Wayland and X11,
  no root required when the device is accessible to your user).
- **Windows** - `SendInput`, with no drivers and no administrator rights.

## Features

- 0.2–1000 clicks per second with configurable hold duration and jitter.
- Per-session click limit: `0` means unlimited, otherwise clicking stops after
  N clicks. The session counter is shown in the GUI and in `status`; the actual
  rate and session clicks share one two-column readout.
- **Start guard in cursor mode**: pressing the power button waits 1.5 s before
  the first click so the pointer can leave the window. Otherwise the synthetic
  click could land on the interface itself.
- Start delay of 0–10 000 ms. It applies only to the start of a new session;
  stopping during the delay cancels the start.
- Left, right and middle mouse button.
- **Three click modes**:
  - **Cursor** - clicks go to the current cursor position.
  - **Point** - a single fixed X/Y point; every pulse moves the cursor there
    before clicking.
  - **Sequence** - up to 16 points are clicked in order with a configurable
    interval. The cycle repeats until clicking is stopped.
- **Eyedropper**: the point and sequence editors have a picker button. Click it,
  then click anywhere on screen to capture the coordinates into X/Y. Escape
  cancels. Available on Windows and Linux/X11; on Wayland the GUI explains that
  the feature is unavailable.
- **Multi-click**: send 1 (single), 2 (double) or 3 (triple) clicks per pulse.
  A 40 ms pause separates the clicks inside a pulse; the session limit counts
  every individual click.
- Background daemon plus configurable hotkey: global on Windows, through a
  compositor bind on Linux.
- Free hotkeys on Windows: a single key without modifiers (`f8`, `space`,
  `esc`) or any Ctrl/Alt/Shift/Win combination.
- Hold-to-click on Windows: a separate key starts clicking while it is held and
  stops on release. It does not interfere when clicking was already enabled by
  the toggle hotkey.
- **Automatic UI scaling**: the window fits the monitor and the system DPI
  without a manual scale setting. It never grows above 100% and stops shrinking
  at 75%, so it stays usable on small or high-DPI screens.
- Six themes: Paper, Graphite, Sand, Ocean, Forest and Contrast. Theme,
  language, hotkeys and all click settings are
  saved automatically in `gui.toml`, including when the window is closed
  accidentally.
- **Restore defaults**: the System tab can reset click settings, mode, points,
  hotkeys, theme and language. OS autostart and the tray preference are kept.
- **Interface languages**: Auto, English, Русский, Deutsch, Français, Español
  and Português. Auto follows the system locale; an unknown locale falls back to
  English.
- **Autostart on Linux and Windows**: the System tab installs a freedesktop
  entry or a per-user registry value that launches the GUI with the session.
- **Windows tray**: optional **close to tray**. Closing the window hides it in
  the notification area instead of quitting, while minimizing keeps the normal
  taskbar button. Left-click the icon or use **Show** to restore the window;
  **Exit** always quits, even with close-to-tray enabled.
- **Windows elevation check**: on startup the app detects whether it runs as
  administrator and warns that elevated applications and games ignore clicks
  from a non-elevated process.
- CLI: `toggle`, `on`, `off`, `status`, `quit`.
- GUI in the same binary: running without arguments opens the window, while
  subcommands stay on the CLI. Four tabs - **Clicks**, **Mode**, **Hotkeys**,
  **System** - with live settings, statistics and hotkey recording. The window
  is 460×560 (minimum 420×520) at 100% scale, scales automatically to fit the
  monitor and scrolls when the content does not fit.
- Numeric fields use one uniform size that fits the largest values, so values
  such as `1000.0`, `5000.0` or `1000000000` are never cut off.
- The mouse button is always released on stop, Ctrl+C and daemon shutdown.
- A single daemon instance, controlled through a local socket
  (Unix socket / Windows named pipe).
- Application icon: embedded in the Windows executable and used for the GUI
  window; on Linux, enabling autostart also installs a desktop entry and icon.

## Build

CLI only:

```bash
cargo build --release
```

CLI and GUI:

```bash
cargo build --release --features gui
```

- Linux: `target/release/savv-autoclicker`
- Windows: `target\release\savv-autoclicker.exe` (requires a Rust stable MSVC
  toolchain and Visual Studio Build Tools)
- The GUI requires Rust 1.92+ (an `egui` 0.34 requirement); the CLI builds on
  older Rust versions.

### Cross-compiling for Windows from Linux

You can build the `.exe` without a Windows machine through the MSVC target and
`cargo-xwin`. `clang` and `lld` are required:

```bash
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin
cargo xwin build --release --features gui --target x86_64-pc-windows-msvc
```

The result is
`target/x86_64-pc-windows-msvc/release/savv-autoclicker.exe`.

The first build downloads the MSVC CRT and the Windows SDK. The binary is built
as a windowed application; when started from a terminal it attaches to the
parent console so `status`, `toggle` and `--help` print output as usual. On a
fatal GUI error, `%APPDATA%\SAVV-AutoClicker\gui-error.log` is written and a
system message box is shown.

### Prebuilt binaries

The release workflow (`.github/workflows/release.yml`) builds both platforms:

- manual run through **Actions → Release → Run workflow** builds
  `savv-autoclicker-windows-x86_64.zip` and
  `savv-autoclicker-linux-x86_64.tar.gz` as artifacts;
- pushing a `v*` tag additionally creates a GitHub Release with both archives
  and bilingual notes from `.github/release-notes/<tag>.md`.

The Windows archive contains `savv-autoclicker.exe`, the README and the font
license. The Linux archive contains `savv-autoclicker`, the README and the font
license.

## Quick start

One-off run in the terminal (stop with Ctrl+C):

```bash
savv-autoclicker run --cps 50 --button left --hold-ms 10
```

Daemon:

```bash
savv-autoclicker toggle          # start the daemon if needed and enable clicking
savv-autoclicker toggle          # disable
savv-autoclicker status          # state and measured CPS
savv-autoclicker off             # disable
savv-autoclicker quit            # stop the daemon
```

Start with other settings:

```bash
savv-autoclicker on --cps 100 --button right --hold-ms 8 --jitter-pct 10
savv-autoclicker on --cps 25 --button middle --limit 200
savv-autoclicker on --cps 50 --delay-ms 1500
```

`on` and `toggle` start the daemon automatically. Without the GUI the settings
live only while the daemon runs; a new daemon starts with the defaults
(`50 cps`, left button, `hold 10 ms`, no jitter, no limit and no delay).

Double/triple clicks and fixed-point clicks:

```bash
savv-autoclicker on --cps 20 --multi-click 2
savv-autoclicker on --cps 10 --multi-click 3 --x 640 --y 360
```

`--x` and `--y` must be supplied together. The limit counts every individual
click, including the clicks inside a multi-click pulse.

Start the daemon in the background:

```bash
savv-autoclicker daemon --detach   # return control immediately
savv-autoclicker daemon            # foreground, useful for debugging
```

## GUI

```bash
./target/release/savv-autoclicker
```

![SAVV-AutoClicker panel](docs/gui.png)

On Windows:

```powershell
.\target\release\savv-autoclicker.exe
```

Without arguments the GUI starts. With arguments the same file works as a CLI:
`savv-autoclicker.exe status`, `savv-autoclicker.exe toggle`,
`savv-autoclicker.exe daemon --detach`.

The interface is deliberately minimal: warm paper, graphite, sand, ocean,
forest or high-contrast surfaces with ink-like typography, a single signal
accent, a large power button and flat parameter rows without cards or a
decorative grid. IBM Plex Sans / IBM Plex Mono are embedded in the binary; the
license is in `assets/fonts/LICENSE.txt`. The theme dropdown in the System tab
changes the palette on the fly and is saved to `gui.toml`.

Tabs:

- **Clicks** - speed (hidden in sequence mode), mouse button, hold duration,
  jitter, limit and delay. A limit of `0` disables the restriction; after N
  clicks the session stops by itself. The delay of 0–10 000 ms applies only to
  the start of a new session: stopping while waiting cancels the start. All
  values are applied live, including during clicking.
- **Mode** - choose Cursor, Point or Sequence mode, edit X/Y coordinates,
  capture a point with the eyedropper, manage up to 16 sequence points and their
  interval, and select single, double or triple clicks per pulse.
- **Hotkeys** - on Windows, record the global toggle hotkey and the separate
  hold-to-click key. On Linux, the recommended `MOD+ALT+C → TOGGLE` bind is
  shown with a clickable label that copies a ready-to-use line containing the
  full path to the current binary (see “Hotkey” below).
- **System** - interface language (Auto or one of six languages), theme
  dropdown, autostart, Windows close-to-tray and elevation options, and the
  **Restore defaults** button.

The window is designed for 460×560 px at 100% scale; the minimum size is
420×520. The scale is chosen automatically from the monitor size and DPI: it is
capped at 100% and never goes below 75%. The tab body scrolls when needed.

### Point and sequence modes

In **Point** mode the X/Y fields hold one fixed target. Press the eyedropper
button next to them and click the desired place on screen; Escape cancels the
capture. The coordinates can also be typed manually.

In **Sequence** mode press **Add point** to add a row. Each row has its own
eyedropper and X/Y fields, and can be removed with `×`. The **Point interval**
(1–60 000 ms) is the pause between points; sequence speed is controlled by this
interval instead of CPS. At least two points are required to start. Sequence
points are stored separately from the single Point target, so switching modes
does not lose either.

### Restore defaults

**System → Restore defaults** asks for confirmation and resets click settings,
mode, points and interval, hotkeys, theme and language. OS autostart and the
Windows tray preference are left unchanged.

### Windows elevation

Windows blocks synthetic input from a non-elevated process to elevated windows
(UIPI). At startup the GUI checks whether it runs as administrator. If it does
not, the System tab shows a warning and a **Restart as administrator** button
that relaunches the app through the UAC prompt.

On Windows, a hotkey is recorded directly in the interface: click the field,
then press the key - either without modifiers (`f8`, `space`, `esc`) or combined
with Ctrl/Alt/Shift. Esc does not cancel recording; it is assigned as a normal
key. To cancel recording, click the field again. While recording, the daemon
unregisters the hotkeys, so an already active combination can be re-recorded. A
conflict with another program is shown below the row and the previous
combination stays active. F12 may be reserved by Windows; if so, the usual error
is shown and the previous key keeps working. If the daemon is an older version
without hotkey-change support, the GUI restarts it once with the current
combination. As a fallback,
`savv-autoclicker.exe daemon --hotkey ctrl+shift+f8` applies the combination to
a running daemon without a restart. The second hotkey, hold-to-click, has its
own field; identical keys for toggle and hold are rejected.

Hold-to-click works only on Windows: press and hold the key - clicks run;
release - they stop. If clicking is already enabled by the toggle, the hold key
does nothing. On Linux the field is hidden: the compositor owns the global key
and hold-to-click is not supported there.

On Windows the GUI starts the daemon when the window opens, so the global key
works immediately without pressing the start button. On Linux the GUI also
starts the daemon automatically, so clicking works right after the window
opens. Without close-to-tray, closing the window stops clicking and quits the
Windows daemon; on Linux the independent daemon keeps running in the
background. Use `savv-autoclicker quit` to stop it.

On Windows, enabling **Close to tray** hides the window when it is closed
(the title-bar X or Alt+F4) instead of quitting; minimizing still uses the
taskbar. Left-click the tray icon - or use **Show** in its menu - to restore the
window; **Exit** always quits and stops the daemon. With close-to-tray enabled,
autostart launches the GUI hidden (`--hidden`), so login does not open a window.

Windows 11 hides new notification-area icons behind the overflow chevron by
default. If the icon is not visible next to the clock, open the chevron and drag
it onto the taskbar, or enable it in **Settings → Personalisation → Taskbar →
Other system tray icons**. If the tray icon still does not appear, the app writes
diagnostics to `%APPDATA%\SAVV-AutoClicker\tray.log`.

GUI settings (click parameters, theme, language, tray preference and hotkeys)
are saved automatically:

- Linux: `$XDG_CONFIG_HOME/savv-autoclicker/gui.toml` or
  `~/.config/savv-autoclicker/gui.toml`
- Windows: `%APPDATA%\SAVV-AutoClicker\gui.toml`

An existing `autoclicker/gui.toml` from an older version is migrated on first
start. The draft is debounced and applied on change, then saved after the daemon
confirms it. The current draft is also saved when the window receives a close
request, so an accidental close does not lose changes.

Memory: on Linux the GUI uses roughly 18–22 MB PSS / 25–30 MB RSS at rest; on
Windows expect about 20–30 MB private working set. The GUI renders in software
(CPU), so it pulls in no GPU driver or video memory. While the window is hidden
in the tray, status updates are coalesced into a bounded queue, so memory stays
flat even if the hidden window cannot repaint.

## Linux

Check access to `/dev/uinput`:

```bash
test -w /dev/uinput && echo "uinput: writable"
```

If access is missing, load the module and add a udev rule:

```bash
sudo modprobe uinput
```

`/etc/udev/rules.d/99-uinput.rules`:

```udev
KERNEL=="uinput", SUBSYSTEM=="misc", MODE="0660", GROUP="input", TAG+="uaccess"
```

Then:

```bash
sudo udevadm control --reload-rules
sudo udevadm trigger
```

After logging in again, `/dev/uinput` will be accessible. On CachyOS/niri the
access is usually already configured through ACL; after a kernel update or
reboot you may need to run `sudo modprobe uinput` again.

### Autostart

Open the **System** tab and enable **Start at login**. This writes
`$XDG_CONFIG_HOME/autostart/savv-autoclicker.desktop` (or
`~/.config/autostart/savv-autoclicker.desktop`) with the command
`<current executable>`, and installs a desktop entry and a 256×256 icon under
`$XDG_DATA_HOME` so the app also appears in the launcher. Disable the switch to
remove the autostart entry. If you move the binary, re-enable autostart so the
entry points at the new path. Legacy `autoclicker.desktop` entries are removed
automatically.

### Hotkey

On Wayland global keys are handled by the compositor, so `--hotkey` and hotkey
recording in the GUI do not register on Linux. Configure the key with the
compositor; the GUI shows the recommended `MOD+ALT+C` combination and copies a
ready-to-use bind containing the full path.

**niri** - in `~/.config/niri/config.kdl` or an included binds file:

```kdl
binds {
    Mod+Alt+C { spawn "savv-autoclicker" "toggle"; }
}
```

If the binary is not in `PATH`, use the full path:

```kdl
Mod+Alt+C { spawn "/home/vvv/autoclicker/target/release/savv-autoclicker" "toggle"; }
```

Do not use `Mod+Shift+C`: it is already bound in many niri configurations
(`center-column`). After changing the config, run
`niri msg action load-config-file` or restart niri.

**GNOME / KDE** - Settings → Keyboard → Custom Shortcut, command:

```bash
savv-autoclicker toggle
```

Clicks go to the current cursor position, to the fixed point, or through the
configured sequence, so arrange the target before enabling clicking.

### Fixed-point clicking

Fixed-point clicking uses X11 `XWarpPointer`, so it works on X11 sessions and in
XWayland applications but not on a native Wayland session. The Mode tab detects
Wayland and shows a notice instead of silently doing nothing. On X11, the
eyedropper reads the cursor position through the X server.

## Windows

Start the daemon:

```powershell
.\savv-autoclicker.exe daemon
```

Or detached, without holding the console:

```powershell
.\savv-autoclicker.exe daemon --detach
```

The default global hotkey is **Ctrl+Alt+C**. It toggles clicking while the
daemon is running. A single key without modifiers, such as **F8**, can also be
assigned.

The combination can be recorded in the GUI: it is saved to `gui.toml` and
applied to the running daemon without a restart. If another program already owns
the combination, the daemon keeps the previous one and reports the error.

For the CLI, set another combination when starting the daemon:

```powershell
.\savv-autoclicker.exe daemon --hotkey f8
.\savv-autoclicker.exe daemon --hotkey ctrl+shift+f8
.\savv-autoclicker.exe daemon --hotkey alt+space --hold-hotkey f9
```

If the daemon is already running, this command applies the new keys without a
restart. The requested and actually active combinations are available in:

```powershell
.\savv-autoclicker.exe status
```

The output includes `hotkey`, `hotkey_active`, `hold_hotkey`,
`hold_hotkey_active` and `hotkey_error`. If the active combination is `none` or
differs from the requested one, check `hotkey_error` - usually it is a conflict
with another program.

Supported modifiers are `ctrl`, `alt`, `shift` and `win`; supported keys are
`A-Z`, `0-9`, `F1-F24`, `Space`, `Tab`, `Enter`, `Esc`, `Backspace`, `Delete`,
`Insert`, `Home`, `End`, `PageUp`, `PageDown`, the arrow keys and the symbols
`-`, `=`, `[`, `]`, `\`, `;`, `'`, `,`, `.`, `/`, `` ` ``. A combination made
only of modifiers is rejected. The GUI records only Ctrl/Alt/Shift: egui does
not forward the Win key, but it can still be specified through `--hotkey`. F12
may be reserved by Windows - a normal error is shown and the previous key stays
active.

Hold-to-click uses a separate key - in the GUI on the **Hotkeys** tab or through
the CLI:

```powershell
.\savv-autoclicker.exe daemon --hold-hotkey f9
```

Press and hold the key - clicks run; release - they stop. Hold works only while
clicking is disabled by the toggle. On Linux hold-to-click is unavailable; the
compositor owns the global key.

### Windows autostart and tray

Enable **Start at login** in the System tab to add the per-user value
`SAVV-AutoClicker` under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. The value points at the
current executable. If **Close to tray** is also enabled, autostart adds
`--hidden`, so the app starts in the notification area without opening a window.
Disable the checkbox to remove the value.

With **Close to tray** enabled, closing the window hides it and keeps the app in
the notification area. Minimizing still shows a normal taskbar button.
Left-click the tray icon or choose **Show** to restore the window; **Exit**
always quits and stops the daemon. On Windows 11 the icon may first appear in
the overflow menu - see the note in the GUI section above.

### Windows limitations

- `SendInput` cannot click into windows running as administrator unless
  SAVV-AutoClicker is elevated too (UIPI). The GUI warns at startup and can
  restart itself as administrator.
- Some games and anti-cheat systems ignore or block synthetic input.
- If the process is force-terminated with `TerminateProcess`, the mouse button
  may stay pressed: stop with `off`, `quit` or the hotkey instead. A short
  `hold` reduces the impact.
- The binary is not signed, so SmartScreen may show a warning.

When upgrading from an older `autoclicker` build: stop the old daemon
(`autoclicker.exe quit`) and remove the old binary. The new
`savv-autoclicker.exe` opens the GUI without arguments. A daemon left running by
the old version is shut down automatically, and legacy autostart entries are
removed when autostart is toggled.

The CLI works the same as on Linux:

```powershell
.\savv-autoclicker.exe toggle
.\savv-autoclicker.exe on --cps 100 --button left
.\savv-autoclicker.exe status
.\savv-autoclicker.exe off
.\savv-autoclicker.exe quit
```

## Parameters

| Parameter | Default | Description |
|---|---:|---|
| `--cps N` | `50` | Clicks per second, `0.2–1000` |
| `--button left\|right\|middle` | `left` | Mouse button |
| `--hold-ms MS` | `10` | How long the button stays pressed, `0.1–5000`; automatically limited by the interval |
| `--jitter-pct PCT` | `0` | Random interval deviation, `0–100` |
| `--limit N` | `0` | Clicks per session; `0` means unlimited; clicking stops after N clicks |
| `--delay-ms MS` | `0` | Pause before the first click of a new session, `0–10000`; not restarted by settings changes |
| `--multi-click N` | `1` | Clicks per pulse: `1` single, `2` double, `3` triple |
| `--x X` | - | Fixed-point X coordinate; requires `--y` |
| `--y Y` | - | Fixed-point Y coordinate; requires `--x` |
| `--duration-secs SECS` | - | `run` only: stop after N seconds |
| `--hotkey COMBO` | `ctrl+alt+c` | Windows only; any key or combination, applied live to a running daemon |
| `--hold-hotkey COMBO` | - | Windows and `daemon` only: hold-to-click key |
| `--detach` | - | `daemon` only: continue in the background |

Sequence mode and the point interval are configured in the GUI.

Examples:

```bash
savv-autoclicker run --cps 20 --hold-ms 25 --jitter-pct 15
savv-autoclicker on --cps 200 --button left --hold-ms 2 --limit 500
savv-autoclicker on --cps 50 --delay-ms 2000
savv-autoclicker on --cps 20 --multi-click 2
savv-autoclicker on --cps 10 --multi-click 3 --x 640 --y 360
savv-autoclicker run --cps 5 --duration-secs 3
```

## How it works

- The scheduler uses absolute `Instant` deadlines, so delays do not accumulate.
- The last ~0.2 ms before a deadline (Linux) or ~1 ms (Windows) runs a spin-loop;
  on Windows the system timer resolution is additionally raised to 1 ms.
- `hold` is automatically shortened so there is a gap between release and the
  next press.
- The start delay is bound to the session: changing settings while waiting does
  not restart the timer, and stopping cancels the start.
- The session counter resets on every start. After the Nth click the scheduler
  stops itself with the reason `limit` and releases the button.
- A multi-click pulse moves to the target point once, then sends the configured
  number of clicks with a 40 ms gap. The limit is checked after every click.
- Sequence mode moves to each point in turn and waits for the point interval
  between clicks, wrapping around after the last point.
- Fixed-point clicking clamps coordinates to the 16-bit X11 range and moves the
  cursor before each pulse. Windows uses `SetCursorPos`.
- Linux: the uinput device is removed when the process exits and the kernel
  releases held buttons by itself.
- Windows: release is called on `off`/`quit`/Ctrl+C/panic through a guard; there
  is no guarantee after a forced process kill. Hold-to-click uses a dedicated
  watcher thread polling `GetAsyncKeyState` every 5 ms, because `RegisterHotKey`
  reports only the initial key-down.
- IPC: a Unix socket with `0600` permissions in `$XDG_RUNTIME_DIR` or a Windows
  named pipe; a file lock prevents a second daemon from starting.
- On Linux, clicking works in native Wayland applications and in X11/XWayland.
- GUI diagnostics written to the config are in English; interface text follows
  the selected language.

## Limitations

- Fixed-point and sequence clicking require Windows or Linux/X11; they are
  unavailable on native Wayland, where the eyedropper cannot read the global
  pointer.
- The real CPS ceiling depends on the target application and its polling rate.
- At high CPS (hundreds of hertz) a CPU core may be busy because of the precise
  wait.
- Some applications and anti-cheat systems block uinput/SendInput.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo xwin check --all-targets --all-features --target x86_64-pc-windows-msvc
```

CI (`.github/workflows/ci.yml`) builds and tests the binary with the GUI on
`ubuntu-latest` and `windows-latest`.

Manual check on Linux:

```bash
savv-autoclicker run --cps 5 --duration-secs 2
```

Point the cursor at a text editor - clicks should appear, and the button must
not stay pressed after the run finishes.
