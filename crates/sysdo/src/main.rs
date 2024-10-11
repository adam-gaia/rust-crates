use clap::{Args, Parser, Subcommand, ValueEnum};
use color_eyre::eyre::bail;
use color_eyre::Result;
use directories::BaseDirs;
use jiff::{Unit, Zoned};
use log::debug;
use log::error;
use log::info;
use log::warn;
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
mod settings;
use settings::Settings;
mod sysdo;
use sysdo::Sysdo;
use which::which;

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run initial setup on a new machine
    Setup,

    /// Run nixos-rebuild build
    Build,

    /// Run nixos-rebuild switch
    Switch,

    /// Get a quick looko at the status of the system
    Status,
}

#[derive(Debug, Parser)]
#[clap(version)]
struct Cli {
    #[clap(short, long)]
    dry_run: bool,

    #[clap(long)]
    hostname: Option<String>,

    #[clap(long)]
    username: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

fn u8_to_string(bytes: Vec<u8>) -> Result<String> {
    let string = String::from_utf8(bytes)?;
    Ok(string)
}

fn capture_stdout(command: &Path, args: &[&str]) -> Result<String> {
    debug!("Running {} with args {:?}", command.display(), args);
    let output = Command::new(&command).args(args).output()?;
    let stderr = u8_to_string(output.stderr)?;
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        warn!("{}", stderr);
    }
    let stdout = u8_to_string(output.stdout)?;
    Ok(stdout.trim().to_string())
}

fn run(command: &Path, args: &[&str]) -> Result<i32> {
    debug!("Running {} with args {:?}", command.display(), args);
    let output = Command::new(&command).args(args).output()?;
    Ok(output.status.code().unwrap())
}

fn username(arg: Option<String>) -> Result<String> {
    let username = match arg {
        Some(username) => username,
        None => {
            let cmd = which("id")?;
            capture_stdout(&cmd, &["--user", "--name"])?
        }
    };
    Ok(username)
}

fn hostname(arg: Option<String>) -> Result<String> {
    let hostname = match arg {
        Some(hostname) => hostname,
        None => {
            let cmd = which("hostname")?;
            capture_stdout(&cmd, &[])?
        }
    };
    Ok(hostname)
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Cli::parse();

    let username = username(args.username.clone())?;
    let hostname = hostname(args.hostname.clone())?;
    let settings = Settings::new(args.dry_run, &username, &hostname)?;

    let app = Sysdo::new(settings)?;
    let code = match args.command {
        Commands::Setup => app.setup()?,
        Commands::Build => app.build().await?,
        Commands::Switch => app.switch().await?,
        Commands::Status => app.status()?,
    };
    std::process::exit(code);
}
