use crate::filemode::FileMode;
use nix::unistd::{Gid, Group, Uid, User};
use std::fmt;
use std::fmt::Display;
use std::fs;
use std::os::linux::fs::MetadataExt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
pub struct PathInfo {
    /// file path
    pub path: PathBuf,
    /// metadata associated with path
    pub meta: fs::Metadata,
}

// an alternative to defining these on the field that matters
// are crates like derive-where or derive_more
impl PartialEq for PathInfo {
    fn eq(&self, other: &Self) -> bool {
        self.path.eq(&other.path)
    }
}
impl PartialOrd for PathInfo {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for PathInfo {}
impl Ord for PathInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.path.cmp(&other.path)
    }
}

impl Display for PathInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = self
            .path
            .file_name()
            .map(|s| s.to_string_lossy())
            .map(|s| s.to_string())
            .unwrap_or_default();
        if self.meta.is_dir() {
            s.push('/');
        }
        write!(f, "{}", s)
    }
}

impl PathInfo {
    pub fn new(path: PathBuf, meta: fs::Metadata) -> Self {
        Self { path, meta }
    }
}

/// Used for displaying path and metadata information using the `-l/--long` option
pub struct LongPathInfo {
    pub filetype_mode: String,
    pub num_links: String,
    pub file_owner: String,
    pub file_group: String,
    pub size: String,
    pub last_modified: String,
    pub path: PathInfo,
}

/// Collects `ls` long output metadata from `PathInfo` and produce a `LongPathInfo`
impl From<PathInfo> for LongPathInfo {
    fn from(p: PathInfo) -> Self {
        // filetype and mode
        let filetype = if p.meta.is_dir() {
            "d"
        } else if p.meta.is_symlink() {
            "l"
        } else {
            "-"
        };
        let mode = FileMode(p.meta.st_mode()).to_string();
        let filetype_mode = format!("{filetype}{mode}");
        // number of links
        let num_links = p.meta.st_nlink();
        // file owner
        let owner_uid = Uid::from_raw(p.meta.st_uid());
        let owner_user = User::from_uid(owner_uid).unwrap_or_default();
        let file_owner = owner_user.map(|u| u.name).unwrap_or_default();
        // file groups
        let owner_gid = Gid::from_raw(p.meta.st_gid());
        let owner_group = Group::from_gid(owner_gid).unwrap_or_default();
        let file_group = owner_group.map(|g| g.name).unwrap_or_default();
        // size
        let size = p.meta.st_size();
        // last modified
        // when the modified date is more than 1 year ago, the time is replaced by the
        // modification year
        let last_mod_secs = p.meta.st_mtime();
        let one_year_ago = SystemTime::now() - Duration::from_secs(60 * 24 * 365);
        let one_year_ago_ts = time_format::from_system_time(one_year_ago).unwrap_or_default();
        let last_modified = if last_mod_secs < one_year_ago_ts {
            time_format::strftime_local("%b %d %Y", last_mod_secs).unwrap_or_default()
        } else {
            time_format::strftime_local("%b %d %H:%M", last_mod_secs).unwrap_or_default()
        };

        Self {
            filetype_mode,
            num_links: num_links.to_string(),
            file_owner,
            file_group,
            size: size.to_string(),
            last_modified,
            path: p,
        }
    }
}
