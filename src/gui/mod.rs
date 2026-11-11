mod app;
mod config;
mod theme;
#[cfg(windows)]
mod tray;
mod widgets;
mod worker;

pub use app::run;
