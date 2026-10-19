use super::first_byte;
use crate::error::{ParseError, RangeError, require_len};
use crate::frame::{Message, request};

const GET: u8 = 0;
const SET: u8 = 1;
const INFO: u8 = 2;
const MAX_BALANCE: u8 = 12;

/// Game/voice balance, 0 to 12 with 6 in the centre. Which end is Game and
/// which is Voice was not confirmed. Parsed values are not range-checked; the
/// set builder is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameVoiceBalance(pub u8);

/// Guessed reading of `0c 05 04`: the first byte is the maximum step (12).
/// The other two are unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BalanceInfo {
    pub max: u8,
    pub unknown: [u8; 2],
}

/// Guessed meaning of fn0: getBalance, read from the reply content.
pub fn get_request(feature_index: u8) -> Message {
    request(feature_index, GET, &[])
}

pub fn set_request(feature_index: u8, balance: GameVoiceBalance) -> Result<Message, RangeError> {
    RangeError::check_max("balance", balance.0, MAX_BALANCE)?;
    Ok(request(feature_index, SET, &[balance.0]))
}

/// Guessed meaning of fn2: getInfo, read from the reply content.
pub fn info_request(feature_index: u8) -> Message {
    request(feature_index, INFO, &[])
}

/// Parses a fn0 reply or a balance event.
pub fn parse_balance(params: &[u8]) -> Result<GameVoiceBalance, ParseError> {
    first_byte(params).map(GameVoiceBalance)
}

pub fn parse_info(params: &[u8]) -> Result<BalanceInfo, ParseError> {
    require_len(params, 3)?;
    Ok(BalanceInfo {
        max: params[0],
        unknown: [params[1], params[2]],
    })
}
