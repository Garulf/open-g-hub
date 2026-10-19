use super::first_byte;
use crate::error::{ParseError, RangeError};
use crate::frame::{Message, request};

const GET: u8 = 0;
const SET: u8 = 1;
pub const MAX_PERCENT: u8 = 100;

pub fn get_request(feature_index: u8) -> Message {
    request(feature_index, GET, &[])
}

/// G Hub sends its 0 to 100 slider value unchanged.
pub fn set_request(feature_index: u8, percent: u8) -> Result<Message, RangeError> {
    RangeError::check_max("brightness", percent, MAX_PERCENT)?;
    Ok(request(feature_index, SET, &[percent]))
}

/// Brightness in percent; 100 was captured.
pub fn parse(params: &[u8]) -> Result<u8, ParseError> {
    first_byte(params)
}
