use crate::error::{ParseError, require_len};
use crate::feature_id::FeatureId;
use crate::frame::{Message, request};

const ROOT_INDEX: u8 = 0;
const ROOT_GET_FEATURE: u8 = 0;
const ROOT_PING: u8 = 1;
const FEATURE_SET_GET_COUNT: u8 = 0;
const FEATURE_SET_LIST: u8 = 1;
const FEATURE_RECORD_LEN: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureInfo {
    pub index: u8,
    pub id: FeatureId,
    pub flags: u8,
    pub version: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ping {
    pub major: u8,
    pub minor: u8,
    pub echo: u8,
}

pub fn ping_request(data: u8) -> Message {
    request(ROOT_INDEX, ROOT_PING, &[data, 0, 0])
}

/// `major` and `minor` are a guessed reading of the two constant reply bytes.
pub fn parse_ping(params: &[u8]) -> Result<Ping, ParseError> {
    require_len(params, 3)?;
    Ok(Ping {
        major: params[0],
        minor: params[1],
        echo: params[2],
    })
}

pub fn get_feature_request(id: FeatureId) -> Message {
    request(ROOT_INDEX, ROOT_GET_FEATURE, &id.0.to_be_bytes())
}

/// Index 0 for any id other than Root is read as "absent". That follows
/// HID++ 2.0 and is unverified: no absent-id reply was ever captured.
pub fn parse_get_feature(id: FeatureId, params: &[u8]) -> Result<Option<FeatureInfo>, ParseError> {
    require_len(params, 3)?;
    let index = params[0];
    if index == 0 && id != FeatureId::ROOT {
        return Ok(None);
    }
    Ok(Some(FeatureInfo {
        index,
        id,
        flags: params[1],
        version: params[2],
    }))
}

pub fn get_count_request(feature_set_index: u8) -> Message {
    request(feature_set_index, FEATURE_SET_GET_COUNT, &[])
}

/// The count includes Root, so valid indices are `0..count`.
pub fn parse_count(params: &[u8]) -> Result<u8, ParseError> {
    require_len(params, 1)?;
    Ok(params[0])
}

pub fn list_features_request(feature_set_index: u8, start: u8) -> Message {
    request(feature_set_index, FEATURE_SET_LIST, &[start])
}

pub fn parse_feature_page(start: u8, params: &[u8]) -> Result<Vec<FeatureInfo>, ParseError> {
    require_len(params, 1)?;
    let count = usize::from(params[0]);
    require_len(params, 1 + count * FEATURE_RECORD_LEN)?;
    if usize::from(start) + count > usize::from(u8::MAX) + 1 {
        return Err(ParseError::IndexOverflow);
    }
    let (records, _) = params[1..].as_chunks::<FEATURE_RECORD_LEN>();
    Ok(records
        .iter()
        .take(count)
        .zip(start..=u8::MAX)
        .map(|(record, index)| FeatureInfo {
            index,
            id: FeatureId(u16::from_be_bytes([record[0], record[1]])),
            flags: record[2],
            version: record[3],
        })
        .collect())
}
