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
            return PathBuf::from(dir).join("savv-autoclicker");
        }
    }

    env::temp_dir().join(format!("savv-autoclicker-{}", unsafe { libc::getuid() }))
}

#[cfg(windows)]
fn instance_dir() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("SAVV-AutoClicker")
}

/// Runtime directory used by `autoclicker` releases before the rename. It is
/// only used to shut down a daemon that may still be running.
#[cfg(unix)]
fn legacy_instance_dir() -> PathBuf {
    if let Some(dir) = env::var_os("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("autoclicker");
        }
    }

    env::temp_dir().join(format!("autoclicker-{}", unsafe { libc::getuid() }))
}

#[cfg(windows)]
fn legacy_instance_dir() -> PathBuf {
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

#[cfg(unix)]
pub fn legacy_socket_path() -> PathBuf {
    legacy_instance_dir().join("daemon.sock")
}

#[cfg(windows)]
fn pipe_name_for(dir: &std::path::Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;

    for byte in dir.as_os_str().to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    format!("savv-autoclicker-{hash:016x}")
}

#[cfg(windows)]
pub fn pipe_name() -> String {
    pipe_name_for(&instance_dir())
}

#[cfg(windows)]
pub fn legacy_pipe_name() -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;

    for byte in legacy_instance_dir()
        .as_os_str()
        .to_string_lossy()
        .as_bytes()
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    format!("autoclicker-{hash:016x}")
}

/// Whether the current process runs with administrator privileges. UIPI keeps
/// a non-elevated process from sending input to elevated windows, so the GUI
/// warns the user instead of silently doing nothing.
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);

        result.is_ok() && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

/// Relaunch the GUI through the UAC prompt so it can interact with elevated
/// applications.
#[cfg(windows)]
pub fn restart_elevated() -> Result<()> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::HSTRING;

    let executable = env::current_exe().context("cannot resolve the current executable path")?;
    let file = HSTRING::from(executable.as_os_str());
    let operation = HSTRING::from("runas");
    let result = unsafe {
        ShellExecuteW(
            None,
            &operation,
            &file,
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    if result.0 as isize <= 32 {
        anyhow::bail!(
            "failed to restart with administrator privileges (ShellExecute returned {})",
            result.0 as isize
        );
    }

    Ok(())
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
