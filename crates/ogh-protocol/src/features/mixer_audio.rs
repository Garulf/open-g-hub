use super::{bool_byte, level_to_percent, percent_to_level};
use crate::error::{ParseError, RangeError, require_len};
use crate::frame::{Message, request};

const GET_INPUT_STATE: u8 = 2;
const SET_SETTINGS: u8 = 6;
/// The A50 X has one output, USB_STREAM, at index 0 (getOutputInfo).
const STREAM_OUTPUT: u8 = 0;
const INPUT_COUNT: usize = 4;
const RECORD_LEN: usize = 3;
const SETTINGS_LEN: usize = 4 + INPUT_COUNT * RECORD_LEN;

/// Levels run 0 to 32: getMixerInfo reports 33 steps.
pub const MAX_LEVEL: u8 = 32;

/// Guessed meaning of fn2: getInputState, read from the reply content.
pub fn input_state_request(feature_index: u8) -> Message {
    request(feature_index, GET_INPUT_STATE, &[])
}

/// Parses a fn2 reply or a fn0 mixer event: a big-endian u32 bitmask.
/// Guessed: bit `n` is mixer input `n`.
pub fn parse_input_state(params: &[u8]) -> Result<u32, ParseError> {
    require_len(params, 4)?;
    Ok(u32::from_be_bytes([
        params[0], params[1], params[2], params[3],
    ]))
}

/// Parsed levels are not range-checked; the set builder is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixLevel(pub u8);

impl MixLevel {
    /// `roundf(32 * percent / 100)`, as G Hub maps its 0 to 100 slider:
    /// 0, 50, 80 and 100 were captured as 0x00, 0x10, 0x1a and 0x20. Other
    /// percentages follow the formula found in the G Hub agent, unverified.
    /// Values above 100 count as 100.
    pub fn from_percent(percent: u8) -> Self {
        Self(percent_to_level(percent, MAX_LEVEL))
    }

    /// Inverse of `from_percent`, rounded to the nearest percent. A guess
    /// beyond the captured points.
    pub fn to_percent(self) -> u8 {
        level_to_percent(self.0, MAX_LEVEL)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixChannel {
    pub level: MixLevel,
    pub muted: bool,
}

/// Inputs of the stream mix, named as G Hub's Routing page shows them.
/// Game (1) and Voice (3) were identified by changing them in G Hub; Mic Out
/// (0) and Bluetooth (2) were greyed out and are placed by the input types
/// getInputInfo lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixInput {
    MicOut,
    Game,
    Bluetooth,
    Voice,
}

impl MixInput {
    pub const ALL: [Self; INPUT_COUNT] = [Self::MicOut, Self::Game, Self::Bluetooth, Self::Voice];

    pub fn index(self) -> u8 {
        self as u8
    }
}

/// What G Hub's Routing page sets: the Stream Port output and the four
/// inputs mixed into it, `inputs` in `MixInput` order. G Hub always wrote
/// all four inputs; leaving one out (unrouting it) was never captured, so it
/// cannot be expressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamMix {
    pub stream: MixChannel,
    pub inputs: [MixChannel; INPUT_COUNT],
}

impl StreamMix {
    pub fn input(&self, input: MixInput) -> MixChannel {
        self.inputs[usize::from(input.index())]
    }

    pub fn input_mut(&mut self, input: MixInput) -> &mut MixChannel {
        &mut self.inputs[usize::from(input.index())]
    }
}

/// fn6 setMixerSettings: `00 <level> <mute> 04` then `<index> <level>
/// <mute>` for each input.
pub fn set_request(feature_index: u8, mix: &StreamMix) -> Result<Message, RangeError> {
    let mut params = Vec::with_capacity(SETTINGS_LEN);
    params.push(STREAM_OUTPUT);
    push_channel(&mut params, mix.stream)?;
    params.push(INPUT_COUNT as u8);
    for input in MixInput::ALL {
        params.push(input.index());
        push_channel(&mut params, mix.input(input))?;
    }
    Ok(request(feature_index, SET_SETTINGS, &params))
}

fn push_channel(params: &mut Vec<u8>, channel: MixChannel) -> Result<(), RangeError> {
    RangeError::check_max("mixer level", channel.level.0, MAX_LEVEL)?;
    params.extend([channel.level.0, u8::from(channel.muted)]);
    Ok(())
}

/// Parses the fn2 "mixer settings changed" event, which repeats the fn6
/// params. Only the layout G Hub wrote is accepted: output 0 and the four
/// inputs in index order.
pub fn parse_settings(params: &[u8]) -> Result<StreamMix, ParseError> {
    require_len(params, SETTINGS_LEN)?;
    expect_byte(params[0], STREAM_OUTPUT)?;
    let stream = parse_channel(&params[1..3])?;
    expect_byte(params[3], INPUT_COUNT as u8)?;
    let mut inputs = [stream; INPUT_COUNT];
    for (input, record) in MixInput::ALL
        .iter()
        .zip(params[4..SETTINGS_LEN].as_chunks::<RECORD_LEN>().0)
    {
        expect_byte(record[0], input.index())?;
        inputs[usize::from(input.index())] = parse_channel(&record[1..])?;
    }
    Ok(StreamMix { stream, inputs })
}

fn parse_channel(bytes: &[u8]) -> Result<MixChannel, ParseError> {
    Ok(MixChannel {
        level: MixLevel(bytes[0]),
        muted: bool_byte(bytes[1])?,
    })
}

fn expect_byte(byte: u8, expected: u8) -> Result<(), ParseError> {
    if byte == expected {
        Ok(())
    } else {
        Err(ParseError::UnknownValue(byte))
    }
}
