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
        }
    }
}

impl std::error::Error for ParseError {}

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
