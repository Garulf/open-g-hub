use crate::error::{ParseError, RangeError, require_len};
use crate::frame::{Message, request};

const GET: u8 = 0;
const SET: u8 = 1;
const INFO: u8 = 2;
const MAX_LEVEL: u8 = 6;
const LEVEL_AT: usize = 3;
const MIC_ID: u8 = 0x01;
const UNKNOWN_BYTE: u8 = 0xff;

/// Device level 0 to 6. Parsed values are not range-checked; the set builder
/// is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sidetone(pub u8);

impl Sidetone {
    /// Rounds `percent * 6 / 100` to the nearest level. G Hub was seen
    /// mapping 68 to 4, 100 to 6, 51 to 3 and 0 to 0; the formula is a guess
    /// for every other percentage. Values above 100 count as 100.
    pub fn from_percent(percent: u8) -> Self {
        let percent = u16::from(percent.min(100));
        let steps = u16::from(MAX_LEVEL);
        Self(((percent * steps + 50) / 100) as u8)
    }

    /// Inverse of `from_percent`, rounded to the nearest percent. A guess
    /// beyond the captured points.
    pub fn to_percent(self) -> u8 {
        let steps = u16::from(MAX_LEVEL);
        let level = u16::from(self.0.min(MAX_LEVEL));
        ((level * 100 + steps / 2) / steps) as u8
    }
}

/// Guessed reading of `ee 00 07`: minimum and maximum in dB, then the number
/// of steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SidetoneInfo {
    pub min_db: i8,
    pub max_db: i8,
    pub steps: u8,
}

pub fn get_request(feature_index: u8) -> Message {
    request(feature_index, GET, &[])
}

pub fn set_request(feature_index: u8, level: Sidetone) -> Result<Message, RangeError> {
    if level.0 > MAX_LEVEL {
        return Err(RangeError {
            value: level.0,
            max: MAX_LEVEL,
        });
    }
    Ok(request(
        feature_index,
        SET,
        &[MIC_ID, UNKNOWN_BYTE, level.0],
    ))
}

pub fn info_request(feature_index: u8) -> Message {
    request(feature_index, INFO, &[])
}

/// Parses a fn0 reply or a sidetone event: `01 01 ff <level>`. The guessed
/// meaning of the leading bytes is entry count and microphone id; the `ff`
/// is unknown. Only the level is read.
pub fn parse_level(params: &[u8]) -> Result<Sidetone, ParseError> {
    require_len(params, LEVEL_AT + 1)?;
    Ok(Sidetone(params[LEVEL_AT]))
}

pub fn parse_info(params: &[u8]) -> Result<SidetoneInfo, ParseError> {
    require_len(params, 3)?;
    Ok(SidetoneInfo {
        min_db: params[0] as i8,
        max_db: params[1] as i8,
        steps: params[2],
    })
}
