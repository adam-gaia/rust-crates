use clap::Parser;
use color_eyre::eyre::bail;
use color_eyre::Result;
use derive_more::{Display, FromStr};
use std::path::{Path, PathBuf};

#[derive(Debug, FromStr, Default, Copy, Clone, PartialEq, Eq, Display)]
pub enum Mode {
    Always,
    #[default]
    Auto,
    Never,
}

#[derive(Debug, FromStr, Default, Copy, Clone, PartialEq, Eq, Display)]
pub enum IconTheme {
    #[default]
    Fancy,
    Unicode,
}

#[derive(Debug, FromStr, Default, PartialEq, Eq, Copy, Clone, Display)]
pub enum PermissionMode {
    #[default]
    RWX,
    Octal,
}

#[derive(Debug, FromStr, Default, Copy, Clone, PartialEq, Eq, Display)]
pub enum SizeMode {
    #[default]
    Default,
    Short,
    Bytes,
}

#[derive(Debug, FromStr, Default, Clone, Copy, PartialEq, Eq, Display)]
pub enum DateOption {
    #[default]
    Date,
    Locale,
    Relative,
}

#[derive(Debug, Default, FromStr, Copy, Clone, PartialEq, Eq, Display)]
pub enum SortOption {
    #[default]
    Name,
    Size,
    Time,
    Version,
    Extension,
    Git,
    None,
}

#[derive(Debug, FromStr, Clone, Copy, PartialEq, Eq, Display)]
pub enum BlockTypes {
    Permissions,
    Owner,
    Group,
    Context,
    Size,
    Date,
    Name,
    Git,
}

#[derive(Debug, Clone)]
pub struct Blocks {
    blocks: Vec<BlockTypes>,
    index: usize,
}
impl Blocks {
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    fn default() -> Self {
        Self {
            blocks: vec![BlockTypes::Name],
            index: 0,
        }
    }
    fn new() -> Self {
        Self {
            blocks: Vec::new(),
            index: 0,
        }
    }

    fn enable_permissions(&mut self) {
        self.blocks.push(BlockTypes::Permissions);
    }
    fn enable_owner(&mut self) {
        self.blocks.push(BlockTypes::Owner);
    }
    fn enable_group(&mut self) {
        self.blocks.push(BlockTypes::Group);
    }
    fn enable_size(&mut self) {
        self.blocks.push(BlockTypes::Size);
    }
    fn enable_date(&mut self) {
        self.blocks.push(BlockTypes::Date);
    }
    fn enable_name(&mut self) {
        self.blocks.push(BlockTypes::Name);
    }
    fn enable_git(&mut self) {
        self.blocks.push(BlockTypes::Git);
    }
    fn enable_context(&mut self) {
        self.blocks.push(BlockTypes::Context);
    }

    fn from_parts(parts: &[BlockTypes]) -> Self {
        Self {
            blocks: parts.to_vec(),
            index: 0,
        }
    }
}

impl Iterator for Blocks {
    type Item = BlockTypes;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(block) = self.blocks.get(self.index) {
            self.index += 1;
            return Some(block.clone());
        };
        None
    }
}

/// Control what blocks of info are displayed
#[derive(Debug, Parser, Clone)]
pub struct BlockSettings {
    /// Display extended file metadata as table
    #[clap(long, short)]
    pub long: bool,

    /// Show git status on file and directory. Only when used with the '--long/-l' option
    #[clap(short, long)]
    pub git: bool,

    /// Print any security context of each file
    #[clap(short = 'Z', long)]
    pub context: bool,

    /// Specify the blocks that will be displayed and in what order
    #[clap(long)]
    pub blocks: Option<Vec<BlockTypes>>,
}

impl BlockSettings {
    pub fn blocks(&self) -> Blocks {
        if let Some(parts) = &self.blocks {
            return Blocks::from_parts(parts);
        }

        if self.long {
            let mut blocks = Blocks::new();
            blocks.enable_permissions();
            blocks.enable_owner();
            blocks.enable_group();
            if self.context {
                blocks.enable_context()
            }
            blocks.enable_size();
            blocks.enable_date();
            if self.git {
                blocks.enable_git();
            }
            blocks.enable_name();
            return blocks;
        }
        Blocks::default()
    }
}

/// Options specific to displaying output
#[derive(Debug, Parser, Clone)]
pub struct DisplaySettings {
    #[clap(flatten)]
    pub block_settings: BlockSettings,

    /// Do not ignore entries starting with '.'
    #[clap(long, short)]
    pub all: bool,

    /// Do not list implied '.' and '..'
    #[clap(long, short = 'A')]
    pub almost_all: bool,

    /// Attach hyperlink to file names
    #[clap(long, default_value_t = Mode::default())]
    pub hyperlink: Mode,

    /// Print literal entry names without quoting
    #[clap(short = 'N', long)]
    pub literal: bool,

    /// When to use terminal colors
    #[clap(long, default_value_t = Mode::default())]
    pub color: Mode,

    /// When to print the icons
    #[clap(long, default_value_t = Mode::default())]
    pub icon: Mode,

    #[clap(long, default_value_t = IconTheme::default())]
    pub icon_theme: IconTheme,

    /// Print json output
    #[clap(long, short)]
    pub json: bool,

    /// Append indicator (one of '*', '/', '=', '>', '@', '|') at the end of file names
    #[clap(long, short = 'F')]
    pub classify: bool,

    /// Display one entry per line
    #[clap(short = '1', long)]
    pub oneline: bool,

    /// How to display permissions
    #[clap(long, default_value_t = PermissionMode::default())]
    pub permission: PermissionMode,

    /// How to display size
    #[clap(long, default_value_t = SizeMode::default())]
    pub size: SizeMode,

    /// Display the total size of directories
    #[clap(long)]
    pub total_size: bool,

    /// How to display the date
    #[clap(long, default_value_t = DateOption::default())]
    pub date: DateOption,

    /// Display the index number of each file
    #[clap(short, long)]
    pub inode: bool,
}

/// Options specific to discovering files
#[derive(Debug, Parser)]
pub struct DiscoverySettings {
    /// Recurse into diectories
    #[clap(short, long)]
    pub recursive: bool,

    // TODO: standard ls -h is for human readable but its convention to have for help
    //#[clap(short, long)]
    //pub human_readable: bool,
    /// Display directories themselves, and not their contents
    #[clap(short, long)]
    pub directory_only: bool,

    /// Do not display symlink target
    #[clap(long)]
    pub no_symlink: bool,

    /// Do not display files/directories with names matching the glog pattern(s)
    /// TODO: clap collect multiple of this into the vec
    #[clap(short = 'I', long)]
    pub ignore_glob: Vec<String>,

    /// When showing file information for a symbolic link, show information for the tile the link references rather than for the link itself
    #[clap(short = 'L', long)]
    pub dereference: bool,
}

/// Options specific to sorting outputs
#[derive(Debug, Parser)]
pub struct SortingSettings {
    /// Sort by time modified
    #[clap(long, short)]
    pub timesort: bool,

    /// Sort by size
    #[clap(long, short)]
    pub sizesort: bool,

    /// Sort by file extension
    #[clap(long, short)]
    pub extensionsort: bool,

    /// Sort by git status
    #[clap(short = 'G', long)]
    pub gitsort: bool,

    /// Sort by TYPE instead of name
    #[clap(long, default_value_t = SortOption::default())]
    pub sort: SortOption,

    /// Reverse the order of the sort
    #[clap(long)]
    pub reverse: bool,

    /// Do not group directories at the top before files.
    #[clap(long)]
    pub no_group_dirs: bool,
}

/// Options specific to discovering config file
#[derive(Debug, Parser)]
pub struct ConfigFileSettings {
    /// Ignore config file
    #[clap(long)]
    pub ignore_config: bool,

    /// Provide custom config file
    #[clap(long)]
    pub config: Option<PathBuf>,
}

// TODO: use clap arg groups to make some args mutually exclusive

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[clap(flatten)]
    pub display_settings: DisplaySettings,

    #[clap(flatten)]
    pub discovery_settings: DiscoverySettings,

    #[clap(flatten)]
    pub sorting_settings: SortingSettings,

    #[clap(flatten)]
    pub config_settings: ConfigFileSettings,

    /// File or list of files to operate on
    pub file: Option<Vec<PathBuf>>,
}
