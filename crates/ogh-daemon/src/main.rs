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
    logged(ogh_daemon::run(paths))
}

fn logged(result: Result<(), String>) -> Result<(), String> {
    result.inspect_err(|message| tracing::error!("ogh-daemon stopped: {message}"))
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    use super::logged;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn log_of(result: Result<(), String>) -> (Result<(), String>, String) {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let returned = tracing::subscriber::with_default(subscriber, || logged(result));
        let text = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        (returned, text)
    }

    #[test]
    fn startup_failure_is_written_to_the_log() {
        let (returned, text) = log_of(Err("cannot bind the pipe".into()));
        assert_eq!(returned, Err("cannot bind the pipe".into()));
        assert!(text.contains("ERROR"), "{text}");
        assert!(text.contains("cannot bind the pipe"), "{text}");
    }

    #[test]
    fn clean_exit_logs_nothing() {
        let (returned, text) = log_of(Ok(()));
        assert_eq!(returned, Ok(()));
        assert_eq!(text, "");
    }
}
