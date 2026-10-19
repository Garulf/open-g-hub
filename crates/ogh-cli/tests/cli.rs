use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use clap::Parser;
use ogh_cli::commands::{Cli, RunOptions, run};
use ogh_cli::error::CliError;
use ogh_device::testing::{ScriptedTransport, frame_from_hex};
use ogh_device::{DeviceError, FeatureId, Refusal, Session};
use serde_json::{Value, json};

const LAUNCH: &str = "session-a/ghub-launch";
const DOCK_CYCLE: &str = "session-a/dock-cycle";
const SIDETONE: &str = "session-b/sidetone";
const SHORT_TIMEOUT: Duration = Duration::from_millis(100);

const MIC_TEST_REQUEST: usize = 132;
const BATTERY_REQUEST: usize = 38;
const SIDETONE_GET_REQUEST: usize = 88;
const BALANCE_GET_REQUEST: usize = 130;
const NOISE_GATE_REQUEST: usize = 56;
const FIRMWARE_REQUEST: usize = 12;
const MIC_TEST_DOCKED_REPLY: usize = 133;
const BATTERY_FULL_REPLY: usize = 39;
const SIDETONE_ZERO_REPLY: usize = 89;
const BALANCE_REPLY: usize = 131;
const NOISE_GATE_OFF_REPLY: usize = 57;
const FIRMWARE_REPLY: usize = 13;

const MIC_TEST_IN_USE: &str = "020c0400136b00";
const BATTERY_DISCHARGING: &str = "020c0600060b646400";
const BATTERY_CHARGING: &str = "020c0600060b646401";
const NOT_CONNECTED_ERROR: &str = "020c0600ff060b08";
const SIDETONE_LEVEL_FOUR: &str = "020c0700090b0101ff04";
const NOISE_GATE_TOURNAMENT: &str = "020c0400161b04";

fn fixture_frame(name: &str, line: usize) -> [u8; 64] {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(format!("{name}.jsonl"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let entry = text.lines().nth(line - 1).expect("fixture line");
    let parsed: Value = serde_json::from_str(entry).expect("fixture json");
    frame_from_hex(parsed["hex"].as_str().expect("hex field"))
}

fn open() -> (Session<ScriptedTransport>, ScriptedTransport) {
    let device = ScriptedTransport::with_launch_discovery();
    let handle = device.clone();
    let mut session = Session::with_transport(device).expect("open sequence succeeds");
    session.set_request_timeout(SHORT_TIMEOUT);
    (session, handle)
}

struct StatusReplies {
    mic_test: [u8; 64],
    battery: Option<[u8; 64]>,
    sidetone: [u8; 64],
    noise_gate: [u8; 64],
}

impl StatusReplies {
    fn docked() -> Self {
        Self {
            mic_test: fixture_frame(LAUNCH, MIC_TEST_DOCKED_REPLY),
            battery: Some(fixture_frame(LAUNCH, BATTERY_FULL_REPLY)),
            sidetone: fixture_frame(LAUNCH, SIDETONE_ZERO_REPLY),
            noise_gate: fixture_frame(LAUNCH, NOISE_GATE_OFF_REPLY),
        }
    }

    fn in_use() -> Self {
        Self {
            mic_test: frame_from_hex(MIC_TEST_IN_USE),
            battery: Some(frame_from_hex(BATTERY_DISCHARGING)),
            sidetone: frame_from_hex(SIDETONE_LEVEL_FOUR),
            noise_gate: frame_from_hex(NOISE_GATE_TOURNAMENT),
        }
    }

    fn headset_off() -> Self {
        Self {
            battery: None,
            ..Self::docked()
        }
    }

    fn script(&self, device: &ScriptedTransport) {
        device.push_frame(self.mic_test);
        match self.battery {
            Some(frame) => device.push_frame(frame),
            None => device.push_timeout(),
        }
        device.push_frame(self.sidetone);
        device.push_frame(fixture_frame(LAUNCH, BALANCE_REPLY));
        device.push_frame(self.noise_gate);
        device.push_frame(fixture_frame(LAUNCH, FIRMWARE_REPLY));
    }
}

fn execute(
    session: &mut Session<ScriptedTransport>,
    args: &[&str],
    options: &RunOptions,
) -> (Result<(), CliError>, String) {
    let cli = Cli::try_parse_from(std::iter::once("ogh").chain(args.iter().copied()))
        .expect("arguments parse");
    let mut out = Vec::new();
    let result = run(&cli.command, session, &mut out, options);
    (result, String::from_utf8(out).expect("utf-8 output"))
}

fn status_json(replies: &StatusReplies) -> Value {
    let (mut session, device) = open();
    replies.script(&device);
    let (result, out) = execute(&mut session, &["status", "--json"], &RunOptions::default());
    result.expect("status succeeds");
    serde_json::from_str(&out).expect("one json document")
}

fn status_human(replies: &StatusReplies) -> String {
    let (mut session, device) = open();
    replies.script(&device);
    let (result, out) = execute(&mut session, &["status"], &RunOptions::default());
    result.expect("status succeeds");
    out
}

fn human_value<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
        .unwrap_or_else(|| panic!("no {key} line in {text}"))
        .trim()
}

#[test]
fn status_json_shape() {
    let status = status_json(&StatusReplies::docked());

    let object = status.as_object().expect("a json object");
    let mut keys: Vec<_> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "balance",
            "battery_percent",
            "charging",
            "docked",
            "firmware",
            "headset",
            "headset_connected",
            "noise_gate",
            "sidetone",
        ]
    );
    assert_eq!(
        status,
        json!({
            "headset": "docked",
            "battery_percent": 100,
            "charging": false,
            "docked": true,
            "headset_connected": false,
            "sidetone": 0,
            "balance": 6,
            "noise_gate": "off",
            "firmware": "1.2.18",
        })
    );
}

#[test]
fn status_reads_like_ghub() {
    let (mut session, device) = open();
    let before = device.writes().len();
    StatusReplies::docked().script(&device);

    execute(&mut session, &["status"], &RunOptions::default())
        .0
        .expect("status succeeds");

    let sent: Vec<_> = device.writes().split_off(before);
    let ghub: Vec<_> = [
        MIC_TEST_REQUEST,
        BATTERY_REQUEST,
        SIDETONE_GET_REQUEST,
        BALANCE_GET_REQUEST,
        NOISE_GATE_REQUEST,
        FIRMWARE_REQUEST,
    ]
    .map(|line| fixture_frame(LAUNCH, line))
    .to_vec();
    assert_eq!(sent, ghub);
}

#[test]
fn status_in_use() {
    let status = status_json(&StatusReplies::in_use());

    assert_eq!(
        status,
        json!({
            "headset": "in-use",
            "battery_percent": 100,
            "charging": false,
            "docked": false,
            "headset_connected": true,
            "sidetone": 67,
            "balance": 6,
            "noise_gate": "tournament",
            "firmware": "1.2.18",
        })
    );
}

#[test]
fn headset_off_status_exits_zero_and_says_disconnected() {
    let replies = StatusReplies::headset_off();

    let human = status_human(&replies);
    assert_eq!(human_value(&human, "headset"), "disconnected");
    assert_eq!(human_value(&human, "battery"), "unknown");

    let status = status_json(&replies);
    assert_eq!(status["headset"], "disconnected");
    assert_eq!(status["headset_connected"], false);
    for key in ["battery_percent", "charging", "docked"] {
        assert_eq!(status[key], Value::Null, "{key}");
    }
    assert_eq!(status["firmware"], "1.2.18");
}

#[test]
fn status_human_output_is_an_aligned_list() {
    let human = status_human(&StatusReplies::in_use());

    let columns: Vec<_> = human
        .lines()
        .map(|line| {
            let value_at = line.find(": ").expect("key: value") + 2;
            value_at + line[value_at..].len() - line[value_at..].trim_start().len()
        })
        .collect();
    assert!(columns.windows(2).all(|pair| pair[0] == pair[1]), "{human}");
    assert_eq!(human_value(&human, "headset"), "in-use");
    assert_eq!(human_value(&human, "battery"), "100%");
    assert_eq!(human_value(&human, "sidetone"), "67%");
    assert_eq!(human_value(&human, "balance"), "6");
    assert_eq!(human_value(&human, "noise gate"), "tournament");
    assert_eq!(human_value(&human, "firmware"), "1.2.18");
}

#[test]
fn status_does_not_hide_a_timeout_while_the_headset_is_in_use() {
    let (mut session, device) = open();
    let replies = StatusReplies {
        battery: None,
        ..StatusReplies::in_use()
    };
    replies.script(&device);

    let (result, out) = execute(&mut session, &["status"], &RunOptions::default());

    assert!(matches!(
        result,
        Err(CliError::Device(DeviceError::Timeout))
    ));
    assert_eq!(out, "");
    assert_eq!(session.request_timeout(), SHORT_TIMEOUT);
}

#[test]
fn an_in_use_headset_keeps_the_full_timeout_for_optional_reads() {
    let (mut session, device) = open();
    session.set_request_timeout(Duration::from_millis(1500));
    StatusReplies {
        battery: None,
        ..StatusReplies::in_use()
    }
    .script(&device);

    let started = Instant::now();
    let (result, _) = execute(&mut session, &["status"], &RunOptions::default());
    let elapsed = started.elapsed();

    assert!(matches!(
        result,
        Err(CliError::Device(DeviceError::Timeout))
    ));
    assert!(elapsed >= Duration::from_millis(1400), "{elapsed:?}");
}

#[test]
fn headset_leaving_during_status_reads_as_disconnected() {
    let (mut session, device) = open();
    device.push_frame(frame_from_hex(MIC_TEST_IN_USE));
    device.push_frame(frame_from_hex(NOT_CONNECTED_ERROR));
    device.push_frame(frame_from_hex(SIDETONE_LEVEL_FOUR));
    device.push_frame(fixture_frame(LAUNCH, BALANCE_REPLY));
    device.push_frame(fixture_frame(LAUNCH, NOISE_GATE_OFF_REPLY));
    device.push_frame(fixture_frame(LAUNCH, FIRMWARE_REPLY));

    let (result, out) = execute(&mut session, &["status", "--json"], &RunOptions::default());

    result.expect("status succeeds");
    let status: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(status["headset"], "disconnected");
    assert_eq!(status["headset_connected"], false);
    assert_eq!(status["battery_percent"], Value::Null);
}

#[test]
fn a_dock_battery_status_reads_as_charging_and_docked() {
    let replies = StatusReplies {
        battery: Some(frame_from_hex(BATTERY_CHARGING)),
        ..StatusReplies::docked()
    };

    let status = status_json(&replies);

    assert_eq!(status["headset"], "docked");
    assert_eq!(status["charging"], true);
    assert_eq!(status["docked"], true);
    assert_eq!(status["battery_percent"], 100);
    assert_eq!(
        human_value(&status_human(&replies), "battery"),
        "100% charging"
    );
}

#[test]
fn a_discharging_battery_while_not_in_use_is_not_docked() {
    let status = status_json(&StatusReplies {
        battery: Some(frame_from_hex(BATTERY_DISCHARGING)),
        ..StatusReplies::docked()
    });

    assert_eq!(status["headset"], "disconnected");
    assert_eq!(status["docked"], false);
    assert_eq!(status["charging"], false);
    assert_eq!(status["headset_connected"], false);
}

#[test]
fn a_silent_headset_does_not_cost_the_full_timeout_per_read() {
    let (mut session, device) = open();
    session.set_request_timeout(Duration::from_secs(5));
    StatusReplies::headset_off().script(&device);

    let started = Instant::now();
    let (result, _) = execute(&mut session, &["status"], &RunOptions::default());
    let elapsed = started.elapsed();

    result.expect("status succeeds");
    assert!(elapsed >= Duration::from_millis(900), "{elapsed:?}");
    assert!(elapsed < Duration::from_millis(2500), "{elapsed:?}");
    assert_eq!(session.request_timeout(), Duration::from_secs(5));
}

#[test]
fn ghub_running_exits_4_and_names_lghub_agent() {
    for error in [
        CliError::Opening(DeviceError::GHubRunning),
        CliError::Device(DeviceError::GHubRunning),
    ] {
        let message = error.to_string();

        assert_eq!(error.exit_code(), 4);
        assert!(message.contains("lghub_agent.exe"), "{message}");
        assert!(message.contains("tray"), "{message}");
        assert!(!message.contains('\n'), "{message}");
    }
}

#[test]
fn not_found_exits_3() {
    let error = CliError::Opening(DeviceError::NotFound);

    assert_eq!(error.exit_code(), 3);
    assert!(!error.to_string().contains('\n'));
}

#[test]
fn device_error_exits_5() {
    for device_error in [
        DeviceError::Timeout,
        DeviceError::Disconnected,
        DeviceError::HeadsetDisconnected,
        DeviceError::Rejected {
            feature_index: 6,
            code: 5,
        },
        DeviceError::Io("boom".into()),
        DeviceError::NotAllowed {
            feature: FeatureId::BATTERY_SOC,
            function: 1,
        },
        DeviceError::Refused {
            feature: FeatureId::HEADSET_SIDETONE,
            function: 1,
            reason: Refusal::Params,
        },
    ] {
        let error = CliError::Device(device_error);

        assert_eq!(error.exit_code(), 5, "{error}");
        assert!(!error.to_string().contains('\n'));
    }
}

#[test]
fn unplugged_base_station_exits_5_with_a_message() {
    let (mut session, device) = open();
    device.push_failure();

    let (result, _) = execute(&mut session, &["status"], &RunOptions::default());

    let error = result.expect_err("status fails");
    assert_eq!(error.exit_code(), 5);
    assert!(error.to_string().contains("disconnected"), "{error}");
}

#[test]
fn permission_denied_when_opening_explains_udev_on_linux() {
    let error = CliError::Opening(DeviceError::AccessDenied("Permission denied".into()));

    assert_eq!(error.exit_code(), 5);
    if cfg!(target_os = "linux") {
        let message = error.to_string();
        assert!(message.contains("udev"), "{message}");
        assert!(message.contains("hidraw"), "{message}");
    }
    assert!(!error.to_string().contains('\n'));
}

#[test]
fn other_open_failures_get_no_permission_hint() {
    let error = CliError::Opening(DeviceError::Io("unexpected reply: too short".into()));

    assert_eq!(error.exit_code(), 5);
    let message = error.to_string();
    assert!(!message.contains("udev"), "{message}");
    assert!(!message.contains("hidraw"), "{message}");
}

#[test]
fn sidetone_out_of_range_exits_2() {
    for value in ["101", "255", "256", "-1", "abc", ""] {
        let error = Cli::try_parse_from(["ogh", "sidetone", value])
            .err()
            .unwrap_or_else(|| panic!("{value:?} was accepted"));

        assert_eq!(error.exit_code(), 2, "{value:?}");
    }
    for value in ["0", "68", "100"] {
        assert!(Cli::try_parse_from(["ogh", "sidetone", value]).is_ok());
    }
}

#[test]
fn unknown_command_exits_2() {
    let error = Cli::try_parse_from(["ogh", "reboot"])
        .err()
        .expect("rejected");

    assert_eq!(error.exit_code(), 2);
}

#[test]
fn sidetone_writes_like_ghub() {
    let (mut session, device) = open();
    device.push_frame(fixture_frame(SIDETONE, 3));
    device.push_frame(fixture_frame(SIDETONE, 4));
    device.push_frame(frame_from_hex(SIDETONE_LEVEL_FOUR));

    let (result, out) = execute(&mut session, &["sidetone", "68"], &RunOptions::default());

    result.expect("sidetone succeeds");
    let writes = device.writes();
    assert_eq!(writes[writes.len() - 2], fixture_frame(SIDETONE, 2));
    assert_eq!(
        writes[writes.len() - 1],
        fixture_frame(LAUNCH, SIDETONE_GET_REQUEST)
    );
    assert_eq!(out, "sidetone: 67%\n");
}

#[test]
fn features_table_lists_launch_features() {
    let (mut session, _) = open();

    let (result, out) = execute(&mut session, &["features"], &RunOptions::default());

    result.expect("features succeeds");
    let lines: Vec<Vec<&str>> = out
        .lines()
        .map(|line| line.split_whitespace().collect())
        .collect();
    assert_eq!(lines[0], ["index", "id", "name", "version"]);
    assert_eq!(lines.len(), 1 + 24);
    assert_eq!(lines[1], ["0", "0x0000", "Root", "0"]);
    assert_eq!(lines[3], ["2", "0x010A", "GenericDfu", "6"]);
    assert_eq!(lines[4], ["3", "0x0102", "unknown", "1"]);
    assert_eq!(lines[10], ["9", "0x0604", "HeadsetSidetone", "2"]);
    assert_eq!(lines[24], ["23", "0x0807", "unknown", "1"]);
}

fn script_dock_cycle(device: &ScriptedTransport) {
    for line in [2, 3, 4, 6, 14, 15, 20] {
        device.push_frame(fixture_frame(DOCK_CYCLE, line));
    }
}

fn bounded(max_events: usize) -> RunOptions {
    RunOptions {
        max_events: Some(max_events),
        ..RunOptions::default()
    }
}

#[test]
fn watch_prints_dock_cycle_events() {
    let (mut session, device) = open();
    script_dock_cycle(&device);

    let (result, out) = execute(&mut session, &["watch"], &bounded(7));

    result.expect("watch succeeds");
    assert_eq!(
        out,
        "headset: in-use\n\
         battery: 100%\n\
         bluetooth devices: 1\n\
         mixer inputs: 0x0e\n\
         balance: 6\n\
         headset: not in use\n\
         battery: 100% charging\n"
    );
}

#[test]
fn watch_json_prints_one_object_per_line() {
    let (mut session, device) = open();
    script_dock_cycle(&device);

    let (result, out) = execute(&mut session, &["watch", "--json"], &bounded(7));

    result.expect("watch succeeds");
    let events: Vec<Value> = out
        .lines()
        .map(|line| serde_json::from_str(line).expect("one json object per line"))
        .collect();
    assert_eq!(
        events,
        [
            json!({"event": "headset", "in_use": true}),
            json!({"event": "battery", "percent": 100, "status": "discharging"}),
            json!({"event": "bluetooth_devices", "count": 1}),
            json!({"event": "mixer_inputs", "mask": 14}),
            json!({"event": "balance", "value": 6}),
            json!({"event": "headset", "in_use": false}),
            json!({"event": "battery", "percent": 100, "status": "charging"}),
        ]
    );
}

#[test]
fn watch_prints_sidetone_events_as_percent() {
    let (mut session, device) = open();
    device.push_frame(fixture_frame(SIDETONE, 4));

    let (result, out) = execute(&mut session, &["watch"], &bounded(1));

    result.expect("watch succeeds");
    assert_eq!(out, "sidetone: 67%\n");
}

fn script_session_b_settings(device: &ScriptedTransport) {
    for (fixture, line) in [
        ("session-b/small-settings", 4),
        ("session-b/small-settings", 22),
        ("session-b/small-settings", 32),
        ("session-b/mixer", 4),
        ("session-b/eq-mic", 4),
        ("session-b/eq-headphone", 6),
    ] {
        device.push_frame(fixture_frame(fixture, line));
    }
}

#[test]
fn watch_prints_session_b_setting_events() {
    let (mut session, device) = open();
    script_session_b_settings(&device);

    let (result, out) = execute(&mut session, &["watch"], &bounded(6));

    result.expect("watch succeeds");
    assert_eq!(
        out,
        "volume: 0%\n\
         sleep after: 30 min\n\
         tones: 2\n\
         stream mix: stream 13%, mic out 50% muted, game 50%, bluetooth 50% muted, voice 50%\n\
         noise gate: night\n\
         headphone eq: custom slot 3 changed\n"
    );
}

#[test]
fn watch_json_prints_session_b_setting_events() {
    let (mut session, device) = open();
    script_session_b_settings(&device);

    let (result, out) = execute(&mut session, &["watch", "--json"], &bounded(6));

    result.expect("watch succeeds");
    let events: Vec<Value> = out
        .lines()
        .map(|line| serde_json::from_str(line).expect("one json object per line"))
        .collect();
    let channel = |percent: u8, muted: bool| json!({"percent": percent, "muted": muted});
    assert_eq!(
        events[0],
        json!({"event": "volume", "percent": 0, "muted": false})
    );
    assert_eq!(events[1], json!({"event": "sleep_after", "minutes": 30}));
    assert_eq!(events[2], json!({"event": "tones", "value": 2}));
    assert_eq!(
        events[3],
        json!({
            "event": "stream_mix",
            "stream": channel(13, false),
            "inputs": {
                "mic_out": channel(50, true),
                "game": channel(50, false),
                "bluetooth": channel(50, true),
                "voice": channel(50, false),
            },
        })
    );
    assert_eq!(events[4], json!({"event": "noise_gate", "mode": "night"}));
    assert_eq!(events[5]["event"], "eq");
    assert_eq!(events[5]["instance"], "headphone");
    assert_eq!(events[5]["slot"], 3);
    assert_eq!(
        events[5]["bands"][0],
        json!({"freq_hz": 20, "q": 0.6875, "gain_db": -5.0})
    );
}

const VOLUME_FULL_MUTED_EVENT: &str = "020c060008001f0115";
const NOISE_GATE_UNKNOWN_EVENT: &str = "020c0400160003";

fn script_remaining_setting_branches(device: &ScriptedTransport) {
    for (fixture, line) in [
        ("session-b/small-settings", 45),
        ("session-b/small-settings", 25),
        ("session-b/small-settings", 7),
    ] {
        device.push_frame(fixture_frame(fixture, line));
    }
    device.push_frame(frame_from_hex(VOLUME_FULL_MUTED_EVENT));
    device.push_frame(frame_from_hex(NOISE_GATE_UNKNOWN_EVENT));
}

#[test]
fn watch_prints_all_never_muted_and_unknown_settings() {
    let (mut session, device) = open();
    script_remaining_setting_branches(&device);

    let (result, out) = execute(&mut session, &["watch"], &bounded(5));

    result.expect("watch succeeds");
    assert_eq!(
        out,
        "tones: all\n\
         sleep after: never\n\
         volume: 52%\n\
         volume: 100% muted\n\
         noise gate: other-3\n"
    );
}

#[test]
fn watch_json_prints_all_never_muted_and_unknown_settings() {
    let (mut session, device) = open();
    script_remaining_setting_branches(&device);

    let (result, out) = execute(&mut session, &["watch", "--json"], &bounded(5));

    result.expect("watch succeeds");
    let events: Vec<Value> = out
        .lines()
        .map(|line| serde_json::from_str(line).expect("one json object per line"))
        .collect();
    assert_eq!(
        events,
        [
            json!({"event": "tones", "value": 1}),
            json!({"event": "sleep_after", "minutes": null}),
            json!({"event": "volume", "percent": 52, "muted": false}),
            json!({"event": "volume", "percent": 100, "muted": true}),
            json!({"event": "noise_gate", "mode": "other-3"}),
        ]
    );
}

#[test]
fn watch_stops_cleanly_when_interrupted() {
    let (mut session, _) = open();
    let options = RunOptions::default();
    options.stop.store(true, Ordering::SeqCst);

    let (result, out) = execute(&mut session, &["watch"], &options);

    result.expect("interrupt is not an error");
    assert_eq!(out, "");
}

#[test]
fn watch_reports_an_unplugged_base_station() {
    let (mut session, device) = open();
    device.push_frame(fixture_frame(DOCK_CYCLE, 2));
    device.push_failure();

    let (result, out) = execute(&mut session, &["watch"], &RunOptions::default());

    let error = result.expect_err("watch fails");
    assert_eq!(out, "headset: in-use\n");
    assert_eq!(error.exit_code(), 5);
    assert!(error.to_string().contains("disconnected"), "{error}");
}
