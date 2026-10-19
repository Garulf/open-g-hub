#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

use ogh_protocol::{A50X_PREFIX, Decoded, FRAME_LEN, Message, decode};
use serde::Deserialize;

pub const FIXTURES: [&str; 5] = [
    "base-replug",
    "base-replug-2",
    "dock-cycle",
    "ghub-launch",
    "headset-power",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Out,
    In,
}

#[derive(Clone, Debug)]
pub struct Transfer {
    pub line: usize,
    pub dir: Dir,
    pub frame: [u8; FRAME_LEN],
}

#[derive(Deserialize)]
struct RawTransfer {
    dir: String,
    hex: String,
}

pub fn load(name: &str) -> Vec<Transfer> {
    load_session("session-a", name)
}

pub fn load_session(session: &str, name: &str) -> Vec<Transfer> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(session)
        .join(format!("{name}.jsonl"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .enumerate()
        .skip(1)
        .map(|(i, line)| {
            let raw: RawTransfer = serde_json::from_str(line).expect("transfer line");
            let bytes = hex::decode(&raw.hex).expect("hex");
            Transfer {
                line: i + 1,
                dir: match raw.dir.as_str() {
                    "out" => Dir::Out,
                    "in" => Dir::In,
                    other => panic!("unknown dir {other}"),
                },
                frame: bytes.try_into().expect("64-byte frame"),
            }
        })
        .collect()
}

pub fn frame_from_hex(hex_str: &str) -> [u8; FRAME_LEN] {
    let mut bytes = hex::decode(hex_str).expect("hex");
    bytes.resize(FRAME_LEN, 0);
    bytes.try_into().expect("64-byte frame")
}

pub fn request_reply_pairs(transfers: &[Transfer]) -> Vec<(Message, Message)> {
    let mut pending: Vec<Message> = Vec::new();
    let mut pairs = Vec::new();
    for transfer in transfers {
        match (
            transfer.dir,
            decode(A50X_PREFIX, &transfer.frame).expect("decode"),
        ) {
            (Dir::Out, Decoded::Reply { msg, .. }) => pending.push(msg),
            (Dir::In, Decoded::Reply { msg, .. }) if msg.sw_id != 0 => {
                let position = pending
                    .iter()
                    .position(|r| {
                        r.feature_index == msg.feature_index && r.function == msg.function
                    })
                    .unwrap_or_else(|| panic!("line {}: reply without request", transfer.line));
                pairs.push((pending.remove(position), msg));
            }
            _ => {}
        }
    }
    pairs
}
