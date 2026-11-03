use anyhow::Result;
use clap::Parser;

use autoclicker::cli::{Cli, Command};
use autoclicker::{daemon, ipc};

fn main() {
    if let Err(error) = run() {
        eprintln!("autoclicker: error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Run(args) => daemon::run_foreground(args),

        Command::Daemon(args) => daemon::run_daemon(args),

        Command::Toggle => {
            let response = ipc::send_or_spawn("TOGGLE")?;
            ipc::ensure_ok(&response)?;
            println!(
                "autoclicker: {}",
                if response.contains("enabled=true") {
                    "on"
                } else {
                    "off"
                }
            );
            Ok(())
        }

        Command::On(args) => {
            let command = args.click.to_wire_command()?;
            let response = ipc::send_or_spawn(&command)?;
            ipc::ensure_ok(&response)?;
            println!("autoclicker: on");
            Ok(())
        }

        Command::Off => {
            match ipc::send("OFF") {
                Ok(response) => {
                    ipc::ensure_ok(&response)?;
                    println!("autoclicker: off");
                }
                Err(_) => println!("autoclicker: not running"),
            }
            Ok(())
        }

        Command::Status => {
            match ipc::send("STATUS") {
                Ok(response) => {
                    ipc::ensure_ok(&response)?;
                    let body = response.strip_prefix("OK\n").unwrap_or(&response);
                    print!("{body}");
                }
                Err(_) => println!("autoclicker: not running"),
            }
            Ok(())
        }

        Command::Quit => {
            match ipc::send("QUIT") {
                Ok(response) => {
                    ipc::ensure_ok(&response)?;
                    println!("autoclicker: stopped");
                }
                Err(_) => println!("autoclicker: not running"),
            }
            Ok(())
        }
    }
}
