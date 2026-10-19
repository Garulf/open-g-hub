use super::first_byte;
use crate::error::ParseError;
use crate::frame::{Message, request};

const GET_CONNECTED_COUNT: u8 = 0;

/// Guessed meaning of fn0: getConnectedDeviceNum, read from the reply
/// content. The name and address calls are not modelled: their replies are
/// personal data.
pub fn connected_count_request(feature_index: u8) -> Message {
    request(feature_index, GET_CONNECTED_COUNT, &[])
}

/// Parses a fn0 reply or a Bluetooth event.
pub fn parse_connected_count(params: &[u8]) -> Result<u8, ParseError> {
    first_byte(params)
}
