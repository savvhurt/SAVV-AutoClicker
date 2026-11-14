<p align="center">
  <a href="https://github.com/savvhurt/V-AutoClicker/releases">
    <img src="https://img.shields.io/github/downloads/savvhurt/V-AutoClicker/total?style=for-the-badge&color=brightgreen&label=DOWNLOADS" alt="Downloads">
  </a>
  <a href="https://github.com/savvhurt/V-AutoClicker/releases/latest">
    <img src="https://img.shields.io/github/v/release/savvhurt/V-AutoClicker?style=for-the-badge&color=blue&label=VERSION&sort=semver" alt="Version">
  </a>
  <a href="https://github.com/savvhurt/V-AutoClicker/stargazers">
    <img src="https://img.shields.io/github/stars/savvhurt/V-AutoClicker?style=for-the-badge&color=yellow" alt="Stars">
  </a>
</p>

<div align="center">

# V-AutoClicker

**A fast, minimal autoclicker for Windows and Linux.**<br>
Native input, four click modes, twelve themes, one small binary.

[Download](https://github.com/savvhurt/V-AutoClicker/releases/latest) ·
[CLI](#cli) ·
[Build from source](#build-from-source)

<img src="docs/gui.png" alt="V-AutoClicker interface" width="460">

</div>

## Highlights

- **0.2-1000 CPS** with hold duration, jitter, start delay and a per-session limit.
- **Cursor, Point, Sequence and Parallel modes** - click where the mouse is,
  at fixed coordinates, through up to 16 points in order, or at every point
  once per round.
- **Eyedropper** for capturing coordinates; single, double and triple clicks.
- **Global hotkeys** on Windows, compositor bindings on Linux.
- **Hold-to-click** on Windows with a second hotkey.
- **Autostart** on both platforms, plus optional **close to tray** on Windows.
- **12 themes**, six interface languages and automatic DPI scaling.
- **One binary**: no arguments opens the GUI, subcommands stay on the CLI.
- Settings live in `gui.toml` and are saved automatically.

Clicks are injected by the OS: `SendInput` on Windows, `/dev/uinput` on Linux
(Wayland and X11). No drivers and no administrator rights required.

## Download

Grab the latest archive from the
[releases page](https://github.com/savvhurt/V-AutoClicker/releases/latest):

| Platform | Archive |
|---|---|
| Windows 10/11 x86_64 | `v-autoclicker-windows-x86_64.zip` |
| Linux x86_64 | `v-autoclicker-linux-x86_64.tar.gz` |

Both archives contain the binary, this README and `FONT-LICENSE.txt`.

## Linux

### Quick install

```bash
tar -xzf v-autoclicker-linux-x86_64.tar.gz
mkdir -p ~/.local/bin
install -m 755 v-autoclicker-linux-x86_64/v-autoclicker ~/.local/bin/
```

Make sure `~/.local/bin` is in `PATH`, then run:

```bash
v-autoclicker
```

### Allow access to `/dev/uinput`

```bash
test -w /dev/uinput && echo "uinput: writable"
```

If the check fails, load the module and add a udev rule:

```bash
sudo modprobe uinput
```

Create `/etc/udev/rules.d/99-uinput.rules`:

```udev
KERNEL=="uinput", SUBSYSTEM=="misc", MODE="0660", GROUP="input", TAG+="uaccess"
```

Then reload the rules and log in again:

```bash
sudo udevadm control --reload-rules
sudo udevadm trigger
```

On CachyOS/niri access is usually configured through ACL; after a kernel update
you may need `sudo modprobe uinput` again.

### Autostart and hotkey

Enable **Start at login** in the **System** tab. The app writes
`~/.config/autostart/v-autoclicker.desktop`, installs a launcher entry and a
256×256 icon.

On Wayland global keys belong to the compositor. **niri** example:

```kdl
binds {
    Mod+Alt+C { spawn "v-autoclicker" "toggle"; }
}
```

For GNOME or KDE create a custom shortcut with the command `v-autoclicker toggle`.
Do not use `Mod+Shift+C` in niri: it is already bound to `center-column`.

### Point, sequence and parallel modes

Point, sequence and parallel modes use `XWarpPointer`. They work on X11 and
XWayland but not in native Wayland windows; the GUI shows a notice on Wayland.

## Windows

### Quick start

1. Download and extract `v-autoclicker-windows-x86_64.zip`.
2. Run `v-autoclicker.exe`. The GUI opens.
3. Press the power button, or use the default hotkey **Ctrl+Alt+C**.

No installer, drivers or administrator rights are needed. SmartScreen may warn
about an unsigned binary.

### Hotkeys, tray and autostart

- The global hotkey is recorded on the **Hotkeys** tab and applied live.
- A second key can start clicking while held (**hold-to-click**).
- **Close to tray** hides the window instead of quitting; **Exit** in the tray
  menu always stops the daemon.
- **Start at login** writes the per-user value `V-AutoClicker` under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

### Windows limitations

- Elevated windows and games ignore input from a non-elevated process (UIPI).
  The GUI warns at startup and can restart itself as administrator.
- Some anti-cheat systems block synthetic input.
- If the process is force-killed, the mouse button may stay pressed. Stop with
  `off`, `quit` or the hotkey instead.

## CLI

```bash
v-autoclicker                 # open the GUI
v-autoclicker toggle          # start the daemon if needed and toggle clicking
v-autoclicker on              # start clicking
v-autoclicker off             # stop clicking
v-autoclicker status          # state, measured CPS and hotkeys
v-autoclicker quit            # stop the daemon
v-autoclicker daemon --detach # background daemon
```

One-off run (Ctrl+C stops):

```bash
v-autoclicker run --cps 50 --button left --hold-ms 10
v-autoclicker on --cps 100 --button right --limit 200 --delay-ms 1500
v-autoclicker on --cps 20 --multi-click 2 --x 640 --y 360
```

| Option | Default | Description |
|---|---:|---|
| `--cps N` | `50` | Clicks per second, `0.2-1000` |
| `--button left\|right\|middle` | `left` | Mouse button |
| `--hold-ms MS` | `10` | Button hold time, `0.1-5000` |
| `--jitter-pct PCT` | `0` | Random interval deviation, `0-100` |
| `--limit N` | `0` | Clicks per session; `0` means unlimited |
| `--delay-ms MS` | `0` | Delay before a new session, `0-10000` |
| `--multi-click N` | `1` | Clicks per pulse: `1`, `2` or `3` |
| `--x X`, `--y Y` | - | Fixed point; both are required together |
| `--duration-secs SECS` | - | `run` only: stop after N seconds |
| `--hotkey COMBO` | `ctrl+alt+c` | Windows only, applied live |
| `--hold-hotkey COMBO` | - | Windows hold-to-click key |
| `--detach` | - | `daemon` only: continue in the background |

Sequence and Parallel modes and their intervals are configured in the GUI.
Parallel mode clicks every point once per round with a minimal gap, then waits
for the round interval. It needs at least two points.

## GUI and themes

The window has four tabs: **Clicks**, **Mode**, **Hotkeys** and **System**.
Settings apply live, including while clicking.

Available themes:

| Light | Dark |
|---|---|
| Paper, Sand, Contrast, Rose, Lavender, Coffee | Graphite, Ocean, Forest, Midnight, Nord, Matcha |

Languages: Auto, English, Русский, Deutsch, Français, Español, Português.
Auto follows the system locale and falls back to English.

Settings are stored in:

- Linux: `~/.config/v-autoclicker/gui.toml`
- Windows: `%APPDATA%\V-AutoClicker\gui.toml`

## Build from source

Requires Rust 1.92+ for the GUI (the CLI builds on older versions).

```bash
git clone https://github.com/savvhurt/V-AutoClicker.git
cd V-AutoClicker
cargo build --release --features gui
```

CLI only:

```bash
cargo build --release
```

The binary is at `target/release/v-autoclicker` (`v-autoclicker.exe` on Windows).

### Linux build dependencies

| Distribution | Command |
|---|---|
| Arch / CachyOS | `sudo pacman -S --needed base-devel pkgconf` |
| Debian / Ubuntu | `sudo apt install build-essential pkg-config` |
| Fedora | `sudo dnf install gcc make pkgconf-pkg-config` |
| openSUSE | `sudo zypper install -t pattern devel_basis` |

### Windows build dependencies

Install [Rust](https://rustup.rs) with the MSVC toolchain and
[Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
with **Desktop development with C++**.

```powershell
rustup default stable-msvc
cargo build --release --features gui
```

### Cross-compiling for Windows from Linux

```bash
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin
cargo xwin build --release --features gui --target x86_64-pc-windows-msvc
```

The result is
`target/x86_64-pc-windows-msvc/release/v-autoclicker.exe`.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo xwin check --all-targets --all-features --target x86_64-pc-windows-msvc
```

CI runs format, clippy, tests and a release build on Linux and Windows.
Releases are built by `.github/workflows/release.yml` from `v*` tags.

## How it works

- Absolute `Instant` deadlines keep the schedule stable; a short spin-loop
  removes the last fraction of a millisecond.
- The mouse button is released on stop, Ctrl+C and daemon shutdown.
- A single daemon instance is controlled through a Unix socket or a Windows
  named pipe.
- Windows uses `SendInput`; Linux creates a virtual mouse through
  `/dev/uinput`, with X11 `XWarpPointer` for fixed-point movement.
- The GUI renders in software: no GPU driver or video memory required.
