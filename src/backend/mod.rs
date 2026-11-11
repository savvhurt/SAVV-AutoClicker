use std::sync::OnceLock;

use anyhow::Result;

use crate::state::{Button, Point};

pub trait Backend: Send {
    fn press(&mut self, button: Button) -> Result<()>;
    fn release(&mut self, button: Button) -> Result<()>;

    /// Move the system cursor to a fixed desktop position. Backends that
    /// cannot do this keep the default implementation and the scheduler stops
    /// the session with a readable error.
    fn move_to(&mut self, _point: Point) -> Result<()> {
        anyhow::bail!("clicking at a fixed point is not supported on this platform")
    }

    fn supports_target(&self) -> bool {
        false
    }
}

pub fn target_supported() -> bool {
    // The display server and environment do not change while the process is
    // running, so probe them once instead of on every click or repaint.
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(platform_target_supported)
}

#[cfg(windows)]
fn platform_target_supported() -> bool {
    true
}

#[cfg(target_os = "linux")]
fn platform_target_supported() -> bool {
    linux::target_supported()
}

#[cfg(not(any(target_os = "linux", windows)))]
fn platform_target_supported() -> bool {
    false
}

pub fn cursor_position() -> Result<Point> {
    #[cfg(windows)]
    {
        win::cursor_position()
    }

    #[cfg(target_os = "linux")]
    {
        linux::cursor_position()
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    {
        anyhow::bail!("the cursor position is not available on this platform")
    }
}

pub fn create() -> Result<Box<dyn Backend>> {
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::LinuxBackend::new()?))
    }

    #[cfg(windows)]
    {
        Ok(Box::new(win::WindowsBackend))
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    {
        anyhow::bail!("this platform is not supported (Linux and Windows only)")
    }
}

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(windows)]
pub mod win;
