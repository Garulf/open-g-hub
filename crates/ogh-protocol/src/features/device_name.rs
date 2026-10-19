use super::length_prefixed_str;
use crate::error::ParseError;
use crate::frame::{Message, request};

const GET_NAME: u8 = 0;
const NAME_LEN_AT: usize = 1;

pub fn get_name_request(feature_index: u8) -> Message {
    request(feature_index, GET_NAME, &[])
}

/// Reply layout: maximum length (30), length, name.
pub fn parse_name(params: &[u8]) -> Result<String, ParseError> {
    length_prefixed_str(params, NAME_LEN_AT)
}
