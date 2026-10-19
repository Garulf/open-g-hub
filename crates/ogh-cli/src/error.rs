use std::fmt;
use std::io;

use ogh_device::DeviceError;

#[derive(Debug)]
pub enum CliError {
    /// A failure after the device was open.
    Device(DeviceError),
    /// A failure while finding, guarding or opening the device.
    Opening(DeviceError),
    /// A failure on our side of the pipe, such as a closed stdout.
    Local(String),
}

const NOT_FOUND: u8 = 3;
const GHUB_RUNNING: u8 = 4;
const OTHER_FAILURE: u8 = 5;

impl CliError {
    /// 3 device not found, 4 G Hub running, 5 anything else. Usage errors
    /// (2) are reported by argument parsing before a session exists.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Device(DeviceError::NotFound) | Self::Opening(DeviceError::NotFound) => NOT_FOUND,
            Self::Device(DeviceError::GHubRunning) | Self::Opening(DeviceError::GHubRunning) => {
                GHUB_RUNNING
            }
            _ => OTHER_FAILURE,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Device(error) => write!(f, "{error}"),
            Self::Opening(error @ DeviceError::Io(_)) => {
                write!(f, "{error}")?;
                if cfg!(target_os = "linux") {
                    write!(
                        f,
                        "; on Linux this usually means no permission for /dev/hidraw*: \
                         install a udev rule that gives your user access to USB 046d:0b0b, \
                         then replug the base station"
                    )?;
                }
                Ok(())
            }
            Self::Opening(error) => write!(f, "{error}"),
            Self::Local(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<DeviceError> for CliError {
    fn from(error: DeviceError) -> Self {
        Self::Device(error)
    }
}

impl From<io::Error> for CliError {
    fn from(error: io::Error) -> Self {
        Self::Local(format!("cannot write output: {error}"))
    }
}
