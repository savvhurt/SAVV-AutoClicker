use std::env;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use fs2::FileExt;

pub struct TimerResolution;

impl TimerResolution {
    fn new() -> Self {
        #[cfg(windows)]
        unsafe {
            windows::Win32::Media::timeBeginPeriod(1);
        }
        Self
    }
}

impl Drop for TimerResolution {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            windows::Win32::Media::timeEndPeriod(1);
        }
    }
}

pub fn timer_resolution() -> TimerResolution {
    TimerResolution::new()
}

#[cfg(unix)]
fn instance_dir() -> PathBuf {
    if let Some(dir) = env::var_os("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("autoclicker");
        }
    }

    env::temp_dir().join(format!("autoclicker-{}", unsafe { libc::getuid() }))
}

#[cfg(windows)]
fn instance_dir() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("autoclicker")
}

pub fn ensure_instance_dir() -> io::Result<PathBuf> {
    let dir = instance_dir();

    if !dir.exists() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&dir)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;

        let metadata = fs::symlink_metadata(&dir)?;
        if !metadata.file_type().is_dir() || metadata.uid() != unsafe { libc::getuid() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("unsafe runtime directory: {}", dir.display()),
            ));
        }
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }

    Ok(dir)
}

#[cfg(unix)]
pub fn socket_path() -> io::Result<PathBuf> {
    Ok(ensure_instance_dir()?.join("daemon.sock"))
}

#[cfg(windows)]
pub fn pipe_name() -> String {
    let dir = instance_dir();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;

    for byte in dir.as_os_str().to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    format!("autoclicker-{hash:016x}")
}

pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    pub fn acquire() -> Result<Option<Self>> {
        let dir = ensure_instance_dir().context("failed to prepare the runtime directory")?;
        let path = dir.join("daemon.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;

        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
                Ok(None)
            }
            Err(error) => Err(error).with_context(|| format!("failed to lock {}", path.display())),
        }
    }
}

pub fn daemon_executable() -> Result<PathBuf> {
    let current = env::current_exe().context("cannot resolve the current executable path")?;
    Ok(current)
}

pub fn spawn_daemon(extra_args: &[String]) -> Result<()> {
    let executable = daemon_executable()?;
    let mut command = Command::new(executable);
    command
        .args(extra_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }

    command
        .spawn()
        .context("failed to spawn the daemon process")?;

    Ok(())
}
