use std::fmt;
use std::io;

use ogh_ipc::{ErrorKind, IpcError, TransportError};

pub const USAGE: u8 = 2;
pub const NOT_FOUND: u8 = 3;
pub const GHUB_RUNNING: u8 = 4;
pub const OTHER_FAILURE: u8 = 5;
pub const DAEMON_UNAVAILABLE: u8 = 6;

const DAEMON_NOT_RUNNING: &str = "ogh-daemon is not running; start it or enable its service";

#[derive(Debug)]
pub enum CliError {
    /// A rejected argument, found before anything was sent to the daemon.
    Usage(String),
    /// The daemon could not be reached or the connection broke.
    Transport(TransportError),
    /// There is no endpoint to connect to, such as an unset `XDG_RUNTIME_DIR`.
    Endpoint(TransportError),
    /// The daemon answered with an error.
    Daemon(IpcError),
    /// A failure on our side of the pipe, such as a closed stdout.
    Local(String),
}

impl CliError {
    /// 2 usage, 3 device not found, 4 G Hub running, 5 anything else, 6 no usable daemon.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => USAGE,
            Self::Transport(TransportError::DaemonNotRunning)
            | Self::Transport(TransportError::VersionMismatch { .. })
            | Self::Endpoint(_) => DAEMON_UNAVAILABLE,
            Self::Daemon(error) => match error.kind {
                ErrorKind::NotConnected => NOT_FOUND,
                ErrorKind::GhubRunning => GHUB_RUNNING,
                _ => OTHER_FAILURE,
            },
            Self::Transport(_) | Self::Local(_) => OTHER_FAILURE,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) | Self::Local(message) => write!(f, "{message}"),
            Self::Transport(TransportError::DaemonNotRunning) => write!(f, "{DAEMON_NOT_RUNNING}"),
            Self::Transport(error) => write!(f, "{error}"),
            Self::Endpoint(error) => write!(f, "{DAEMON_NOT_RUNNING} ({error})"),
            Self::Daemon(error) => {
                write!(f, "{}", error.message)?;
                if cfg!(target_os = "linux") && is_access_denied(&error.message) {
                    write!(
                        f,
                        "; on Linux this usually means no permission for /dev/hidraw*: \
                         install a udev rule that gives your user access to USB 046d:0b0b, \
                         then replug the base station"
                    )?;
                }
                Ok(())
            }
        }
    }
}

fn is_access_denied(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("access denied") || message.contains("permission denied")
}

impl std::error::Error for CliError {}

impl From<TransportError> for CliError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}

impl From<IpcError> for CliError {
    fn from(error: IpcError) -> Self {
        Self::Daemon(error)
    }
}

impl From<io::Error> for CliError {
    fn from(error: io::Error) -> Self {
        Self::Local(format!("cannot write output: {error}"))
    }
}
