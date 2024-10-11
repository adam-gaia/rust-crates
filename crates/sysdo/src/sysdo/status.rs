use color_eyre::eyre::bail;
use color_eyre::Result;
use directories::BaseDirs;
use jiff::{Unit, Zoned};
use log::debug;
use log::error;
use log::info;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

pub fn status() -> Result<()> {
    todo!();
}
