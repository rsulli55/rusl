use clap::ArgAction;
use clap::Parser;
use itertools::Itertools;
use nix::unistd::{Gid, Group, Uid, User};
use std::fmt;
use std::fmt::Display;
use std::fs;
use std::fs::Metadata;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result as IOResult;
use std::os::linux::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::slice::ChunksExact;
use std::time::{Duration, SystemTime};
use termion::color;
use termion::style;
use termion::terminal_size;

const PROGRAM: &str = "rusl";
const ERR_NO_SUCH_FILE_OR_DIR: &str = "No such file or directory";
const ERR_PERM_DENIED: &str = "Permission denied";
const MIN_COL_SIZE: usize = 3;
const COL_SEP_LEN: usize = 2;

#[derive(Debug, Parser)]
#[command(disable_help_flag(true))]
struct Args {
    /// filepaths to process
    paths: Option<Vec<String>>,

    /// show hidden paths
    #[arg(short, long, default_value_t = false)]
    all: bool,

    /// use long listing format
    #[arg(short, default_value_t = false)]
    long: bool,

    /// display entries by lines instead of by columns
    #[arg(short = 'x', default_value_t = false)]
    by_lines: bool,

    /// Print help
    #[arg(long, action = ArgAction::HelpShort)]
    help: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayOptions {
    /// show hidden paths
    all: bool,

    /// use long listing format
    long: bool,

    /// display entries by lines instead of by columns
    by_lines: bool,
}

impl From<&Args> for DisplayOptions {
    fn from(value: &Args) -> Self {
        Self {
            all: value.all,
            long: value.long,
            by_lines: value.by_lines,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LayoutInfo {
    num_cols: usize,
    col_width: Vec<usize>,
}

impl LayoutInfo {
    pub fn new(num_cols: usize, col_width: Vec<usize>) -> Self {
        Self {
            num_cols,
            col_width,
        }
    }
}

impl Default for LayoutInfo {
    fn default() -> Self {
        Self {
            num_cols: 1,
            col_width: vec![0],
        }
    }
}

#[derive(Debug, Clone)]
struct PathInfo {
    /// file path
    path: PathBuf,
    /// metadata associated with path
    meta: fs::Metadata,
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

fn print_error_msg(what: &str, why: &str) {
    eprintln!("{PROGRAM}: {what}: {why}");
}

fn inspect_io_error(path: &Path, err: &Error) {
    match err.kind() {
        ErrorKind::NotFound => {
            print_error_msg(
                &format!("cannot access '{}'", path.display()),
                ERR_NO_SUCH_FILE_OR_DIR,
            );
        }
        ErrorKind::PermissionDenied => {
            print_error_msg(
                &format!("cannot access '{}'", path.display()),
                ERR_PERM_DENIED,
            );
        }
        _ => {
            print_error_msg(
                &format!("cannot access '{}'", path.display()),
                "unknown error",
            );
        }
    }
}

fn stat_path(path: &Path) -> Option<fs::Metadata> {
    match fs::metadata(path) {
        Ok(meta) => Some(meta),
        Err(err) => {
            inspect_io_error(path, &err);
            None
        }
    }
}

fn collect_pathinfo(paths: &[&Path]) -> Vec<PathInfo> {
    paths
        .iter()
        .filter_map(|p| stat_path(p).map(|meta| PathInfo::new(p.to_path_buf(), meta)))
        .collect()
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .map(|s| s.to_string_lossy())
        .is_some_and(|s| s.starts_with('.'))
}

fn recurse_dir(ignore_hidden: bool, dir: &Path) -> Vec<PathInfo> {
    match dir.read_dir() {
        Ok(entries) => entries
            .flat_map(|p| {
                if let Err(err) = &p {
                    print_error_msg("failed reading directory entry", &err.to_string());
                }
                p
            })
            .filter_map(|p| match p.metadata() {
                Ok(meta) => {
                    // filter out hidden paths if asked
                    if ignore_hidden && is_hidden(&p.path()) {
                        None
                    } else {
                        Some(PathInfo::new(p.path(), meta))
                    }
                }
                Err(err) => {
                    inspect_io_error(&p.path(), &err);
                    None
                }
            })
            .collect(),
        Err(err) => {
            inspect_io_error(dir, &err);
            Vec::new()
        }
    }
}

fn col_widths_by_cols(min_width: usize, num_cols: usize, lens: &[usize]) -> Vec<usize> {
    let num_rows = lens.len() / num_cols;
    let rem = lens.len() % num_cols;
    // the first `rem` cols must accomodate num_rows + 1 rows
    let end = rem * (num_rows + 1);

    // calculate appropriate col width from chunks of columns
    let chunks_to_col_width = |chunks: ChunksExact<_>| {
        chunks
            .into_iter()
            .map(|col| col.iter().fold(min_width, |acc, l| std::cmp::max(acc, *l)))
            .collect_vec()
    };

    // chunk the first rem columns by num_rows + 1 elements
    let chunks = lens[..end].chunks_exact(num_rows + 1);
    debug_assert!(chunks.remainder().is_empty());
    let start_col_widths = chunks_to_col_width(chunks);

    // chunk the rest of the columns by num_rows elements
    let chunks = lens[end..].chunks_exact(num_rows);
    debug_assert!(chunks.remainder().is_empty());
    let fin_col_widths = chunks_to_col_width(chunks);
    [start_col_widths, fin_col_widths].concat()
}

fn col_widths_by_lines(min_width: usize, num_cols: usize, lens: &[usize]) -> Vec<usize> {
    let mut col_width = Vec::with_capacity(num_cols);
    for offset in 0..num_cols {
        let width = lens[offset..]
            .iter()
            .step_by(num_cols)
            .fold(min_width, |acc, l| std::cmp::max(acc, *l));
        dbg!(num_cols, offset, width);
        col_width.push(width);
    }
    dbg!(&col_width);
    col_width
}

/// Determines the layout for displaying a list of strings within the current terminal width with
/// the maximal amount of columns.
///
/// This function calculates possible layouts for the given `paths` based on the available terminal columns (`term_cols`)
/// and the minimum column size. It tries different numbers of columns, computes the required width for each layout,
/// and selects the most space-efficient layout that fits within the terminal.
///
/// # Parameters
/// - `by_lines`: If `true`, strings in `str_paths` are layed out across rows, otherwise down columns.
/// - `term_cols`: The total number of columns available in the terminal.
/// - `paths`: A slice of PathInfo representing the items to be displayed.
///
/// # Returns
/// A `LayoutInfo` struct describing the chosen layout (number of columns and their widths). If no valid layout fits,
/// returns a default `LayoutInfo`.
fn determine_layout(by_lines: bool, term_cols: usize, lens: &[usize]) -> LayoutInfo {
    let max_cols = std::cmp::min(term_cols / MIN_COL_SIZE, lens.len());

    let mut valid_layouts = Vec::with_capacity(max_cols);
    for num_cols in 1..=max_cols {
        let col_width = if by_lines {
            col_widths_by_lines(MIN_COL_SIZE, num_cols, lens)
        } else {
            col_widths_by_cols(MIN_COL_SIZE, num_cols, lens)
        };
        dbg!(&col_width);
        let total_width: usize = col_width.iter().sum();
        if total_width <= term_cols {
            valid_layouts.push(LayoutInfo::new(num_cols, col_width));
        }
    }
    dbg!(&valid_layouts);
    valid_layouts.pop().unwrap_or_default()
}

struct FileMode(u32);

impl FileMode {
    fn user_execute(&self) -> bool {
        self.0 & 0o100 != 0
    }
    fn user_write(&self) -> bool {
        self.0 & 0o200 != 0
    }
    fn user_read(&self) -> bool {
        self.0 & 0o400 != 0
    }
    fn group_execute(&self) -> bool {
        self.0 & 0o10 != 0
    }
    fn group_write(&self) -> bool {
        self.0 & 0o20 != 0
    }
    fn group_read(&self) -> bool {
        self.0 & 0o40 != 0
    }
    fn other_execute(&self) -> bool {
        self.0 & 0o1 != 0
    }
    fn other_write(&self) -> bool {
        self.0 & 0o2 != 0
    }
    fn other_read(&self) -> bool {
        self.0 & 0o4 != 0
    }
    fn sticky_bit(&self) -> bool {
        self.0 & 0o1000 != 0
    }
    fn sgid_bit(&self) -> bool {
        self.0 & 0o2000 != 0
    }
    fn suid_bit(&self) -> bool {
        self.0 & 0o4000 != 0
    }
}

/// Displays the mode using ls character symbols. This implementation does not fully replicate
/// ls output because it will never output `S` for the setuid or setgid bits.
///
/// See this stackexchange answer for more details about `s` vs `S`
/// https://unix.stackexchange.com/a/28412
impl Display for FileMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let user_write = if self.user_write() { "w" } else { "-" };
        let user_read = if self.user_read() { "r" } else { "-" };
        let user_execute = if self.user_execute() {
            "x"
        } else if self.suid_bit() {
            "s"
        } else {
            "-"
        };
        let group_write = if self.group_write() { "w" } else { "-" };
        let group_read = if self.group_read() { "r" } else { "-" };
        let group_execute = if self.group_execute() {
            "x"
        } else if self.sgid_bit() {
            "s"
        } else {
            "-"
        };
        let other_write = if self.other_write() { "w" } else { "-" };
        let other_read = if self.other_read() { "r" } else { "-" };
        let other_execute = if self.other_execute() { "x" } else { "-" };
        let sticky_bit = if self.sticky_bit() { "t" } else { "." };
        write!(
            f,
            "{}{}{}{}{}{}{}{}{}{}",
            user_write,
            user_read,
            user_execute,
            group_write,
            group_read,
            group_execute,
            other_write,
            other_read,
            other_execute,
            sticky_bit
        )
    }
}

/// Used for displaying path and metadata information using the `-l/--long` option
struct LongPathInfo {
    filetype_mode: String,
    num_links: String,
    file_owner: String,
    file_group: String,
    size: String,
    last_modified: String,
    path: PathInfo,
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

/// Display `paths` using the long format for ls. The structure for the format is
/// filetype_and_mode number_of_links file_owner file_group file_size last_modified file_name
fn display_pathinfo_long(paths: &[PathInfo]) {
    let longpaths = paths
        .iter()
        .map(|p| LongPathInfo::from(p.clone()))
        .collect_vec();
    let (
        filetype_mode_width,
        num_links_width,
        file_owner_width,
        file_group_width,
        size_width,
        last_modified_width,
    ) = longpaths.iter().fold((0, 0, 0, 0, 0, 0), |acc, p| {
        (
            std::cmp::max(acc.0, p.filetype_mode.len()),
            std::cmp::max(acc.1, p.num_links.len()),
            std::cmp::max(acc.2, p.file_owner.len()),
            std::cmp::max(acc.3, p.file_group.len()),
            std::cmp::max(acc.4, p.size.len()),
            std::cmp::max(acc.5, p.last_modified.len()),
        )
    });
    for p in &longpaths {
        // print all the fields with width and alignment
        print!("{:filetype_mode_width$} ", p.filetype_mode);
        print!("{:num_links_width$} ", p.num_links);
        print!("{:file_owner_width$} ", p.file_owner);
        print!("{:file_group_width$} ", p.file_group);
        print!("{:size_width$} ", p.size);
        print!("{:last_modified_width$} ", p.last_modified);
        // file_name
        print_pathinfo(&p.path, 0);
        // optionally print link info
        if p.path.meta.is_symlink() {
            print!(" -> ");
            let link_target = p.path.path.canonicalize().unwrap_or_default();
            print!(
                "{}{}{}{}",
                style::Bold,
                color::Fg(color::Green),
                link_target.display(),
                style::Reset,
            );
        }
        println!();
    }
}

/// Determine a layout for the `paths` based on `term_cols` and display them
fn display_paths(opts: &DisplayOptions, term_cols: usize, paths: &[PathInfo]) {
    let lens = paths
        .iter()
        .map(|p| p.to_string().len() + COL_SEP_LEN)
        .collect_vec();
    let layout = determine_layout(opts.by_lines, term_cols, &lens);
    if opts.long {
        display_pathinfo_long(paths);
    } else if opts.by_lines {
        display_by_lines(&layout, paths);
    } else {
        display_by_cols(&layout, paths);
    }
}

fn display_by_cols(layout: &LayoutInfo, paths: &[PathInfo]) {
    let num_cols = layout.num_cols;
    // the first num_rows rows will be full
    let num_rows = paths.len() / num_cols;
    // the final row will consist of rem columns
    let rem = paths.len() % num_cols;
    // dbg!(num_rows, rem, str_paths.len(), str_paths);
    // print the full rows
    for r in 0..num_rows {
        let skip = num_rows + 1;
        let end = r + skip * rem;
        let strs = paths[r..end].iter().step_by(skip);
        for (c, p) in strs.enumerate() {
            print_pathinfo(p, layout.col_width[c]);
        }
        let skip = num_rows;
        let strs = paths[end..].iter().step_by(skip);
        for (c, p) in strs.enumerate() {
            // we've already done the first rem columns
            print_pathinfo(p, layout.col_width[c + rem]);
        }
        println!();
    }
    if rem > 0 {
        // print the final partial row
        let skip = num_rows + 1;
        let strs = paths[num_rows..].iter().step_by(skip);
        for (c, p) in strs.enumerate() {
            print_pathinfo(p, layout.col_width[c]);
        }
    }
}

fn is_executable(metadata: &Metadata) -> bool {
    let mode = FileMode(metadata.st_mode());
    mode.user_execute() || mode.group_execute() || mode.other_execute()
}

fn print_pathinfo(path: &PathInfo, col_width: usize) {
    let s = path.to_string();
    // when there is only 1 column, it is possible that the width does not accomodate the
    // the string
    let indent_len = col_width.saturating_sub(s.len());
    if path.meta.is_dir() {
        print!(
            "{}{}{}{}{}",
            style::Bold,
            color::Fg(color::Blue),
            s,
            style::Reset,
            " ".repeat(indent_len)
        );
    } else if path.meta.is_symlink() {
        print!(
            "{}{}{}{}{}",
            style::Bold,
            color::Fg(color::Cyan),
            s,
            style::Reset,
            " ".repeat(indent_len)
        );
    } else if is_executable(&path.meta) {
        print!(
            "{}{}{}{}{}",
            style::Bold,
            color::Fg(color::Green),
            s,
            style::Reset,
            " ".repeat(indent_len)
        );
    } else {
        print!("{}{}", s, " ".repeat(indent_len));
    }
}

fn display_by_lines(layout: &LayoutInfo, paths: &[PathInfo]) {
    let chunks = paths.chunks(layout.num_cols);
    for chunk in chunks {
        for (ind, p) in chunk.iter().enumerate() {
            print_pathinfo(p, layout.col_width[ind]);
        }
        println!();
    }
}

fn display_dir_contents(opts: &DisplayOptions, term_cols: usize, dir: &PathInfo) {
    if opts.long {
        println!("total {}", dir.meta.st_size());
    }
    let children: Vec<_> = recurse_dir(!opts.all, &dir.path)
        .into_iter()
        .sorted()
        .collect();
    display_paths(opts, term_cols, &children);
}

fn display_dirs(opts: &DisplayOptions, term_cols: usize, dirs: &[PathInfo]) {
    // print files from dirs grouped if there are more than one
    if dirs.len() > 1 {
        println!("{}:", dirs[0].path.display());
    }
    for (ind, dir) in dirs[..dirs.len() - 1].iter().enumerate() {
        display_dir_contents(opts, term_cols, dir);
        println!();
        println!("{}:", dirs[ind + 1].path.display())
    }
    // print the final dir
    display_dir_contents(opts, term_cols, &dirs[dirs.len() - 1]);
}

fn main() -> IOResult<()> {
    let args = Args::parse();
    let opts = DisplayOptions::from(&args);

    // default to checking the cwd
    let string_paths = if args.paths.is_none() {
        vec![".".to_string()]
    } else {
        args.paths.unwrap()
    };

    let paths = string_paths.iter().map(Path::new).collect_vec();

    let pathsinfo = collect_pathinfo(&paths).into_iter().sorted();

    let (dirs, files): (Vec<_>, Vec<_>) = pathsinfo.partition(|p| p.meta.is_dir());

    let (term_cols, _) = terminal_size()?;
    let term_cols = term_cols as usize;
    if !files.is_empty() {
        display_paths(&opts, term_cols, &files);
    }

    if !dirs.is_empty() {
        display_dirs(&opts, term_cols, &dirs);
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_determine_layout1() {
        let term_cols = 10;
        let lens = vec![5, 5, 4, 4];
        let layout_by_cols = LayoutInfo::new(2, vec![5, 4]);
        assert_eq!(determine_layout(false, term_cols, &lens), layout_by_cols);
        let layout_by_lines = LayoutInfo::new(2, vec![5, 5]);
        assert_eq!(determine_layout(true, term_cols, &lens), layout_by_lines);
    }

    #[test]
    fn test_determine_layout2() {
        let term_cols = 13;
        let lens = vec![3, 3, 7, 7, 3];
        // by columns:
        // attempting to layout this in 2 columns fails:
        // the widths are:
        // 3   7
        // 3   3
        // 7
        // but three columns works:
        // 3   7   3
        // 3   7
        let layout_by_cols = LayoutInfo::new(3, vec![3, 7, 3]);
        assert_eq!(determine_layout(false, term_cols, &lens), layout_by_cols);
        // by lines:
        // attempting to layout this in 2 columns fails:
        // the widths are:
        // 3   3
        // 7   7
        // 3
        // three columns doesn't work either:
        // 3   3   7
        // 7   3
        // only 1 column will work
        let layout_by_lines = LayoutInfo::new(1, vec![7]);
        assert_eq!(determine_layout(true, term_cols, &lens), layout_by_lines);
    }
}
