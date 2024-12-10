use color_eyre::Result;
use ignore_more::{glob::Globs, TreeSettings};
use std::path::Path;

#[derive(Debug)]
pub struct FilterSettings {
    ignore_hidden: bool,
    tree_settings: TreeSettings,
}

impl FilterSettings {
    pub fn new(source_dir: &Path, ignore_hidden: bool, excludes: Vec<String>) -> Result<Self> {
        let builder = TreeSettings::builder()
            .all_off()
            .leaves_only(true)
            .max_depth(1)
            .add_ignore_rules(excludes, Some(source_dir.to_path_buf()));
        let tree_settings = builder.build();
        Ok(Self {
            ignore_hidden,
            tree_settings,
        })
    }

    pub fn tree_settings(&self) -> &TreeSettings {
        &self.tree_settings
    }
}
