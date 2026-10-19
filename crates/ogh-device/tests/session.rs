mod common;

use std::cell::Cell;
use std::collections::HashSet;

use common::{
    LAUNCH, LAUNCH_DISCOVERY_REQUESTS, discovered_ids, fixture_path, frame_at, frame_from_hex,
    load, script_discovery, sent_calls,
};
use ogh_device::allowlist::Allowlist;
use ogh_device::testing::ScriptedTransport;
use ogh_device::{DeviceError, Event, FeatureId, Session};
use ogh_protocol::features::device_info;

const DOCK_CYCLE: &str = "session-a/dock-cycle";
const BATTERY_REQUEST: usize = 38;
const BATTERY_REPLY: usize = 39;
const MIC_TEST_GET_STATE_REPLY_NOT_CONNECTED: usize = 133;
const BT_COUNT_REQUEST: usize = 5;
const MIXER_EVENT_DURING_BT_REQUEST: usize = 6;
const BT_COUNT_REPLY: usize = 7;
const MEMFAULT_EVENT: usize = 21;
const MEMFAULT_FIRST_FRAGMENT: usize = 23;
const MEMFAULT_CONTINUATION: usize = 24;

fn no_ghub() -> bool {
    false
}

fn ghub_running() -> bool {
    true
}

fn open_scripted() -> (Session<ScriptedTransport>, ScriptedTransport) {
    let device = ScriptedTransport::new();
    script_discovery(&device);
    let handle = device.clone();
    let session = Session::open_with(no_ghub, move || Ok(device)).expect("discovery succeeds");
    (session, handle)
}

#[test]
fn open_discovers_the_launch_feature_table() {
    let (session, device) = open_scripted();

    let written = device.writes();
    let ghub_requests: Vec<_> = LAUNCH_DISCOVERY_REQUESTS
        .iter()
        .map(|&line| frame_at(LAUNCH, line))
        .collect();
    assert_eq!(written, ghub_requests);

    let table: Vec<(u8, u16)> = session
        .features()
        .iter()
        .map(|f| (f.index, f.id.0))
        .collect();
    assert_eq!(table.len(), 23);
    assert_eq!(table[0], (1, 0x0001));
    assert_eq!(table[5], (6, 0x0104));
    assert_eq!(table[22], (23, 0x0807));
    assert_eq!(device.pending_reads(), 0);
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

    assert_eq!(
        session.request(FeatureId::BATTERY_SOC, 1, &[]),
        Err(DeviceError::NotAllowed {
            feature: FeatureId::BATTERY_SOC,
            function: 1
        })
    );
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
    let before = device.writes().len();

    for function in [0, 1] {
        assert_eq!(
            session.request(FeatureId::MEMFAULT, function, &[0]),
            Err(DeviceError::NotAllowed {
                feature: FeatureId::MEMFAULT,
                function
            })
        );
    }
    assert_eq!(device.writes().len(), before);
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
fn guard_blocks_open() {
    let opened = Cell::new(false);

    let result = Session::open_with(ghub_running, || {
        opened.set(true);
        Ok(ScriptedTransport::new())
    });

    assert!(matches!(result, Err(DeviceError::GHubRunning)));
    assert!(!opened.get());
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
    for line in [2, 3, MEMFAULT_EVENT] {
        device.push_frame(frame_at(DOCK_CYCLE, line));
    }

    let lift = session.next_event(100).unwrap();
    let battery = session.next_event(100).unwrap();
    let after = session.next_event(100).unwrap();

    assert_eq!(lift, Some(Event::HeadsetAvailability(true)));
    assert!(matches!(battery, Some(Event::Battery(_))));
    assert_eq!(after, None);

    device.push_frame(frame_at(DOCK_CYCLE, 15));
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
