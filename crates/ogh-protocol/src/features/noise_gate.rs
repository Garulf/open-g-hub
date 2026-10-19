use super::first_byte;
use crate::error::{ParseError, require_len};
use crate::frame::{Message, request};

const GET_CURRENT: u8 = 1;
const GET_OPTIONS: u8 = 3;

/// The values match the ordinals G Hub uses for its noise gate modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseGate {
    Off = 0,
    Home = 1,
    Night = 2,
    Tournament = 4,
}

impl NoiseGate {
    pub fn from_byte(byte: u8) -> Result<Self, ParseError> {
        match byte {
            0 => Ok(Self::Off),
            1 => Ok(Self::Home),
            2 => Ok(Self::Night),
            4 => Ok(Self::Tournament),
            other => Err(ParseError::UnknownValue(other)),
        }
    }
}

pub fn options_request(feature_index: u8) -> Message {
    request(feature_index, GET_OPTIONS, &[])
}

pub fn current_request(feature_index: u8) -> Message {
    request(feature_index, GET_CURRENT, &[])
}

/// Reply layout: option count, then one byte per option.
pub fn parse_options(params: &[u8]) -> Result<Vec<NoiseGate>, ParseError> {
    let count = usize::from(first_byte(params)?);
    require_len(params, 1 + count)?;
    params[1..1 + count]
        .iter()
        .map(|&byte| NoiseGate::from_byte(byte))
        .collect()
}

pub fn parse_current(params: &[u8]) -> Result<NoiseGate, ParseError> {
    NoiseGate::from_byte(first_byte(params)?)
}
