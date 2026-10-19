use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::Path;

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
        let api = HidApi::new().map_err(|e| open_error(e, || false))?;
        let info = control_interface(api.device_list()).ok_or(DeviceError::NotFound)?;
        let device = info.open_device(&api).map_err(|e| {
            open_error(e, || {
                info.path()
                    .to_str()
                    .is_ok_and(|path| os_denies_access(Path::new(path)))
            })
        })?;
        Ok(Self { device, _api: api })
    }
}

/// hidapi's Linux backend reports a refused open as a message string with
/// no error kind, so the system is asked about `path` itself. Only an
/// `IoError` that carries the kind (other backends) is trusted directly.
fn open_error(error: HidError, path_is_denied: impl FnOnce() -> bool) -> DeviceError {
    let detail = format!("cannot open the base station: {error}");
    let denied_by_kind = matches!(
        &error,
        HidError::IoError { error } if error.kind() == ErrorKind::PermissionDenied
    );
    if denied_by_kind || path_is_denied() {
        DeviceError::AccessDenied(detail)
    } else {
        DeviceError::Io(detail)
    }
}

fn os_denies_access(path: &Path) -> bool {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .is_err_and(|e| e.kind() == ErrorKind::PermissionDenied)
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
    fn an_io_error_of_kind_permission_denied_is_access_denied() {
        assert!(matches!(
            open_error(io_error(ErrorKind::PermissionDenied), || false),
            DeviceError::AccessDenied(_)
        ));
    }

    #[test]
    fn a_hidapi_open_message_is_access_denied_only_when_the_path_is() {
        let message =
            "failed to open device with path /dev/hidraw3: Permission denied (os error 13)";
        let refused = || HidError::HidApiError {
            message: message.into(),
        };

        assert!(matches!(
            open_error(refused(), || true),
            DeviceError::AccessDenied(_)
        ));
        assert!(matches!(
            open_error(refused(), || false),
            DeviceError::Io(_)
        ));
    }

    #[test]
    fn other_open_errors_stay_io() {
        assert!(matches!(
            open_error(io_error(ErrorKind::NotFound), || false),
            DeviceError::Io(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn os_denies_access_to_a_file_the_user_cannot_open() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("ogh-access-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let locked = dir.join("locked");
        let open = dir.join("open");
        fs::write(&locked, b"").unwrap();
        fs::write(&open, b"").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let running_as_root = fs::File::open(&locked).is_ok();

        let locked_denied = os_denies_access(&locked);
        let open_denied = os_denies_access(&open);
        let missing_denied = os_denies_access(&dir.join("missing"));
        fs::remove_dir_all(&dir).unwrap();

        if !running_as_root {
            assert!(locked_denied);
        }
        assert!(!open_denied);
        assert!(!missing_denied);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn hidapi_refusing_a_locked_path_is_access_denied() {
        use std::ffi::CString;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let locked = std::env::temp_dir().join(format!("ogh-hidapi-{}", std::process::id()));
        fs::write(&locked, b"").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let running_as_root = fs::File::open(&locked).is_ok();
        let api = HidApi::new().unwrap();
        let path = CString::new(locked.to_str().unwrap()).unwrap();

        let refusal = api.open_path(&path).err();
        let mapped = refusal.map(|e| {
            open_error(e, || {
                path.to_str().is_ok_and(|p| os_denies_access(Path::new(p)))
            })
        });
        fs::remove_file(&locked).unwrap();

        if !running_as_root {
            assert!(
                matches!(mapped, Some(DeviceError::AccessDenied(_))),
                "{mapped:?}"
            );
        }
    }
}
