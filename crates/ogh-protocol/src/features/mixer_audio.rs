use crate::error::{ParseError, require_len};
use crate::frame::{Message, request};

const GET_INPUT_STATE: u8 = 2;

pub fn input_state_request(feature_index: u8) -> Message {
    request(feature_index, GET_INPUT_STATE, &[])
}

/// Parses a fn2 reply or a mixer event: a big-endian u32 bitmask. Guessed:
/// bit `n` is mixer input `n`.
pub fn parse_input_state(params: &[u8]) -> Result<u32, ParseError> {
    require_len(params, 4)?;
    Ok(u32::from_be_bytes([
        params[0], params[1], params[2], params[3],
    ]))
}
