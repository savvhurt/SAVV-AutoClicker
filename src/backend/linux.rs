use std::env;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use evdev::uinput::VirtualDevice;
use evdev::{
    AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode, RelativeAxisCode,
    SynchronizationCode,
};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ConnectionExt as XprotoConnectionExt, KeyButMask, QueryPointerReply,
};
use x11rb::rust_connection::RustConnection;

use crate::state::{Button, Point};

pub struct LinuxBackend {
    device: VirtualDevice,
    x11: Option<X11>,
    x11_attempted: bool,
    target_supported: bool,
}

struct X11 {
    connection: RustConnection,
    root: u32,
}

impl X11 {
    fn connect() -> Result<Self> {
        if is_wayland() {
            bail!("clicking at a fixed point is not available on Wayland");
        }
        if !has_display() {
            bail!("the X11 display is not available");
        }

        let (connection, screen) =
            x11rb::connect(None).context("failed to connect to the X11 display")?;
        let root = connection.setup().roots[screen].root;
        Ok(Self { connection, root })
    }

    fn move_to(&self, point: Point) -> Result<()> {
        let x = point.x.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        let y = point.y.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        self.connection
            .warp_pointer(x11rb::NONE, self.root, 0, 0, 0, 0, x, y)
            .context("failed to move the X11 cursor")?;
        self.connection
            .flush()
            .context("failed to flush the X11 cursor request")
    }
}

impl LinuxBackend {
    pub fn new() -> Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        keys.insert(KeyCode::BTN_LEFT);
        keys.insert(KeyCode::BTN_RIGHT);
        keys.insert(KeyCode::BTN_MIDDLE);

        let mut axes = AttributeSet::<RelativeAxisCode>::new();
        axes.insert(RelativeAxisCode::REL_X);
        axes.insert(RelativeAxisCode::REL_Y);

        let device = VirtualDevice::builder()
            .context(
                "cannot open /dev/uinput; make it writable for your user or install a udev rule",
            )?
            .name("v-autoclicker virtual mouse")
            .input_id(InputId::new(BusType::BUS_VIRTUAL, 0x1, 0x1, 1))
            .with_keys(&keys)?
            .with_relative_axes(&axes)?
            .build()
            .context("failed to create the virtual mouse device")?;

        Ok(Self {
            device,
            x11: None,
            x11_attempted: false,
            target_supported: target_supported(),
        })
    }

    fn send(&mut self, button: Button, value: i32) -> Result<()> {
        let code = match button {
            Button::Left => KeyCode::BTN_LEFT,
            Button::Right => KeyCode::BTN_RIGHT,
            Button::Middle => KeyCode::BTN_MIDDLE,
        };

        self.device
            .emit(&[
                InputEvent::new(EventType::KEY.0, code.0, value),
                InputEvent::new(
                    EventType::SYNCHRONIZATION.0,
                    SynchronizationCode::SYN_REPORT.0,
                    0,
                ),
            ])
            .context("failed to emit a uinput event")
    }

    fn x11(&mut self) -> Result<&X11> {
        if !self.x11_attempted {
            self.x11_attempted = true;
            self.x11 = Some(X11::connect()?);
        }

        self.x11
            .as_ref()
            .context("clicking at a fixed point is not available on this display")
    }
}

impl super::Backend for LinuxBackend {
    fn press(&mut self, button: Button) -> Result<()> {
        self.send(button, 1)
    }

    fn release(&mut self, button: Button) -> Result<()> {
        self.send(button, 0)
    }

    fn move_to(&mut self, point: Point) -> Result<()> {
        self.x11()?.move_to(point)
    }

    fn supports_target(&self) -> bool {
        self.target_supported
    }
}

pub fn target_supported() -> bool {
    !is_wayland() && has_display()
}

pub fn cursor_position() -> Result<Point> {
    let x11 = X11::connect()?;
    let reply = x11
        .connection
        .query_pointer(x11.root)
        .context("failed to query the X11 cursor")?
        .reply()
        .context("failed to read the X11 cursor position")?;
    Ok(Point::new(reply.root_x as i32, reply.root_y as i32))
}

/// Wait for the next mouse click and return the pointer position. Escape
/// cancels the capture. Only X11 is supported: Wayland does not let a normal
/// client read the global pointer position.
pub fn capture_point() -> Result<Option<Point>> {
    const POLL: Duration = Duration::from_millis(8);
    const RELEASE_WAIT: Duration = Duration::from_secs(3);

    let x11 = X11::connect()?;
    let escape = escape_keycode(&x11);

    let query = || -> Result<QueryPointerReply> {
        x11.connection
            .query_pointer(x11.root)
            .context("failed to query the X11 cursor")?
            .reply()
            .context("failed to read the X11 cursor position")
    };
    let any_button = |reply: &QueryPointerReply| {
        reply
            .mask
            .intersects(KeyButMask::BUTTON1 | KeyButMask::BUTTON2 | KeyButMask::BUTTON3)
    };

    // The eyedropper button was clicked with the mouse; wait for the release
    // before looking for the target click.
    let deadline = Instant::now() + RELEASE_WAIT;
    while any_button(&query()?) && Instant::now() < deadline {
        thread::sleep(POLL);
    }

    loop {
        thread::sleep(POLL);

        if let Some(code) = escape {
            let keys = x11
                .connection
                .query_keymap()
                .context("failed to query the X11 keyboard")?
                .reply()
                .context("failed to read the X11 keyboard state")?
                .keys;
            let index = usize::from(code / 8);
            if keys
                .get(index)
                .is_some_and(|byte| byte & (1 << (code % 8)) != 0)
            {
                return Ok(None);
            }
        }

        let reply = query()?;
        if any_button(&reply) {
            return Ok(Some(Point::new(reply.root_x as i32, reply.root_y as i32)));
        }
    }
}

fn escape_keycode(x11: &X11) -> Option<u8> {
    const XK_ESCAPE: u32 = 0xff1b;

    let setup = x11.connection.setup();
    let count = (u16::from(setup.max_keycode) - u16::from(setup.min_keycode) + 1)
        .try_into()
        .unwrap_or(u8::MAX);
    let reply = x11
        .connection
        .get_keyboard_mapping(setup.min_keycode, count)
        .ok()?
        .reply()
        .ok()?;
    let per_keycode = usize::from(reply.keysyms_per_keycode).max(1);

    for (index, keysyms) in reply.keysyms.chunks(per_keycode).enumerate() {
        if keysyms.contains(&XK_ESCAPE) {
            return u8::try_from(u16::from(setup.min_keycode) + index as u16).ok();
        }
    }

    None
}

pub fn is_wayland() -> bool {
    if env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty()) {
        return true;
    }

    env::var("XDG_SESSION_TYPE").is_ok_and(|value| value.eq_ignore_ascii_case("wayland"))
}

fn has_display() -> bool {
    env::var_os("DISPLAY").is_some_and(|value| !value.is_empty())
}
