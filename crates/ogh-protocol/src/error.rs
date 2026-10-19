use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    TooLong { len: usize, max: usize },
    WrongPrefix,
    BadFrameLength(usize),
    BadPayloadLength(u8),
    UnknownFlags(u8),
    FieldTooWide,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { len, max } => write!(f, "{len} param bytes exceed the {max} that fit"),
            Self::WrongPrefix => write!(f, "frame does not start with the expected prefix"),
            Self::BadFrameLength(len) => write!(f, "frame is {len} bytes, expected 64"),
            Self::BadPayloadLength(len) => {
                write!(f, "length byte {len:#04x} is inconsistent with the frame")
            }
            Self::UnknownFlags(flags) => write!(f, "unknown fragment flags {flags:#04x}"),
            Self::FieldTooWide => write!(f, "function and sw_id must each fit in four bits"),
        }
    }
}

impl std::error::Error for FrameError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    TooShort { expected: usize, actual: usize },
    IndexOverflow,
    InvalidUtf8,
    UnknownValue(u8),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { expected, actual } => {
                write!(
                    f,
                    "reply has {actual} param bytes, expected at least {expected}"
                )
            }
            Self::IndexOverflow => write!(f, "feature page runs past index 255"),
            Self::InvalidUtf8 => write!(f, "text field is not valid UTF-8"),
            Self::UnknownValue(value) => write!(f, "unknown value {value:#04x}"),
        }
    }
}

impl std::error::Error for ParseError {}

pub(crate) fn require_len(params: &[u8], expected: usize) -> Result<(), ParseError> {
    if params.len() < expected {
        return Err(ParseError::TooShort {
            expected,
            actual: params.len(),
        });
    }
    Ok(())
}

/// A value outside what the device was seen accepting, caught before
/// anything is sent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeError {
    /// The setting and its unit, for example "EQ gain (dB)".
    pub what: &'static str,
    pub value: f32,
    pub allowed: Allowed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Allowed {
    /// Inclusive bounds.
    Between {
        min: f32,
        max: f32,
    },
    OneOf(&'static [u8]),
}

impl RangeError {
    pub(crate) fn check_max(what: &'static str, value: u8, max: u8) -> Result<(), Self> {
        Self::check_between(what, f32::from(value), 0.0, f32::from(max))
    }

    /// NaN is outside every range.
    pub(crate) fn check_between(
        what: &'static str,
        value: f32,
        min: f32,
        max: f32,
    ) -> Result<(), Self> {
        if (min..=max).contains(&value) {
            return Ok(());
        }
        Err(Self {
            what,
            value,
            allowed: Allowed::Between { min, max },
        })
    }

    pub(crate) fn check_one_of(
        what: &'static str,
        value: u8,
        allowed: &'static [u8],
    ) -> Result<(), Self> {
        if allowed.contains(&value) {
            return Ok(());
        }
        Err(Self {
            what,
            value: f32::from(value),
            allowed: Allowed::OneOf(allowed),
        })
    }
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { what, value, .. } = self;
        match self.allowed {
            Allowed::Between { max, .. } if *value > max => {
                write!(f, "{what} {value} is above the maximum of {max}")
            }
            Allowed::Between { min, .. } if *value < min => {
                write!(f, "{what} {value} is below the minimum of {min}")
            }
            Allowed::Between { min, max } => write!(f, "{what} {value} is outside {min} to {max}"),
            Allowed::OneOf(allowed) => {
                let list: Vec<String> = allowed.iter().map(u8::to_string).collect();
                write!(f, "{what} {value} is not one of {}", list.join(", "))
            }
        }
    }
}

impl std::error::Error for RangeError {}

/// Error codes named in the G Hub agent, in the agent's order. The numeric
/// values are assumed to be the position in that list; no error frame was
/// ever captured from this device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CentppError {
    NoError,
    InvalidFeature,
    InvalidFunctionId,
    InvalidSize,
    InvalidSwid,
    InvalidParam,
    Timeout,
    LowResource,
    NotConnected,
    Busy,
    NotAvailable,
    Failed,
    Malformed,
    NotAuthorized,
    UnknownError,
}

impl CentppError {
    const ALL: [Self; 15] = [
        Self::NoError,
        Self::InvalidFeature,
        Self::InvalidFunctionId,
        Self::InvalidSize,
        Self::InvalidSwid,
        Self::InvalidParam,
        Self::Timeout,
        Self::LowResource,
        Self::NotConnected,
        Self::Busy,
        Self::NotAvailable,
        Self::Failed,
        Self::Malformed,
        Self::NotAuthorized,
        Self::UnknownError,
    ];

    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.get(usize::from(code)).copied()
    }

    pub fn code(self) -> u8 {
        self as u8
    }
}
