mod common;

use common::{Dir, FIXTURES, frame_from_hex, load};
use ogh_protocol::{A50X_PREFIX, Decoded, FRAME_LEN, FrameError, Message, decode, encode};

#[test]
fn encode_matches_every_session_a_request() {
    let mut checked = 0;
    for name in FIXTURES {
        for transfer in load(name).iter().filter(|t| t.dir == Dir::Out) {
            let Decoded::Reply { flags, msg } = decode(A50X_PREFIX, &transfer.frame).unwrap()
            else {
                panic!(
                    "{name} line {}: request did not decode as a message",
                    transfer.line
                );
            };
            let encoded = encode(A50X_PREFIX, flags, &msg).unwrap();
            assert_eq!(encoded, transfer.frame, "{name} line {}", transfer.line);
            checked += 1;
        }
    }
    assert!(checked > 100, "only {checked} requests checked");
}

#[test]
fn decode_round_trips_every_in_frame() {
    let mut replies = 0;
    let mut continuations = 0;
    for name in FIXTURES {
        for transfer in load(name).iter().filter(|t| t.dir == Dir::In) {
            match decode(A50X_PREFIX, &transfer.frame)
                .unwrap_or_else(|e| panic!("{name} line {}: {e}", transfer.line))
            {
                Decoded::Reply { flags, msg } => {
                    let encoded = encode(A50X_PREFIX, flags, &msg).unwrap();
                    assert_eq!(encoded, transfer.frame, "{name} line {}", transfer.line);
                    replies += 1;
                }
                Decoded::Continuation { .. } => continuations += 1,
                Decoded::Error { .. } => {
                    panic!("{name} line {}: unexpected error", transfer.line)
                }
            }
        }
    }
    assert!(replies > 100);
    assert!(continuations > 0);
}

#[test]
fn decode_rejects_wrong_prefix() {
    let mut frame = frame_from_hex("020c0600001b39");
    frame[1] = 0x0d;
    assert_eq!(decode(A50X_PREFIX, &frame), Err(FrameError::WrongPrefix));
    frame[1] = 0x0c;
    frame[0] = 0x03;
    assert_eq!(decode(A50X_PREFIX, &frame), Err(FrameError::WrongPrefix));
}

#[test]
fn decode_rejects_wrong_frame_length() {
    assert_eq!(
        decode(A50X_PREFIX, &[0x02, 0x0c, 0x03]),
        Err(FrameError::BadFrameLength(3))
    );
}

#[test]
fn decode_rejects_unknown_flags() {
    let frame = frame_from_hex("020c0603001b39");
    assert_eq!(
        decode(A50X_PREFIX, &frame),
        Err(FrameError::UnknownFlags(3))
    );
}

#[test]
fn decode_rejects_length_byte_beyond_frame() {
    let frame = frame_from_hex("020c3f00001b39");
    assert_eq!(
        decode(A50X_PREFIX, &frame),
        Err(FrameError::BadPayloadLength(0x3f))
    );
}

#[test]
fn encode_rejects_oversized_params() {
    let fits = Message {
        feature_index: 0x0d,
        function: 2,
        sw_id: 0xb,
        params: vec![0xaa; 58],
    };
    let frame = encode(A50X_PREFIX, 0, &fits).unwrap();
    assert_eq!(frame.len(), FRAME_LEN);
    assert_eq!(frame[2], 0x3d);

    let too_long = Message {
        params: vec![0xaa; 59],
        ..fits
    };
    assert_eq!(
        encode(A50X_PREFIX, 0, &too_long),
        Err(FrameError::TooLong { len: 59, max: 58 })
    );
}

#[test]
fn encode_rejects_fields_wider_than_a_nibble() {
    let msg = Message {
        feature_index: 1,
        function: 0x10,
        sw_id: 0xb,
        params: vec![],
    };
    assert_eq!(encode(A50X_PREFIX, 0, &msg), Err(FrameError::FieldTooWide));
}

#[test]
fn error_frame_decodes_to_error_variant() {
    let frame = frame_from_hex("020c0500ff091b05");
    assert_eq!(
        decode(A50X_PREFIX, &frame),
        Ok(Decoded::Error {
            feature_index: 0x09,
            function: 1,
            sw_id: 0xb,
            code: 5,
        })
    );
}

#[test]
fn continuation_frame_decodes() {
    let transfers = load("dock-cycle");
    let first = &transfers[21];
    let second = &transfers[22];
    assert_eq!(first.line, 23);
    assert_eq!(second.line, 24);

    let Decoded::Reply { flags, msg } = decode(A50X_PREFIX, &first.frame).unwrap() else {
        panic!("first fragment is a reply");
    };
    assert_eq!(flags, 1);
    assert_eq!((msg.feature_index, msg.function, msg.sw_id), (0x11, 1, 0xb));
    assert_eq!(msg.params.len(), 58);

    assert_eq!(
        decode(A50X_PREFIX, &second.frame),
        Ok(Decoded::Continuation {
            data: vec![0x12, 0xf6, 0x7a, 0x9c],
        })
    );
}
