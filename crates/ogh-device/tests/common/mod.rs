#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use ogh_device::FeatureId;
use ogh_protocol::discovery::{parse_feature_page, parse_get_feature};
use ogh_protocol::{A50X_PREFIX, Decoded, FRAME_LEN, Message, decode};
use serde::Deserialize;

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

pub fn fixture_path(seen_in: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(format!("{seen_in}.jsonl"))
}

/// `name` is `<session dir>/<label>`, as cited in `allowlist.toml`.
pub fn load(name: &str) -> Vec<Transfer> {
    let path = fixture_path(name);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .enumerate()
        .skip(1)
        .map(|(i, line)| {
            let raw: RawTransfer = serde_json::from_str(line).expect("transfer line");
            Transfer {
                line: i + 1,
                dir: match raw.dir.as_str() {
                    "out" => Dir::Out,
                    "in" => Dir::In,
                    other => panic!("unknown dir {other}"),
                },
                frame: hex::decode(&raw.hex)
                    .expect("hex")
                    .try_into()
                    .expect("64-byte frame"),
            }
        })
        .collect()
}

/// The frame at a fixture line number, counting the metadata line as line 1.
pub fn frame_at(name: &str, line: usize) -> [u8; FRAME_LEN] {
    load(name)
        .into_iter()
        .find(|t| t.line == line)
        .unwrap_or_else(|| panic!("{name} has no line {line}"))
        .frame
}

pub fn frame_from_hex(hex_str: &str) -> [u8; FRAME_LEN] {
    let mut bytes = hex::decode(hex_str.replace(' ', "")).expect("hex");
    bytes.resize(FRAME_LEN, 0);
    bytes.try_into().expect("64-byte frame")
}

pub const LAUNCH: &str = "session-a/ghub-launch";
pub const LAUNCH_OPEN_REQUESTS: [usize; 5] = [4, 6, 16, 20, 132];
pub const LAUNCH_OPEN_REPLIES: [usize; 5] = [5, 7, 17, 21, 133];

fn reply(frame: &[u8; FRAME_LEN]) -> Option<Message> {
    match decode(A50X_PREFIX, frame) {
        Ok(Decoded::Reply { flags: 0, msg }) => Some(msg),
        _ => None,
    }
}

/// Index to id from the discovery replies inside one capture, or `None` if
/// the capture holds no discovery.
pub fn discovered_ids(transfers: &[Transfer]) -> Option<HashMap<u8, FeatureId>> {
    let mut ids = HashMap::from([(0, FeatureId::ROOT)]);
    let mut pending_starts = Vec::new();
    let mut found = false;
    for transfer in transfers {
        let Some(msg) = reply(&transfer.frame) else {
            continue;
        };
        if msg.sw_id == 0 {
            continue;
        }
        match (transfer.dir, msg.feature_index, msg.function) {
            (Dir::Out, 1, 1) => pending_starts.push(msg.params[0]),
            (Dir::In, 0, 0) if msg.params.len() >= 3 => {
                if let Ok(Some(info)) = parse_get_feature(FeatureId::FEATURE_SET, &msg.params)
                    && info.index == 1
                {
                    ids.insert(1, FeatureId::FEATURE_SET);
                }
            }
            (Dir::In, 1, 1) => {
                let start = pending_starts.remove(0);
                for info in parse_feature_page(start, &msg.params).expect("feature page") {
                    ids.insert(info.index, info.id);
                }
                found = true;
            }
            _ => {}
        }
    }
    found.then_some(ids)
}

/// Every (feature id, function) sent host to device in a capture, with the
/// index mapped through `ids`.
pub fn sent_calls(transfers: &[Transfer], ids: &HashMap<u8, FeatureId>) -> Vec<(FeatureId, u8)> {
    transfers
        .iter()
        .filter(|t| t.dir == Dir::Out)
        .filter_map(|t| reply(&t.frame))
        .map(|msg| {
            let id = *ids
                .get(&msg.feature_index)
                .unwrap_or_else(|| panic!("index {} not in the table", msg.feature_index));
            (id, msg.function)
        })
        .collect()
}
