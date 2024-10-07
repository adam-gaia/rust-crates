use async_stream::stream;
use eyre::bail;
use eyre::Result;
use futures::pin_mut;
use log::debug;
use log::error;
use log::info;
use nix::pty::openpty;
use nix::pty::OpenptyResult;
use nix::sys::signal::Signal;
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::dup2;
use nix::unistd::execve;
use nix::unistd::pipe;
use nix::unistd::setsid;
use nix::unistd::ForkResult;
use nix::unistd::{close, fork, Pid};
use std::convert::From;
use std::env;
use std::ffi::CString;
use std::fs::File;
use std::mem;
use std::os::fd::AsRawFd;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::FromRawFd;
use std::os::unix::prelude::RawFd;
use std::path::Path;
use std::pin::Pin;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::sync::oneshot;
use tokio_fd::AsyncFd;
use tokio_stream::wrappers::LinesStream;
use tokio_stream::{Stream, StreamExt, StreamMap};
use which::which;

#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone)]
pub enum StdioType {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone)]
pub enum XStatus {
    Exited(i32),
    Signaled(Signal),
    Running,
}

impl From<WaitStatus> for XStatus {
    fn from(input: WaitStatus) -> Self {
        match input {
            WaitStatus::Exited(_, code) => Self::Exited(code),
            WaitStatus::Signaled(_, signal, _) | WaitStatus::Stopped(_, signal) => {
                Self::Signaled(signal)
            }
            _ => Self::Running,
        }
    }
}

mod builder;
pub use builder::XCommandBuilder;

mod command;
pub use command::XCommand;

mod child_handle;
pub use child_handle::XChildHandle;

mod env_var;
pub use env_var::EnvVar;

/*
pub async fn run<P: AsRef<Path>>(command: P, args: &[String]) -> Result<i32> {
    let command: &Path = command.as_ref();
    let mut cmd = XCommand::new(command);

    // Create a oneshot to get the status code of the child process back to our main task
    let (tx, rx) = oneshot::channel();

    cmd.stdout(Stdio::piped());
    if args.len() > 0 {
        cmd.args(args);
    }
    debug!("{:?}", cmd);

    let Ok(mut child) = cmd.spawn() else {
        bail!("Failed to spawn command '{}'", command.display());
    };

    let Some(stdout) = child.stdout.take() else {
        bail!(
            "Unable to get a handle to child process' ({}) stdout",
            command.display()
        );
    };

    let mut reader = BufReader::new(stdout).lines();
    tokio::spawn(async move {
        let status = child
            .wait()
            .await
            .expect("Child process encountered an error");
        // Send the status to the main task
        tx.send(status)
            .expect("Error sending the status to the main task");
    });

    while let Some(line) = reader.next_line().await? {
        println!("{}", line);
    }

    let Ok(status) = rx.await else {
        bail!("Unable to read status code from child process");
    };
    let Some(code) = status.code() else {
        bail!("Unablae to get status code from child process");
    };
    Ok(code)
}
*/
