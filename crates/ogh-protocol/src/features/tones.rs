use super::first_byte;
use crate::error::{ParseError, RangeError};
use crate::frame::{Message, request};

const GET: u8 = 0;
const SET: u8 = 1;
const SETTABLE: &[u8] = &[0, 1, 2];

/// The Tones setting (G Hub's prompt frequency). G Hub offered three
/// options and wrote 0, 1 and 2; only 1 is known to be "All". Parsed values
/// are not checked; the set builder is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tones(pub u8);

impl Tones {
    pub const ALL: Self = Self(1);
}

pub fn get_request(feature_index: u8) -> Message {
    request(feature_index, GET, &[])
}

pub fn set_request(feature_index: u8, tones: Tones) -> Result<Message, RangeError> {
    RangeError::check_one_of("tones", tones.0, SETTABLE)?;
    Ok(request(feature_index, SET, &[tones.0]))
}

/// Parses a fn0 reply or a tones event.
pub fn parse(params: &[u8]) -> Result<Tones, ParseError> {
    first_byte(params).map(Tones)
}
