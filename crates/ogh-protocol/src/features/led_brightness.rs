use super::first_byte;
use crate::error::ParseError;
use crate::frame::{Message, request};

const GET: u8 = 0;

pub fn get_request(feature_index: u8) -> Message {
    request(feature_index, GET, &[])
}

/// Brightness as the device reports it; 100 was captured.
pub fn parse(params: &[u8]) -> Result<u8, ParseError> {
    first_byte(params)
}
