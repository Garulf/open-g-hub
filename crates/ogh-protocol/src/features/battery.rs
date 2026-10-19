use crate::error::{ParseError, require_len};
use crate::frame::{Message, request};

const GET: u8 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub level_a: u8,
    pub level_b: u8,
    pub status: ChargeStatus,
}

impl Battery {
    /// Returns `level_a`. Both bytes were 100 in every capture, so which of
    /// the two is the percentage is unverified.
    pub fn percent(&self) -> u8 {
        self.level_a
    }
}

/// The names are guesses from the order of events: 0 after a lift, 1 after
/// docking, 3 in reads after a long dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChargeStatus {
    Discharging,
    Charging,
    Full,
    Other(u8),
}

impl ChargeStatus {
    pub fn from_byte(byte: u8) -> Self {
        match byte {
            0 => Self::Discharging,
            1 => Self::Charging,
            3 => Self::Full,
            other => Self::Other(other),
        }
    }

    pub fn to_byte(self) -> u8 {
        match self {
            Self::Discharging => 0,
            Self::Charging => 1,
            Self::Full => 3,
            Self::Other(byte) => byte,
        }
    }
}

pub fn get_request(feature_index: u8) -> Message {
    request(feature_index, GET, &[])
}

/// Parses a fn0 reply or a battery event: `<a> <b> <status>`.
pub fn parse(params: &[u8]) -> Result<Battery, ParseError> {
    require_len(params, 3)?;
    Ok(Battery {
        level_a: params[0],
        level_b: params[1],
        status: ChargeStatus::from_byte(params[2]),
    })
}
