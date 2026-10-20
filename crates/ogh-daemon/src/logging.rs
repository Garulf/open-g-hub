use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing_subscriber::EnvFilter;

pub const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;

pub enum Destination {
    Stderr,
    File(PathBuf),
}

/// `RUST_LOG` is honoured, `info` is the default.
pub fn init(destination: Destination) -> Result<(), String> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    let result = match destination {
        Destination::Stderr => builder.with_writer(std::io::stderr).try_init(),
        Destination::File(path) => {
            let _ = rotate_if_large(&path, LOG_ROTATE_BYTES);
            let file = open_log_file(&path)?;
            builder
                .with_ansi(false)
                .with_writer(Mutex::new(file))
                .try_init()
        }
    };
    result.map_err(|e| format!("cannot start logging: {e}"))
}

fn open_log_file(path: &Path) -> Result<std::fs::File, String> {
    if let Some(parent) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("cannot open log file {}: {e}", path.display()))
}

/// Keeps one previous log. A log that does not exist yet is not an error.
pub fn rotate_if_large(path: &Path, limit_bytes: u64) -> io::Result<()> {
    match fs::metadata(path) {
        Ok(meta) if meta.len() > limit_bytes => {
            let mut rotated = path.as_os_str().to_owned();
            rotated.push(".1");
            fs::rename(path, rotated)
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
