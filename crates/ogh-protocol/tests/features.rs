mod common;

use std::collections::HashMap;

use common::{Dir, Transfer, load, load_session, request_reply_pairs};
use ogh_protocol::discovery::{parse_feature_page, parse_get_feature};
use ogh_protocol::features::battery::{self, Battery, ChargeStatus};
use ogh_protocol::features::device_info::{self, FirmwareVersion};
use ogh_protocol::features::headset_mix::{self, BalanceInfo, GameVoiceBalance};
use ogh_protocol::features::mic_test::{self, MicTestState};
use ogh_protocol::features::noise_gate::{self, NoiseGate};
use ogh_protocol::features::sidetone::{self, Sidetone, SidetoneInfo};
use ogh_protocol::features::{
    Event, auto_sleep, bt_host_info, device_name, led_brightness, mixer_audio, parse_event,
};
use ogh_protocol::{
    A50X_PREFIX, Decoded, FeatureId, Message, ParseError, RangeError, decode, encode, request,
};

const BATTERY: u8 = 6;
const DEVICE_INFO: u8 = 4;
const DEVICE_NAME: u8 = 5;
const AUTO_SLEEP: u8 = 7;
const SIDETONE: u8 = 9;
const HEADSET_MIX: u8 = 10;
const MIXER_AUDIO: u8 = 12;
const BT_HOST_INFO: u8 = 14;
const LED_BRIGHTNESS: u8 = 15;
const MIC_TEST: u8 = 19;
const NOISE_GATE: u8 = 22;

fn feature_table() -> HashMap<u8, FeatureId> {
    let pairs = request_reply_pairs(&load("ghub-launch"));
    let mut table = HashMap::new();
    for (req, reply) in pairs
        .iter()
        .filter(|(req, _)| (req.feature_index, req.function) == (1, 1))
    {
        for info in parse_feature_page(req.params[0], &reply.params).unwrap() {
            table.insert(info.index, info.id);
        }
    }
    let (_, reply) = pairs
        .iter()
        .find(|(req, _)| req.feature_index == 0 && req.params == [0x00, 0x01])
        .unwrap();
    let feature_set = parse_get_feature(FeatureId::FEATURE_SET, &reply.params)
        .unwrap()
        .unwrap();
    table.insert(feature_set.index, feature_set.id);
    table
}

fn reply_to<'a>(pairs: &'a [(Message, Message)], request: &Message) -> &'a Message {
    &pairs
        .iter()
        .find(|(req, _)| req == request)
        .unwrap_or_else(|| panic!("no captured exchange for {request:?}"))
        .1
}

fn events(transfers: &[Transfer]) -> Vec<(usize, u8, Message)> {
    transfers
        .iter()
        .filter(|t| t.dir == Dir::In)
        .filter_map(|t| match decode(A50X_PREFIX, &t.frame).unwrap() {
            Decoded::Reply { msg, .. } if msg.sw_id == 0 => Some((t.line, msg.feature_index, msg)),
            _ => None,
        })
        .collect()
}

fn parsed_events(name: &str) -> Vec<(usize, Option<Event>)> {
    let table = feature_table();
    events(&load(name))
        .into_iter()
        .map(|(line, index, msg)| (line, parse_event(table[&index], &msg)))
        .collect()
}

fn battery(percent: u8, status: ChargeStatus) -> Event {
    Event::Battery(Battery {
        level_a: percent,
        level_b: percent,
        status,
    })
}

#[test]
fn launch_reads_parse() {
    let pairs = request_reply_pairs(&load("ghub-launch"));

    let reply = reply_to(&pairs, &battery::get_request(BATTERY));
    let parsed = battery::parse(&reply.params).unwrap();
    assert_eq!(
        parsed,
        Battery {
            level_a: 100,
            level_b: 100,
            status: ChargeStatus::Full
        }
    );
    assert_eq!(parsed.percent(), 100);

    let reply = reply_to(
        &pairs,
        &device_info::firmware_version_request(DEVICE_INFO, 0),
    );
    assert_eq!(
        device_info::parse_firmware_version(&reply.params),
        Ok(FirmwareVersion {
            major: 1,
            minor: 2,
            build: 18,
            name: "main".to_string()
        })
    );

    let reply = reply_to(&pairs, &device_name::get_name_request(DEVICE_NAME));
    assert_eq!(
        device_name::parse_name(&reply.params),
        Ok("A50 X".to_string())
    );

    let reply = reply_to(&pairs, &auto_sleep::get_request(AUTO_SLEEP));
    assert_eq!(
        auto_sleep::parse(&reply.params),
        Ok(auto_sleep::AutoSleep {
            first_minutes: 60,
            second_minutes: 15
        })
    );

    let reply = reply_to(&pairs, &led_brightness::get_request(LED_BRIGHTNESS));
    assert_eq!(led_brightness::parse(&reply.params), Ok(100));

    let reply = reply_to(&pairs, &noise_gate::options_request(NOISE_GATE));
    assert_eq!(
        noise_gate::parse_options(&reply.params),
        Ok(vec![
            NoiseGate::Off,
            NoiseGate::Home,
            NoiseGate::Night,
            NoiseGate::Tournament
        ])
    );
    let reply = reply_to(&pairs, &noise_gate::current_request(NOISE_GATE));
    assert_eq!(noise_gate::parse_current(&reply.params), Ok(NoiseGate::Off));

    let reply = reply_to(&pairs, &sidetone::get_request(SIDETONE));
    assert_eq!(sidetone::parse_level(&reply.params), Ok(Sidetone(0)));
    let reply = reply_to(&pairs, &sidetone::info_request(SIDETONE));
    assert_eq!(
        sidetone::parse_info(&reply.params),
        Ok(SidetoneInfo {
            min_db: -18,
            max_db: 0,
            steps: 7
        })
    );

    let reply = reply_to(&pairs, &headset_mix::get_request(HEADSET_MIX));
    assert_eq!(
        headset_mix::parse_balance(&reply.params),
        Ok(GameVoiceBalance(6))
    );
    let reply = reply_to(&pairs, &headset_mix::info_request(HEADSET_MIX));
    assert_eq!(
        headset_mix::parse_info(&reply.params),
        Ok(BalanceInfo {
            max: 12,
            unknown: [0x05, 0x04]
        })
    );

    let reply = reply_to(&pairs, &mic_test::get_state_request(MIC_TEST));
    let state = mic_test::parse_state(&reply.params).unwrap();
    assert_eq!(state, MicTestState(0x05));
    assert!(!state.is_headset_available());

    let reply = reply_to(&pairs, &bt_host_info::connected_count_request(BT_HOST_INFO));
    assert_eq!(bt_host_info::parse_connected_count(&reply.params), Ok(0));

    let reply = reply_to(&pairs, &mixer_audio::input_state_request(MIXER_AUDIO));
    assert_eq!(mixer_audio::parse_input_state(&reply.params), Ok(0x0a));
}

#[test]
fn dock_and_undock_events_parse() {
    assert_eq!(
        parsed_events("dock-cycle"),
        [
            (2, Some(Event::HeadsetAvailability(true))),
            (3, Some(battery(100, ChargeStatus::Discharging))),
            (4, Some(Event::BluetoothDevices(1))),
            (6, Some(Event::MixerInputs(0x0e))),
            (12, None),
            (13, None),
            (14, Some(Event::Balance(GameVoiceBalance(6)))),
            (15, Some(Event::HeadsetAvailability(false))),
            (16, Some(Event::BluetoothDevices(0))),
            (18, Some(Event::MixerInputs(0x0a))),
            (20, Some(battery(100, ChargeStatus::Charging))),
            (21, None),
        ]
    );
}

#[test]
fn power_cycle_events_parse() {
    let parsed = parsed_events("headset-power");
    let known: Vec<_> = parsed
        .iter()
        .filter_map(|(line, event)| event.clone().map(|e| (*line, e)))
        .collect();
    assert_eq!(
        known,
        [
            (2, Event::Balance(GameVoiceBalance(6))),
            (3, Event::HeadsetAvailability(false)),
            (4, Event::BluetoothDevices(0)),
            (6, Event::MixerInputs(0x0a)),
            (14, Event::HeadsetAvailability(true)),
            (15, battery(100, ChargeStatus::Discharging)),
            (22, Event::BluetoothDevices(1)),
            (24, Event::MixerInputs(0x0e)),
        ]
    );
    assert!(
        parsed.iter().any(|(_, event)| event.is_none()),
        "memfault events must be ignored"
    );
}

fn captured_writes(session_file: &str) -> Vec<[u8; 64]> {
    load_session("session-b", session_file)
        .into_iter()
        .filter(|t| t.dir == Dir::Out)
        .map(|t| t.frame)
        .collect()
}

fn encoded(msg: &Message) -> [u8; 64] {
    encode(A50X_PREFIX, 0, msg).unwrap()
}

fn sidetone_write(position: usize, level: u8) {
    let captured = captured_writes("sidetone")[position];
    let built = sidetone::set_request(SIDETONE, Sidetone(level)).unwrap();
    assert_eq!(encoded(&built), captured);
}

#[test]
fn sidetone_68_encodes_like_ghub() {
    sidetone_write(0, 4);
}

#[test]
fn sidetone_100_encodes_like_ghub() {
    sidetone_write(1, 6);
}

#[test]
fn sidetone_51_encodes_like_ghub() {
    sidetone_write(2, 3);
}

#[test]
fn sidetone_0_encodes_like_ghub() {
    sidetone_write(3, 0);
}

#[test]
fn sidetone_event_parses() {
    let table = feature_table();
    let parsed: Vec<_> = events(&load_session("session-b", "sidetone"))
        .into_iter()
        .map(|(_, index, msg)| parse_event(table[&index], &msg))
        .collect();
    assert_eq!(
        parsed,
        [4, 6, 3, 0].map(|level| Some(Event::Sidetone(Sidetone(level))))
    );
}

fn balance_write(position: usize, value: u8) {
    let captured = captured_writes("balance")[position];
    let built = headset_mix::set_request(HEADSET_MIX, GameVoiceBalance(value)).unwrap();
    assert_eq!(encoded(&built), captured);
}

#[test]
fn balance_12_encodes_like_ghub() {
    balance_write(0, 12);
}

#[test]
fn balance_0_encodes_like_ghub() {
    balance_write(1, 0);
}

#[test]
fn balance_6_encodes_like_ghub() {
    balance_write(2, 6);
}

#[test]
fn balance_event_parses() {
    let table = feature_table();
    let parsed: Vec<_> = events(&load_session("session-b", "balance"))
        .into_iter()
        .map(|(_, index, msg)| parse_event(table[&index], &msg))
        .collect();
    assert_eq!(
        parsed,
        [12, 0, 6].map(|value| Some(Event::Balance(GameVoiceBalance(value))))
    );
}

#[test]
fn sidetone_out_of_range_is_rejected() {
    assert_eq!(
        sidetone::set_request(SIDETONE, Sidetone(7)),
        Err(RangeError { value: 7, max: 6 })
    );
    assert!(sidetone::set_request(SIDETONE, Sidetone(6)).is_ok());
}

#[test]
fn balance_out_of_range_is_rejected() {
    assert_eq!(
        headset_mix::set_request(HEADSET_MIX, GameVoiceBalance(13)),
        Err(RangeError { value: 13, max: 12 })
    );
    assert!(headset_mix::set_request(HEADSET_MIX, GameVoiceBalance(12)).is_ok());
}

#[test]
fn sidetone_percent_mapping_matches_ghub() {
    for (percent, level) in [(68, 4), (100, 6), (51, 3), (0, 0)] {
        assert_eq!(Sidetone::from_percent(percent), Sidetone(level));
    }
    assert_eq!(Sidetone::from_percent(255), Sidetone(6));
    for level in 0..=6 {
        assert_eq!(
            Sidetone::from_percent(Sidetone(level).to_percent()),
            Sidetone(level)
        );
    }
    assert_eq!(Sidetone(6).to_percent(), 100);
    assert_eq!(Sidetone(0).to_percent(), 0);
}

#[test]
fn parse_event_ignores_replies_and_unknown_features() {
    let event = |function, sw_id, params: &[u8]| Message {
        feature_index: 0,
        function,
        sw_id,
        params: params.to_vec(),
    };
    let battery_params = [100, 100, 1];

    assert!(parse_event(FeatureId::BATTERY_SOC, &event(0, 0, &battery_params)).is_some());
    assert_eq!(
        parse_event(FeatureId::BATTERY_SOC, &event(0, 0x0b, &battery_params)),
        None
    );
    assert_eq!(
        parse_event(FeatureId::BATTERY_SOC, &event(1, 0, &battery_params)),
        None
    );
    assert_eq!(
        parse_event(FeatureId::BATTERY_SOC, &event(0, 0, &[100])),
        None
    );

    for ignored in [
        FeatureId::MEMFAULT,
        FeatureId::PROFILE,
        FeatureId::ADVANCED_PARA_EQ,
        FeatureId::VOLUME_SET,
        FeatureId(0x0999),
    ] {
        assert_eq!(
            parse_event(ignored, &event(0, 0, &[1, 1])),
            None,
            "{ignored:?}"
        );
    }
}

#[test]
fn requests_use_the_ghub_software_id() {
    assert_eq!(battery::get_request(BATTERY), request(BATTERY, 0, &[]));
    assert_eq!(
        mic_test::get_state_request(MIC_TEST),
        request(MIC_TEST, 6, &[])
    );
    assert_eq!(
        noise_gate::current_request(NOISE_GATE),
        request(NOISE_GATE, 1, &[])
    );
    assert_eq!(
        noise_gate::options_request(NOISE_GATE),
        request(NOISE_GATE, 3, &[])
    );
}

#[test]
fn charge_status_maps_observed_and_unknown_bytes() {
    assert_eq!(ChargeStatus::from_byte(0), ChargeStatus::Discharging);
    assert_eq!(ChargeStatus::from_byte(1), ChargeStatus::Charging);
    assert_eq!(ChargeStatus::from_byte(3), ChargeStatus::Full);
    assert_eq!(ChargeStatus::from_byte(2), ChargeStatus::Other(2));
    assert_eq!(ChargeStatus::Other(2).to_byte(), 2);
    assert_eq!(ChargeStatus::Full.to_byte(), 3);
}

#[test]
fn mic_test_only_state_five_means_headset_unavailable() {
    for state in 0..=u8::MAX {
        assert_eq!(
            MicTestState(state).is_headset_available(),
            state != 0x05,
            "state {state:#04x}"
        );
    }
}

#[test]
fn malformed_replies_are_errors() {
    assert_eq!(
        battery::parse(&[100, 100]),
        Err(ParseError::TooShort {
            expected: 3,
            actual: 2
        })
    );
    assert_eq!(
        device_info::parse_firmware_version(&[1, 2, 0, 18, 9, b'm']),
        Err(ParseError::TooShort {
            expected: 14,
            actual: 6
        })
    );
    assert_eq!(
        device_name::parse_name(&[30, 2, 0xff, 0xfe]),
        Err(ParseError::InvalidUtf8)
    );
    assert_eq!(
        noise_gate::parse_current(&[3]),
        Err(ParseError::UnknownValue(3))
    );
    assert_eq!(
        noise_gate::parse_options(&[4, 0, 1]),
        Err(ParseError::TooShort {
            expected: 5,
            actual: 3
        })
    );
}
