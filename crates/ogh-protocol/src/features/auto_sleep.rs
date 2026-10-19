use crate::error::{ParseError, require_len};
use crate::frame::{Message, request};

const GET: u8 = 0;

/// Both bytes are guessed to be minutes (60 and 15 were captured). Which one
/// is the shallow sleep timer and which the deep one is unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoSleep {
    pub first_minutes: u8,
    pub second_minutes: u8,
}

pub fn get_request(feature_index: u8) -> Message {
    request(feature_index, GET, &[])
}

pub fn parse(params: &[u8]) -> Result<AutoSleep, ParseError> {
    require_len(params, 2)?;
    Ok(AutoSleep {
        first_minutes: params[0],
        second_minutes: params[1],
    })
}
