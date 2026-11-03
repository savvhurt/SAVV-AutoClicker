use anyhow::Result;

use crate::state::Button;

pub trait Backend: Send {
    fn press(&mut self, button: Button) -> Result<()>;
    fn release(&mut self, button: Button) -> Result<()>;
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
