use clap::ArgAction;
use clap::Parser;
use std::fmt;
use std::fmt::Display;
use std::fs;
use std::fs::File;
use std::io::ErrorKind;
use std::io::Result as IOResult;
use std::path::{Path, PathBuf};

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

struct DisplayOptions {
    /// show hidden paths
    all: bool,

    /// use long listing format
    long: bool,

    /// use human readable sizes
    human_readable: bool,
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

fn collect_filemetas(paths: &[&Path]) -> Vec<fs::Metadata> {
    paths.iter().filter_map(|p| stat_path(p)).collect()
}

fn main() {
    let args = Args::parse();
    println!("Hello");
    let opts = DisplayOptions::from(&args);

    // default to checking the cwd
    let string_paths = if args.paths.is_none() {
        vec![".".to_string()]
    } else {
        args.paths.unwrap()
    };

    let paths: Vec<&Path> = string_paths.iter().map(|s| Path::new(s)).collect();

    let path_metas = collect_filemetas(&paths);

    for (path, meta) in paths.iter().zip(path_metas.iter()) {
        if meta.is_dir() {
            display_dir(&opts, path)?;
        }
    }
}
