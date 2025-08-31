use clap::ArgAction;
use clap::Parser;
use itertools::Itertools;
use std::fmt;
use std::fmt::Display;
use std::fs;
use std::fs::File;
use std::fs::Metadata;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result as IOResult;
use std::path::{Path, PathBuf};
use termion::terminal_size;

const PROGRAM: &str = "rusl";
const ERR_NO_SUCH_FILE_OR_DIR: &str = "No such file or directory";
const ERR_PERM_DENIED: &str = "Permission denied";

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

    /// use human readable sizes
    #[arg(short, long, default_value_t = false)]
    human_readable: bool,

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

    /// use human readable sizes
    human_readable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LayoutInfo {
    num_cols: u16,
    max_lengths: Vec<u16>,
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
        self.path.partial_cmp(&other.path)
    }
}

impl Eq for PathInfo {}
impl Ord for PathInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.path.cmp(&other.path)
    }
}

impl PathInfo {
    pub fn new(path: PathBuf, meta: fs::Metadata) -> Self {
        Self { path, meta }
    }
}

impl From<&Args> for DisplayOptions {
    fn from(value: &Args) -> Self {
        Self {
            all: value.all,
            long: value.long,
            human_readable: value.human_readable,
        }
    }
}

fn display_dir(opts: &DisplayOptions, dir: &Path) -> IOResult<()> {
    let entries = dir.read_dir()?;
    for p in entries {
        println!("{}", p?.path().display())
    }

    Ok(())
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
        .filter_map(|p| {
            if let Some(meta) = stat_path(p) {
                Some(PathInfo::new(p.to_path_buf(), meta))
            } else {
                None
            }
        })
        .collect()
}

fn recurse_dir(dir: &Path) -> Vec<PathInfo> {
    match dir.read_dir() {
        Ok(entries) => entries
            .flat_map(|p| {
                if p.is_err() {
                    print_error_msg(
                        "failed reading directory entry",
                        &p.as_ref().unwrap_err().to_string(),
                    );
                }
                p
            })
            .filter_map(|p| match p.metadata() {
                Ok(meta) => Some(PathInfo::new(p.path(), meta)),
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

/// Checks if `path` is either the cwd "." or parent dir ".."
fn non_trivial_dir(path: &Path) -> bool {
    !(path == Path::new(".") || path == Path::new(".."))
}

/// Determines a valid layout for the current terminal size and
/// the strs in `str_paths`
// TODO!
fn determine_layout(str_paths: &[&str]) -> IOResult<LayoutInfo> {
    let (num_cols, num_rows) = terminal_size()?;

    let mut overall_max_cols = 1u16;
    let mut cur_max_cols = 1u16;
    let mut cols_left = num_cols;
    let col_counts: Vec<_> = str_paths
        .iter()
        .scan(num_cols, |cols_left, p| {
            let path_len = p.len() as u16;
            if path_len < *cols_left {
                *cols_left -= path_len + 1;
            } else {
                *cols_left = num_cols;
            }
            Some(*cols_left)
        })
        .collect();
    println!("** col_counts:\n  {:?}\n", &col_counts);
    let max_col = col_counts
        .iter()
        .fold((u16::MAX, 0u16, u16::MAX), |acc, c| {
            let (prev, cur_run_len, min_run_len) = acc;
            // if c reset the col count we need to update min_run_len and reset
            // cur_run_len
            if prev < *c {
                let mrl = std::cmp::min(min_run_len, cur_run_len);
                let crl = 0u16;
                (*c, crl, mrl)
            } else {
                (*c, cur_run_len + 1, min_run_len)
            }
        });
    println!("** max_col:  {:?}", max_col);
    for p in str_paths {
        let path_len = p.len() as u16;
        if path_len < cols_left {
            cur_max_cols += 1;
            cols_left -= path_len + 1;
        } else {
            overall_max_cols = std::cmp::max(overall_max_cols, cur_max_cols);
            cur_max_cols = 1;
            cols_left = num_cols;
        }
    }
    Ok(overall_max_cols)
}
fn display_paths(opts: &DisplayOptions, paths: &[PathInfo]) {

/// Calculates the maximum number of columns supported for the current terminal size and
/// the strs in `str_paths`
fn calculate_max_cols(str_paths: &[&str]) -> IOResult<u16> {
    let (num_cols, num_rows) = terminal_size()?;

    let mut overall_max_cols = 1u16;
    let mut cur_max_cols = 1u16;
    let mut cols_left = num_cols;
    let col_counts: Vec<_> = str_paths
        .iter()
        .scan(num_cols, |cols_left, p| {
            let path_len = p.len() as u16;
            if path_len < *cols_left {
                *cols_left -= path_len + 1;
            } else {
                *cols_left = num_cols;
            }
            Some(*cols_left)
        })
        .collect();
    println!("** col_counts:\n  {:?}\n", &col_counts);
    let max_col = col_counts
        .iter()
        .fold((u16::MAX, 0u16, u16::MAX), |acc, c| {
            let (prev, cur_run_len, min_run_len) = acc;
            // if c reset the col count we need to update min_run_len and reset
            // cur_run_len
            if prev < *c {
                let mrl = std::cmp::min(min_run_len, cur_run_len);
                let crl = 0u16;
                (*c, crl, mrl)
            } else {
                (*c, cur_run_len + 1, min_run_len)
            }
        });
    println!("** max_col:  {:?}", max_col);
    for p in str_paths {
        let path_len = p.len() as u16;
        if path_len < cols_left {
            cur_max_cols += 1;
            cols_left -= path_len + 1;
        } else {
            overall_max_cols = std::cmp::max(overall_max_cols, cur_max_cols);
            cur_max_cols = 1;
            cols_left = num_cols;
        }
    }
    Ok(overall_max_cols)
}
fn display_paths(opts: &DisplayOptions, paths: &[PathInfo]) {
    let str_paths: Vec<_> = paths
        .iter()
        .flat_map(|p| {
            p.path.file_name().and_then(|s| s.to_str()).and_then(|s| {
                if !opts.all && s.starts_with('.') {
                    None
                } else {
                    Some(s)
                }
            })
        })
        .collect();
    let max_cols = calculate_max_cols(&str_paths).unwrap();
    println!("** Max cols: {max_cols}");
    for s in &str_paths {
        println!("{}", s)
    }
}

fn display_dirs(opts: &DisplayOptions, dirs: &[PathInfo]) {
    // print files from dirs grouped if there are more than one
    if dirs.len() > 1 {
        println!("{}:", dirs[0].path.display());
    }
    for (ind, dir) in dirs[..dirs.len() - 1].iter().enumerate() {
        let children: Vec<_> = recurse_dir(&dir.path).into_iter().sorted().collect();
        display_paths(&opts, &children);
        println!("");
        println!("{}:", dirs[ind + 1].path.display())
    }
    // print the final dir
    let children: Vec<_> = recurse_dir(&dirs[dirs.len() - 1].path)
        .into_iter()
        .sorted()
        .collect();
    display_paths(&opts, &children);
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

    let paths: Vec<&Path> = string_paths.iter().map(|s| Path::new(s)).collect();

    let pathsinfo = collect_pathinfo(&paths).into_iter().sorted();

    // let (dirs, file) : (Vec<_>, Vec<_>) =
    //     paths.into_iter().enumerate().partition(|i, p|
    //     path_metas[i].is_dir());
    let (dirs, files): (Vec<_>, Vec<_>) = pathsinfo.partition(|p| p.meta.is_dir());
    display_paths(&opts, &files);

    display_dirs(&opts, &dirs);

    // if dirs.len() == 1 {
    //        let mut children = recurse_dir(&dirs[0])?;
    //        children.sort();
    //     display_paths(&opts, &);
    // }
    //

    Ok(())
}
