use clap::Parser;
use std::fmt;
use std::fmt::Display;
use std::fs;
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const PROGRAM: &str = "rusl";
const ERR_NO_SUCH_FILE_OR_DIR: &str = "No such file or directory";
const ERR_PERM_DENIED: &str = "Permission denied";

#[derive(Debug, Parser)]
struct Args {
    /// filepaths to process
    paths: Vec<String>,

    /// show hidden paths
    #[arg(short, long, default_value_t = false)]
    all: bool,

    /// use long listing format
    #[arg(short, default_value_t = false)]
    long: bool,

    /// use human readable sizes
    #[arg(short, long, default_value_t = false)]
    human_readable: bool,
}

struct DisplayOptions {
    /// show hidden paths
    all: bool,

    /// use long listing format
    long: bool,

    /// use human readable sizes
    human_readable: bool,
}

fn display_dir(opts: &DisplayOptions, dir: &Path) {
    let entries = dir.read_dir().expect("{dir} is not a directory: ");
    for p in entries {
        if p.is_ok() {
            println!("{}", p.unwrap().path().display())
        }
    }
}

fn print_error_msg(what: &str, why: &str) {
    eprintln!("{PROGRAM}: {what}: {why}");
}

fn stat_path(path: &Path) -> Option<fs::Metadata> {
    match fs::metadata(path) {
        Ok(meta) => Some(meta),
        Err(err) => {
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
            None
        }
    }
}

fn collect_filestats(paths: &[&str]) -> Vec<fs::Metadata> {
    paths
        .iter()
        .map(|p| Path::new(p))
        .filter_map(|p| stat_path(p))
        .collect()
}

fn main() {
    let args = Args::parse();
}
