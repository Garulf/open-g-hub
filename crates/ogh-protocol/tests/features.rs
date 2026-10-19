mod common;

use std::collections::HashMap;

use common::{Dir, Transfer, load, load_session, request_reply_pairs};
use ogh_protocol::discovery::{parse_feature_page, parse_get_feature};
use ogh_protocol::features::advanced_para_eq::{
    self, CustomEq, EqBand, EqInstance, Equalizer, HEADPHONE_DEFAULT, HEADPHONE_GAMING,
    HEADPHONE_MEDIA, MIC_BROADCAST, MIC_COMPETITION, MIC_DEFAULT,
};
use ogh_protocol::features::auto_sleep::{AutoSleep, SleepAfter};
use ogh_protocol::features::battery::{self, Battery, ChargeStatus};
use ogh_protocol::features::device_info::{self, FirmwareVersion};
use ogh_protocol::features::headset_mix::{self, BalanceInfo, GameVoiceBalance};
use ogh_protocol::features::mic_test::{self, MicTestState};
use ogh_protocol::features::mixer_audio::{MixChannel, MixInput, MixLevel, StreamMix};
use ogh_protocol::features::noise_gate::{self, NoiseGate};
use ogh_protocol::features::sidetone::{self, Sidetone, SidetoneInfo};
use ogh_protocol::features::tones::{self, Tones};
use ogh_protocol::features::volume::{self, Volume, VolumeLevel};
use ogh_protocol::features::{
    Event, auto_sleep, bt_host_info, device_name, led_brightness, mixer_audio, parse_event,
};
use ogh_protocol::{
    A50X_PREFIX, Allowed, Decoded, FeatureId, GHUB_SW_ID, Message, ParseError, RangeError, decode,
    encode, request,
};

const BATTERY: u8 = 6;
const DEVICE_INFO: u8 = 4;
const DEVICE_NAME: u8 = 5;
const AUTO_SLEEP: u8 = 7;
const VOLUME: u8 = 8;
const SIDETONE: u8 = 9;
const HEADSET_MIX: u8 = 10;
const MIXER_AUDIO: u8 = 12;
const EQ: u8 = 13;
const BT_HOST_INFO: u8 = 14;
const LED_BRIGHTNESS: u8 = 15;
const TONES: u8 = 16;
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

fn volume_event(scaled: u8, level: u8) -> Event {
    Event::Volume(Volume {
        scaled,
        muted: false,
        level: VolumeLevel(level),
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
        Ok(AutoSleep {
            prefix: 0x3c,
            sleep_after: SleepAfter(15)
        })
    );

    let reply = reply_to(&pairs, &volume::get_request(VOLUME));
    assert_eq!(
        volume::parse(&reply.params),
        Ok(Volume {
            scaled: 0x1f,
            muted: false,
            level: VolumeLevel(21)
        })
    );

    let reply = reply_to(&pairs, &tones::get_request(TONES));
    assert_eq!(tones::parse(&reply.params), Ok(Tones::ALL));

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
            (12, Some(volume_event(0x1d, 20))),
            (13, Some(volume_event(0x1f, 21))),
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
        Err(RangeError {
            what: "sidetone level",
            value: 7.0,
            allowed: Allowed::Between { min: 0.0, max: 6.0 }
        })
    );
    assert!(sidetone::set_request(SIDETONE, Sidetone(6)).is_ok());
}

#[test]
fn balance_out_of_range_is_rejected() {
    assert_eq!(
        headset_mix::set_request(HEADSET_MIX, GameVoiceBalance(13)),
        Err(RangeError {
            what: "balance",
            value: 13.0,
            allowed: Allowed::Between {
                min: 0.0,
                max: 12.0
            }
        })
    );
    assert!(headset_mix::set_request(HEADSET_MIX, GameVoiceBalance(12)).is_ok());
}

#[test]
fn sidetone_from_percent_matches_ghub() {
    for (percent, level) in [(68, 4), (100, 6), (51, 3), (0, 0)] {
        assert_eq!(Sidetone::from_percent(percent), Sidetone(level));
    }
    assert_eq!(Sidetone::from_percent(255), Sidetone(6));
}

#[test]
fn sidetone_to_percent_round_trips_through_from_percent() {
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
        FeatureId::LED_BRIGHTNESS,
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
        noise_gate::parse_current(&[]),
        Err(ParseError::TooShort {
            expected: 1,
            actual: 0
        })
    );
    assert_eq!(
        noise_gate::parse_options(&[4, 0, 1]),
        Err(ParseError::TooShort {
            expected: 5,
            actual: 3
        })
    );
}

fn captured_message(fixture: &str, line: usize) -> Message {
    let transfer = load_session("session-b", fixture)
        .into_iter()
        .find(|t| t.line == line)
        .unwrap_or_else(|| panic!("{fixture} has no line {line}"));
    match decode(A50X_PREFIX, &transfer.frame).unwrap() {
        Decoded::Reply { msg, .. } => msg,
        other => panic!("{fixture} line {line}: {other:?}"),
    }
}

fn assert_encodes_like_ghub(built: &Message, fixture: &str, line: usize) {
    let ghub = captured_message(fixture, line);
    assert_eq!(built.sw_id, GHUB_SW_ID, "{fixture} line {line}");
    assert_eq!(
        (built.feature_index, built.function, &built.params),
        (ghub.feature_index, ghub.function, &ghub.params),
        "{fixture} line {line}"
    );
}

/// OUT messages of one function, with their fixture line.
fn writes(fixture: &str, index: u8, function: u8) -> Vec<(usize, Message)> {
    load_session("session-b", fixture)
        .into_iter()
        .filter(|t| t.dir == Dir::Out)
        .map(|t| (t.line, captured_message(fixture, t.line)))
        .filter(|(_, msg)| msg.feature_index == index && msg.function == function)
        .collect()
}

fn session_b_events(fixture: &str) -> Vec<(usize, Message)> {
    events(&load_session("session-b", fixture))
        .into_iter()
        .map(|(line, _, msg)| (line, msg))
        .collect()
}

fn parsed_session_b_event(fixture: &str, line: usize) -> Option<Event> {
    let table = feature_table();
    let msg = captured_message(fixture, line);
    assert_eq!(msg.sw_id, 0, "{fixture} line {line} is not an event");
    parse_event(table[&msg.feature_index], &msg)
}

fn channel(percent: u8, muted: bool) -> MixChannel {
    MixChannel {
        level: MixLevel::from_percent(percent),
        muted,
    }
}

fn launch_mix() -> StreamMix {
    StreamMix {
        stream: channel(80, false),
        inputs: [
            channel(50, true),
            channel(50, false),
            channel(50, true),
            channel(50, false),
        ],
    }
}

fn mixer_write(line: usize, mix: &StreamMix) {
    let built = mixer_audio::set_request(MIXER_AUDIO, mix).unwrap();
    assert_encodes_like_ghub(&built, "mixer", line);
}

#[test]
fn mix_inputs_use_the_getinputinfo_order() {
    assert_eq!(
        MixInput::ALL.map(MixInput::index),
        [0, 1, 2, 3],
        "Mic Out, Game, Bluetooth, Voice"
    );
    assert_eq!(MixInput::Game.index(), 1);
    assert_eq!(MixInput::Voice.index(), 3);
}

#[test]
fn stream_port_level_0_100_80_encodes_like_ghub() {
    for (line, percent) in [(14, 0), (23, 100), (38, 80)] {
        let mut mix = launch_mix();
        mix.stream.level = MixLevel::from_percent(percent);
        mixer_write(line, &mix);
    }
}

#[test]
fn stream_port_mute_on_off_encodes_like_ghub() {
    for (line, muted) in [(41, true), (44, false)] {
        let mut mix = launch_mix();
        mix.stream.muted = muted;
        mixer_write(line, &mix);
    }
}

#[test]
fn game_level_0_100_50_encodes_like_ghub() {
    for (line, percent) in [(62, 0), (104, 100), (131, 50)] {
        let mut mix = launch_mix();
        mix.input_mut(MixInput::Game).level = MixLevel::from_percent(percent);
        mixer_write(line, &mix);
    }
}

#[test]
fn game_mute_on_off_encodes_like_ghub() {
    for (line, muted) in [(134, true), (137, false)] {
        let mut mix = launch_mix();
        mix.input_mut(MixInput::Game).muted = muted;
        mixer_write(line, &mix);
    }
}

#[test]
fn voice_level_0_100_50_encodes_like_ghub() {
    for (line, percent) in [(161, 0), (209, 100), (233, 50)] {
        let mut mix = launch_mix();
        mix.input_mut(MixInput::Voice).level = MixLevel::from_percent(percent);
        mixer_write(line, &mix);
    }
}

#[test]
fn voice_mute_on_off_encodes_like_ghub() {
    for (line, muted) in [(236, true), (239, false)] {
        let mut mix = launch_mix();
        mix.input_mut(MixInput::Voice).muted = muted;
        mixer_write(line, &mix);
    }
}

#[test]
fn every_mixer_write_round_trips() {
    let writes = writes("mixer", MIXER_AUDIO, 6);
    assert_eq!(writes.len(), 80);
    for (line, ghub) in writes {
        let mix = mixer_audio::parse_settings(&ghub.params)
            .unwrap_or_else(|e| panic!("line {line}: {e}"));
        mixer_write(line, &mix);
    }
}

#[test]
fn mixer_settings_events_echo_each_write() {
    let writes = writes("mixer", MIXER_AUDIO, 6);
    let events = session_b_events("mixer");
    assert_eq!(events.len(), writes.len());
    for ((write_line, write), (event_line, event)) in writes.iter().zip(&events) {
        assert_eq!(*event_line, write_line + 2);
        assert_eq!(event.function, 2, "line {event_line}");
        let expected = mixer_audio::parse_settings(&write.params).unwrap();
        assert_eq!(
            parsed_session_b_event("mixer", *event_line),
            Some(Event::MixerSettings(expected)),
            "line {event_line}"
        );
    }
}

#[test]
fn mixer_input_state_events_still_parse() {
    assert_eq!(
        parsed_events("dock-cycle")
            .into_iter()
            .find(|(line, _)| *line == 6),
        Some((6, Some(Event::MixerInputs(0x0e))))
    );
}

#[test]
fn mix_level_from_percent_matches_ghub() {
    for (percent, level) in [(0, 0x00), (50, 0x10), (80, 0x1a), (100, 0x20)] {
        assert_eq!(
            MixLevel::from_percent(percent),
            MixLevel(level),
            "{percent}"
        );
    }
    assert_eq!(MixLevel::from_percent(255), MixLevel(0x20));
}

#[test]
fn mix_level_to_percent_round_trips_through_from_percent() {
    for level in 0..=0x20 {
        assert_eq!(
            MixLevel::from_percent(MixLevel(level).to_percent()),
            MixLevel(level)
        );
    }
    assert_eq!(MixLevel(0x20).to_percent(), 100);
}

#[test]
fn mixer_level_above_max_is_rejected() {
    let too_loud = RangeError {
        what: "mixer level",
        value: 33.0,
        allowed: Allowed::Between {
            min: 0.0,
            max: 32.0,
        },
    };
    let mut mix = launch_mix();
    mix.stream.level = MixLevel(33);
    assert_eq!(mixer_audio::set_request(MIXER_AUDIO, &mix), Err(too_loud));

    let mut mix = launch_mix();
    mix.input_mut(MixInput::Game).level = MixLevel(33);
    assert_eq!(mixer_audio::set_request(MIXER_AUDIO, &mix), Err(too_loud));

    let mut mix = launch_mix();
    mix.input_mut(MixInput::Voice).level = MixLevel(32);
    assert!(mixer_audio::set_request(MIXER_AUDIO, &mix).is_ok());
}

#[test]
fn malformed_mixer_settings_are_errors() {
    let ghub = captured_message("mixer", 38).params;
    assert!(mixer_audio::parse_settings(&ghub).is_ok());

    assert_eq!(
        mixer_audio::parse_settings(&ghub[..15]),
        Err(ParseError::TooShort {
            expected: 16,
            actual: 15
        })
    );
    for (at, byte) in [(0, 1), (2, 2), (3, 3), (7, 2), (9, 2)] {
        let mut params = ghub.clone();
        params[at] = byte;
        assert_eq!(
            mixer_audio::parse_settings(&params),
            Err(ParseError::UnknownValue(byte)),
            "byte {at} = {byte}"
        );
    }
}

#[test]
fn device_volume_0_50_100_encodes_like_ghub() {
    for (line, percent) in [(2, 0), (5, 50), (8, 100)] {
        let built = volume::set_request(VOLUME, VolumeLevel::from_percent(percent)).unwrap();
        assert_encodes_like_ghub(&built, "small-settings", line);
    }
}

#[test]
fn device_volume_events_parse() {
    for (line, scaled, level) in [(4, 0x00, 0), (7, 0x10, 11), (10, 0x1f, 21)] {
        assert_eq!(
            parsed_session_b_event("small-settings", line),
            Some(volume_event(scaled, level)),
            "line {line}"
        );
    }
}

#[test]
fn volume_from_percent_matches_ghub() {
    for (percent, level) in [(0, 0), (50, 11), (100, 21)] {
        assert_eq!(VolumeLevel::from_percent(percent), VolumeLevel(level));
    }
    assert_eq!(VolumeLevel::from_percent(255), VolumeLevel(21));
    for level in 0..=21 {
        assert_eq!(
            VolumeLevel::from_percent(VolumeLevel(level).to_percent()),
            VolumeLevel(level)
        );
    }
}

#[test]
fn volume_22_is_rejected() {
    assert_eq!(
        volume::set_request(VOLUME, VolumeLevel(22)),
        Err(RangeError {
            what: "volume level",
            value: 22.0,
            allowed: Allowed::Between {
                min: 0.0,
                max: 21.0
            }
        })
    );
}

#[test]
fn base_station_brightness_0_50_100_encodes_like_ghub() {
    for (line, percent) in [(11, 0), (14, 50), (17, 100)] {
        let built = led_brightness::set_request(LED_BRIGHTNESS, percent).unwrap();
        assert_encodes_like_ghub(&built, "small-settings", line);
    }
}

#[test]
fn brightness_writes_are_followed_by_profile_events_only() {
    for line in [13, 16, 19] {
        assert_eq!(parsed_session_b_event("small-settings", line), None);
    }
}

#[test]
fn brightness_101_is_rejected() {
    assert_eq!(
        led_brightness::set_request(LED_BRIGHTNESS, 101),
        Err(RangeError {
            what: "brightness",
            value: 101.0,
            allowed: Allowed::Between {
                min: 0.0,
                max: 100.0
            }
        })
    );
}

#[test]
fn sleep_after_30_never_15_encodes_like_ghub() {
    for (line, sleep_after) in [
        (20, SleepAfter(30)),
        (23, SleepAfter::NEVER),
        (26, SleepAfter(15)),
    ] {
        let built = auto_sleep::set_request(AUTO_SLEEP, sleep_after).unwrap();
        assert_encodes_like_ghub(&built, "small-settings", line);
    }
}

#[test]
fn sleep_timer_events_parse() {
    for (line, minutes) in [(22, 30), (25, 0), (28, 15)] {
        assert_eq!(
            parsed_session_b_event("small-settings", line),
            Some(Event::AutoSleep(AutoSleep {
                prefix: 0x3c,
                sleep_after: SleepAfter(minutes)
            })),
            "line {line}"
        );
    }
}

#[test]
fn sleep_after_values_ghub_never_sent_are_rejected() {
    for minutes in [1, 5, 20, 60, 255] {
        assert_eq!(
            auto_sleep::set_request(AUTO_SLEEP, SleepAfter(minutes)),
            Err(RangeError {
                what: "sleep after (minutes)",
                value: f32::from(minutes),
                allowed: Allowed::OneOf(&[0, 15, 30])
            })
        );
    }
}

#[test]
fn tones_2_0_1_encode_like_ghub() {
    for (line, tones) in [
        (29, Tones(2)),
        (34, Tones(2)),
        (36, Tones(0)),
        (41, Tones(0)),
        (43, Tones::ALL),
        (48, Tones::ALL),
    ] {
        let built = tones::set_request(TONES, tones).unwrap();
        assert_encodes_like_ghub(&built, "small-settings", line);
    }
    assert_eq!(Tones::ALL, Tones(1));
}

#[test]
fn tones_reads_and_events_parse() {
    for (read, reply, event, value) in [(31, 33, 32, 2), (38, 40, 39, 0), (46, 47, 45, 1)] {
        assert_encodes_like_ghub(&tones::get_request(TONES), "small-settings", read);
        assert_eq!(
            tones::parse(&captured_message("small-settings", reply).params),
            Ok(Tones(value))
        );
        assert_eq!(
            parsed_session_b_event("small-settings", event),
            Some(Event::Tones(Tones(value)))
        );
    }
}

#[test]
fn tones_3_is_rejected() {
    assert_eq!(
        tones::set_request(TONES, Tones(3)),
        Err(RangeError {
            what: "tones",
            value: 3.0,
            allowed: Allowed::OneOf(&[0, 1, 2])
        })
    );
}

#[test]
fn noise_gate_night_home_tournament_off_encodes_like_ghub() {
    for (line, gate) in [
        (2, NoiseGate::Night),
        (5, NoiseGate::Home),
        (8, NoiseGate::Tournament),
        (11, NoiseGate::Off),
    ] {
        let built = noise_gate::set_request(NOISE_GATE, gate).unwrap();
        assert_encodes_like_ghub(&built, "eq-mic", line);
    }
}

#[test]
fn noise_gate_events_parse() {
    for (line, gate) in [
        (4, NoiseGate::Night),
        (7, NoiseGate::Home),
        (10, NoiseGate::Tournament),
        (13, NoiseGate::Off),
    ] {
        assert_eq!(
            parsed_session_b_event("eq-mic", line),
            Some(Event::NoiseGate(gate)),
            "line {line}"
        );
    }
}

#[test]
fn noise_gate_3_is_rejected() {
    assert_eq!(
        noise_gate::set_request(NOISE_GATE, NoiseGate::Other(3)),
        Err(RangeError {
            what: "noise gate",
            value: 3.0,
            allowed: Allowed::OneOf(&[0, 1, 2, 4])
        })
    );
}

#[test]
fn unknown_noise_gate_options_are_kept_when_read() {
    assert_eq!(
        noise_gate::parse_options(&[5, 0, 1, 2, 4, 3]),
        Ok(vec![
            NoiseGate::Off,
            NoiseGate::Home,
            NoiseGate::Night,
            NoiseGate::Tournament,
            NoiseGate::Other(3)
        ])
    );
    assert_eq!(noise_gate::parse_current(&[6]), Ok(NoiseGate::Other(6)));
    assert_eq!(NoiseGate::from_byte(4), NoiseGate::Tournament);
    assert_eq!(NoiseGate::Other(6).to_byte(), 6);
    assert_eq!(NoiseGate::Night.to_byte(), 2);
}

fn eq_write(fixture: &str, line: usize, instance: EqInstance, eq: &Equalizer) {
    let built = advanced_para_eq::set_custom_request(EQ, instance, eq).unwrap();
    assert_encodes_like_ghub(&built, fixture, line);
}

fn graphic(eq: Equalizer) -> Equalizer {
    Equalizer {
        bands: eq.bands.map(|band| EqBand { q: None, ..band }),
    }
}

fn with_band(mut eq: Equalizer, band: usize, freq_hz: u16, gain_db: f32) -> Equalizer {
    eq.bands[band].freq_hz = freq_hz;
    eq.bands[band].gain_db = gain_db;
    eq
}

#[test]
fn headphone_curated_presets_encode_like_ghub() {
    for (line, preset) in [
        (2, HEADPHONE_GAMING),
        (7, HEADPHONE_MEDIA),
        (12, HEADPHONE_DEFAULT),
    ] {
        eq_write("eq-headphone", line, EqInstance::Headphone, &preset);
    }
}

#[test]
fn mic_curated_presets_encode_like_ghub() {
    for (line, preset) in [
        (14, MIC_COMPETITION),
        (19, MIC_BROADCAST),
        (84, MIC_BROADCAST),
    ] {
        eq_write("eq-mic", line, EqInstance::Mic, &preset);
    }
}

#[test]
fn mic_default_preset_matches_the_flat_mic_cards() {
    eq_write("eq-mic", 29, EqInstance::Mic, &MIC_DEFAULT);
    eq_write("eq-mic", 24, EqInstance::Mic, &graphic(MIC_DEFAULT));
}

#[test]
fn presets_match_the_factory_slots_read_at_launch() {
    let pairs = request_reply_pairs(&load("ghub-launch"));
    for (instance, slot, preset) in [
        (0, 0, graphic(MIC_DEFAULT)),
        (0, 1, graphic(MIC_COMPETITION)),
        (0, 2, graphic(MIC_BROADCAST)),
        (1, 0, HEADPHONE_DEFAULT),
        (1, 1, HEADPHONE_GAMING),
        (1, 2, HEADPHONE_MEDIA),
    ] {
        let reply = reply_to(&pairs, &request(EQ, 5, &[instance, slot]));
        let card = [&[instance, 3, 0][..], &reply.params].concat();
        assert_eq!(
            advanced_para_eq::parse_custom_eq(&card).map(|custom| custom.equalizer),
            Ok(preset),
            "instance {instance} slot {slot}"
        );
    }
}

#[test]
fn headphone_graphic_custom_edits_encode_like_ghub() {
    let flat = graphic(HEADPHONE_DEFAULT);
    let minus_3_at_1k = with_band(flat, 5, 1000, -3.0);
    let plus_6_at_2k5 = with_band(minus_3_at_1k, 6, 2500, 6.0);
    for (line, eq) in [(17, flat), (37, minus_3_at_1k), (47, plus_6_at_2k5)] {
        eq_write("eq-headphone", line, EqInstance::Headphone, &eq);
    }
}

#[test]
fn headphone_advanced_band_6_drag_encodes_like_ghub() {
    for (line, freq_hz, gain_db) in [(67, 1245, 1.7), (72, 1347, 2.85)] {
        let eq = with_band(HEADPHONE_DEFAULT, 5, freq_hz, gain_db);
        eq_write("eq-headphone", line, EqInstance::Headphone, &eq);
    }
}

#[test]
fn mic_advanced_band_edits_encode_like_ghub() {
    let band_5 = with_band(MIC_DEFAULT, 4, 809, 2.9);
    let band_1_at_60 = with_band(band_5, 0, 60, 0.0);
    for (line, eq) in [(74, band_5), (79, band_1_at_60)] {
        eq_write("eq-mic", line, EqInstance::Mic, &eq);
    }
}

#[test]
fn every_eq_write_round_trips() {
    for (fixture, count) in [("eq-headphone", 15), ("eq-mic", 15)] {
        let writes = writes(fixture, EQ, 2);
        assert_eq!(writes.len(), count, "{fixture}");
        for (line, ghub) in writes {
            let custom = advanced_para_eq::parse_custom_eq(&ghub.params)
                .unwrap_or_else(|e| panic!("{fixture} line {line}: {e}"));
            assert_eq!(custom.slot, 3);
            assert_eq!(custom.leading, 0);
            eq_write(fixture, line, custom.instance, &custom.equalizer);
        }
    }
}

#[test]
fn eq_events_echo_each_write() {
    for fixture in ["eq-headphone", "eq-mic"] {
        let eq_events: Vec<_> = session_b_events(fixture)
            .into_iter()
            .filter(|(_, msg)| msg.feature_index == EQ)
            .collect();
        let writes = writes(fixture, EQ, 2);
        assert_eq!(eq_events.len(), writes.len(), "{fixture}");
        for ((_, write), (line, _)) in writes.iter().zip(&eq_events) {
            let expected: CustomEq = advanced_para_eq::parse_custom_eq(&write.params).unwrap();
            assert_eq!(
                parsed_session_b_event(fixture, *line),
                Some(Event::CustomEq(expected)),
                "{fixture} line {line}"
            );
        }
    }
}

#[test]
fn eq_writes_are_followed_by_profile_events_that_stay_unparsed() {
    for line in [4, 5] {
        assert_eq!(parsed_session_b_event("eq-headphone", line), None);
    }
}

fn eq_rejection(instance: EqInstance, eq: &Equalizer) -> RangeError {
    advanced_para_eq::set_custom_request(EQ, instance, eq).unwrap_err()
}

#[test]
fn eq_gain_out_of_range_is_rejected() {
    for gain_db in [6.05, -6.05, f32::NAN] {
        let error = eq_rejection(
            EqInstance::Headphone,
            &with_band(HEADPHONE_DEFAULT, 3, 250, gain_db),
        );
        assert_eq!(error.what, "EQ gain (dB)");
        assert!(error.value.is_nan() || error.value == gain_db);
        assert_eq!(
            error.allowed,
            Allowed::Between {
                min: -6.0,
                max: 6.0
            }
        );
    }
    for gain_db in [6.0, -6.0] {
        let eq = with_band(HEADPHONE_DEFAULT, 3, 250, gain_db);
        assert!(advanced_para_eq::set_custom_request(EQ, EqInstance::Headphone, &eq).is_ok());
    }
}

#[test]
fn eq_q_out_of_range_is_rejected() {
    for q in [8.0, 0.01, 0.0] {
        let mut eq = HEADPHONE_DEFAULT;
        eq.bands[2].q = Some(q);
        assert_eq!(
            eq_rejection(EqInstance::Headphone, &eq),
            RangeError {
                what: "EQ Q",
                value: q,
                allowed: Allowed::Between {
                    min: 0.031,
                    max: 7.969
                }
            }
        );
    }
    for (q, raw) in [(0.031, 0x01), (7.969, 0xff)] {
        let mut eq = HEADPHONE_DEFAULT;
        eq.bands[2].q = Some(q);
        let built = advanced_para_eq::set_custom_request(EQ, EqInstance::Headphone, &eq).unwrap();
        assert_eq!(built.params[3 + 2 * 5 + 2], raw);
    }
}

#[test]
fn eq_frequency_out_of_range_is_rejected() {
    for (instance, preset, freq_hz, min) in [
        (EqInstance::Headphone, HEADPHONE_DEFAULT, 19, 20.0),
        (EqInstance::Headphone, HEADPHONE_DEFAULT, 20001, 20.0),
        (EqInstance::Mic, MIC_DEFAULT, 59, 60.0),
        (EqInstance::Mic, MIC_DEFAULT, 20001, 60.0),
    ] {
        assert_eq!(
            eq_rejection(instance, &with_band(preset, 0, freq_hz, 0.0)),
            RangeError {
                what: "EQ frequency (Hz)",
                value: f32::from(freq_hz),
                allowed: Allowed::Between { min, max: 20000.0 }
            },
            "{instance:?} {freq_hz}"
        );
    }
}

#[test]
fn malformed_custom_eq_is_an_error() {
    let ghub = captured_message("eq-headphone", 72).params;
    assert_eq!(
        advanced_para_eq::parse_custom_eq(&ghub[..52]),
        Err(ParseError::TooShort {
            expected: 53,
            actual: 52
        })
    );
    let mut params = ghub.clone();
    params[0] = 2;
    assert_eq!(
        advanced_para_eq::parse_custom_eq(&params),
        Err(ParseError::UnknownValue(2))
    );
}

#[test]
fn range_errors_say_what_was_wrong() {
    let above = sidetone::set_request(SIDETONE, Sidetone(7)).unwrap_err();
    assert_eq!(
        above.to_string(),
        "sidetone level 7 is above the maximum of 6"
    );
    let below = eq_rejection(
        EqInstance::Headphone,
        &with_band(HEADPHONE_DEFAULT, 0, 19, 0.0),
    );
    assert_eq!(
        below.to_string(),
        "EQ frequency (Hz) 19 is below the minimum of 20"
    );
    let gain = eq_rejection(
        EqInstance::Headphone,
        &with_band(HEADPHONE_DEFAULT, 0, 20, 6.05),
    );
    assert_eq!(
        gain.to_string(),
        "EQ gain (dB) 6.05 is above the maximum of 6"
    );
    let tones = tones::set_request(TONES, Tones(3)).unwrap_err();
    assert_eq!(tones.to_string(), "tones 3 is not one of 0, 1, 2");
}

#[test]
fn volume_mute_is_bit_0_of_the_middle_byte_as_the_agent_reads_it() {
    for (byte, muted) in [(0x00, false), (0x01, true), (0x02, false), (0x03, true)] {
        assert_eq!(
            volume::parse(&[0x1f, byte, 0x15]).map(|volume| volume.muted),
            Ok(muted),
            "{byte:#04x}"
        );
    }
}

#[test]
fn decoded_eq_gains_equal_the_decimal_values() {
    for (fixture, line, band, gain_db) in [
        ("eq-headphone", 67, 5, 1.7),
        ("eq-headphone", 72, 5, 2.85),
        ("eq-mic", 74, 4, 2.9),
        ("eq-headphone", 2, 2, -3.5),
    ] {
        let custom =
            advanced_para_eq::parse_custom_eq(&captured_message(fixture, line).params).unwrap();
        assert_eq!(
            custom.equalizer.bands[band].gain_db, gain_db,
            "{fixture} line {line}"
        );
    }
}

#[test]
fn mic_out_and_bluetooth_are_pinned_to_the_captured_bytes() {
    assert_eq!(
        mixer_audio::PINNED_INPUT,
        MixChannel {
            level: MixLevel(0x10),
            muted: true
        }
    );
    assert!(MixInput::MicOut.is_pinned());
    assert!(MixInput::Bluetooth.is_pinned());
    assert!(!MixInput::Game.is_pinned());
    assert!(!MixInput::Voice.is_pinned());
    assert_eq!(
        launch_mix().input(MixInput::MicOut),
        mixer_audio::PINNED_INPUT
    );

    let mut mix = launch_mix();
    *mix.input_mut(MixInput::MicOut) = channel(100, false);
    assert_eq!(
        mixer_audio::set_request(MIXER_AUDIO, &mix),
        Err(RangeError {
            what: "Mic Out level (pinned)",
            value: 32.0,
            allowed: Allowed::OneOf(&[0x10])
        })
    );

    for changed in [channel(50, false), channel(0, true), channel(100, true)] {
        for input in [MixInput::MicOut, MixInput::Bluetooth] {
            let mut mix = launch_mix();
            *mix.input_mut(input) = changed;
            assert!(
                mixer_audio::set_request(MIXER_AUDIO, &mix).is_err(),
                "{input:?} {changed:?}"
            );
        }
    }
}

#[test]
fn set_request_refuses_changes_to_exactly_the_inputs_is_pinned_reports() {
    for input in MixInput::ALL {
        let mut mix = launch_mix();
        *mix.input_mut(input) = channel(0, false);
        assert_eq!(
            mixer_audio::set_request(MIXER_AUDIO, &mix).is_err(),
            input.is_pinned(),
            "{input:?}"
        );
    }
}

#[test]
fn eq_cards_mixing_graphic_and_parametric_bands_are_rejected() {
    let mut parametric = HEADPHONE_DEFAULT;
    parametric.bands[4].q = None;
    assert_eq!(
        eq_rejection(EqInstance::Headphone, &parametric),
        RangeError {
            what: "EQ Q",
            value: 0.0,
            allowed: Allowed::Between {
                min: 0.031,
                max: 7.969
            }
        }
    );

    let mut graphic_card = graphic(HEADPHONE_DEFAULT);
    graphic_card.bands[4].q = Some(0.6875);
    assert_eq!(
        eq_rejection(EqInstance::Headphone, &graphic_card),
        RangeError {
            what: "EQ Q on a graphic card",
            value: 0.6875,
            allowed: Allowed::Between { min: 0.0, max: 0.0 }
        }
    );
}

#[test]
fn eq_frequencies_must_ascend() {
    for (band, freq_hz, previous) in [(1, 20, 20.0), (1, 19, 20.0), (9, 9999, 10000.0)] {
        let error = eq_rejection(
            EqInstance::Headphone,
            &with_band(HEADPHONE_DEFAULT, band, freq_hz, 0.0),
        );
        assert_eq!(
            error,
            RangeError {
                what: "EQ frequency (Hz)",
                value: f32::from(freq_hz),
                allowed: Allowed::Between {
                    min: previous + 1.0,
                    max: 20000.0
                }
            },
            "band {band} at {freq_hz}"
        );
    }
    let mut reversed = HEADPHONE_DEFAULT;
    reversed.bands.reverse();
    assert!(advanced_para_eq::set_custom_request(EQ, EqInstance::Headphone, &reversed).is_err());
}

#[test]
fn eq_gain_off_the_grid_rounds_to_the_nearest_step() {
    for (gain_db, raw) in [
        (2.87, 177),
        (2.88, 178),
        (2.874, 177),
        (2.876, 178),
        (-0.01, 120),
    ] {
        let eq = with_band(HEADPHONE_DEFAULT, 5, 1000, gain_db);
        let built = advanced_para_eq::set_custom_request(EQ, EqInstance::Headphone, &eq).unwrap();
        let at = 3 + 5 * 5 + 3;
        assert_eq!(
            u16::from_be_bytes([built.params[at], built.params[at + 1]]),
            raw,
            "{gain_db} dB"
        );
    }
}
