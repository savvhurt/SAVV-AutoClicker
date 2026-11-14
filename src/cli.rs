use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::state::{Button, ClickMode, Point, Settings};

#[derive(Parser, Debug)]
#[command(
    name = "v-autoclicker",
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

    /// Stop after N clicks in the session (0 means unlimited)
    #[arg(long, default_value_t = 0, value_name = "N")]
    pub limit: u64,

    /// Wait MS milliseconds before the first click (0 - 10000)
    #[arg(long, default_value_t = 0.0, value_name = "MS")]
    pub delay_ms: f64,

    /// Clicks per pulse: 1, 2 (double) or 3 (triple)
    #[arg(long, default_value_t = 1, value_name = "N", value_parser = clap::value_parser!(u8).range(1..=3))]
    pub multi_click: u8,

    /// Click at this X coordinate instead of the current cursor position
    #[arg(long, value_name = "X", requires = "y")]
    pub x: Option<i32>,

    /// Click at this Y coordinate instead of the current cursor position
    #[arg(long, value_name = "Y", requires = "x")]
    pub y: Option<i32>,
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
    /// Windows global toggle hotkey, e.g. ctrl+alt+c, f8 or shift+space
    #[arg(long, default_value = "ctrl+alt+c", value_name = "COMBO")]
    pub hotkey: String,

    /// Windows global hold-to-click hotkey (empty disables it)
    #[arg(long, value_name = "COMBO")]
    pub hold_hotkey: Option<String>,

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
        let target = match (self.x, self.y) {
            (Some(x), Some(y)) => Some(Point::new(x, y)),
            (None, None) => None,
            _ => anyhow::bail!("--x and --y must be supplied together"),
        };

        Ok(
            Settings::new(self.cps, self.button.into(), self.hold_ms, self.jitter_pct)?
                .with_limit(self.limit)?
                .with_delay_ms(self.delay_ms)?
                .with_multi_click(self.multi_click)?
                .with_target(target)?
                .with_mode(if target.is_some() {
                    ClickMode::Point
                } else {
                    ClickMode::Cursor
                }),
        )
    }

    pub fn to_wire_command(&self) -> Result<String> {
        let settings = self.to_settings()?;
        Ok(settings.to_wire_command("ON"))
    }
}
