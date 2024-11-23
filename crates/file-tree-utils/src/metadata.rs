use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub struct Metadata {
    pub perms: String,
    pub owner: String,
    pub group: String,
    pub size: String,
    pub timestamp: String,
    pub git: GitStatus,
    pub icon: String,
}

pub fn metadata(path: &Path) -> Metadata {
    todo!()
}
