mod common;

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::{
    LAUNCH, LAUNCH_OPEN_REPLIES, LAUNCH_OPEN_REQUESTS, SHORT_TIMEOUT, all_fixtures, empty_reply_to,
    feature_ids, fixture_path, frame_at, frame_from_hex, index_in, load, open_scripted, send_built,
    sent_calls,
};
use ogh_device::allowlist::Allowlist;
use ogh_device::testing::{self, ScriptedTransport};
use ogh_device::{DeviceError, Event, FeatureId, Refusal, Session};
use ogh_protocol::features::advanced_para_eq::{
    self, EqBand, EqInstance, Equalizer, MAX_GAIN_DB, MAX_Q, MIN_GAIN_DB, MIN_Q,
};
use ogh_protocol::features::auto_sleep::{self, SleepAfter};
use ogh_protocol::features::battery;
use ogh_protocol::features::headset_mix::{self, GameVoiceBalance};
use ogh_protocol::features::led_brightness;
use ogh_protocol::features::mixer_audio::{self, MixChannel, MixInput, MixLevel, StreamMix};
use ogh_protocol::features::noise_gate::{self, NoiseGate};
use ogh_protocol::features::sidetone::{self, Sidetone};
use ogh_protocol::features::tones::{self, Tones};
use ogh_protocol::features::volume::{self, VolumeLevel};
use ogh_protocol::{A50X_PREFIX, Message, ParseError, RangeError, encode, request};

const DOCK_CYCLE: &str = "session-a/dock-cycle";
const BATTERY_REQUEST: usize = 38;
const BATTERY_REPLY: usize = 39;
const SIDETONE_INFO_REPLY: usize = 87;
const SIDETONE_LEVEL_REPLY: usize = 89;
const SET_CUSTOM_EQ_REQUEST: usize = 114;
const SET_CUSTOM_EQ_REPLY: usize = 115;
const MIC_TEST_GET_STATE_REPLY_NOT_CONNECTED: usize = 133;
const LIFT_MIC_TEST_EVENT: usize = 2;
const LIFT_BATTERY_EVENT: usize = 3;
const BT_COUNT_REQUEST: usize = 5;
const MIXER_EVENT_DURING_BT_REQUEST: usize = 6;
const BT_COUNT_REPLY: usize = 7;
const DOCK_MIC_TEST_EVENT: usize = 15;
const MEMFAULT_EVENT: usize = 21;
const MEMFAULT_FIRST_FRAGMENT: usize = 23;
const MEMFAULT_CONTINUATION: usize = 24;

fn assert_not_allowlisted_without_writing(
    session: &mut Session<ScriptedTransport>,
    device: &ScriptedTransport,
    id: FeatureId,
    function: u8,
    params: &[u8],
) {
    let before = device.writes().len();
    assert_eq!(
        session.request(id, function, params),
        Err(DeviceError::NotAllowed {
            feature: id,
            function
        })
    );
    assert_eq!(device.writes().len(), before);
}

fn assert_params_refused_without_writing(
    session: &mut Session<ScriptedTransport>,
    device: &ScriptedTransport,
    id: FeatureId,
    function: u8,
    params: &[u8],
) {
    let before = device.writes().len();
    assert_eq!(
        session.request(id, function, params),
        Err(DeviceError::Refused {
            feature: id,
            function,
            reason: Refusal::Params
        })
    );
    assert_eq!(device.writes().len(), before);
}

fn embedded_allowlist_text() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("allowlist.toml");
    fs::read_to_string(path).expect("allowlist.toml")
}

fn with_extra_allow(feature: u16, function: u8) -> String {
    format!(
        "{}\n[[allow]]\nfeature = {feature:#06x}\nfunction = {function}\nseen_in = \"{LAUNCH}\"\n",
        embedded_allowlist_text()
    )
}

#[test]
fn launch_replies_in_testing_match_the_fixture() {
    let scripted: Vec<_> = testing::LAUNCH_OPEN_REPLIES
        .iter()
        .map(|hex| testing::frame_from_hex(hex))
        .collect();
    let captured: Vec<_> = LAUNCH_OPEN_REPLIES
        .iter()
        .map(|&line| frame_at(LAUNCH, line))
        .collect();
    assert_eq!(scripted, captured);
}

#[test]
fn open_discovers_the_launch_feature_table() {
    let (session, device) = open_scripted();

    let ghub_requests: Vec<_> = LAUNCH_OPEN_REQUESTS
        .iter()
        .map(|&line| frame_at(LAUNCH, line))
        .collect();
    assert_eq!(device.writes(), ghub_requests);

    let table: Vec<(u8, u16)> = session
        .features()
        .iter()
        .map(|f| (f.index, f.id.0))
        .collect();
    assert_eq!(table.len(), 24);
    assert_eq!(table[0], (0, 0x0000));
    assert_eq!(table[1], (1, 0x0001));
    assert_eq!(table[6], (6, 0x0104));
    assert_eq!(table[23], (23, 0x0807));
    assert_eq!(device.pending_reads(), 0);
}

#[test]
fn open_reads_headset_availability() {
    let (mut session, device) = open_scripted();
    device.push_timeout();

    assert_eq!(
        session.request(FeatureId::BATTERY_SOC, 0, &[]),
        Err(DeviceError::HeadsetDisconnected)
    );
}

fn discovery_only_device() -> ScriptedTransport {
    let device = ScriptedTransport::new();
    for hex in &testing::LAUNCH_OPEN_REPLIES[..4] {
        device.push_frame(testing::frame_from_hex(hex));
    }
    device
}

#[test]
fn open_survives_a_timed_out_availability_read() {
    let device = discovery_only_device();
    device.push_timeout();

    let mut session =
        Session::with_transport_and_timeout(device.clone(), SHORT_TIMEOUT).expect("open succeeds");

    device.push_frame(frame_from_hex("020c0400136b00"));
    assert_eq!(session.headset_available(), Ok(true));
}

#[test]
fn open_survives_a_rejected_availability_read() {
    let device = discovery_only_device();
    device.push_frame(frame_from_hex("020c0600ff136b05"));

    let mut session =
        Session::with_transport_and_timeout(device.clone(), SHORT_TIMEOUT).expect("open succeeds");

    device.push_frame(frame_at(LAUNCH, MIC_TEST_GET_STATE_REPLY_NOT_CONNECTED));
    assert_eq!(session.headset_available(), Ok(false));
}

#[test]
fn open_still_fails_when_the_availability_read_loses_the_device() {
    let device = discovery_only_device();
    device.push_failure();

    let result = Session::with_transport_and_timeout(device, SHORT_TIMEOUT);

    assert_eq!(result.err(), Some(DeviceError::Disconnected));
}

#[test]
fn discovery_rejects_a_repeated_feature_id() {
    let device = ScriptedTransport::new();
    device.push_frame(frame_at(LAUNCH, 5));
    device.push_frame(frame_from_hex("020c0400010b04"));
    device.push_frame(frame_from_hex("020c0c00011b02 01040004 01040004"));

    let result = Session::with_transport(device);

    assert!(
        matches!(&result, Err(DeviceError::Io(message)) if message.contains("0x0104")),
        "{:?}",
        result.err()
    );
}

#[test]
fn request_returns_matching_reply() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_at(LAUNCH, BATTERY_REPLY));

    let params = session.request(FeatureId::BATTERY_SOC, 0, &[]).unwrap();

    assert_eq!(params, [0x64, 0x64, 0x03]);
    assert_eq!(
        device.writes().last(),
        Some(&frame_at(LAUNCH, BATTERY_REQUEST))
    );
}

#[test]
fn event_between_request_and_reply_is_queued() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_at(DOCK_CYCLE, MIXER_EVENT_DURING_BT_REQUEST));
    device.push_frame(frame_at(DOCK_CYCLE, BT_COUNT_REPLY));

    let params = session.request(FeatureId::BT_HOST_INFO, 0, &[]).unwrap();

    assert_eq!(params, [0x01]);
    assert_eq!(
        device.writes().last(),
        Some(&frame_at(DOCK_CYCLE, BT_COUNT_REQUEST))
    );
    assert_eq!(session.take_events(), [Event::MixerInputs(0x0e)]);
    assert_eq!(session.take_events(), []);
}

#[test]
fn event_queued_during_request_reaches_next_event() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_at(DOCK_CYCLE, MIXER_EVENT_DURING_BT_REQUEST));
    device.push_frame(frame_at(DOCK_CYCLE, BT_COUNT_REPLY));
    session.request(FeatureId::BT_HOST_INFO, 0, &[]).unwrap();

    assert_eq!(session.next_event(10), Ok(Some(Event::MixerInputs(0x0e))));
}

#[test]
fn event_with_the_request_index_and_function_is_not_the_reply() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_at(DOCK_CYCLE, LIFT_BATTERY_EVENT));
    device.push_frame(frame_at(LAUNCH, BATTERY_REPLY));

    let params = session.request(FeatureId::BATTERY_SOC, 0, &[]).unwrap();

    assert_eq!(params, [0x64, 0x64, 0x03]);
    assert!(matches!(session.take_events()[..], [Event::Battery(_)]));
}

#[test]
fn reply_from_another_function_is_not_the_reply() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_at(LAUNCH, SIDETONE_INFO_REPLY));
    device.push_frame(frame_at(LAUNCH, SIDETONE_LEVEL_REPLY));

    let params = session
        .request(FeatureId::HEADSET_SIDETONE, 0, &[])
        .unwrap();

    assert_eq!(params, [0x01, 0x01, 0xff, 0x00]);
}

#[test]
fn late_reply_to_a_timed_out_request_is_not_the_next_reply() {
    let (mut session, device) = open_scripted();
    device.push_timeout();
    assert!(
        session
            .request(FeatureId::ADVANCED_PARA_EQ, 1, &[0, 3])
            .is_err()
    );

    device.push_arrived(frame_from_hex("020c05000d1baaaa"));
    device.push_frame(frame_from_hex("020c05000d1bbbbb"));

    assert_eq!(
        session.request(FeatureId::ADVANCED_PARA_EQ, 1, &[1, 3]),
        Ok(vec![0xbb, 0xbb])
    );
}

#[test]
fn events_already_waiting_before_a_request_are_kept() {
    let (mut session, device) = open_scripted();
    device.push_arrived(frame_at(DOCK_CYCLE, LIFT_MIC_TEST_EVENT));
    device.push_frame(frame_at(LAUNCH, BATTERY_REPLY));

    session.request(FeatureId::BATTERY_SOC, 0, &[]).unwrap();

    assert_eq!(session.take_events(), [Event::HeadsetAvailability(true)]);
}

#[test]
fn request_timeout_defaults_to_three_seconds() {
    let session = Session::with_transport(ScriptedTransport::with_launch_discovery()).unwrap();

    assert_eq!(session.request_timeout(), Duration::from_secs(3));
}

#[test]
fn huge_request_timeout_is_clamped_instead_of_overflowing() {
    let (mut session, device) = open_scripted();
    session.set_request_timeout(Duration::MAX);
    device.push_frame(frame_at(LAUNCH, BATTERY_REPLY));

    assert!(session.request_timeout() <= Duration::from_secs(3600));
    assert_eq!(
        session.request(FeatureId::BATTERY_SOC, 0, &[]),
        Ok(vec![0x64, 0x64, 0x03])
    );
}

#[test]
fn reply_that_arrives_with_the_write_is_returned_not_drained() {
    let (mut session, device) = open_scripted();
    device.push_reply(frame_at(LAUNCH, BATTERY_REPLY));

    assert_eq!(
        session.request(FeatureId::BATTERY_SOC, 0, &[]),
        Ok(vec![0x64, 0x64, 0x03])
    );
}

#[test]
fn drain_before_a_write_is_bounded() {
    let (mut session, device) = open_scripted();
    let chatty = 200;
    for _ in 0..chatty {
        device.push_arrived(frame_at(DOCK_CYCLE, LIFT_MIC_TEST_EVENT));
    }
    device.push_reply(frame_at(LAUNCH, BATTERY_REPLY));

    assert_eq!(
        session.request(FeatureId::BATTERY_SOC, 0, &[]),
        Ok(vec![0x64, 0x64, 0x03])
    );

    let pending_at_write = *device.pending_reads_at_writes().last().unwrap();
    assert!(pending_at_write > chatty + 1 - 64 - 1, "{pending_at_write}");
    assert_eq!(session.take_events().len(), chatty);
}

#[test]
fn request_deadline_is_wall_time_across_reads() {
    let (mut session, device) = open_scripted();
    for _ in 0..20 {
        device.push_frame(frame_from_hex("020c00"));
    }
    device.push_frame(frame_at(LAUNCH, BATTERY_REPLY));
    assert!(session.request(FeatureId::BATTERY_SOC, 0, &[]).is_ok());

    session.set_request_timeout(Duration::from_millis(200));
    device.push_timeout();
    let started = Instant::now();
    assert!(session.request(FeatureId::BATTERY_SOC, 0, &[]).is_err());
    let elapsed = started.elapsed();

    assert!(elapsed >= Duration::from_millis(200), "{elapsed:?}");
    assert!(elapsed < Duration::from_millis(400), "{elapsed:?}");
}

#[test]
fn error_frame_becomes_rejected() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_from_hex("020c0600ff090b05"));
    device.push_frame(frame_from_hex("020c0600ff060b05"));

    let result = session.request(FeatureId::BATTERY_SOC, 0, &[]);

    assert_eq!(
        result,
        Err(DeviceError::Rejected {
            feature_index: 6,
            code: 5
        })
    );
}

#[test]
fn unlisted_function_is_refused_without_writing() {
    let (mut session, device) = open_scripted();
    let before = device.writes().len();

    assert_not_allowlisted_without_writing(&mut session, &device, FeatureId::BATTERY_SOC, 1, &[]);
    assert_not_allowlisted_without_writing(&mut session, &device, FeatureId::DEVICE_INFO, 2, &[]);
    assert_eq!(device.writes().len(), before);
}

#[test]
fn denied_feature_is_refused_without_writing() {
    let (mut session, device) = open_scripted();

    for function in [0, 1] {
        assert_not_allowlisted_without_writing(
            &mut session,
            &device,
            FeatureId::MEMFAULT,
            function,
            &[0],
        );
    }
}

#[test]
fn builder_targeting_another_index_is_refused_without_writing() {
    let (mut session, device) = open_scripted();
    let before = device.writes().len();
    let dfu_index = 2;

    assert_eq!(
        session.call(FeatureId::HEADSET_SIDETONE, |_| request(dfu_index, 1, &[])),
        Err(DeviceError::Refused {
            feature: FeatureId::HEADSET_SIDETONE,
            function: 1,
            reason: Refusal::FeatureIndex
        })
    );
    assert_eq!(device.writes().len(), before);
}

#[test]
fn builder_with_another_sw_id_is_refused_without_writing() {
    let (mut session, device) = open_scripted();
    let before = device.writes().len();

    let result = session.call(FeatureId::BATTERY_SOC, |index| Message {
        feature_index: index,
        function: 0,
        sw_id: 0,
        params: Vec::new(),
    });

    assert_eq!(
        result,
        Err(DeviceError::Refused {
            feature: FeatureId::BATTERY_SOC,
            function: 0,
            reason: Refusal::SwId
        })
    );
    assert_eq!(device.writes().len(), before);
}

#[test]
fn set_custom_eq_to_a_factory_slot_is_refused_without_writing() {
    let (mut session, device) = open_scripted();

    for slot in [0, 1, 2, 4, 0xff] {
        assert_params_refused_without_writing(
            &mut session,
            &device,
            FeatureId::ADVANCED_PARA_EQ,
            2,
            &[1, slot, 0],
        );
    }
    assert_params_refused_without_writing(
        &mut session,
        &device,
        FeatureId::ADVANCED_PARA_EQ,
        2,
        &[1],
    );
}

#[test]
fn set_custom_eq_to_the_custom_slot_is_sent() {
    let (mut session, device) = open_scripted();
    let ghub_write = frame_at(LAUNCH, SET_CUSTOM_EQ_REQUEST);
    let params = &ghub_write[6..usize::from(ghub_write[2]) + 3];
    device.push_frame(frame_at(LAUNCH, SET_CUSTOM_EQ_REPLY));

    assert_eq!(
        session.request(FeatureId::ADVANCED_PARA_EQ, 2, params),
        Ok(vec![])
    );
    assert_eq!(device.writes().last(), Some(&ghub_write));
}

#[test]
fn try_call_reports_a_range_error_as_invalid_value() {
    let (mut session, device) = open_scripted();
    let before = device.writes().len();
    let range_error = sidetone::set_request(0, Sidetone(7)).unwrap_err();

    let result = session.try_call(FeatureId::HEADSET_SIDETONE, |index| {
        sidetone::set_request(index, Sidetone(7))
    });

    assert_eq!(
        result,
        Err(DeviceError::InvalidValue(range_error.to_string()))
    );
    assert_eq!(device.writes().len(), before);
}

#[test]
fn try_call_sends_a_valid_message() {
    let (mut session, device) = open_scripted();
    let index = index_in(&session, FeatureId::HEADSET_SIDETONE);
    let msg = sidetone::set_request(index, Sidetone(3)).unwrap();
    let frame = encode(A50X_PREFIX, 0, &msg).unwrap();
    device.push_frame(empty_reply_to(&frame));

    let result = session.try_call(FeatureId::HEADSET_SIDETONE, |index| {
        sidetone::set_request(index, Sidetone(3))
    });

    assert_eq!(result, Ok(vec![]));
    assert_eq!(device.writes().last(), Some(&frame));
}

#[test]
fn invalid_value_displays_its_detail() {
    assert_eq!(
        DeviceError::InvalidValue("EQ gain 7 is above the maximum of 6".to_string()).to_string(),
        "invalid value: EQ gain 7 is above the maximum of 6"
    );
}

#[test]
fn read_parses_the_reply() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_at(LAUNCH, BATTERY_REPLY));

    let battery = session.read(FeatureId::BATTERY_SOC, battery::get_request, battery::parse);

    assert_eq!(battery.map(|b| b.percent()), Ok(100));
    assert_eq!(
        device.writes().last(),
        Some(&frame_at(LAUNCH, BATTERY_REQUEST))
    );
}

#[test]
fn read_maps_a_bad_reply() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_at(LAUNCH, BATTERY_REPLY));

    let result = session.read(
        FeatureId::BATTERY_SOC,
        battery::get_request,
        advanced_para_eq::parse_custom_reply,
    );

    let parse_error = ParseError::TooShort {
        expected: 51,
        actual: 3,
    };
    assert_eq!(
        result,
        Err(DeviceError::Io(format!("unexpected reply: {parse_error}")))
    );
}

#[test]
fn custom_eq_read_with_slot_other_than_3_is_refused() {
    let (mut session, device) = open_scripted();

    for params in [
        &[1, 0][..],
        &[1, 2],
        &[1, 4],
        &[0, 0xff],
        &[2, 3],
        &[1],
        &[1, 3, 0],
        &[],
    ] {
        assert_params_refused_without_writing(
            &mut session,
            &device,
            FeatureId::ADVANCED_PARA_EQ,
            1,
            params,
        );
    }
}

#[test]
fn custom_eq_read_of_each_instance_is_sent() {
    for (instance, reply_line) in [(EqInstance::Mic, 71), (EqInstance::Headphone, 83)] {
        let (mut session, device) = open_scripted();
        device.push_frame(frame_at(LAUNCH, reply_line));

        let equalizer = session.read(
            FeatureId::ADVANCED_PARA_EQ,
            |index| advanced_para_eq::get_custom_request(index, instance),
            advanced_para_eq::parse_custom_reply,
        );

        assert!(equalizer.is_ok(), "{instance:?}");
        assert_eq!(
            device.writes().last(),
            Some(&frame_at(LAUNCH, reply_line - 1)),
            "{instance:?}"
        );
    }
}

#[test]
fn mixer_read_for_output_1_is_refused() {
    let (mut session, device) = open_scripted();

    for params in [&[1][..], &[0xff], &[0, 0], &[]] {
        assert_params_refused_without_writing(
            &mut session,
            &device,
            FeatureId::MIXER_AUDIO,
            5,
            params,
        );
    }
}

#[test]
fn mixer_settings_read_is_sent() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_at(LAUNCH, 111));

    let mix = session
        .read(
            FeatureId::MIXER_AUDIO,
            mixer_audio::get_settings_request,
            mixer_audio::parse_settings_reply,
        )
        .unwrap();

    assert_eq!(mix.stream.level, MixLevel(0x1a));
    assert_eq!(device.writes().last(), Some(&frame_at(LAUNCH, 110)));
}

const SESSION_B_SETTINGS: [&str; 6] = [
    "session-b/sidetone",
    "session-b/balance",
    "session-b/mixer",
    "session-b/small-settings",
    "session-b/eq-headphone",
    "session-b/eq-mic",
];
const OUR_SW_ID: u8 = 0x0b;

fn params_at(name: &str, line: usize) -> Vec<u8> {
    let frame = frame_at(name, line);
    frame[6..usize::from(frame[2]) + 3].to_vec()
}

fn with_our_sw_id(mut frame: [u8; 64]) -> [u8; 64] {
    frame[5] = frame[5] & 0xf0 | OUR_SW_ID;
    frame
}

#[test]
fn every_call_ghub_sent_in_session_b_settings_passes_the_rules() {
    let (mut session, device) = open_scripted();
    for name in SESSION_B_SETTINGS {
        let transfers = load(name);
        let ids = feature_ids(&transfers);
        for transfer in transfers.iter().filter(|t| t.dir == common::Dir::Out) {
            let ours = with_our_sw_id(transfer.frame);
            let id = ids[&ours[4]];
            let function = ours[5] >> 4;
            device.push_frame(empty_reply_to(&ours));

            let result = session.request(id, function, &params_at(name, transfer.line));

            assert_eq!(result, Ok(vec![]), "{name} line {}", transfer.line);
            assert_eq!(
                device.writes().last(),
                Some(&ours),
                "{name} line {}",
                transfer.line
            );
        }
    }
}

fn send_every_accepted_value(
    id: FeatureId,
    build: impl Fn(u8, u8) -> Result<Message, RangeError>,
) -> Vec<u8> {
    let (mut session, device) = open_scripted();
    let index = index_in(&session, id);
    let mut accepted = Vec::new();
    for value in 0..=u8::MAX {
        let Ok(msg) = build(index, value) else {
            continue;
        };
        send_built(&mut session, &device, id, msg, &format!("{id:?} {value}"));
        accepted.push(value);
    }
    accepted
}

type ByteBuilder = fn(u8, u8) -> Result<Message, RangeError>;

#[test]
fn every_value_the_small_setting_builders_accept_is_sent() {
    let cases: [(FeatureId, ByteBuilder, Vec<u8>); 7] = [
        (
            FeatureId::HEADSET_SIDETONE,
            |index, level| sidetone::set_request(index, Sidetone(level)),
            (0..=6).collect(),
        ),
        (
            FeatureId::HEADSET_MIX,
            |index, balance| headset_mix::set_request(index, GameVoiceBalance(balance)),
            (0..=12).collect(),
        ),
        (
            FeatureId::VOLUME_SET,
            |index, level| volume::set_request(index, VolumeLevel(level)),
            (0..=21).collect(),
        ),
        (
            FeatureId::LED_BRIGHTNESS,
            led_brightness::set_request,
            (0..=100).collect(),
        ),
        (
            FeatureId::AUTO_SLEEP,
            |index, minutes| auto_sleep::set_request(index, SleepAfter(minutes)),
            vec![0, 15, 30],
        ),
        (
            FeatureId::HEADSET_TONES,
            |index, tones| tones::set_request(index, Tones(tones)),
            vec![0, 1, 2],
        ),
        (
            FeatureId::MIXER_MIC,
            |index, gate| noise_gate::set_request(index, NoiseGate::from_byte(gate)),
            vec![0, 1, 2, 4],
        ),
    ];
    for (id, build, expected) in cases {
        assert_eq!(send_every_accepted_value(id, build), expected, "{id:?}");
    }
}

#[test]
fn every_mixer_level_the_builder_accepts_is_sent_on_game_voice_and_stream() {
    for muted in [false, true] {
        let accepted = send_every_accepted_value(FeatureId::MIXER_AUDIO, |index, level| {
            let channel = MixChannel {
                level: MixLevel(level),
                muted,
            };
            let mut mix = StreamMix {
                stream: channel,
                inputs: [mixer_audio::PINNED_INPUT; 4],
            };
            *mix.input_mut(MixInput::Game) = channel;
            *mix.input_mut(MixInput::Voice) = channel;
            mixer_audio::set_request(index, &mix)
        });

        assert_eq!(accepted, (0..=mixer_audio::MAX_LEVEL).collect::<Vec<_>>());
    }
}

const HEADPHONE_RANGE_FREQS: [u16; 10] = [20, 50, 125, 250, 500, 1000, 2500, 5000, 10000, 20000];
const MIC_RANGE_FREQS: [u16; 10] = [60, 150, 250, 400, 800, 1500, 2500, 5000, 10000, 20000];

#[test]
fn eq_cards_at_the_builder_limits_are_sent() {
    let (mut session, device) = open_scripted();
    let index = index_in(&session, FeatureId::ADVANCED_PARA_EQ);
    for (instance, freqs) in [
        (EqInstance::Headphone, HEADPHONE_RANGE_FREQS),
        (EqInstance::Mic, MIC_RANGE_FREQS),
    ] {
        for q in [Some(MIN_Q), Some(MAX_Q), None] {
            for gain_db in [MIN_GAIN_DB, MAX_GAIN_DB] {
                let card = Equalizer {
                    bands: freqs.map(|freq_hz| EqBand {
                        freq_hz,
                        q,
                        gain_db,
                    }),
                };
                let case = format!("{instance:?} Q {q:?} gain {gain_db}");
                let msg = advanced_para_eq::set_custom_request(index, instance, &card)
                    .unwrap_or_else(|e| panic!("{case}: {e}"));

                send_built(
                    &mut session,
                    &device,
                    FeatureId::ADVANCED_PARA_EQ,
                    msg,
                    &case,
                );
            }
        }
    }
}

fn assert_all_refused(id: FeatureId, function: u8, cases: &[Vec<u8>]) {
    let (mut session, device) = open_scripted();
    for params in cases {
        assert_params_refused_without_writing(&mut session, &device, id, function, params);
    }
}

fn changed(mut params: Vec<u8>, at: usize, byte: u8) -> Vec<u8> {
    params[at] = byte;
    params
}

fn resized(mut params: Vec<u8>, len: usize) -> Vec<u8> {
    params.resize(len, 0);
    params
}

#[test]
fn off_shape_custom_eq_writes_are_refused_without_writing() {
    let headphone = params_at("session-b/eq-headphone", 72);
    let mic = params_at("session-b/eq-mic", 79);
    assert_all_refused(
        FeatureId::ADVANCED_PARA_EQ,
        2,
        &[
            changed(headphone.clone(), 1, 2),
            changed(headphone.clone(), 0, 2),
            changed(headphone.clone(), 2, 1),
            resized(headphone.clone(), 52),
            resized(headphone.clone(), 54),
            changed(changed(headphone.clone(), 6, 0), 7, 0xf1),
            changed(changed(headphone.clone(), 3, 0x00), 4, 0x13),
            changed(changed(headphone.clone(), 3, 0x4e), 4, 0x21),
            changed(changed(mic.clone(), 3, 0x00), 4, 0x3b),
            changed(changed(mic.clone(), 3, 0x4e), 4, 0x21),
            changed(mic, 0, 2),
            [&headphone[..], &headphone[3..8]].concat(),
        ],
    );
}

#[test]
fn off_shape_mixer_writes_are_refused_without_writing() {
    let ghub = params_at("session-b/mixer", 38);
    assert_all_refused(
        FeatureId::MIXER_AUDIO,
        6,
        &[
            resized(ghub.clone(), 15),
            resized(ghub.clone(), 17),
            changed(ghub.clone(), 0, 1),
            changed(ghub.clone(), 3, 3),
            changed(changed(ghub.clone(), 4, 1), 7, 0),
            changed(ghub.clone(), 1, 0x21),
            changed(ghub.clone(), 8, 0x21),
            changed(ghub.clone(), 2, 2),
            changed(ghub.clone(), 15, 2),
            [&ghub[..], &[0x04, 0x10, 0x00]].concat(),
        ],
    );
}

#[test]
fn mixer_writes_changing_mic_out_or_bluetooth_are_refused_without_writing() {
    let ghub = params_at("session-b/mixer", 38);
    let mic_out_full = changed(changed(ghub.clone(), 5, 0x20), 6, 0x00);
    let bluetooth_full = changed(changed(ghub.clone(), 11, 0x20), 12, 0x00);
    assert_all_refused(
        FeatureId::MIXER_AUDIO,
        6,
        &[
            mic_out_full,
            bluetooth_full,
            changed(ghub.clone(), 5, 0x0f),
            changed(ghub.clone(), 6, 0x00),
            changed(ghub.clone(), 11, 0x11),
            changed(ghub, 12, 0x00),
        ],
    );
}

#[test]
fn off_shape_sidetone_writes_are_refused_without_writing() {
    assert_all_refused(
        FeatureId::HEADSET_SIDETONE,
        1,
        &[
            vec![0x01, 0xff, 0x07],
            vec![0x02, 0xff, 0x03],
            vec![0x01, 0xfe, 0x03],
            vec![0x01, 0x03],
            vec![0x03],
            vec![0x01, 0xff, 0x03, 0x00],
            vec![],
        ],
    );
}

#[test]
fn off_shape_balance_writes_are_refused_without_writing() {
    assert_all_refused(
        FeatureId::HEADSET_MIX,
        1,
        &[vec![13], vec![0xff], vec![6, 0], vec![]],
    );
}

#[test]
fn eq_cards_mixing_graphic_and_parametric_bands_are_refused_without_writing() {
    let parametric = params_at("session-b/eq-headphone", 72);
    let graphic = params_at("session-b/eq-headphone", 47);
    let q_at = |band: usize| 3 + band * 5 + 2;
    assert_all_refused(
        FeatureId::ADVANCED_PARA_EQ,
        2,
        &[
            changed(parametric.clone(), q_at(0), 0x00),
            changed(parametric, q_at(9), 0x00),
            changed(graphic.clone(), q_at(0), 0x16),
            changed(graphic, q_at(4), 0x16),
        ],
    );
}

#[test]
fn eq_cards_with_frequencies_out_of_order_are_refused_without_writing() {
    let ghub = params_at("session-b/eq-headphone", 72);
    let freq_at = |band: usize| 3 + band * 5;
    let with_freq = |band: usize, freq: u16| {
        let [hi, lo] = freq.to_be_bytes();
        changed(
            changed(ghub.clone(), freq_at(band), hi),
            freq_at(band) + 1,
            lo,
        )
    };
    assert_all_refused(
        FeatureId::ADVANCED_PARA_EQ,
        2,
        &[
            with_freq(1, 20),
            with_freq(2, 40),
            with_freq(9, 10000),
            with_freq(4, 1347),
        ],
    );
}

#[test]
fn off_shape_volume_writes_are_refused_without_writing() {
    assert_all_refused(
        FeatureId::VOLUME_SET,
        1,
        &[
            vec![0x15],
            vec![0xfe, 0x15],
            vec![0xff, 0x16],
            vec![0xff, 0x15, 0x00],
            vec![],
        ],
    );
}

#[test]
fn off_shape_brightness_writes_are_refused_without_writing() {
    assert_all_refused(
        FeatureId::LED_BRIGHTNESS,
        1,
        &[vec![101], vec![0xff], vec![50, 0], vec![]],
    );
}

#[test]
fn off_shape_sleep_timer_writes_are_refused_without_writing() {
    assert_all_refused(
        FeatureId::AUTO_SLEEP,
        1,
        &[
            vec![0x1e],
            vec![0x3b, 0x1e],
            vec![0x00, 0x0f],
            vec![0x3c, 20],
            vec![0x3c, 60],
            vec![0x3c, 0x1e, 0x00],
            vec![],
        ],
    );
}

#[test]
fn off_shape_tones_writes_are_refused_without_writing() {
    assert_all_refused(
        FeatureId::HEADSET_TONES,
        1,
        &[vec![3], vec![0xff], vec![1, 1], vec![]],
    );
}

#[test]
fn off_shape_noise_gate_writes_are_refused_without_writing() {
    assert_all_refused(
        FeatureId::MIXER_MIC,
        2,
        &[vec![3], vec![5], vec![0xff], vec![1, 1], vec![]],
    );
}

#[test]
fn missing_feature_is_reported_without_writing() {
    let (mut session, device) = open_scripted();
    let before = device.writes().len();

    assert_eq!(
        session.request(FeatureId::HEADSET_ANC, 0, &[]),
        Err(DeviceError::FeatureMissing(FeatureId::HEADSET_ANC))
    );
    assert_eq!(device.writes().len(), before);
}

#[test]
fn headset_off_maps_to_headset_disconnected() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_at(LAUNCH, MIC_TEST_GET_STATE_REPLY_NOT_CONNECTED));
    assert_eq!(session.headset_available(), Ok(false));

    device.push_timeout();

    assert_eq!(
        session.request(FeatureId::BATTERY_SOC, 0, &[]),
        Err(DeviceError::HeadsetDisconnected)
    );
}

#[test]
fn headset_available_is_false_when_its_read_fails_for_a_disconnected_headset() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_at(DOCK_CYCLE, LIFT_MIC_TEST_EVENT));
    assert_eq!(
        session.next_event(100),
        Ok(Some(Event::HeadsetAvailability(true)))
    );
    device.push_frame(frame_at(DOCK_CYCLE, DOCK_MIC_TEST_EVENT));
    assert_eq!(
        session.next_event(100),
        Ok(Some(Event::HeadsetAvailability(false)))
    );

    device.push_timeout();
    assert_eq!(session.headset_available(), Ok(false));

    device.push_frame(frame_from_hex("020c0600ff136b08"));
    assert_eq!(session.headset_available(), Ok(false));
}

#[test]
fn not_connected_error_code_maps_to_headset_disconnected() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_from_hex("020c0600ff060b08"));

    assert_eq!(
        session.request(FeatureId::BATTERY_SOC, 0, &[]),
        Err(DeviceError::HeadsetDisconnected)
    );
}

#[test]
fn timeout_with_headset_available_is_timeout() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_from_hex("020c0400136b00"));
    assert_eq!(session.headset_available(), Ok(true));

    device.push_timeout();

    assert_eq!(
        session.request(FeatureId::BATTERY_SOC, 0, &[]),
        Err(DeviceError::Timeout)
    );
}

#[test]
fn transport_gone_maps_to_disconnected() {
    let (mut session, device) = open_scripted();
    device.push_failure();

    assert_eq!(
        session.request(FeatureId::BATTERY_SOC, 0, &[]),
        Err(DeviceError::Disconnected)
    );
    assert_eq!(
        session.request(FeatureId::BATTERY_SOC, 0, &[]),
        Err(DeviceError::Disconnected)
    );
    assert_eq!(session.next_event(10), Err(DeviceError::Disconnected));
}

#[test]
fn timeout_message_does_not_claim_a_duration() {
    let message = DeviceError::Timeout.to_string();

    assert!(!message.contains("second"), "{message}");
}

#[test]
fn refusal_messages_do_not_blame_the_allowlist() {
    for reason in [Refusal::FeatureIndex, Refusal::SwId, Refusal::Params] {
        let message = DeviceError::Refused {
            feature: FeatureId::ADVANCED_PARA_EQ,
            function: 2,
            reason,
        }
        .to_string();

        assert!(!message.contains("allowlist"), "{message}");
        assert!(!message.contains("never seen"), "{message}");
    }
    assert_eq!(
        DeviceError::Refused {
            feature: FeatureId::ADVANCED_PARA_EQ,
            function: 2,
            reason: Refusal::Params
        }
        .to_string(),
        "refusing to send function 2 of feature 0x020d (AdvancedParaEq): \
         its parameters are outside what G Hub was seen sending"
    );
}

#[test]
fn allowlist_miss_message_names_the_allowlist() {
    let message = DeviceError::NotAllowed {
        feature: FeatureId::BATTERY_SOC,
        function: 1,
    }
    .to_string();

    assert!(message.contains("not on the allowlist"), "{message}");
}

#[test]
fn ghub_running_message_names_the_agent_and_the_tray() {
    let message = DeviceError::GHubRunning.to_string();

    assert!(message.contains("lghub_agent.exe"), "{message}");
    assert!(message.contains("tray"), "{message}");
}

#[test]
fn continuation_and_fragment_frames_are_not_replies() {
    let (mut session, device) = open_scripted();
    device.push_frame(frame_from_hex("020c3d01060b0102030405"));
    device.push_frame(frame_from_hex("020c0502060b0101"));
    device.push_frame(frame_from_hex("0300060000060b646403"));
    device.push_frame(frame_from_hex("020c0000060b"));
    device.push_frame(frame_at(DOCK_CYCLE, MEMFAULT_EVENT));
    device.push_frame(frame_at(DOCK_CYCLE, MEMFAULT_FIRST_FRAGMENT));
    device.push_frame(frame_at(DOCK_CYCLE, MEMFAULT_CONTINUATION));
    device.push_frame(frame_at(LAUNCH, BATTERY_REPLY));

    let params = session.request(FeatureId::BATTERY_SOC, 0, &[]).unwrap();

    assert_eq!(params, [0x64, 0x64, 0x03]);
    assert_eq!(device.pending_reads(), 0);
    assert_eq!(session.take_events(), []);
}

#[test]
fn next_event_returns_dock_cycle_events_and_tracks_availability() {
    let (mut session, device) = open_scripted();
    for line in [LIFT_MIC_TEST_EVENT, LIFT_BATTERY_EVENT, MEMFAULT_EVENT] {
        device.push_frame(frame_at(DOCK_CYCLE, line));
    }

    let lift = session.next_event(100).unwrap();
    let battery = session.next_event(100).unwrap();
    let after = session.next_event(100).unwrap();

    assert_eq!(lift, Some(Event::HeadsetAvailability(true)));
    assert!(matches!(battery, Some(Event::Battery(_))));
    assert_eq!(after, None);

    device.push_frame(frame_at(DOCK_CYCLE, DOCK_MIC_TEST_EVENT));
    assert_eq!(
        session.next_event(100).unwrap(),
        Some(Event::HeadsetAvailability(false))
    );
    device.push_timeout();
    assert_eq!(
        session.request(FeatureId::BATTERY_SOC, 0, &[]),
        Err(DeviceError::HeadsetDisconnected)
    );
}

#[test]
fn next_event_with_zero_timeout_reads_without_blocking() {
    let (mut session, device) = open_scripted();
    device.push_arrived(frame_at(DOCK_CYCLE, LIFT_MIC_TEST_EVENT));

    assert_eq!(
        session.next_event(0),
        Ok(Some(Event::HeadsetAvailability(true)))
    );
    assert_eq!(session.next_event(0), Ok(None));
}

#[test]
fn allowlist_entries_are_observed_in_cited_fixtures() {
    let allowlist = Allowlist::embedded().expect("embedded allowlist parses");
    assert!(!allowlist.allowed().is_empty());

    let mut seen = HashSet::new();
    for entry in allowlist.allowed() {
        assert!(
            fixture_path(&entry.seen_in).is_file(),
            "{} is not a fixture",
            entry.seen_in
        );
        let transfers = load(&entry.seen_in);
        let call = (FeatureId(entry.feature), entry.function);
        assert!(
            sent_calls(&transfers, &feature_ids(&transfers)).contains(&call),
            "{:#06x} fn{} never sent in {}",
            entry.feature,
            entry.function,
            entry.seen_in
        );
        assert!(seen.insert(call), "duplicate entry {call:?}");
        assert!(allowlist.permits(call.0, call.1));
    }
}

#[test]
fn every_call_ghub_sent_is_allowed_or_denied() {
    let allowlist = Allowlist::embedded().unwrap();
    let denied = |FeatureId(id): FeatureId, function: u8| {
        allowlist
            .denied()
            .iter()
            .any(|deny| deny.feature == id && deny.function.is_none_or(|f| f == function))
    };

    let fixtures = all_fixtures();
    assert!(fixtures.len() >= 12, "fixtures not found: {fixtures:?}");
    for name in fixtures {
        let transfers = load(&name);
        for (id, function) in sent_calls(&transfers, &feature_ids(&transfers)) {
            assert!(
                allowlist.permits(id, function) || denied(id, function),
                "{name}: G Hub sent {:#06x} fn{function}, which is neither allowed nor denied",
                id.0
            );
        }
    }
}

#[test]
fn allowlist_denies_firmware_diagnostics_and_personal_data() {
    let allowlist = Allowlist::embedded().unwrap();

    for function in 0..16 {
        assert!(!allowlist.permits(FeatureId::GENERIC_DFU, function));
        assert!(!allowlist.permits(FeatureId::MEMFAULT, function));
    }
    assert!(!allowlist.permits(FeatureId::BT_HOST_INFO, 1));
    assert!(!allowlist.permits(FeatureId::BT_HOST_INFO, 2));
    assert!(!allowlist.permits(FeatureId::DEVICE_INFO, 2));
}

/// agent-handlers.md section 3, plus R17's exclusions.
#[test]
fn deny_table_covers_the_never_send_list() {
    let every_function = |feature: u16| (0..16).map(move |function| (feature, function));
    let never_send = [
        (0x0100, 2),
        (0x0100, 4),
        (0x0101, 1),
        (0x0305, 1),
        (0x0305, 2),
        (0x0609, 3),
        (0x0609, 4),
    ]
    .into_iter()
    .chain([2, 3, 5, 6, 8, 9].map(|function| (0x0004, function)))
    .chain([6, 7, 8, 9].map(|function| (0x020D, function)))
    .chain((0..6).map(|function| (0x020E, function)))
    .chain(
        [0x010A, 0x0103, 0x0102, 0x0303, 0x0805, 0x0806, 0x0807]
            .into_iter()
            .flat_map(every_function),
    );

    for (feature, function) in never_send {
        assert!(
            Allowlist::parse(&with_extra_allow(feature, function)).is_err(),
            "{feature:#06x} fn{function} is not denied"
        );
    }
}

#[test]
fn allowlist_rejects_an_allow_entry_hitting_any_deny_entry() {
    let allowlist = Allowlist::embedded().unwrap();
    assert!(!allowlist.denied().is_empty());

    for deny in allowlist.denied() {
        let function = deny.function.unwrap_or(0);
        assert!(
            Allowlist::parse(&with_extra_allow(deny.feature, function)).is_err(),
            "{:#06x} fn{function}",
            deny.feature
        );
    }
}

#[test]
fn allowlist_rejects_allow_deny_overlap() {
    let whole_feature = r#"
        [[allow]]
        feature = 0x0103
        function = 0
        seen_in = "session-a/ghub-launch"

        [[deny]]
        feature = 0x0103
        reason = "diagnostics"
    "#;
    let one_function = r#"
        [[allow]]
        feature = 0x0100
        function = 2
        seen_in = "session-a/ghub-launch"

        [[deny]]
        feature = 0x0100
        function = 2
        reason = "serial number"
    "#;
    let disjoint = r#"
        [[allow]]
        feature = 0x0100
        function = 1
        seen_in = "session-a/ghub-launch"

        [[deny]]
        feature = 0x0100
        function = 2
        reason = "serial number"
    "#;

    assert!(Allowlist::parse(whole_feature).is_err());
    assert!(Allowlist::parse(one_function).is_err());
    assert!(Allowlist::parse(disjoint).is_ok());
}

#[test]
fn allowlist_rejects_duplicates_and_wide_functions() {
    let entry = |function: u8| {
        format!("[[allow]]\nfeature = 0x0104\nfunction = {function}\nseen_in = \"{LAUNCH}\"\n")
    };

    assert!(Allowlist::parse(&entry(0)).is_ok());
    assert!(Allowlist::parse(&format!("{}{}", entry(0), entry(0))).is_err());
    assert!(Allowlist::parse(&entry(16)).is_err());
}
