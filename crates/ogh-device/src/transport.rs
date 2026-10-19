use std::io::ErrorKind;

use hidapi::{DeviceInfo, HidApi, HidDevice, HidError};
use ogh_protocol::FRAME_LEN;

use crate::error::DeviceError;

const VENDOR_ID: u16 = 0x046d;
const PRODUCT_ID: u16 = 0x0b0b;
const USAGE_PAGE: u16 = 0xff32;
const USAGE: u16 = 0x0074;
const INTERFACE: i32 = 8;
const REPORT_ID: u8 = 0x02;

/// One full 64-byte frame each way, report id first.
pub trait Transport {
    fn write(&mut self, frame: &[u8; FRAME_LEN]) -> Result<(), DeviceError>;
    /// `Ok(None)` when nothing arrived within `timeout_ms`.
    fn read(&mut self, timeout_ms: i32) -> Result<Option<[u8; FRAME_LEN]>, DeviceError>;
}

/// The A50 X control interface through hidapi. Only `Session::open` can
/// open one, so every write to the real device passes the session's gate.
pub struct HidTransport {
    device: HidDevice,
    _api: HidApi,
}

impl HidTransport {
    pub(crate) fn open() -> Result<Self, DeviceError> {
        let api = HidApi::new().map_err(open_error)?;
        let info = control_interface(api.device_list()).ok_or(DeviceError::NotFound)?;
        let device = info.open_device(&api).map_err(open_error)?;
        Ok(Self { device, _api: api })
    }
}

fn open_error(error: HidError) -> DeviceError {
    let detail = format!("cannot open the base station: {error}");
    match error {
        HidError::IoError { error } if error.kind() == ErrorKind::PermissionDenied => {
            DeviceError::AccessDenied(detail)
        }
        _ => DeviceError::Io(detail),
    }
}

/// Usage page and usage when the platform reports them, else interface 8.
fn control_interface<'a>(devices: impl Iterator<Item = &'a DeviceInfo>) -> Option<&'a DeviceInfo> {
    let candidates: Vec<&DeviceInfo> = devices
        .filter(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
        .collect();
    candidates
        .iter()
        .find(|d| d.usage_page() == USAGE_PAGE && d.usage() == USAGE)
        .or_else(|| {
            candidates
                .iter()
                .find(|d| d.interface_number() == INTERFACE)
        })
        .copied()
}

impl Transport for HidTransport {
    fn write(&mut self, frame: &[u8; FRAME_LEN]) -> Result<(), DeviceError> {
        self.device
            .write(frame)
            .map(|_| ())
            .map_err(|_| DeviceError::Disconnected)
    }

    fn read(&mut self, timeout_ms: i32) -> Result<Option<[u8; FRAME_LEN]>, DeviceError> {
        let mut buf = [0u8; FRAME_LEN + 1];
        let len = read_length(self.device.read_timeout(&mut buf, timeout_ms))?;
        Ok((len > 0).then(|| with_report_id(&buf[..len])))
    }
}

/// A signal that interrupts the blocking read (Ctrl-C) is no data, not a
/// lost device.
fn read_length(result: Result<usize, HidError>) -> Result<usize, DeviceError> {
    match result {
        Ok(len) => Ok(len),
        Err(HidError::IoError { error }) if error.kind() == ErrorKind::Interrupted => Ok(0),
        Err(_) => Err(DeviceError::Disconnected),
    }
}

/// Some backends hand over the report without its id byte.
fn with_report_id(report: &[u8]) -> [u8; FRAME_LEN] {
    let mut frame = [0u8; FRAME_LEN];
    if report[0] == REPORT_ID {
        let len = report.len().min(FRAME_LEN);
        frame[..len].copy_from_slice(&report[..len]);
    } else {
        frame[0] = REPORT_ID;
        let len = report.len().min(FRAME_LEN - 1);
        frame[1..=len].copy_from_slice(&report[..len]);
    }
    frame
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    fn io_error(kind: ErrorKind) -> HidError {
        HidError::IoError {
            error: io::Error::from(kind),
        }
    }

    #[test]
    fn read_that_a_signal_interrupted_is_no_data() {
        assert_eq!(read_length(Err(io_error(ErrorKind::Interrupted))), Ok(0));
    }

    #[test]
    fn read_that_got_data_returns_its_length() {
        assert_eq!(read_length(Ok(64)), Ok(64));
        assert_eq!(read_length(Ok(0)), Ok(0));
    }

    #[test]
    fn other_read_errors_mean_the_device_is_gone() {
        for error in [
            io_error(ErrorKind::BrokenPipe),
            io_error(ErrorKind::PermissionDenied),
            HidError::HidApiError {
                message: "unexpected poll error (device disconnected)".into(),
            },
        ] {
            assert_eq!(read_length(Err(error)), Err(DeviceError::Disconnected));
        }
    }

    #[test]
    fn permission_denied_is_told_apart_from_other_open_errors() {
        assert!(matches!(
            open_error(io_error(ErrorKind::PermissionDenied)),
            DeviceError::AccessDenied(_)
        ));
        assert!(matches!(
            open_error(io_error(ErrorKind::NotFound)),
            DeviceError::Io(_)
        ));
        assert!(matches!(
            open_error(HidError::HidApiError {
                message: "init failed".into()
            }),
            DeviceError::Io(_)
        ));
    }
}
