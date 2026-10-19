use super::first_byte;
use crate::error::ParseError;
use crate::frame::{Message, request};

const GET_STATE: u8 = 6;
const NOT_CONNECTED: u8 = 0x05;

/// Raw state of the mic test state machine. A G Hub mic test showed the wire
/// value is G Hub's state ordinal minus one: 0 ready to record, 1 recording,
/// 2 ready for playback, 3 playing, 4 erasing, 5 not connected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MicTestState(pub u8);

impl MicTestState {
    /// 5 (not connected) came with every dock and power-off, and every other
    /// value with a lift or power-on, across six transitions on one
    /// firmware. A running mic test reports 0 to 4, so all of them count as
    /// in use.
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
