use anyhow::{Context, Result};
use evdev::uinput::VirtualDevice;
use evdev::{
    AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode, RelativeAxisCode,
    SynchronizationCode,
};

use crate::state::Button;

pub struct LinuxBackend {
    device: VirtualDevice,
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
            .name("autoclicker virtual mouse")
            .input_id(InputId::new(BusType::BUS_VIRTUAL, 0x1, 0x1, 1))
            .with_keys(&keys)?
            .with_relative_axes(&axes)?
            .build()
            .context("failed to create the virtual mouse device")?;

        Ok(Self { device })
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
}

impl super::Backend for LinuxBackend {
    fn press(&mut self, button: Button) -> Result<()> {
        self.send(button, 1)
    }

    fn release(&mut self, button: Button) -> Result<()> {
        self.send(button, 0)
    }
}
