use clap::Parser;
use std::fmt;
use std::fs;
use std::fs::File;
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

fn print_error_msg(what: &str, why: &str) {
    eprintln!("{PROGRAM}: {what}: {why}");
}

fn check_path(path: &Path) -> bool {
    match   fs::metadata(path) {
        Ok(_) => todo!(),
        Err(_) => todo!(),
    }
}

fn collect_filebufs(paths: &[&str]) -> Vec<PathBuf> {
    let paths = paths.iter().map(|p| Path::new(p));
    let paths = paths.filter_map(|p| {
        match p.ex
    })
    let mut res = Vec::new();
    for p in paths {
        match std::path::exists(p) {
            Ok(b) if !b => {
                print_error_msg(&format!("cannot access '{p}'"), ERR_NO_SUCH_FILE_OR_DIR);
            }
            Ok(b) => res.push(PathBuf::from(p)),
            Err(e) => match e {},
        }
    }

    res
}

fn main() {
    let args = Args::parse();
}
