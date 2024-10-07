use crate::builder::XCommandBuilder;
use crate::child_handle::XChildHandle;
use crate::env_var::EnvVar;
use async_stream::stream;
use eyre::bail;
use eyre::Result;
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
use tokio_fd::AsyncFd;
use tokio_stream::wrappers::LinesStream;
use tokio_stream::{Stream, StreamExt, StreamMap};
use which::which;

#[derive(Debug)]
pub struct XCommand {
    command: CString,
    args: Vec<CString>,
    env: Vec<EnvVar>,
}

impl XCommand {
    // TODO: this should be like std::process::Command and new() should only take in command name. XCommandBuilder should have public access and set struct fields
    pub fn new(command: CString, args: Vec<CString>, env: Vec<EnvVar>) -> Self {
        Self { command, args, env }
    }
    pub fn builder<P: AsRef<Path>>(command: P) -> Result<XCommandBuilder> {
        XCommandBuilder::new(command)
    }

    /// Replace the current process with the executed command
    fn exec(&self) -> Result<()> {
        // Prepend the comnand name to the array of args
        let mut args = self.args.clone();
        args.insert(0, self.command.clone());

        // Format each variable as 'key=value'
        let env: Vec<CString> = self
            .env
            .iter()
            .map(|var| {
                let k = var.key.clone();
                let v = var.value.clone();
                let key_bytes = k.as_bytes();
                let eq_bytes = "=".as_bytes();
                let value_bytes = v.as_bytes();
                let mut formatted =
                    Vec::with_capacity(key_bytes.len() + value_bytes.len() + eq_bytes.len());
                formatted.extend_from_slice(key_bytes);
                formatted.extend_from_slice(eq_bytes);
                formatted.extend_from_slice(value_bytes);
                CString::new(formatted).unwrap()
            })
            .collect();

        // Cannot call println or unwrap in child - see
        // https://docs.rs/nix/0.25.0/nix/unistd/fn.fork.html#safety
        //nix::unistd::write(libc::STDOUT_FILENO, "I'm a new child process - stdout\n".as_bytes()).ok();
        //nix::unistd::write(libc::STDERR_FILENO, "I'm a new child process - stderr\n".as_bytes()).ok();

        match execve(&self.command, &args, &env) {
            Ok(_) => {}
            Err(e) => {
                bail!(
                    "Unable to execve command '{:?}' with args {:?} and environment {:?}. Reason: {}",
                    self.command,
                    args,
                    env,
                    e
                );
            }
        }
        Ok(())
    }

    pub fn spawn(&self) -> Result<XChildHandle> {
        debug!(
            "Running '{:?}' with args {:?} and env {:?}",
            self.command, self.args, self.env
        );
        // Open two ptys, one for stdout and one for stderr
        // This seems ludicrous however I cannot find a way to seprately send both streams and
        // fake a pty.
        // This SO question summs it up
        // https://stackoverflow.com/questions/34186035/can-you-fool-isatty-and-log-stdout-and-stderr-separately

        let res = openpty(None, None)?;
        let master = res.master;
        let slave = res.slave;
        let stdout_master = master.as_raw_fd();
        let stdout_slave = slave.as_raw_fd();
        // Stop drop from closing descriptors
        mem::forget(master);
        mem::forget(slave);

        let res = openpty(None, None)?;
        let master = res.master;
        let slave = res.slave;
        let stderr_master = master.as_raw_fd();
        let stderr_slave = slave.as_raw_fd();
        // Stop drop from closing descriptors
        mem::forget(master);
        mem::forget(slave);

        let Ok(res) = (unsafe { fork() }) else {
            bail!("fork() failed");
        };

        match res {
            ForkResult::Parent { child } => {
                // We are the parent
                close(stdout_slave).unwrap();
                close(stderr_slave).unwrap();
                // Return a handle to the child
                Ok(XChildHandle::new(child, stdout_master, stderr_master).unwrap())
            }
            ForkResult::Child => {
                // We are the child
                close(stdout_master).unwrap();
                close(stderr_master).unwrap();

                /*
                // TODO: what is this?
                setsid()?;
                let _ = unsafe { libc::ioctl(stdout_slave, libc::TIOCSCTTY, libc::STDOUT_FILENO) };
                let _ = unsafe { libc::ioctl(stderr_slave, libc::TIOCSCTTY, libc::STDERR_FILENO) };
                */

                // Redirect the pty stdout/err to this process's stdout/err
                dup2(stdout_slave.as_raw_fd(), libc::STDOUT_FILENO).unwrap();
                dup2(stderr_slave.as_raw_fd(), libc::STDERR_FILENO).unwrap();

                // TODO: pass through stdin

                //Exec the command
                let Err(e) = self.exec() else {
                    unreachable!();
                };

                error!("failed to exec: {}", e);
                // TODO: set exit code based on error
                std::process::exit(1);
            }
        }
    }
}
