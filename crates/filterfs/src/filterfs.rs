//! Modified from example https://github.com/cberner/fuser/blob/master/examples/simple.rs#L203

use crate::error::Error;
use crate::settings::FilterSettings;
use crate::util::{system_time_from_time, time_from_system_time, time_now};
use color_eyre::eyre::bail;
use color_eyre::Result;
use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request,
};
use ignore_more::file_entry::FileType as OtherFileType;
use ignore_more::glob::Globs;
use ignore_more::{FileTree, TreeSettings, TreeWalker};
use log::{debug, error, info};
use nix::unistd::{chown, Gid, Uid};
use std::cmp::min;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fs::{self, create_dir_all, metadata, File, FileTimes, Metadata, OpenOptions};
use std::fs::{FileType as StdFileType, Permissions};
use std::io::{Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::time::{Duration, UNIX_EPOCH};

const BLOCK_SIZE: u64 = 512;
const MAX_NAME_LENGTH: u32 = 255;
const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024 * 1024;

const TTL: Duration = Duration::from_secs(0);

type Inode = u64;

const FMODE_EXEC: i32 = 0x20;

const ROOT_INODE: Inode = 1;

#[derive(Debug)]
struct InodeAttributes {
    pub inode: Inode,
    pub open_file_handles: u64, // Ref count of open file handles to this inode
    pub size: u64,
    pub last_accessed: (i64, u32),
    pub last_modified: (i64, u32),
    pub last_metadata_changed: (i64, u32),
    pub kind: FileType,
    // Permissions and special mode bits
    pub mode: u16,
    pub hardlinks: u32,
    pub uid: u32,
    pub gid: u32,
    pub xattrs: BTreeMap<Vec<u8>, Vec<u8>>,
}

fn attrs_from_metadata(inode: Inode, metadata: Metadata) -> FileAttr {
    let ttype = metadata.file_type();
    let kind = if ttype.is_file() {
        FileType::RegularFile
    } else if ttype.is_dir() {
        FileType::Directory
    } else if ttype.is_symlink() {
        FileType::Symlink
    } else if ttype.is_socket() {
        FileType::Socket
    } else if ttype.is_block_device() {
        FileType::BlockDevice
    } else if ttype.is_char_device() {
        FileType::CharDevice
    } else if ttype.is_fifo() {
        FileType::NamedPipe
    } else {
        unreachable!();
    };

    FileAttr {
        ino: inode,
        size: metadata.size(),
        blocks: metadata.blocks(),
        atime: system_time_from_time(metadata.atime(), metadata.atime_nsec()),
        mtime: system_time_from_time(metadata.mtime(), metadata.mtime_nsec()),
        ctime: system_time_from_time(metadata.ctime(), metadata.ctime_nsec()),
        crtime: system_time_from_time(0, 0), // TODO: Would like to fix this but, I don't have a mac to try this on
        kind,
        perm: metadata.permissions().mode() as u16, // TODO: save to cast to u16??
        nlink: metadata.nlink() as u32,             // TODO: safe to cast?
        uid: metadata.uid(),
        gid: metadata.gid(),
        rdev: metadata.rdev() as u32,       // TODO: safe to cast?
        blksize: metadata.blksize() as u32, // TODO: safe to cast?
        flags: 0,                           // TODO: what flags?
    }
}

#[derive(Debug)]
struct Maps {
    /// Hold the next inode to assign (gets updated when used)
    next_inode: Inode,
    /// Map file path to inode
    path_to_inode: BTreeMap<PathBuf, Inode>,
    /// Map inode to file path
    inode_to_path: BTreeMap<Inode, PathBuf>,
    /// Map inode to parent's inode
    parents_map: BTreeMap<Inode, Option<Inode>>,
    /// Hold a list of inodes we can recycle
    removed_inodes: BTreeSet<Inode>,
}

impl Maps {
    fn new(root_inode: Inode, root_path: PathBuf) -> Self {
        let next_inode = root_inode + 1;
        Self {
            next_inode,
            path_to_inode: BTreeMap::from([(root_path.clone(), root_inode)]),
            inode_to_path: BTreeMap::from([(root_inode, root_path)]),
            parents_map: BTreeMap::from([(root_inode, None)]),
            removed_inodes: BTreeSet::new(),
        }
    }

    fn get_next_inode(&mut self) -> Inode {
        // Use up any recycled inodes first
        if let Some(next) = self.removed_inodes.pop_first() {
            return next;
        };
        // Otherwise, get the next one
        let next = self.next_inode;
        self.next_inode += 1;
        next
    }

    fn register(&mut self, path: PathBuf, parent: Inode) -> Inode {
        let inode = self.get_next_inode();

        self.path_to_inode.insert(path.clone(), inode);
        self.inode_to_path.insert(inode, path);
        self.parents_map.insert(inode, Some(parent));

        inode
    }

    fn remove(&mut self, inode: Inode) -> Result<(), Error> {
        let Some(path) = self.inode_to_path.remove(&inode) else {
            return Err(Error::NoSuchFileOrDirectory);
        };
        if self.path_to_inode.remove(&path).is_none() {
            return Err(Error::NoSuchFileOrDirectory);
        }
        if self.parents_map.remove(&inode).is_none() {
            return Err(Error::NoSuchFileOrDirectory);
        }
        self.removed_inodes.insert(inode);
        Ok(())
    }

    fn get_path(&self, inode: &Inode) -> Option<&PathBuf> {
        self.inode_to_path.get(inode)
    }

    fn get_inode(&self, path: &Path) -> Option<Inode> {
        self.path_to_inode.get(path).map(|i| *i)
    }

    fn get_parent(&self, inode: &Inode) -> Option<&Option<Inode>> {
        self.parents_map.get(inode)
    }
}

#[derive(Debug)]
pub struct FilterFS {
    source_dir: PathBuf,
    map: Maps,
    settings: FilterSettings,
}

impl FilterFS {
    pub fn new(source_dir: PathBuf, settings: FilterSettings) -> Result<Self> {
        debug!("source: {}", source_dir.display());

        if !source_dir.is_dir() {
            bail!("Source {} is not a valid directory", source_dir.display());
        }

        let root_path = source_dir.clone();
        let map = Maps::new(ROOT_INODE, root_path);

        Ok(Self {
            source_dir,
            map,
            settings,
        })
    }
}

fn convert_file_type(input: &OtherFileType) -> FileType {
    match input {
        OtherFileType::Directory { .. } => FileType::Directory,
        OtherFileType::Symlink => FileType::Symlink,
        OtherFileType::Pipe => FileType::NamedPipe,
        OtherFileType::Block => FileType::BlockDevice,
        OtherFileType::Socket => FileType::Socket,
        OtherFileType::Character => FileType::CharDevice,
        OtherFileType::File => FileType::RegularFile,
    }
}
fn as_file_type(mut mode: u32) -> Result<FileType> {
    mode &= libc::S_IFMT as u32;
    let file_type = match mode {
        libc::S_IFREG => FileType::RegularFile,
        libc::S_IFLNK => FileType::Symlink,
        libc::S_IFDIR => FileType::Directory,
        libc::S_IFBLK => FileType::BlockDevice,
        libc::S_IFCHR => FileType::CharDevice,
        libc::S_IFIFO => FileType::NamedPipe,
        libc::S_IFSOCK => FileType::Socket,
        _ => {
            bail!("Unknown file type from mode {}", mode);
        }
    };
    Ok(file_type)
}

/// Helper methods not part of public API or trait impls
impl FilterFS {
    fn readdir_helper(
        &mut self,
        _req: &Request,
        ino: Inode,
        _fh: u64,
        offset: i64,
        reply: &mut ReplyDirectory,
    ) -> Result<()> {
        let path = self.map.get_path(&ino).unwrap();

        let mut entries = vec![
            (ROOT_INODE, FileType::Directory, OsString::from(".")),
            (ROOT_INODE, FileType::Directory, OsString::from("..")),
        ];

        let tree = FileTree::new(&path, self.settings.tree_settings())?;
        let walker = TreeWalker::new(&tree);

        for entry in walker {
            let ttype = convert_file_type(entry.file_type());
            let name = entry.name();
            let path = entry.path();

            debug!("Entry: {}", path.display());

            let inode = match self.map.get_inode(&path) {
                Some(inode) => inode,
                None => {
                    let parent = ino;
                    let inode = self.map.register(path.to_path_buf(), parent);
                    inode
                }
            };

            entries.push((inode, ttype, name.clone()));
        }

        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
            let next_index = (i + 1) as i64;
            let (inode, ttype, name) = entry;
            let buffer_full = reply.add(inode, next_index, ttype, name);
            if buffer_full {
                // TODO: what do if buffer is full??
                break;
            }
        }

        Ok(())
    }

    fn get_attrs(&self, inode: Inode) -> Result<FileAttr, Error> {
        let Some(path) = self.map.get_path(&inode) else {
            return Err(Error::NoSuchFileOrDirectory);
        };
        let Ok(metadata) = metadata(path) else {
            return Err(Error::IOError);
        };
        let attrs = attrs_from_metadata(inode, metadata);
        Ok(attrs)
    }
}

/// Functions intentionally left unimplemented:
///   - init
///   - destoy
///   - forget
///   - flush
///   - readdirplus
///   - statfs
///   - ioctl
///   - getlk
///   - setlk
///   - bmap
///   - lseek
/// because they weren't in the example I followed. I think trait defaults should be fine
///   - setxattr()
///   - getxattr()
///   - listxattr()
///   - removexattr()
/// because I'm not sure I need to worry about extended attributes for most usecases
///
/// TODO: need to actually do the filtering
impl Filesystem for FilterFS {
    /// Create directory
    fn mkdir(
        &mut self,
        req: &Request,
        parent: Inode,
        name: &OsStr,
        mode: u32,
        umask: u32,
        reply: ReplyEntry,
    ) {
        debug!("mkdir");
        // TODO: it would be cool if files created by working in the filteredfs persisteds and were not filtered out, if they would match a filter rule.
        // Otherwise, it would be possible to create a file that would then get filtered out while trying to access it. BUT then what if we try to create a file that exists and is filtered out????
        // Thats probably good enough reason to stop filtered files from being created in the first place

        let parent_path = self.map.get_path(&parent).unwrap();
        let path = parent_path.join(name);
        if let Some(_) = self.map.get_inode(&path) {
            reply.error(Error::FileExists.into());
            return;
        };

        fs::create_dir(&path).unwrap();
        let metadata = metadata(&path).unwrap();
        let inode = self.map.register(path, parent);
        let attrs = attrs_from_metadata(inode, metadata);
        reply.entry(&TTL, &attrs, 0);
    }

    /// Look up directory by name and get attributes
    fn lookup(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEntry) {
        debug!("lookup");
        let Some(parent_path) = self.map.get_path(&parent) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };
        let path = parent_path.join(name);
        let Some(inode) = self.map.get_inode(&path) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };
        match self.get_attrs(inode) {
            Ok(attrs) => {
                reply.entry(&TTL, &attrs, 0);
            }
            Err(e) => {
                reply.error(e.into());
            }
        }
    }

    /// Get file attributes
    fn getattr(&mut self, _req: &Request, inode: Inode, _fh: Option<u64>, reply: ReplyAttr) {
        debug!("getattr");
        match self.get_attrs(inode) {
            Ok(attrs) => reply.attr(&TTL, &attrs),
            Err(e) => reply.error(e.into()),
        }
    }

    /// Set file attributes
    fn setattr(
        &mut self,
        req: &Request<'_>,
        inode: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<fuser::TimeOrNow>,
        mtime: Option<fuser::TimeOrNow>,
        ctime: Option<SystemTime>,
        fh: Option<u64>,
        crtime: Option<SystemTime>,
        chgtime: Option<SystemTime>,
        bkuptime: Option<SystemTime>,
        flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        debug!("setattr");
        let Some(path) = self.map.get_path(&inode) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };

        if let Some(mode) = mode {
            let perms = Permissions::from_mode(mode);
            if fs::set_permissions(&path, perms).is_err() {
                // TODO: better error?
                reply.error(Error::OperationNotPermitted.into());
                return;
            }
        };

        if gid.is_some() || uid.is_some() {
            if chown(
                path,
                uid.map(|u| Uid::from_raw(u)),
                gid.map(|g| Gid::from_raw(g)),
            )
            .is_err()
            {
                reply.error(Error::PermissionDenied.into()); // TODO: check err returned from chown
                return;
            }
        }

        if let Some(size) = size {
            // TODO hope the cast is ok
            if nix::unistd::truncate(path, size as i64).is_err() {
                reply.error(Error::IOError.into()); // TODO: idk how to get more specific error
                return;
            }
        };

        let now = time_now();
        if let Some(atime) = atime {
            let Ok(file) = File::options().write(true).open(&path) else {
                reply.error(Error::IOError.into());
                return;
            };

            // TODO: condense time conversion stuff into a function
            let atime = match atime {
                fuser::TimeOrNow::SpecificTime(time) => time_from_system_time(&time),
                fuser::TimeOrNow::Now => now,
            };
            let atime = system_time_from_time(atime.0, atime.1.into());

            let times = FileTimes::new().set_accessed(atime);
            if file.set_times(times).is_err() {
                reply.error(Error::IOError.into());
                return;
            }
        };

        if let Some(mtime) = mtime {
            let Ok(file) = File::options().write(true).open(&path) else {
                reply.error(Error::IOError.into());
                return;
            };

            let mtime = match mtime {
                fuser::TimeOrNow::SpecificTime(time) => time_from_system_time(&time),
                fuser::TimeOrNow::Now => now,
            };
            let mtime = system_time_from_time(mtime.0, mtime.1.into());

            let times = FileTimes::new().set_modified(mtime);
            if file.set_times(times).is_err() {
                reply.error(Error::IOError.into());
                return;
            }
        };

        if let Some(ctime) = ctime {
            todo!();
        };

        if let Some(fh) = fh {
            todo!();
        };

        if let Some(crtime) = crtime {
            todo!();
        };

        if let Some(chgtime) = chgtime {
            todo!();
        }

        if let Some(bkuptime) = bkuptime {
            todo!();
        }

        if let Some(flags) = flags {
            todo!();
        }

        // Grab the attrs after making changes
        match self.get_attrs(inode) {
            Ok(attrs) => reply.attr(&TTL, &attrs),
            Err(e) => reply.error(e.into()),
        }
    }

    /// Read symlinks
    fn readlink(&mut self, req: &Request<'_>, inode: u64, reply: ReplyData) {
        debug!("readlink");
        let Some(path) = self.map.get_path(&inode) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };
        let Ok(real_path) = fs::read_link(&path) else {
            reply.error(Error::IOError.into());
            return;
        };
        let real_path = real_path.into_os_string();
        let real_path = real_path.as_bytes();
        reply.data(&real_path);
    }

    /// Create node: file, char device, block device, pipe, or socket
    fn mknod(
        &mut self,
        req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        umask: u32,
        rdev: u32,
        reply: ReplyEntry,
    ) {
        debug!("mknod");
        let file_type = mode & libc::S_IFMT as u32;

        let parent_path = self.map.get_path(&parent).unwrap();
        let path = parent_path.join(name);
        if let Some(_) = self.map.get_inode(&path) {
            reply.error(Error::FileExists.into());
            return;
        };

        let Ok(file_type) = as_file_type(mode) else {
            error!("Unknown file type {:o}", mode);
            reply.error(Error::InvalidArgument.into());
            return;
        };

        match file_type {
            FileType::RegularFile => {
                if File::create(&path).is_err() {
                    reply.error(Error::IOError.into());
                    return;
                }
            }
            FileType::Directory => {
                self.mkdir(req, parent, name, mode, umask, reply);
                return;
            }
            FileType::Symlink => {
                // TODO: how do we create a symlink without a target?
                reply.error(Error::FunctionNotImplemented.into());
                return;
            }
            FileType::Socket => {
                // TODO
                reply.error(Error::FunctionNotImplemented.into());
                return;
            }
            FileType::CharDevice => {
                // TODO
                reply.error(Error::FunctionNotImplemented.into());
                return;
            }
            FileType::BlockDevice => {
                // TODO
                reply.error(Error::FunctionNotImplemented.into());
                return;
            }
            FileType::NamedPipe => {
                // TODO
                reply.error(Error::FunctionNotImplemented.into());
                return;
            }
        }

        let inode = self.map.register(path, parent);
        match self.get_attrs(inode) {
            Ok(attrs) => reply.entry(&TTL, &attrs, 0),
            Err(e) => reply.error(e.into()),
        }
    }

    /// Remove file
    fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: fuser::ReplyEmpty) {
        debug!("unlink");
        let parent_path = self.map.get_path(&parent).unwrap();
        let path = parent_path.join(name);
        let Some(inode) = self.map.get_inode(&path) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };

        if fs::remove_file(&path).is_err() {
            reply.error(Error::IOError.into());
            return;
        }

        self.map.remove(inode);

        reply.ok();
    }

    /// Remove directory
    fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: fuser::ReplyEmpty) {
        debug!("rmdir");
        let parent_path = self.map.get_path(&parent).unwrap();
        let path = parent_path.join(name);
        let Some(inode) = self.map.get_inode(&path) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };

        if !path.is_dir() {
            reply.error(Error::NotADirectory.into());
            return;
        }

        if fs::remove_dir(path).is_err() {
            reply.error(Error::IOError.into());
            return;
        }

        self.map.remove(inode);

        reply.ok();
    }

    /// Create symlink
    fn symlink(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        link_name: &OsStr,
        target: &Path,
        reply: ReplyEntry,
    ) {
        debug!("symlink");
        let parent_path = self.map.get_path(&parent).unwrap();
        let source = parent_path.join(link_name);
        if let Some(_) = self.map.get_inode(&source) {
            error!("Link source exists");
            reply.error(Error::FileExists.into());
            return;
        };

        if let Some(_) = self.map.get_inode(&target) {
            error!("Link target does not exist");
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };

        if std::os::unix::fs::symlink(target, source).is_err() {
            reply.error(Error::IOError.into());
            return;
        }

        let inode = self.map.register(target.to_path_buf(), parent);
        match self.get_attrs(inode) {
            Ok(attrs) => {
                reply.entry(&TTL, &attrs, 0);
            }
            Err(e) => {
                reply.error(e.into());
            }
        }
    }

    /// Rename/move file
    fn rename(
        &mut self,
        _req: &Request<'_>,
        parent: Inode,
        name: &OsStr,
        new_parent: u64,
        new_name: &OsStr,
        flags: u32,
        reply: fuser::ReplyEmpty,
    ) {
        debug!("rename");
        let Some(parent_path) = self.map.get_path(&parent) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };
        let original_path = parent_path.join(name);
        let Some(old_inode) = self.map.get_inode(&original_path) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };

        let Some(new_parent_path) = self.map.get_path(&new_parent) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };

        let new_path = new_parent_path.join(new_name);
        if self.map.get_inode(&new_path).is_some() {
            reply.error(Error::FileExists.into());
            return;
        };

        if fs::rename(original_path, &new_path).is_err() {
            reply.error(Error::IOError.into());
            return;
        }

        self.map.register(new_path, parent);
        self.map.remove(old_inode);
        reply.ok();
    }

    /// Create hardlink
    fn link(
        &mut self,
        _req: &Request<'_>,
        inode: u64,
        new_parent: u64,
        new_name: &OsStr,
        reply: ReplyEntry,
    ) {
        debug!("link");
        let Some(original_path) = self.map.get_path(&inode) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };

        let Some(new_parent_path) = self.map.get_path(&new_parent) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };

        let link = new_parent_path.join(new_name);
        if link.is_file() {
            reply.error(Error::FileExists.into());
            return;
        }

        if fs::hard_link(original_path, &link).is_err() {
            reply.error(Error::IOError.into());
            return;
        }

        // TODO: This will assign a new inode, which might not be a problem since the underlying base layer's inode is what really matters
        let inode = self.map.register(link.clone(), new_parent);
        match self.get_attrs(inode) {
            Ok(attrs) => reply.entry(&TTL, &attrs, 0),
            Err(e) => reply.error(e.into()),
        }
    }

    /// Open a file
    fn open(&mut self, _req: &Request<'_>, inode: u64, flags: i32, reply: fuser::ReplyOpen) {
        debug!("open");
        let (access_mask, read, write) = match flags & libc::O_ACCMODE {
            libc::O_RDONLY => {
                // Behavior is undefined, but most filesystems return EACCES
                if flags & libc::O_TRUNC != 0 {
                    reply.error(libc::EACCES);
                    return;
                }
                if flags & FMODE_EXEC != 0 {
                    // Open is from internal exec syscall
                    (libc::X_OK, true, false)
                } else {
                    (libc::R_OK, true, false)
                }
            }
            libc::O_WRONLY => (libc::W_OK, false, true),
            libc::O_RDWR => (libc::R_OK | libc::W_OK, true, true),
            // Exactly one access mode flag must be specified
            _ => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        let Some(path) = self.map.get_path(&inode) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };

        let Ok(f) = OpenOptions::new().read(read).write(write).open(path) else {
            reply.error(Error::IOError.into());
            return;
        };

        let fd = f.as_raw_fd();
        // TODO: better double check this
        reply.opened(fd.try_into().unwrap(), 0);
    }

    /// Read data
    fn read(
        &mut self,
        _req: &Request,
        inode: Inode,
        _fh: u64,
        offset: i64,
        size: u32,
        flags: i32,
        lock: Option<u64>,
        reply: ReplyData,
    ) {
        debug!("read");
        let Some(path) = self.map.get_path(&inode) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };

        if let Ok(file) = File::open(path) {
            let file_size = file.metadata().unwrap().len();
            // Could underflow if file length is less than local_start
            let read_size = min(size, file_size.saturating_sub(offset as u64) as u32);

            let mut buffer = vec![0; read_size as usize];
            file.read_exact_at(&mut buffer, offset as u64).unwrap();
            reply.data(&buffer);
        } else {
            reply.error(Error::IOError.into());
        }
    }

    // Write data
    fn write(
        &mut self,
        _req: &Request<'_>,
        inode: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        write_flags: u32,
        flags: i32,
        lock_owner: Option<u64>,
        reply: fuser::ReplyWrite,
    ) {
        debug!("write");
        let Some(path) = self.map.get_path(&inode) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };

        if let Ok(mut file) = OpenOptions::new().write(true).open(path) {
            file.seek(SeekFrom::Start(offset as u64)).unwrap();
            file.write_all(data).unwrap();
            reply.written(data.len() as u32);
        } else {
            reply.error(Error::NoSuchFileOrDirectory.into())
        }
    }

    /// Release an open file
    fn release(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        debug!("release");
        // TODO: do we need to do this one or will underlying actual filesystem take care of it?
        reply.ok();
    }

    /// Open a directory
    fn opendir(&mut self, _req: &Request<'_>, inode: u64, flags: i32, reply: fuser::ReplyOpen) {
        debug!("opendir");
        let (access_mask, read, write) = match flags & libc::O_ACCMODE {
            libc::O_RDONLY => {
                // Behavior is undefined, but most filesystems return EACCES
                if flags & libc::O_TRUNC != 0 {
                    reply.error(libc::EACCES);
                    return;
                }
                (libc::R_OK, true, false)
            }
            libc::O_WRONLY => (libc::W_OK, false, true),
            libc::O_RDWR => (libc::R_OK | libc::W_OK, true, true),
            // Exactly one access mode flag must be specified
            _ => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        let Some(path) = self.map.get_path(&inode) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };

        let Ok(f) = OpenOptions::new().read(read).write(write).open(path) else {
            reply.error(Error::IOError.into());
            return;
        };

        let fd = f.as_raw_fd();
        // TODO: better double check this
        reply.opened(fd.try_into().unwrap(), 0);
    }

    /// Read directory's contents
    fn readdir(
        &mut self,
        _req: &Request,
        ino: Inode,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        debug!("readdir");
        match self.readdir_helper(_req, ino, _fh, offset, &mut reply) {
            Ok(_) => reply.ok(),
            Err(e) => reply.error(Error::NoSuchFileOrDirectory.into()), // TODO: better errors
        }
    }

    /// Release an open directory
    fn releasedir(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _fh: u64,
        _flags: i32,
        reply: fuser::ReplyEmpty,
    ) {
        debug!("releasedir");
        // TODO: again, not sure we need to do this one
        reply.ok();
    }

    /// Check access permissions
    fn access(&mut self, _req: &Request<'_>, inode: u64, mask: i32, reply: fuser::ReplyEmpty) {
        debug!("access");
        let Some(path) = self.map.get_path(&inode) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };

        // If we cant get the metadata we probably can't access the file. Good enough for now
        let Ok(metadata) = metadata(&path) else {
            reply.error(Error::PermissionDenied.into());
            return;
        };

        reply.ok();
    }

    /// Create and open a file
    fn create(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        umask: u32,
        flags: i32,
        reply: fuser::ReplyCreate,
    ) {
        debug!("create");
        let Some(parent_path) = self.map.get_path(&parent) else {
            reply.error(Error::NoSuchFileOrDirectory.into());
            return;
        };
        let path = parent_path.join(name);
        if self.map.get_inode(&path).is_some() {
            reply.error(Error::FileExists.into());
            return;
        }

        let (read, write) = match flags & libc::O_ACCMODE {
            libc::O_RDONLY => (true, false),
            libc::O_WRONLY => (false, true),
            libc::O_RDWR => (true, true),
            // Exactly one access mode flag must be specified
            _ => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        let Ok(file_type) = as_file_type(mode) else {
            error!("Unknown file type {:o}", mode);
            reply.error(Error::InvalidArgument.into());
            return;
        };

        let fd = match file_type {
            FileType::RegularFile => {
                let Ok(f) = File::create(&path) else {
                    reply.error(Error::IOError.into());
                    return;
                };

                let Ok(f) = OpenOptions::new().read(read).write(write).open(&path) else {
                    reply.error(Error::IOError.into());
                    return;
                };

                let fd = f.as_raw_fd();
                fd
            }
            FileType::Directory => {
                if fs::create_dir(&path).is_err() {
                    reply.error(Error::IOError.into());
                    return;
                }
                let Ok(f) = OpenOptions::new().read(read).write(write).open(&path) else {
                    reply.error(Error::IOError.into());
                    return;
                };

                let fd = f.as_raw_fd();
                fd
            }
            FileType::NamedPipe => {
                // TODO
                reply.error(Error::FunctionNotImplemented.into());
                return;
            }
            FileType::CharDevice => {
                // TODO
                reply.error(Error::FunctionNotImplemented.into());
                return;
            }

            FileType::BlockDevice => {
                // TODO
                reply.error(Error::FunctionNotImplemented.into());
                return;
            }
            FileType::Symlink => {
                // TODO
                reply.error(Error::FunctionNotImplemented.into());
                return;
            }
            FileType::Socket => {
                // TODO
                reply.error(Error::FunctionNotImplemented.into());
                return;
            }
        };

        let inode = self.map.register(path, parent);
        let attrs = self.get_attrs(inode).unwrap();

        // TODO: better double check this and raw fd
        reply.created(&TTL, &attrs, 0, fd.try_into().unwrap(), 0);
    }

    #[cfg(target_os = "linux")]
    fn fallocate(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        length: i64,
        mode: i32,
        reply: fuser::ReplyEmpty,
    ) {
        debug!("fallocate");
        todo!();
    }

    fn copy_file_range(
        &mut self,
        _req: &Request<'_>,
        ino_in: u64,
        fh_in: u64,
        offset_in: i64,
        ino_out: u64,
        fh_out: u64,
        offset_out: i64,
        len: u64,
        flags: u32,
        reply: fuser::ReplyWrite,
    ) {
        debug!("copy_file_range");
        todo!();
    }
}
