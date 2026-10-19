use super::{level_to_percent, percent_to_level};
use crate::error::{ParseError, RangeError, require_len};
use crate::frame::{Message, request};

const GET: u8 = 0;
const SET_LEVEL: u8 = 1;
/// G Hub puts `ff` before the level on feature version 4 and later.
const SET_LEVEL_PREFIX: u8 = 0xff;
const MUTE_BIT: u8 = 0x01;

/// Levels run 0 to 21: getInfo (fn3) reports 22 steps.
pub const MAX_LEVEL: u8 = 21;

/// Parsed levels are not range-checked; the set builder is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeLevel(pub u8);

impl VolumeLevel {
    /// `roundf(21 * percent / 100)`, as G Hub maps its Device Volume slider:
    /// 0, 50 and 100 were captured as 0, 11 and 21. Other percentages follow
    /// the formula found in the G Hub agent, unverified. Values above 100
    /// count as 100.
    pub fn from_percent(percent: u8) -> Self {
        Self(percent_to_level(percent, MAX_LEVEL))
    }

    /// Inverse of `from_percent`, rounded to the nearest percent. A guess
    /// beyond the captured points.
    pub fn to_percent(self) -> u8 {
        level_to_percent(self.0, MAX_LEVEL)
    }
}

/// The fn0 reply and event: `<scaled> <mute> <level>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volume {
    /// Tracks `level` on a 0 to 31 scale (guessed); G Hub ignores it.
    pub scaled: u8,
    /// Bit 0 of the middle byte according to the G Hub agent. It was 0 in
    /// every capture.
    pub muted: bool,
    pub level: VolumeLevel,
}

pub fn get_request(feature_index: u8) -> Message {
    request(feature_index, GET, &[])
}

pub fn set_request(feature_index: u8, level: VolumeLevel) -> Result<Message, RangeError> {
    RangeError::check_max("volume level", level.0, MAX_LEVEL)?;
    Ok(request(
        feature_index,
        SET_LEVEL,
        &[SET_LEVEL_PREFIX, level.0],
    ))
}

/// Parses a fn0 reply or a volume event.
pub fn parse(params: &[u8]) -> Result<Volume, ParseError> {
    require_len(params, 3)?;
    Ok(Volume {
        scaled: params[0],
        muted: params[1] & MUTE_BIT != 0,
        level: VolumeLevel(params[2]),
    })
}
