mod common;

use common::{load, request_reply_pairs};
use ogh_protocol::discovery::{
    FeatureInfo, Ping, get_count_request, get_feature_request, list_features_request, parse_count,
    parse_feature_page, parse_get_feature, parse_ping, ping_request,
};
use ogh_protocol::{FeatureId, Message, ParseError};

const EXPECTED_TABLE: [(u8, u16, u8, u8); 23] = [
    (1, 0x0001, 0, 1),
    (2, 0x010A, 0, 6),
    (3, 0x0102, 0, 1),
    (4, 0x0100, 0, 6),
    (5, 0x0101, 0, 2),
    (6, 0x0104, 0, 4),
    (7, 0x0108, 0, 3),
    (8, 0x0200, 0, 4),
    (9, 0x0604, 0, 2),
    (10, 0x0609, 0, 2),
    (11, 0x0303, 0, 4),
    (12, 0x0800, 0, 1),
    (13, 0x020D, 0, 1),
    (14, 0x0305, 0, 1),
    (15, 0x0110, 0, 1),
    (16, 0x060B, 0, 1),
    (17, 0x0103, 0, 1),
    (18, 0x0004, 0, 4),
    (19, 0x020E, 0, 2),
    (20, 0x0806, 0, 1),
    (21, 0x0805, 0, 1),
    (22, 0x0801, 0, 1),
    (23, 0x0807, 0, 1),
];

fn info(index: u8, id: u16, flags: u8, version: u8) -> FeatureInfo {
    FeatureInfo {
        index,
        id: FeatureId(id),
        flags,
        version,
    }
}

fn request(feature_index: u8, function: u8, params: &[u8]) -> Message {
    Message {
        feature_index,
        function,
        sw_id: 0xb,
        params: params.to_vec(),
    }
}

#[test]
fn feature_table_matches_capture() {
    let pairs = request_reply_pairs(&load("ghub-launch"));

    let counts: Vec<u8> = pairs
        .iter()
        .filter(|(req, _)| (req.feature_index, req.function) == (1, 0))
        .map(|(_, reply)| parse_count(&reply.params).unwrap())
        .collect();
    assert_eq!(counts, [24]);

    let mut table = Vec::new();
    for (req, reply) in pairs
        .iter()
        .filter(|(req, _)| (req.feature_index, req.function) == (1, 1))
    {
        table.extend(parse_feature_page(req.params[0], &reply.params).unwrap());
    }
    let feature_set = pairs
        .iter()
        .find(|(req, _)| req.feature_index == 0 && req.params == [0x00, 0x01])
        .map(|(_, reply)| parse_get_feature(FeatureId::FEATURE_SET, &reply.params).unwrap())
        .unwrap()
        .unwrap();
    table.insert(0, feature_set);

    let expected: Vec<FeatureInfo> = EXPECTED_TABLE
        .iter()
        .map(|&(index, id, flags, version)| info(index, id, flags, version))
        .collect();
    assert_eq!(table, expected);
    assert_eq!(table.len() + 1, 24, "root is implicit at index 0");
}

#[test]
fn requests_match_captured_bytes() {
    assert_eq!(
        get_feature_request(FeatureId::FEATURE_SET),
        request(0, 0, &[0x00, 0x01])
    );
    assert_eq!(get_count_request(1), request(1, 0, &[]));
    assert_eq!(list_features_request(1, 16), request(1, 1, &[16]));
    assert_eq!(ping_request(0x39), request(0, 1, &[0x39, 0, 0]));
}

#[test]
fn every_discovery_request_in_the_capture_is_reproduced() {
    let pairs = request_reply_pairs(&load("ghub-launch"));
    let mut seen = 0;
    for (req, _) in &pairs {
        let rebuilt = match (req.feature_index, req.function) {
            (0, 0) => get_feature_request(FeatureId(u16::from_be_bytes([
                req.params[0],
                req.params[1],
            ]))),
            (0, 1) => ping_request(req.params[0]),
            (1, 0) => get_count_request(1),
            (1, 1) => list_features_request(1, req.params[0]),
            _ => continue,
        };
        assert_eq!(&rebuilt, req);
        seen += 1;
    }
    assert!(seen >= 9, "only {seen} discovery requests seen");
}

#[test]
fn ping_and_get_feature_parse() {
    let pairs = request_reply_pairs(&load("ghub-launch"));

    let (req, reply) = &pairs[0];
    assert_eq!(req, &ping_request(0x39));
    assert_eq!(
        parse_ping(&reply.params),
        Ok(Ping {
            major: 1,
            minor: 1,
            echo: 0x39
        })
    );

    let (req, reply) = &pairs[1];
    assert_eq!(req, &get_feature_request(FeatureId::FEATURE_SET));
    assert_eq!(
        parse_get_feature(FeatureId::FEATURE_SET, &reply.params),
        Ok(Some(info(1, 0x0001, 0, 1)))
    );

    let (req, reply) = pairs
        .iter()
        .find(|(req, _)| req.feature_index == 0 && req.params == [0x01, 0x00])
        .unwrap();
    assert_eq!(req, &get_feature_request(FeatureId::DEVICE_INFO));
    assert_eq!(
        parse_get_feature(FeatureId::DEVICE_INFO, &reply.params),
        Ok(Some(info(4, 0x0100, 0, 6)))
    );
}

#[test]
fn get_feature_reports_absent_id_as_none() {
    assert_eq!(
        parse_get_feature(FeatureId::DEVICE_INFO, &[0, 0, 0]),
        Ok(None)
    );
    assert_eq!(
        parse_get_feature(FeatureId::ROOT, &[0, 0, 0]),
        Ok(Some(info(0, 0x0000, 0, 0)))
    );
}

#[test]
fn parsers_reject_short_params() {
    assert_eq!(
        parse_get_feature(FeatureId::DEVICE_INFO, &[4, 0]),
        Err(ParseError::TooShort {
            expected: 3,
            actual: 2
        })
    );
    assert_eq!(
        parse_count(&[]),
        Err(ParseError::TooShort {
            expected: 1,
            actual: 0
        })
    );
    assert_eq!(
        parse_ping(&[1, 1]),
        Err(ParseError::TooShort {
            expected: 3,
            actual: 2
        })
    );
    assert_eq!(
        parse_feature_page(2, &[]),
        Err(ParseError::TooShort {
            expected: 1,
            actual: 0
        })
    );
}

#[test]
fn feature_page_errors_when_records_overrun_params() {
    let params = [2, 0x01, 0x0a, 0x00, 0x06, 0x01, 0x02];
    assert_eq!(
        parse_feature_page(2, &params),
        Err(ParseError::TooShort {
            expected: 9,
            actual: 7
        })
    );
}

#[test]
fn feature_page_rejects_indices_past_255() {
    let params = [2, 0x01, 0x0a, 0x00, 0x06, 0x01, 0x02, 0x00, 0x01];
    assert_eq!(
        parse_feature_page(255, &params),
        Err(ParseError::IndexOverflow)
    );
}

#[test]
fn feature_page_numbers_records_from_start() {
    let params = [2, 0x01, 0x0a, 0x00, 0x06, 0x01, 0x02, 0x00, 0x01];
    assert_eq!(
        parse_feature_page(5, &params),
        Ok(vec![info(5, 0x010a, 0, 6), info(6, 0x0102, 0, 1)])
    );
}

#[test]
fn feature_names() {
    assert_eq!(FeatureId::HEADSET_SIDETONE.name(), Some("HeadsetSidetone"));
    assert_eq!(FeatureId::ROOT.name(), Some("Root"));
    for unknown in [0x0303, 0x0805, 0x0806, 0x0807, 0x0102] {
        assert_eq!(FeatureId(unknown).name(), None, "{unknown:#06x}");
    }
}
