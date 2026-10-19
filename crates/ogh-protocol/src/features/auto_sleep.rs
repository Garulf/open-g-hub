use crate::error::{ParseError, RangeError, require_len};
use crate::frame::{Message, request};

const GET: u8 = 0;
const SET: u8 = 1;
/// G Hub overwrites the first timer byte with 60 for feature version 3,
/// which the A50 X has. What a different value would do is unknown.
const PREFIX: u8 = 0x3c;
/// Never, 15 and 30 minutes were written. The G Hub UI may also offer 60;
/// that was never sent.
const SETTABLE_MINUTES: &[u8] = &[0, 15, 30];

/// G Hub's "Sleep After" timer in minutes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SleepAfter(pub u8);

impl SleepAfter {
    pub const NEVER: Self = Self(0);
}

/// The fn0 reply and event: `3c <minutes>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoSleep {
    /// 0x3c in every capture; meaning unknown.
    pub prefix: u8,
    pub sleep_after: SleepAfter,
}

pub fn get_request(feature_index: u8) -> Message {
    request(feature_index, GET, &[])
}

pub fn set_request(feature_index: u8, sleep_after: SleepAfter) -> Result<Message, RangeError> {
    RangeError::check_one_of("sleep after (minutes)", sleep_after.0, SETTABLE_MINUTES)?;
    Ok(request(feature_index, SET, &[PREFIX, sleep_after.0]))
}

/// Parses a fn0 reply or a sleep timer event.
pub fn parse(params: &[u8]) -> Result<AutoSleep, ParseError> {
    require_len(params, 2)?;
    Ok(AutoSleep {
        prefix: params[0],
        sleep_after: SleepAfter(params[1]),
    })
}
