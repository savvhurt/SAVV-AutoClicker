use std::env;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use fs2::FileExt;

pub struct TimerResolution;

impl TimerResolution {
    pub fn new() -> Self {
        #[cfg(windows)]
        unsafe {
            windows::Win32::Media::timeBeginPeriod(1);
        }
        Self
    }
}

impl Default for TimerResolution {
    fn default() -> Self {
        Self::new()
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
            Err(_) => Ok(None),
        }
    }
}

pub fn daemon_executable() -> Result<PathBuf> {
    let current = env::current_exe().context("cannot resolve the current executable path")?;
    resolve_daemon_executable(&current, env::var_os("PATH").as_deref())
}

fn resolve_daemon_executable(current: &Path, search_path: Option<&OsStr>) -> Result<PathBuf> {
    let gui_name = format!("autoclicker-gui{}", env::consts::EXE_SUFFIX);

    let is_gui = current
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(&gui_name));

    if !is_gui {
        return Ok(current.to_path_buf());
    }

    let daemon_name = format!("autoclicker{}", env::consts::EXE_SUFFIX);
    let sibling = current.with_file_name(&daemon_name);
    if sibling.is_file() {
        return Ok(sibling);
    }

    if let Some(paths) = search_path {
        for directory in env::split_paths(paths) {
            let candidate = directory.join(&daemon_name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    bail!(
        "cannot find `{daemon_name}` next to the GUI binary or in PATH; build the CLI or add it to PATH"
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                env::temp_dir().join(format!("autoclicker-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn touch(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, b"").unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn binary_name(stem: &str) -> String {
        format!("{stem}{}", env::consts::EXE_SUFFIX)
    }

    #[test]
    fn cli_binary_is_returned_as_is() {
        let dir = TempDir::new("cli");
        let cli = dir.touch(&binary_name("autoclicker"));

        assert_eq!(resolve_daemon_executable(&cli, None).unwrap(), cli);
    }

    #[test]
    fn gui_prefers_the_sibling_cli() {
        let dir = TempDir::new("sibling");
        let gui = dir.touch(&binary_name("autoclicker-gui"));
        let cli = dir.touch(&binary_name("autoclicker"));

        assert_eq!(resolve_daemon_executable(&gui, None).unwrap(), cli);
    }

    #[test]
    fn gui_falls_back_to_path() {
        let gui_dir = TempDir::new("path-gui");
        let cli_dir = TempDir::new("path-cli");
        let gui = gui_dir.touch(&binary_name("autoclicker-gui"));
        let cli = cli_dir.touch(&binary_name("autoclicker"));

        assert_eq!(
            resolve_daemon_executable(&gui, Some(cli_dir.path().as_os_str())).unwrap(),
            cli
        );
    }

    #[test]
    fn gui_without_cli_reports_a_clear_error() {
        let dir = TempDir::new("missing");
        let gui = dir.touch(&binary_name("autoclicker-gui"));

        let error = resolve_daemon_executable(&gui, None).unwrap_err();

        assert!(error.to_string().contains("cannot find"));
    }
}
