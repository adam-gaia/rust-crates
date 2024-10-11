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
use tokio::process::Command;

#[derive(Debug)]
struct CmdAbstraction<'a> {
    command: &'a [String],
}

impl<'a> CommandStream<'_> for CmdAbstraction<'a> {
    fn command(&self) -> &[String] {
        &self.command
    }
    fn handle_stdout(&self, line: &str) -> Result<()> {
        info!("{}", line);
        Ok(())
    }
    fn handle_stderr(&self, line: &str) -> Result<()> {
        warn!("{}", line);
        Ok(())
    }
}

fn format_command(command: &str, args: Option<&[&str]>) -> String {
    format!(
        "{} {}",
        command,
        args.map_or_else(|| String::new(), |x| x.join(" "))
    )
    .trim_right()
    .to_string()
}

fn bytes_to_str(bytes: &[u8]) -> Result<String> {
    let s = std::str::from_utf8(bytes)?.trim().to_string();
    Ok(s)
}

pub async fn run(command: &str, args: Option<&[&str]>) -> Result<String> {
    debug!("Running command: '{}'", format_command(command, args));

    let mut cmd_builder = Command::new(command);
    if let Some(args) = args {
        cmd_builder.args(args);
    }
    cmd_builder.env("SSH_TO_AGE_PASSPHRASE", "");

    let err_message = format!("Failed to run command '{}'", command);
    let output = cmd_builder.output().expect(&err_message);
    let stdout = bytes_to_str(&output.stdout)?;
    debug!("stdout: {}", stdout);
    let stderr = bytes_to_str(&output.stderr)?;
    debug!("stderr: {}", stderr);
    if !output.status.success() {
        let code = output.status.code().unwrap();
        let err_message = format!("Command failed with exit code {}", code);
        bail!(err_message);
    }
    Ok(stdout)
}
