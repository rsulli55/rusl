mod constants;
mod filemode;
mod layout;
mod pathinfo;

use crate::constants::*;
use crate::filemode::FileMode;
use crate::layout::{LayoutInfo, determine_layout};
use crate::pathinfo::{LongPathInfo, PathInfo};
use clap::ArgAction;
use clap::Parser;
use itertools::Itertools;
use std::fs;
use std::fs::Metadata;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result as IOResult;
use std::os::linux::fs::MetadataExt;
use std::path::Path;
use termion::color;
use termion::style;
use termion::terminal_size;

/// Command-line arguments for the program.
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

/// Display options for formatting output.
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

/// Format and print an error message.
fn print_error_msg(what: &str, why: &str) {
    eprintln!("{PROGRAM}: {what}: {why}");
}

/// Print `std::io::Error`s using predefined formats taken from ls errors.
fn print_io_error(path: &Path, err: &Error) {
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

/// Get metadata for a path, printing errors if any.
fn stat_path(path: &Path) -> Option<fs::Metadata> {
    match fs::metadata(path) {
        Ok(meta) => Some(meta),
        Err(err) => {
            print_io_error(path, &err);
            None
        }
    }
}

/// Attempt to create and collect `PathInfo` for each path in `paths`
fn collect_pathinfo(paths: &[&Path]) -> Vec<PathInfo> {
    paths
        .iter()
        .filter_map(|p| stat_path(p).map(|meta| PathInfo::new(p.to_path_buf(), meta)))
        .collect()
}

/// Check if a path is hidden (starts with '.').
fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .map(|s| s.to_string_lossy())
        .is_some_and(|s| s.starts_with('.'))
}

/// Recursively collect `PathInfo` for a directory, optionally ignoring hidden files.
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
                    print_io_error(&p.path(), &err);
                    None
                }
            })
            .collect(),
        Err(err) => {
            print_io_error(dir, &err);
            Vec::new()
        }
    }
}

/// Display `paths` using the long format for ls. The structure for the format is
/// ```
/// filetype_and_mode number_of_links file_owner file_group file_size last_modified file_name
/// ```
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

/// Display `paths` ascending down columns using the number of columns and
/// column widths specified by `layout`
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

/// Check if `meta` represents an executable file (i.e., any execute bit set)
fn is_executable(meta: &Metadata) -> bool {
    let mode = FileMode(meta.st_mode());
    mode.user_execute() || mode.group_execute() || mode.other_execute()
}

/// Print `path` using ls-like colors according to the file type.
/// Add whitespace after the path to fill `col_width` characters.
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

/// Display `paths` ascending across rows using the number of columns and
/// column widths specified by `layout`
fn display_by_lines(layout: &LayoutInfo, paths: &[PathInfo]) {
    let chunks = paths.chunks(layout.num_cols);
    for chunk in chunks {
        for (ind, p) in chunk.iter().enumerate() {
            print_pathinfo(p, layout.col_width[ind]);
        }
        println!();
    }
}

/// Collect and print the children of `dir` using `recurse_dir()`.
/// Optionally, include a total size if `opts.long`
/// is `true` and skip hidden children if `opts.all` is `false`.
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
/// Iterate over all directories in `dirs`, displaying each.
/// If there is more than one directory, preface the directory contents with
/// the directory name.
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
