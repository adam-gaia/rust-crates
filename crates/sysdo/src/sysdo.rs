use crate::settings::Settings;
use color_eyre::eyre::bail;
use color_eyre::Result;
use directories::BaseDirs;
use jiff::{Unit, Zoned};
use log::debug;
use log::error;
use log::info;
use log::warn;
use nixgen::label;
use nixgen::RepoRootConfig;
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use xcommand::StdioType;
use xcommand::XCommand;
use xcommand::XStatus;
mod setup;
use setup::setup;
mod status;
use status::status;
use which::which;
use xcommand::StreamExt;

async fn run(bin: &Path, args: &[&str]) -> Result<i32> {
    let command = XCommand::builder(&bin)?.args(args)?.build();
    let Ok(mut child) = command.spawn() else {
        bail!("Unable to run {}", bin.display());
    };

    let mut streamer = child.streamer();
    let mut stream = streamer.stream();
    while let Some(item) = stream.next().await {
        let (message_type, message) = item?;
        match message_type {
            StdioType::Stdout => {
                println!("{}", message);
            }
            StdioType::Stderr => {
                println!("[STDERR]{}", message);
            }
        }
    }
    let XStatus::Exited(code) = child.status().await? else {
        bail!("Process was expected to have finished")
    };
    Ok(code)
}

#[derive(Debug)]
pub struct Sysdo {
    settings: Settings,
    rebuild_cmd: PathBuf,
}

impl Sysdo {
    pub fn new(settings: Settings) -> Result<Self> {
        let rebuild_cmd = which("nixos-rebuild")?;
        Ok(Self {
            settings,
            rebuild_cmd,
        })
    }

    pub fn setup(&self) -> Result<i32> {
        setup(&self.settings)?;
        Ok(0)
    }

    pub async fn build(&self) -> Result<i32> {
        let hostname = &self.settings.hostname;
        let code = run(
            &self.rebuild_cmd,
            &["build", "--flake", &format!(".#{}", hostname)],
        )
        .await?;
        Ok(code)
    }

    pub async fn switch(&self) -> Result<i32> {
        let hostname = &self.settings.hostname;
        let label = label(RepoRootConfig::Discover)?;
        let code = run(
            &self.rebuild_cmd,
            &[
                "switch",
                "--use-remote-sudo",
                "--profile-name",
                &label,
                "--flake",
                &format!(".#{}", hostname),
            ],
        )
        .await?;
        Ok(code)
    }

    pub fn status(&self) -> Result<i32> {
        status()?;
        Ok(0)
    }
}
