use super::length_prefixed_str;
use crate::error::{ParseError, require_len};
use crate::frame::{Message, request};

const GET_FIRMWARE_VERSION: u8 = 1;
const NAME_LEN_AT: usize = 4;

/// Guessed reading of `01 02 00 12 04 "main" 00`: major, minor, a big-endian
/// u16 build, then a length-prefixed name. The final `00` is ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirmwareVersion {
    pub major: u8,
    pub minor: u8,
    pub build: u16,
    pub name: String,
}

pub fn firmware_version_request(feature_index: u8, entity: u8) -> Message {
    request(feature_index, GET_FIRMWARE_VERSION, &[entity])
}

pub fn parse_firmware_version(params: &[u8]) -> Result<FirmwareVersion, ParseError> {
    require_len(params, NAME_LEN_AT)?;
    Ok(FirmwareVersion {
        major: params[0],
        minor: params[1],
        build: u16::from_be_bytes([params[2], params[3]]),
        name: length_prefixed_str(params, NAME_LEN_AT)?,
    })
}
