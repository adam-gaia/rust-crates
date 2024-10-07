use eyre::bail;
use eyre::Result;
use futures_util::pin_mut;
use futures_util::StreamExt;
use log::debug;
use std::path::PathBuf;
use which::which;
use xcommand::StdioType;
use xcommand::XChildHandle;
use xcommand::XCommand;
use xcommand::XStatus;

const DIR: &'static str = env!("CARGO_MANIFEST_DIR");

#[tokio::main]
pub async fn main() -> Result<()> {
    env_logger::init();

    // Build a command
    let bin = PathBuf::from(&format!("{}/examples/long_command.sh", DIR));
    let command = XCommand::builder(&bin)?.build();
    let Ok(mut child) = command.spawn() else {
        bail!("Unable to run '{}'", bin.display());
    };

    // Loop over stdout/err output from the child process
    let mut streamer = child.streamer();
    let mut stream = streamer.stream();
    while let Some(item) = stream.next().await {
        let (message_type, message) = item?;
        match message_type {
            StdioType::Stdout => {
                println!("[stdout]{}", message);
            }
            StdioType::Stderr => {
                println!("[stderr]{}", message);
            }
        }
    }

    // Grab the exit code of the process
    let XStatus::Exited(code) = child.status().await? else {
        bail!("Process was expected to have finished");
    };
    std::process::exit(code);
}
