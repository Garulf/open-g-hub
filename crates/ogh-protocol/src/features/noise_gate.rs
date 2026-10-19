use super::first_byte;
use crate::error::{Allowed, ParseError, RangeError, require_len};
use crate::frame::{Message, request};

const GET_CURRENT: u8 = 1;
const SET: u8 = 2;
const GET_OPTIONS: u8 = 3;
const SETTABLE: &[u8] = &[0, 1, 2, 4];

/// The bytes match the ordinals G Hub uses for its noise gate modes. A byte
/// the device reports outside those four is kept as `Other`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseGate {
    Off,
    Home,
    Night,
    Tournament,
    Other(u8),
}

impl NoiseGate {
    pub fn from_byte(byte: u8) -> Self {
        match byte {
            0 => Self::Off,
            1 => Self::Home,
            2 => Self::Night,
            4 => Self::Tournament,
            other => Self::Other(other),
        }
    }

    pub fn to_byte(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Home => 1,
            Self::Night => 2,
            Self::Tournament => 4,
            Self::Other(byte) => byte,
        }
    }
}

/// Guessed meaning of fn3: getNoiseGateInfo, read from the reply content.
pub fn options_request(feature_index: u8) -> Message {
    request(feature_index, GET_OPTIONS, &[])
}

/// Guessed meaning of fn1: getNoiseGate, read from the reply content.
pub fn current_request(feature_index: u8) -> Message {
    request(feature_index, GET_CURRENT, &[])
}

/// Only the four modes G Hub offers can be set; `Other` is refused, even
/// when it carries one of their bytes.
pub fn set_request(feature_index: u8, gate: NoiseGate) -> Result<Message, RangeError> {
    if let NoiseGate::Other(byte) = gate {
        return Err(RangeError {
            what: "noise gate",
            value: f32::from(byte),
            allowed: Allowed::OneOf(SETTABLE),
        });
    }
    Ok(request(feature_index, SET, &[gate.to_byte()]))
}

/// Reply layout: option count, then one byte per option.
pub fn parse_options(params: &[u8]) -> Result<Vec<NoiseGate>, ParseError> {
    let count = usize::from(first_byte(params)?);
    require_len(params, 1 + count)?;
    Ok(params[1..1 + count]
        .iter()
        .map(|&byte| NoiseGate::from_byte(byte))
        .collect())
}

/// Parses a fn1 reply or a fn0 noise gate event.
pub fn parse_current(params: &[u8]) -> Result<NoiseGate, ParseError> {
    first_byte(params).map(NoiseGate::from_byte)
}
