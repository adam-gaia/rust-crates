use clap::Parser;
use color_eyre::eyre::bail;
use color_eyre::Result;
use file_tree_utils::args::{Args, BlockTypes, Blocks, Mode};
use file_tree_utils::args::{DiscoverySettings, DisplaySettings, SortingSettings};
use ignore_more::metadata::InfoSettings;
use ignore_more::metadata::Metadata;
use ignore_more::FileEntry;
use ignore_more::FileTree;
use ignore_more::Settings;
use ignore_more::TreeWalker;
use log::debug;
use std::borrow::Borrow;
use std::collections::hash_map::VacantEntry;
use std::env;
use std::fs;
use std::ops::Deref;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

fn display(entry: &FileEntry, settings: &DisplaySettings, blocks: Blocks) -> Result<()> {
    let path = entry.path();
    let m = entry.metadata();

    let mut buf = String::new();
    let num_blocks = blocks.len();
    for (i, block) in blocks.enumerate() {
        let s = match block {
            BlockTypes::Permissions => m.permissions().mode().to_string(),
            BlockTypes::Owner => {
                // TODO
                String::from("owner")
            }
            BlockTypes::Group => {
                // TODO
                String::from("group")
            }
            BlockTypes::Size => {
                // TODO
                String::from("size")
            }
            BlockTypes::Context => {
                // TODO
                String::from("context")
            }
            BlockTypes::Date => {
                // TODO
                String::from("date")
            }
            BlockTypes::Git => {
                // TODO
                String::from("git")
            }
            BlockTypes::Name => {
                let mut name = String::new();
                match settings.icon {
                    Mode::Always => {
                        // TODO
                    }
                    Mode::Never => {
                        // TODO
                    }
                    Mode::Auto => {
                        // TODO
                    }
                }
                let actual_name = entry.name().to_str().unwrap();
                name.push_str(actual_name);
                name
            }
        };
        buf.push_str(&s);
        if i < (num_blocks - 1) {
            buf.push(' ');
        }
    }

    println!("{}", buf);

    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    let display_settings = args.display_settings.clone();
    let display_blocks = args.display_settings.block_settings.blocks();

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
            display(entry, &display_settings, display_blocks.clone())?;
        }

        if i < num_files {
            println!();
        }
        i += 1;
    }

    Ok(())
}
