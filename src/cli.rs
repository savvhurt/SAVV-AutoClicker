use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::state::{Button, Settings};

#[derive(Parser, Debug)]
#[command(
    name = "autoclicker",
    version,
    about = "Fast virtual-mouse autoclicker for Linux and Windows",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Click in the foreground until Ctrl+C
    Run(RunArgs),
    /// Run the control daemon (usually started automatically by `toggle`/`on`)
    Daemon(DaemonArgs),
    /// Toggle clicking on or off, starting the daemon if necessary
    Toggle,
    /// Enable clicking with the supplied settings
    On(OnArgs),
    /// Disable clicking
    Off,
    /// Print daemon state and measured click rate
    Status,
    /// Stop the daemon and release the mouse button
    Quit,
}

#[derive(Args, Debug, Clone)]
pub struct ClickArgs {
    /// Target clicks per second (0.2 - 1000)
    #[arg(long, default_value_t = 50.0, value_name = "N")]
    pub cps: f64,

    /// Mouse button to click
    #[arg(long, value_enum, default_value_t = ButtonArg::Left)]
    pub button: ButtonArg,

    /// How long the button stays pressed, in milliseconds
    #[arg(long, default_value_t = 10.0, value_name = "MS")]
    pub hold_ms: f64,

    /// Randomize each interval by +/- PCT percent
    #[arg(long, default_value_t = 0.0, value_name = "PCT")]
    pub jitter_pct: f64,
}

#[derive(Args, Debug)]
pub struct RunArgs {
    #[command(flatten)]
    pub click: ClickArgs,

    /// Stop automatically after SECS seconds (useful for testing)
    #[arg(long, value_name = "SECS")]
    pub duration_secs: Option<f64>,
}

#[derive(Args, Debug)]
pub struct OnArgs {
    #[command(flatten)]
    pub click: ClickArgs,
}

#[derive(Args, Debug)]
pub struct DaemonArgs {
    /// Windows global hotkey, e.g. ctrl+alt+c, ctrl+shift+f8
    #[arg(long, default_value = "ctrl+alt+c", value_name = "COMBO")]
    pub hotkey: String,

    /// Start the daemon in the background and return
    #[arg(long)]
    pub detach: bool,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ButtonArg {
    Left,
    Right,
    Middle,
}

impl From<ButtonArg> for Button {
    fn from(value: ButtonArg) -> Self {
        match value {
            ButtonArg::Left => Self::Left,
            ButtonArg::Right => Self::Right,
            ButtonArg::Middle => Self::Middle,
        }
    }
}

impl ClickArgs {
    pub fn to_settings(&self) -> Result<Settings> {
        Settings::new(self.cps, self.button.into(), self.hold_ms, self.jitter_pct)
    }

    pub fn to_wire_command(&self) -> Result<String> {
        let settings = self.to_settings()?;
        Ok(settings.to_wire_command("ON"))
    }
}
