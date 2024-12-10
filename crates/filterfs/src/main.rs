use clap::Parser;
use color_eyre::Result;
use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request,
};
use ignore_more::glob::Globs;
use libc::{ENOENT, ENOSYS};
use log::{debug, info};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};
use time;
mod error;
mod settings;
mod util;
use settings::FilterSettings;
mod filterfs;
use filterfs::FilterFS;

const NAME: &'static str = "filterfs";

#[derive(Debug, Parser)]
struct Cli {
    /// Act as a client, and mount FUSE at given path
    mountpoint: PathBuf,

    /// Directory to filters files from
    source: PathBuf,

    /// Paths/glob patterns to exclude
    #[arg(short, long)]
    exclude: Vec<String>,

    /// Automatically unmount on process exit
    #[clap(long)]
    auto_unmount: bool,

    /// Allow root user to access filesystem
    #[clap(long)]
    allow_root: bool,

    /// Mount readonly
    #[clap(long)]
    read_only: bool,

    /// Exclude all hidden files
    #[clap(long)]
    hidden: bool,
}
// TODO: add arg to get exclude patterns from ignorefile
// TODO: consider adding a '--exclude-regex' option for regexes instead of globs

fn main() -> Result<()> {
    env_logger::init();
    let args = Cli::parse();

    let mut options = vec![MountOption::FSName(String::from(NAME))];
    if args.auto_unmount {
        options.push(MountOption::AutoUnmount);
    }

    if args.allow_root {
        options.push(MountOption::AllowRoot);
    }

    if args.read_only {
        options.push(MountOption::RO);
    } else {
        options.push(MountOption::RW);
    }

    let mountpoint = args.mountpoint.canonicalize()?;
    let source_dir = args.source.canonicalize()?;

    let settings = FilterSettings::new(&source_dir, args.hidden, args.exclude)?;
    let fs = FilterFS::new(source_dir, settings)?;

    info!("Mounting {} to {}", NAME, mountpoint.display());
    fuser::mount2(fs, mountpoint, &options).unwrap();

    Ok(())
}
