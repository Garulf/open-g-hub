use hidapi::{DeviceInfo, HidApi, HidDevice};
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
        let api = HidApi::new().map_err(|e| DeviceError::Io(e.to_string()))?;
        let info = control_interface(api.device_list()).ok_or(DeviceError::NotFound)?;
        let device = info
            .open_device(&api)
            .map_err(|e| DeviceError::Io(format!("cannot open the base station: {e}")))?;
        Ok(Self { device, _api: api })
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
        let len = self
            .device
            .read_timeout(&mut buf, timeout_ms)
            .map_err(|_| DeviceError::Disconnected)?;
        Ok((len > 0).then(|| with_report_id(&buf[..len])))
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
