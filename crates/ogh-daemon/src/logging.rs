use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing_subscriber::EnvFilter;

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
