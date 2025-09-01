use clap::ArgAction;
use clap::Parser;
use itertools::Chunk;
use itertools::Itertools;
use itertools::concat;
use std::fmt;
use std::fmt::Display;
use std::fs;
use std::fs::File;
use std::fs::Metadata;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result as IOResult;
use std::path::{Path, PathBuf};
use std::slice::ChunksExact;
use termion::color::{Blue, Fg, Reset};
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

    /// display entries by lines instead of by columns
    by_lines: bool,
}

impl From<&Args> for DisplayOptions {
    fn from(value: &Args) -> Self {
        Self {
            all: value.all,
            long: value.long,
            human_readable: value.human_readable,
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
            col_width: Vec::new(),
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
        self.path.partial_cmp(&other.path)
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

    pub fn to_string_color(&self) -> String {
        let mut s = self
            .path
            .file_name()
            .map(|s| s.to_string_lossy())
            .map(|s| s.to_string())
            .unwrap_or_default();
        if self.meta.is_dir() {
            s = format!("{}{s}/{}", Fg(Blue), Fg(Reset));
        }
        s
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

/// Determines layout of `str_paths` by descending down columns
// fn determine_layout_by_cols(term_cols: usize, str_paths: &[&str]) -> LayoutInfo {
//     const MIN_COL_SIZE: usize = 3;
//     const COL_SEP_LEN: usize = 2;
//     let max_cols = std::cmp::min(term_cols / MIN_COL_SIZE, str_paths.len());

//     let mut valid_layouts = Vec::with_capacity(max_cols);
//     for num_cols in 1..=max_cols {
//         let num_rows = str_paths.len() / num_cols;
//         let chunks = str_paths.chunks_exact(num_rows);
//         let rem = chunks.remainder();
//         let col_width = chunks
//             .into_iter()
//             .enumerate()
//             .map(|(ind, col)| {
//                 let init = if ind < rem.len() {
//                     rem[ind].len() + COL_SEP_LEN
//                 } else {
//                     MIN_COL_SIZE
//                 };
//                 col.iter()
//                     .fold(init, |acc, s| std::cmp::max(acc, s.len() + COL_SEP_LEN))
//             })
//             .collect_vec();
//         let total_width: usize = col_width.iter().sum();
//         if total_width <= term_cols {
//             valid_layouts.push(LayoutInfo::new(num_cols, col_width));
//         }
//     }
//     valid_layouts.pop().unwrap_or_default()
// }

fn col_widths_by_cols_bad(min_width: usize, num_cols: usize, lens: &[usize]) -> Vec<usize> {
    let num_rows = lens.len() / num_cols;
    let chunks = lens.chunks_exact(num_rows);
    let rem = chunks.remainder();
    chunks
        .into_iter()
        .enumerate()
        .map(|(ind, col)| {
            let init = if ind < rem.len() { rem[ind] } else { min_width };
            col.iter().fold(init, |acc, l| std::cmp::max(acc, *l))
        })
        .collect_vec()
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

/// Determines layout of `str_paths` by running across rows
// fn determine_layout_by_lines(term_cols: usize, str_paths: &[&str]) -> LayoutInfo {
//     let max_cols = std::cmp::min(term_cols / MIN_COL_SIZE, str_paths.len());

//     let mut valid_num_cols = Vec::with_capacity(max_cols);
//     let mut col_widths = Vec::with_capacity(max_cols);
//     let mut valid_layouts = Vec::with_capacity(max_cols);
//     for num_cols in 1..=max_cols {
//         let mut col_width = Vec::with_capacity(num_cols);
//         for offset in 0..num_cols {
//             let paths_in_col: Vec<_> = str_paths[offset..].iter().step_by(num_cols).collect();
//             dbg!(&paths_in_col);
//             let width = str_paths[offset..]
//                 .iter()
//                 .step_by(num_cols)
//                 .fold(MIN_COL_SIZE, |acc, s| {
//                     std::cmp::max(acc, s.len() + COL_SEP_LEN)
//                 });
//             dbg!(num_cols, offset, width);
//             col_width.push(width);
//         }
//         dbg!(&col_width);
//         let total_width: usize = col_width.iter().sum();
//         col_widths.push(col_width.clone());
//         valid_num_cols.push(if total_width <= term_cols {
//             true
//         } else {
//             false
//         });
//         if total_width <= term_cols {
//             valid_layouts.push(LayoutInfo::new(num_cols, col_width));
//         }
//     }
//     let max_cols = valid_num_cols.iter().rposition(|b| *b).unwrap_or(1);
//     valid_layouts.pop().unwrap_or_default()
// }

/// Determines a valid layout for the current terminal size and
/// the strs in `str_paths`
fn determine_layout2(by_lines: bool, term_cols: usize, str_paths: &[&str]) -> () {
    let max_cols = std::cmp::min(term_cols / MIN_COL_SIZE, str_paths.len());
    let lens = str_paths
        .iter()
        .map(|s| s.len() + COL_SEP_LEN)
        .collect_vec();

    let mut valid_num_cols = Vec::with_capacity(max_cols);
    let mut col_widths = Vec::with_capacity(max_cols);
    let mut valid_layouts = Vec::with_capacity(max_cols);
    for num_cols in 1..=max_cols {
        let num_rows = str_paths.len() / num_cols;
        let chunks = str_paths.chunks_exact(num_rows);
        let rem = chunks.remainder();
        let col_width = chunks
            .into_iter()
            .enumerate()
            .map(|(ind, col)| {
                let init = if ind < rem.len() {
                    rem[ind].len() + COL_SEP_LEN
                } else {
                    MIN_COL_SIZE
                };
                col.iter()
                    .fold(init, |acc, s| std::cmp::max(acc, s.len() + COL_SEP_LEN))
            })
            .collect_vec();
        let total_width: usize = col_width.iter().sum();
        if total_width <= term_cols {
            valid_layouts.push(LayoutInfo::new(num_cols, col_width));
        }
    }
    for num_cols in 1..=max_cols {
        let mut col_width = Vec::with_capacity(num_cols);
        for offset in 0..num_cols {
            let width = if by_lines {
                // let num_rows = str_paths.len() / num_cols;

                // let paths_in_col: Vec<_> = str_paths[offset..].iter().take()
                // dbg!(by_lines, &paths_in_col);
                todo!()
            } else {
                let paths_in_col: Vec<_> = str_paths[offset..].iter().step_by(num_cols).collect();
                dbg!(by_lines, &paths_in_col);
                str_paths[offset..]
                    .iter()
                    .step_by(num_cols)
                    .fold(MIN_COL_SIZE, |acc, s| {
                        std::cmp::max(acc, s.len() + COL_SEP_LEN)
                    })
            };
            dbg!(num_cols, by_lines, offset, width);
            col_width.push(width);
        }
        dbg!(&col_width);
        let total_width: usize = col_width.iter().sum();
        col_widths.push(col_width.clone());
        valid_num_cols.push(if total_width <= term_cols {
            true
        } else {
            false
        });
        if total_width <= term_cols {
            valid_layouts.push(LayoutInfo::new(num_cols, col_width));
        }
    }
    let max_cols = valid_num_cols.iter().rposition(|b| *b).unwrap_or(1);
    dbg!(&valid_num_cols, max_cols);
    dbg!(&valid_layouts);
}

/// Determines the layout for displaying a list of strings within the current terminal width with
/// the maximal amount of columns.
///
/// This function calculates possible layouts for the given `str_paths` based on the available terminal columns (`term_cols`)
/// and the minimum column size. It tries different numbers of columns, computes the required width for each layout,
/// and selects the most space-efficient layout that fits within the terminal.
///
/// # Parameters
/// - `by_lines`: If `true`, strings in `str_paths` are layed out across rows, otherwise down columns.
/// - `term_cols`: The total number of columns available in the terminal.
/// - `str_paths`: A slice of string references representing the items to be displayed.
///
/// # Returns
/// A `LayoutInfo` struct describing the chosen layout (number of columns and their widths). If no valid layout fits,
/// returns a default `LayoutInfo`.
fn determine_layout(by_lines: bool, term_cols: usize, str_paths: &[&str]) -> LayoutInfo {
    let max_cols = std::cmp::min(term_cols / MIN_COL_SIZE, str_paths.len());
    let lens = str_paths
        .iter()
        .map(|s| s.len() + COL_SEP_LEN)
        .collect_vec();

    let mut valid_layouts = Vec::with_capacity(max_cols);
    for num_cols in 1..=max_cols {
        let col_width = if by_lines {
            col_widths_by_lines(MIN_COL_SIZE, num_cols, &lens)
        } else {
            col_widths_by_cols(MIN_COL_SIZE, num_cols, &lens)
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

fn display_paths(opts: &DisplayOptions, term_cols: usize, paths: &[PathInfo]) {
    let string_paths = paths.iter().map(|p| p.to_string()).collect_vec();
    let str_paths = if !opts.all {
        string_paths
            .iter()
            .filter_map(|s| {
                if s.starts_with('.') {
                    None
                } else {
                    Some(s.as_str())
                }
            })
            .collect_vec()
    } else {
        string_paths.iter().map(|s| s.as_str()).collect_vec()
    };

    let layout = determine_layout(opts.by_lines, term_cols, &str_paths);
    if opts.by_lines {
        display_by_lines(&layout, &str_paths);
    } else {
        display_by_cols(&layout, &str_paths);
    }
}

fn display_by_cols(layout: &LayoutInfo, str_paths: &[&str]) {
    let num_cols = layout.num_cols;
    // the first num_rows rows will be full
    let num_rows = str_paths.len() / num_cols;
    // the final row will consist of rem columns
    let rem = str_paths.len() % num_cols;
    // dbg!(num_rows, rem, str_paths.len(), str_paths);
    // print the full rows
    for r in 0..num_rows {
        let skip = num_rows + 1;
        let end = r + skip * rem;
        let strs = str_paths[r..end].iter().step_by(skip);
        for (c, s) in strs.enumerate() {
            let indent_len = layout.col_width[c] - s.len();
            print!("{s}{}", " ".repeat(indent_len));
        }
        let skip = num_rows;
        let strs = str_paths[end..].iter().step_by(skip);
        for (c, s) in strs.enumerate() {
            // we've already done the first rem columns
            let indent_len = layout.col_width[c + rem] - s.len();
            print!("{s}{}", " ".repeat(indent_len));
        }
        println!();
    }
    // print the final partial row
    let skip = num_rows + 1;
    let strs = str_paths[num_rows..].iter().step_by(skip);
    for (c, s) in strs.enumerate() {
        let indent_len = layout.col_width[c] - s.len();
        print!(
            "{}{s}{}",
            termion::color::Fg(termion::color::Blue),
            " ".repeat(indent_len)
        );
    }
    // print the full rows
    for r in 0..num_rows {
        let mut ind = r;
        for c in 0..num_cols {
            let s = str_paths[ind];
            let skip = num_rows + if c < rem { 1 } else { 0 };
            ind += skip;
            let indent_len = layout.col_width[c].saturating_sub(s.len());
            dbg!(c, ind, s, layout.col_width[c], indent_len);
            print!("{s}{}", " ".repeat(indent_len));
        }
        println!();
    }
    // print the final partial row
    let mut ind = num_rows;
    let skip = num_rows + 1;
    for c in 0..rem {
        let s = str_paths[ind];
        ind += skip;
        let indent_len = layout.col_width[c].saturating_sub(s.len());
        dbg!(s, c, layout.col_width[c], indent_len);
        print!("{s}{}", " ".repeat(indent_len));
    }
    // // the first rem columns skip num_rows + 1 elements
    // let start = r * num_cols;
    // let end = r * num_cols + rem;
    // let skip = num_rows + 1;
    // let first_batch = str_paths[start..end].iter().step_by(skip).enumerate();
    // let print_batch = |batch| {
    //     for (ind, s) in batch {
    //         let indent_len = (layout.col_width[ind] as usize).saturating_sub((s as &str).len());
    //         dbg!(s, ind, layout.col_width[ind], indent_len);
    //         print!("{s}{}", " ".repeat(indent_len));
    //     }
    // };
    // print_batch(first_batch);
    // // the rest of the columns skip num_rows elements
    // let start = end;
    // let end = (r + 1) * num_cols;
    // let skip = num_rows;
    // let second_batch = str_paths[start..end].iter().step_by(skip).enumerate();
    // print_batch(second_batch);
    // let row_elements = str_paths[col..].iter().step_by(skip).enumerate();
    // for
    // // print the first element of the row and calculate the indent needed
    // row_elements.
    // if let Some(s) = row_elements.next() {
    //     print!("{s}");
    // }
    // // print the rest of the elements and the

    // row_elements.
    // for (ind, s) in str_paths[col..].iter().step_by(skip).enumerate() {
    //     let indent_len = layout.col_width[ind] - s.len();
    //     print!("{s}{}", " ".repeat(indent_len));
    // }
}

fn display_by_lines(layout: &LayoutInfo, str_paths: &[&str]) {
    let chunks = str_paths.chunks(layout.num_cols);
    for chunk in chunks {
        for (ind, s) in chunk.iter().enumerate() {
            let indent_len = layout.col_width[ind] - s.len();
            print!("{s}{}", " ".repeat(indent_len));
        }
        println!();
    }
}

fn display_dirs(opts: &DisplayOptions, term_cols: usize, dirs: &[PathInfo]) {
    // print files from dirs grouped if there are more than one
    if dirs.len() > 1 {
        println!("{}:", dirs[0].path.display());
    }
    for (ind, dir) in dirs[..dirs.len() - 1].iter().enumerate() {
        let children: Vec<_> = recurse_dir(&dir.path).into_iter().sorted().collect();
        display_paths(&opts, term_cols, &children);
        println!("");
        println!("{}:", dirs[ind + 1].path.display())
    }
    // print the final dir
    let children: Vec<_> = recurse_dir(&dirs[dirs.len() - 1].path)
        .into_iter()
        .sorted()
        .collect();
    display_paths(&opts, term_cols, &children);
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

    let (term_cols, _) = terminal_size()?;
    let term_cols = term_cols as usize;
    if !files.is_empty() {
        display_paths(&opts, term_cols, &files);
    }

    if !dirs.is_empty() {
        display_dirs(&opts, term_cols, &dirs);
    }

    // if dirs.len() == 1 {
    //        let mut children = recurse_dir(&dirs[0])?;
    //        children.sort();
    //     display_paths(&opts, &);
    // }
    //

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_determine_layout1() {
        let term_cols = 10;
        let str_paths = vec!["aaa", "bbb", "cc", "dd"];
        let layout_by_cols = LayoutInfo::new(2, vec![5, 4]);
        assert_eq!(
            determine_layout(false, term_cols, &str_paths),
            layout_by_cols
        );
        let layout_by_lines = LayoutInfo::new(2, vec![5, 5]);
        assert_eq!(
            determine_layout(true, term_cols, &str_paths),
            layout_by_lines
        );
    }

    #[test]
    fn test_determine_layout2() {
        let term_cols = 13;
        let str_paths = vec!["a", "b", "ccccc", "ddddd", "e"];
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
        assert_eq!(
            determine_layout(false, term_cols, &str_paths),
            layout_by_cols
        );
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
        assert_eq!(
            determine_layout(true, term_cols, &str_paths),
            layout_by_lines
        );
    }
}
