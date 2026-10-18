use crate::error::FrameError;

pub const FRAME_LEN: usize = 64;

/// Report id followed by the constant byte seen at offset 1 of every frame.
pub const A50X_PREFIX: &[u8] = &[0x02, 0x0c];

/// Software id G Hub puts on every request.
pub const GHUB_SW_ID: u8 = 0x0b;

const FLAGS_SINGLE: u8 = 0x00;
const FLAGS_FIRST_FRAGMENT: u8 = 0x01;
const FLAGS_CONTINUATION: u8 = 0x02;
const ERROR_FEATURE_INDEX: u8 = 0xff;
const MESSAGE_HEADER_LEN: usize = 3;
const ERROR_PAYLOAD_LEN: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub feature_index: u8,
    pub function: u8,
    /// Zero marks an unsolicited event.
    pub sw_id: u8,
    pub params: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decoded {
    /// `flags` is 1 on the first frame of a fragmented reply, so more follows.
    Reply { flags: u8, msg: Message },
    /// Raw bytes of a flags `0x02` frame, with no index or function byte.
    Continuation { data: Vec<u8> },
    /// Unverified on this device: no error frame was ever captured. The
    /// layout is the HID++ 2.0 one (feature index `0xff`, then the request's
    /// index, function and sw_id, then a code).
    Error {
        feature_index: u8,
        function: u8,
        sw_id: u8,
        code: u8,
    },
}

fn max_params(prefix: &[u8]) -> usize {
    FRAME_LEN - prefix.len() - 1 - MESSAGE_HEADER_LEN
}

pub fn encode(prefix: &[u8], flags: u8, msg: &Message) -> Result<[u8; FRAME_LEN], FrameError> {
    if flags != FLAGS_SINGLE && flags != FLAGS_FIRST_FRAGMENT {
        return Err(FrameError::UnknownFlags(flags));
    }
    if msg.function > 0x0f || msg.sw_id > 0x0f {
        return Err(FrameError::FieldTooWide);
    }
    let max = max_params(prefix);
    if msg.params.len() > max {
        return Err(FrameError::TooLong {
            len: msg.params.len(),
            max,
        });
    }

    let mut frame = [0u8; FRAME_LEN];
    let mut at = prefix.len();
    frame[..at].copy_from_slice(prefix);
    frame[at] = (MESSAGE_HEADER_LEN + msg.params.len()) as u8;
    frame[at + 1] = flags;
    frame[at + 2] = msg.feature_index;
    frame[at + 3] = msg.function << 4 | msg.sw_id;
    at += 4;
    frame[at..at + msg.params.len()].copy_from_slice(&msg.params);
    Ok(frame)
}

pub fn decode(prefix: &[u8], frame: &[u8]) -> Result<Decoded, FrameError> {
    if frame.len() != FRAME_LEN {
        return Err(FrameError::BadFrameLength(frame.len()));
    }
    if !frame.starts_with(prefix) {
        return Err(FrameError::WrongPrefix);
    }
    let len_at = prefix.len();
    let len = frame[len_at];
    let payload = &frame[len_at + 1..];
    let payload_len = usize::from(len);
    if payload_len == 0 || payload_len > payload.len() {
        return Err(FrameError::BadPayloadLength(len));
    }
    let payload = &payload[..payload_len];
    let flags = payload[0];
    let body = &payload[1..];

    match flags {
        FLAGS_CONTINUATION => Ok(Decoded::Continuation {
            data: body.to_vec(),
        }),
        FLAGS_SINGLE | FLAGS_FIRST_FRAGMENT => {
            if payload_len < MESSAGE_HEADER_LEN {
                return Err(FrameError::BadPayloadLength(len));
            }
            if flags == FLAGS_SINGLE && body[0] == ERROR_FEATURE_INDEX {
                return decode_error(body, len);
            }
            Ok(Decoded::Reply {
                flags,
                msg: Message {
                    feature_index: body[0],
                    function: body[1] >> 4,
                    sw_id: body[1] & 0x0f,
                    params: body[2..].to_vec(),
                },
            })
        }
        other => Err(FrameError::UnknownFlags(other)),
    }
}

fn decode_error(body: &[u8], len: u8) -> Result<Decoded, FrameError> {
    if usize::from(len) < ERROR_PAYLOAD_LEN {
        return Err(FrameError::BadPayloadLength(len));
    }
    Ok(Decoded::Error {
        feature_index: body[1],
        function: body[2] >> 4,
        sw_id: body[2] & 0x0f,
        code: body[3],
    })
}
