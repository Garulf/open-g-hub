#![cfg_attr(windows, windows_subsystem = "windows")]

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use ogh_daemon::logging::{self, Destination};
use ogh_daemon::paths::Paths;

const USAGE: &str = "usage: ogh-daemon [--log-file <path>] [--config-dir <dir>]";

#[derive(Debug, Default, PartialEq)]
struct Args {
    log_file: Option<PathBuf>,
    config_dir: Option<PathBuf>,
}

fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args;
    while let Some(flag) = args.next() {
        let slot = match flag.to_str() {
            Some("--log-file") => &mut parsed.log_file,
            Some("--config-dir") => &mut parsed.config_dir,
            _ => return Err(format!("unexpected argument {}", flag.to_string_lossy())),
        };
        let value = args
            .next()
            .ok_or_else(|| format!("{} needs a value", flag.to_string_lossy()))?;
        *slot = Some(PathBuf::from(value));
    }
    Ok(parsed)
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match start(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn start(args: Args) -> Result<(), String> {
    let paths = match &args.config_dir {
        Some(dir) => Paths::in_dir(dir),
        None => Paths::for_current_user()?,
    };
    let destination = match args.log_file {
        Some(path) => Destination::File(path),
        None if cfg!(windows) => Destination::File(paths.log_file.clone()),
        None => Destination::Stderr,
    };
    logging::init(destination)?;
    ogh_daemon::run(paths)
}
