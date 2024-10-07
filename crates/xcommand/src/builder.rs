use crate::child_handle::XChildHandle;
use crate::command::XCommand;
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
use std::collections::HashMap;
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

fn path_to_cstring(path: &Path) -> CString {
    let bytes = path.as_os_str().as_bytes();
    CString::new(bytes).unwrap()
}

pub struct XCommandBuilder {
    command: CString,
    args: Vec<CString>,
    env: Vec<EnvVar>,
}

impl XCommandBuilder {
    /// Create a XCommandBuilder with default values
    pub fn new<P: AsRef<Path>>(command: P) -> Result<Self> {
        Self::inherit_environment(command)
    }

    /// Inherit the parent process' env vars
    pub fn inherit_environment<P: AsRef<Path>>(command: P) -> Result<Self> {
        let path = command.as_ref();
        let mut env = Vec::new();
        for (key, value) in env::vars() {
            env.push(EnvVar::from_str_pair(&key, &value)?);
        }
        Ok(XCommandBuilder {
            command: path_to_cstring(path),
            args: Vec::new(),
            env,
        })
    }

    /// Do not inherit the parent process' env vars
    pub fn clean_environment<P: AsRef<Path>>(command: P) -> Self {
        let path = command.as_ref();
        XCommandBuilder {
            command: path_to_cstring(path),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    /// Set an argument for the process
    pub fn arg(mut self, arg: &str) -> Result<Self> {
        let Ok(arg) = CString::new(arg) else {
            bail!("Unable to create CString from '{}'", arg);
        };
        self.args.push(arg);
        Ok(self)
    }

    /// Set the args of the process
    /// (Replaces any currently assigned args)
    pub fn args(mut self, args: &[&str]) -> Result<Self> {
        let mut cstr_args = Vec::with_capacity(args.len());
        for arg in args {
            let Ok(arg) = CString::new(*arg) else {
                bail!("Unable to create CString from '{}'", arg);
            };
            cstr_args.push(arg);
        }

        self.args = cstr_args;
        Ok(self)
    }

    /// Set env variables from a hashmap of key value pairs.
    /// Note that any prior set env vars are cleared
    pub fn env(mut self, vars: &HashMap<&str, &str>) -> Result<Self> {
        for (k, v) in vars {
            self.env.push(EnvVar::from_str_pair(k, v)?)
        }
        Ok(self)
    }

    /// Add an environment value
    pub fn var(mut self, key: &str, value: &str) -> Result<Self> {
        self.env.push(EnvVar::from_str_pair(key, value)?);
        Ok(self)
    }

    /// Build a XCommand
    pub fn build(self) -> XCommand {
        XCommand::new(self.command, self.args, self.env)
    }
}
