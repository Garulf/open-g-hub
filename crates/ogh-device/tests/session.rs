mod common;

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::{
    LAUNCH, LAUNCH_OPEN_REPLIES, LAUNCH_OPEN_REQUESTS, discovered_ids, fixture_path, frame_at,
    frame_from_hex, load, sent_calls,
};
use ogh_device::allowlist::Allowlist;
use ogh_device::testing::{self, ScriptedTransport};
use ogh_device::{DeviceError, Event, FeatureId, Session};
use ogh_protocol::features::device_info;
use ogh_protocol::{Message, request};

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
const SHORT_TIMEOUT: Duration = Duration::from_millis(100);

fn open_scripted() -> (Session<ScriptedTransport>, ScriptedTransport) {
    let device = ScriptedTransport::with_launch_discovery();
    let handle = device.clone();
    let mut session = Session::with_transport(device).expect("open sequence succeeds");
    session.set_request_timeout(SHORT_TIMEOUT);
    (session, handle)
}

fn assert_refused_without_writing(
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

    assert_refused_without_writing(&mut session, &device, FeatureId::BATTERY_SOC, 1, &[]);
    assert_eq!(
        session.call(FeatureId::DEVICE_INFO, device_info::serial_number_request),
        Err(DeviceError::NotAllowed {
            feature: FeatureId::DEVICE_INFO,
            function: 2
        })
    );
    assert_eq!(device.writes().len(), before);
}

#[test]
fn denied_feature_is_refused_without_writing() {
    let (mut session, device) = open_scripted();

    for function in [0, 1] {
        assert_refused_without_writing(&mut session, &device, FeatureId::MEMFAULT, function, &[0]);
    }
}

#[test]
fn builder_targeting_another_index_is_refused_without_writing() {
    let (mut session, device) = open_scripted();
    let before = device.writes().len();
    let dfu_index = 2;

    assert_eq!(
        session.call(FeatureId::HEADSET_SIDETONE, |_| request(dfu_index, 1, &[])),
        Err(DeviceError::NotAllowed {
            feature: FeatureId::HEADSET_SIDETONE,
            function: 1
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
        Err(DeviceError::NotAllowed {
            feature: FeatureId::BATTERY_SOC,
            function: 0
        })
    );
    assert_eq!(device.writes().len(), before);
}

#[test]
fn set_custom_eq_to_a_factory_slot_is_refused_without_writing() {
    let (mut session, device) = open_scripted();

    for slot in [0, 1, 2] {
        assert_refused_without_writing(
            &mut session,
            &device,
            FeatureId::ADVANCED_PARA_EQ,
            2,
            &[1, slot, 0],
        );
    }
    assert_refused_without_writing(&mut session, &device, FeatureId::ADVANCED_PARA_EQ, 2, &[1]);
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
    let launch_ids = discovered_ids(&load(LAUNCH)).expect("launch capture has discovery");
    assert!(!allowlist.allowed().is_empty());

    let mut seen = HashSet::new();
    for entry in allowlist.allowed() {
        assert!(
            fixture_path(&entry.seen_in).is_file(),
            "{} is not a fixture",
            entry.seen_in
        );
        let transfers = load(&entry.seen_in);
        let ids = discovered_ids(&transfers).unwrap_or_else(|| launch_ids.clone());
        let call = (FeatureId(entry.feature), entry.function);
        assert!(
            sent_calls(&transfers, &ids).contains(&call),
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
