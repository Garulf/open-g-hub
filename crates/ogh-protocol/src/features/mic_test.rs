use super::first_byte;
use crate::error::ParseError;
use crate::frame::{Message, request};

const GET_STATE: u8 = 6;
const NOT_CONNECTED: u8 = 0x05;

/// Raw state of the mic test state machine. Only 0 and 5 were captured, so
/// the meaning of the other values is a guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MicTestState(pub u8);

impl MicTestState {
    /// 5 means not connected (docked or off). Every other value counts as
    /// in use, because a running mic test reports states other than 0.
    pub fn is_headset_available(self) -> bool {
        self.0 != NOT_CONNECTED
    }
}

pub fn get_state_request(feature_index: u8) -> Message {
    request(feature_index, GET_STATE, &[])
}

/// Parses a fn6 reply or a mic test event.
pub fn parse_state(params: &[u8]) -> Result<MicTestState, ParseError> {
    first_byte(params).map(MicTestState)
}
