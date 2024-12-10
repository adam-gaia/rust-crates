use clap::Parser;
use color_eyre::eyre::ContextCompat;
use color_eyre::{eyre::bail, Result};
use ignore_more::file_entry::FileType;
use ignore_more::metadata::InfoSettings;
use ignore_more::metadata::Metadata;
use ignore_more::FileEntry;
use ignore_more::FileTree;
use ignore_more::TreeSettings;
use ignore_more::TreeWalker;
use log::debug;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;

#[derive(Debug, Parser)]
struct Cli {
    /// Respect `.gitignore` (default = false)
    #[clap(short, long)]
    gitignore: bool,

    /// Respect `.ignore` (default = false)
    #[clap(short = 'i', long)]
    dotignore: bool,

    /// Respect `.dockerignore` (default = false)
    #[clap(short, long)]
    dockerignore: bool,

    /// Ignore hidden files (default = false)
    #[clap(short = '.', long)] // I love that the flag is `-.` fuck conventions lol
    hidden: bool,

    /// Path(s) to custom ignore file(s)
    #[clap(short, long)]
    file: Vec<PathBuf>,

    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
}

// TODO: Document that this is unix-only

fn make_relative_to(base: &Path, target: &Path) -> Result<PathBuf> {
    target
        .strip_prefix(base)
        .ok()
        .map(|rel| rel.to_path_buf())
        .context("Base path is not a prefix of target path")
}

fn main() -> Result<()> {
    color_eyre::install()?;
    env_logger::init();
    let args = Cli::parse();

    let Ok(boxxy) = which::which("boxxy") else {
        bail!("Unable to find `boxxy` on the $PATH");
    };

    let cwd = std::env::current_dir()?;

    // TODO: TreeSettings needs an invert option
    let mut settings_builder = TreeSettings::builder()
        .all_off()
        .hidden(args.hidden)
        .ignore_file(args.dotignore)
        .gitignore(args.gitignore)
        .docker_ignore(args.dotignore);

    for file in args.file {
        settings_builder = settings_builder.ignore_from_file(file.to_str().unwrap());
    }

    let settings = settings_builder.build();
    let tree = FileTree::new(&cwd, &settings)?;
    let walker = TreeWalker::new(&tree);

    // Build a boxxy rule for each file found that didn't match an ignore pattern
    let tmp_dir = tempdir()?;
    let tmp_dir_path = tmp_dir.path();
    let mut rules = Vec::new();

    rules.push(String::from("--rule"));
    rules.push(format!(
        "{}:{}:directory",
        tmp_dir_path.display(),
        cwd.display()
    ));

    for entry in walker {
        let entry_path = entry.path();

        /*
        // Make the entry relative to cwd, then append that to the tmp dir
        let relative = make_relative_to(&cwd, entry_path)?;
        let tmp_file = tmp_dir_path.join(relative);

        let tmp_file_display = tmp_file.display().to_string();
        let tmp_file_display = if let Some(foo) = tmp_file_display.strip_suffix('/') {
            foo.to_string()
        } else {
            tmp_file_display
        };
        */

        /*
        let mut rule = format!("{}:{}:", entry_path.display(), entry_path.display());
        if *entry.file_type() == FileType::Directory {
            rule.push_str("directory");
        } else {
            rule.push_str("file");
        }

        rules.push(String::from("--rule"));
        rules.push(rule);
        */
    }
    debug!("Rules: {:#?}", rules);

    let mut cmd = Command::new(boxxy);
    //cmd.current_dir(tmp_dir_path);
    cmd.args(&rules).args(&args.args);
    debug!("cmd: {:#?}", cmd);

    // If the exec call returns something bad happened
    let err = cmd.exec();
    Result::Err(err.into())
}
