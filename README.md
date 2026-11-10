# autoclicker

A cross-platform autoclicker written in Rust for **Linux** (Wayland and X11) and
**Windows**. Clicks are injected at the operating-system level:

- **Linux** — a virtual mouse through `/dev/uinput` (works on Wayland and X11,
  no root required when the device is accessible to your user).
- **Windows** — `SendInput`, with no drivers and no administrator rights.

## Features

- 0.2–1000 clicks per second with configurable hold duration and jitter.
- Per-session click limit: `0` means unlimited, otherwise clicking stops after
  N clicks. The session counter is shown in the GUI and in `status`.
- Start delay of 0–10 000 ms. It applies only to the start of a new session;
  stopping during the delay cancels the start.
- Left, right and middle mouse button.
- **Click at a fixed point**: enter X/Y coordinates and every pulse moves the
  cursor there before clicking. Available on Windows and Linux/X11; on Wayland
  the GUI explains that the feature is unavailable. The **Use current** button
  captures the cursor position.
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
- Light and dark themes. Theme, language, hotkeys and all click settings are
  saved automatically in `gui.toml`, including when the window is closed
  accidentally.
- **Interface languages**: Auto, English, Русский, Deutsch, Français, Español
  and Português. Auto follows the system locale; an unknown locale falls back to
  English.
- **Linux autostart**: the System tab can install a freedesktop autostart entry
  that launches the background daemon (`daemon --detach`) with the session.
- CLI: `toggle`, `on`, `off`, `status`, `quit`.
- GUI in the same binary: running without arguments opens the window, while
  subcommands stay on the CLI. Four tabs — **Clicks**, **Mode**, **Hotkeys**,
  **System** — with live settings, statistics and hotkey recording. The window
  is 460×560 (minimum 420×520) and does not scroll with normal settings.
- Numeric fields widen to fit the maximum value, so large values such as
  `1000.0`, `5000.0` or `1000000000` are never cut off.
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

- Linux: `target/release/autoclicker`
- Windows: `target\release\autoclicker.exe` (requires a Rust stable MSVC
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

The result is `target/x86_64-pc-windows-msvc/release/autoclicker.exe`.

The first build downloads the MSVC CRT and the Windows SDK. The binary is built
as a windowed application; when started from a terminal it attaches to the
parent console so `status`, `toggle` and `--help` print output as usual. On a
fatal GUI error, `%APPDATA%\autoclicker\gui-error.log` is written and a system
message box is shown.

### Prebuilt Windows binaries

The **Windows release** workflow (`.github/workflows/release.yml`) is included:

- manual run through **Actions → Windows release → Run workflow** builds
  `autoclicker-windows-x86_64.zip` and uploads it as an artifact;
- pushing a `v*` tag additionally creates a GitHub Release with the archive.

The archive contains `autoclicker.exe`, the README and the font license.

## Quick start

One-off run in the terminal (stop with Ctrl+C):

```bash
autoclicker run --cps 50 --button left --hold-ms 10
```

Daemon:

```bash
autoclicker toggle          # start the daemon if needed and enable clicking
autoclicker toggle          # disable
autoclicker status          # state and measured CPS
autoclicker off             # disable
autoclicker quit            # stop the daemon
```

Start with other settings:

```bash
autoclicker on --cps 100 --button right --hold-ms 8 --jitter-pct 10
autoclicker on --cps 25 --button middle --limit 200
autoclicker on --cps 50 --delay-ms 1500
```

`on` and `toggle` start the daemon automatically. Without the GUI the settings
live only while the daemon runs; a new daemon starts with the defaults
(`50 cps`, left button, `hold 10 ms`, no jitter, no limit and no delay).

Double/triple clicks and fixed-point clicks:

```bash
autoclicker on --cps 20 --multi-click 2
autoclicker on --cps 10 --multi-click 3 --x 640 --y 360
```

`--x` and `--y` must be supplied together. The limit counts every individual
click, including the clicks inside a multi-click pulse.

Start the daemon in the background:

```bash
autoclicker daemon --detach   # return control immediately
autoclicker daemon            # foreground, useful for debugging
```

## GUI

```bash
./target/release/autoclicker
```

![Minimal autoclicker panel](docs/gui.png)

On Windows:

```powershell
.\target\release\autoclicker.exe
```

Without arguments the GUI starts. With arguments the same file works as a CLI:
`autoclicker.exe status`, `autoclicker.exe toggle`,
`autoclicker.exe daemon --detach`.

The interface is deliberately minimal: a warm paper background or a dark
palette, ink-like typography, a single signal-orange accent, a large power
button and flat parameter rows without cards or a decorative grid. IBM Plex
Sans / IBM Plex Mono are embedded in the binary; the license is in
`assets/fonts/LICENSE.txt`. The theme switch in the top-right corner changes
the palette on the fly and is saved to `gui.toml`.

Tabs:

- **Clicks** — speed, mouse button, hold duration, jitter, limit and delay.
  A limit of `0` disables the restriction; after N clicks the session stops by
  itself. The delay of 0–10 000 ms applies only to the start of a new session:
  stopping while waiting cancels the start. All values are applied live,
  including during clicking.
- **Mode** — choose between clicking at the current cursor position and a fixed
  point, edit the X/Y coordinates, capture the current cursor position, and
  select single, double or triple clicks per pulse.
- **Hotkeys** — on Windows, record the global toggle hotkey and the separate
  hold-to-click key. On Linux, the recommended `MOD+ALT+C → TOGGLE` bind is
  shown with a clickable label that copies a ready-to-use line containing the
  full path to the current binary (see “Hotkey” below).
- **System** — interface language (Auto or one of six languages), light/dark
  theme, and the Linux “Start at login” switch.

The window is designed for 460×560 px and does not scroll with normal settings;
the minimum size is 420×520.

On Windows, a hotkey is recorded directly in the interface: click the field,
then press the key — either without modifiers (`f8`, `space`, `esc`) or combined
with Ctrl/Alt/Shift. Esc does not cancel recording; it is assigned as a normal
key. To cancel recording, click the field again. While recording, the daemon
unregisters the hotkeys, so an already active combination can be re-recorded. A
conflict with another program is shown below the row and the previous
combination stays active. F12 may be reserved by Windows; if so, the usual error
is shown and the previous key keeps working. If the daemon is an older version
without hotkey-change support, the GUI restarts it once with the current
combination. As a fallback,
`autoclicker.exe daemon --hotkey ctrl+shift+f8` applies the combination to a
running daemon without a restart. The second hotkey, hold-to-click, has its own
field; identical keys for toggle and hold are rejected.

Hold-to-click works only on Windows: press and hold the key — clicks run;
release — they stop. If clicking is already enabled by the toggle, the hold key
does nothing. On Linux the field is hidden: the compositor owns the global key
and hold-to-click is not supported there.

On Windows the GUI starts the daemon when the window opens, so the global key
works immediately without pressing the start button. Closing the window
disables clicking and stops the daemon — no background process remains. On
Linux, closing the window disables clicking but the daemon keeps running; use
`autoclicker quit` to stop it.

GUI settings (click parameters, theme, language and hotkeys) are saved
automatically:

- Linux: `$XDG_CONFIG_HOME/autoclicker/gui.toml` or
  `~/.config/autoclicker/gui.toml`
- Windows: `%APPDATA%\autoclicker\gui.toml`

The draft is debounced and applied on change, then saved after the daemon
confirms it. The current draft is also saved when the window receives a close
request, so an accidental close does not lose changes.

Memory: on Linux the GUI uses roughly 18–22 MB PSS / 25–30 MB RSS at rest; on
Windows expect about 20–30 MB private working set. The GUI renders in software
(CPU), so it pulls in no GPU driver or video memory.

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
`$XDG_CONFIG_HOME/autostart/autoclicker.desktop` (or
`~/.config/autostart/autoclicker.desktop`) with the command
`<current executable> daemon --detach`, and installs a desktop entry and a
256×256 icon under `$XDG_DATA_HOME` so the app also appears in the launcher.
Disable the switch to remove the autostart entry. If you move the binary,
re-enable autostart so the entry points at the new path.

### Hotkey

On Wayland global keys are handled by the compositor, so `--hotkey` and hotkey
recording in the GUI do not register on Linux. Configure the key with the
compositor; the GUI shows the recommended `MOD+ALT+C` combination and copies a
ready-to-use bind containing the full path.

**niri** — in `~/.config/niri/config.kdl` or an included binds file:

```kdl
binds {
    Mod+Alt+C { spawn "autoclicker" "toggle"; }
}
```

If the binary is not in `PATH`, use the full path:

```kdl
Mod+Alt+C { spawn "/home/vvv/autoclicker/target/release/autoclicker" "toggle"; }
```

Do not use `Mod+Shift+C`: it is already bound in many niri configurations
(`center-column`). After changing the config, run
`niri msg action load-config-file` or restart niri.

**GNOME / KDE** — Settings → Keyboard → Custom Shortcut, command:

```bash
autoclicker toggle
```

Clicks go to the current cursor position (or to the fixed point when one is
set), so place the pointer where you need it before enabling clicking.

### Fixed-point clicking

Fixed-point clicking uses X11 `XWarpPointer`, so it works on X11 sessions and in
XWayland applications but not on a native Wayland session. The Mode tab detects
Wayland and shows a notice instead of silently doing nothing. On X11, **Use
current** reads the cursor position through the X server.

## Windows

Start the daemon:

```powershell
.\autoclicker.exe daemon
```

Or detached, without holding the console:

```powershell
.\autoclicker.exe daemon --detach
```

The default global hotkey is **Ctrl+Alt+C**. It toggles clicking while the
daemon is running. A single key without modifiers, such as **F8**, can also be
assigned.

The combination can be recorded in the GUI: it is saved to `gui.toml` and
applied to the running daemon without a restart. If another program already owns
the combination, the daemon keeps the previous one and reports the error.

For the CLI, set another combination when starting the daemon:

```powershell
.\autoclicker.exe daemon --hotkey f8
.\autoclicker.exe daemon --hotkey ctrl+shift+f8
.\autoclicker.exe daemon --hotkey alt+space --hold-hotkey f9
```

If the daemon is already running, this command applies the new keys without a
restart. The requested and actually active combinations are available in:

```powershell
.\autoclicker.exe status
```

The output includes `hotkey`, `hotkey_active`, `hold_hotkey`,
`hold_hotkey_active` and `hotkey_error`. If the active combination is `none` or
differs from the requested one, check `hotkey_error` — usually it is a conflict
with another program.

Supported modifiers are `ctrl`, `alt`, `shift` and `win`; supported keys are
`A-Z`, `0-9`, `F1-F24`, `Space`, `Tab`, `Enter`, `Esc`, `Backspace`, `Delete`,
`Insert`, `Home`, `End`, `PageUp`, `PageDown`, the arrow keys and the symbols
`-`, `=`, `[`, `]`, `\`, `;`, `'`, `,`, `.`, `/`, `` ` ``. A combination made
only of modifiers is rejected. The GUI records only Ctrl/Alt/Shift: egui does
not forward the Win key, but it can still be specified through `--hotkey`. F12
may be reserved by Windows — a normal error is shown and the previous key stays
active.

Hold-to-click uses a separate key — in the GUI on the **Hotkeys** tab or through
the CLI:

```powershell
.\autoclicker.exe daemon --hold-hotkey f9
```

Press and hold the key — clicks run; release — they stop. Hold works only while
clicking is disabled by the toggle. On Linux hold-to-click is unavailable; the
compositor owns the global key.

Autostart: create a shortcut to `autoclicker.exe daemon --detach` in the
`shell:startup` folder. A console window flashes at login and the daemon then
stays hidden.

When upgrading from an older version: stop the old daemon
(`autoclicker.exe quit`), close the old GUI and delete `autoclicker-gui.exe`.
The new `autoclicker.exe` opens the GUI without arguments; the separate
`autoclicker-gui.exe` is no longer needed.

The CLI works the same as on Linux:

```powershell
.\autoclicker.exe toggle
.\autoclicker.exe on --cps 100 --button left
.\autoclicker.exe status
.\autoclicker.exe off
.\autoclicker.exe quit
```

### Windows limitations

- `SendInput` cannot click into windows running as administrator unless the
  autoclicker is elevated too (UIPI).
- Some games and anti-cheat systems ignore or block synthetic input.
- If the process is force-terminated with `TerminateProcess`, the mouse button
  may stay pressed: stop with `off`, `quit` or the hotkey instead. A short
  `hold` reduces the impact.
- The binary is not signed, so SmartScreen may show a warning.

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
| `--x X` | — | Fixed-point X coordinate; requires `--y` |
| `--y Y` | — | Fixed-point Y coordinate; requires `--x` |
| `--duration-secs SECS` | — | `run` only: stop after N seconds |
| `--hotkey COMBO` | `ctrl+alt+c` | Windows only; any key or combination, applied live to a running daemon |
| `--hold-hotkey COMBO` | — | Windows and `daemon` only: hold-to-click key |
| `--detach` | — | `daemon` only: continue in the background |

Examples:

```bash
autoclicker run --cps 20 --hold-ms 25 --jitter-pct 15
autoclicker on --cps 200 --button left --hold-ms 2 --limit 500
autoclicker on --cps 50 --delay-ms 2000
autoclicker on --cps 20 --multi-click 2
autoclicker on --cps 10 --multi-click 3 --x 640 --y 360
autoclicker run --cps 5 --duration-secs 3
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

- Fixed-point clicking requires Windows or Linux/X11; it is unavailable on
  native Wayland.
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
autoclicker run --cps 5 --duration-secs 2
```

Point the cursor at a text editor — clicks should appear, and the button must
not stay pressed after the run finishes.
