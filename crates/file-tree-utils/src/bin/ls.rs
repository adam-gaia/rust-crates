use crate::metadata::{metadata, Metadata};
use clap::Parser;
use color_eyre::eyre::bail;
use color_eyre::Result;
use file_tree_utils::args::Args;
use ignore_more::FileEntry;
use ignore_more::FileTree;
use ignore_more::Settings;
use ignore_more::TreeWalker;
use log::debug;
use std::collections::hash_map::VacantEntry;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

fn display(entry: &FileEntry, args: &Args) -> Result<()> {
    let name = entry.name().to_str().unwrap();
    let path = entry.path();
    if args.long {
        let Metadata {
            perms,
            owner,
            group,
            size,
            timestamp,
            git,
            icon,
        } = metadata(path);

        println!(
            "{} {} {} {} {} {} {}{}",
            perms, owner, group, size, timestamp, git, icon, name
        );
    } else {
        println!("{}", name);
    }

    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    let files = match &args.file {
        Some(files) => files.clone(),
        None => {
            vec![env::current_dir()?]
        }
    };

    let tree_settings = Settings::builder().max_depth(1).build();

    let num_files = files.len();
    let multiple_files = num_files > 1;
    let mut i = 1;
    for path in files {
        let path = path.canonicalize()?;
        let name = path.file_name().unwrap().to_str().unwrap();
        if multiple_files {
            println!("{}:", name);
            // TODO: if file is a file and not a dir, no ':' and no need to run ls() on it
        }

        let tree = FileTree::new(&path, &tree_settings)?;
        let mut walker = TreeWalker::new(&tree);
        while let Some(entry) = walker.next() {
            display(entry, &args)?;
        }

        if i < num_files {
            println!();
        }
        i += 1;
    }

    Ok(())
}
