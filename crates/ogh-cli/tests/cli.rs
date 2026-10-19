use std::collections::VecDeque;

use clap::Parser;
use ogh_cli::commands::{Cli, run};
use ogh_cli::daemon::{Daemon, NotificationStream};
use ogh_cli::error::CliError;
use ogh_ipc::{
    AutoSwitchStatus, Battery, Channel, Charge, Connection, DeviceSettings, Eq, EqBand, ErrorKind,
    Feature, FocusStatus, Gate, Headset, IpcError, Notification, ProfileInfo, ProfileSettings,
    Request, Response, Setting, Status, StreamMixSetting, TransportError,
};
use ogh_protocol::features::advanced_para_eq as eq;
use serde_json::Value;

#[derive(Default)]
struct FakeDaemon {
    replies: VecDeque<Result<Response, TransportError>>,
    requests: Vec<Request>,
    notifications: Vec<Result<Notification, TransportError>>,
    subscribe_error: Option<TransportError>,
}

impl FakeDaemon {
    fn replying(replies: impl IntoIterator<Item = Response>) -> Self {
        Self {
            replies: replies.into_iter().map(Ok).collect(),
            ..Self::default()
        }
    }

    fn failing(error: TransportError) -> Self {
        Self {
            replies: VecDeque::from([Err(error)]),
            ..Self::default()
        }
    }
}

impl Daemon for FakeDaemon {
    fn call(&mut self, request: &Request) -> Result<Response, TransportError> {
        self.requests.push(request.clone());
        self.replies
            .pop_front()
            .expect("the test scripted a reply for every request")
    }

    fn subscribe(&mut self) -> Result<NotificationStream, TransportError> {
        if let Some(error) = self.subscribe_error.take() {
            return Err(error);
        }
        Ok(Box::new(
            std::mem::take(&mut self.notifications).into_iter(),
        ))
    }
}

struct Outcome {
    result: Result<(), CliError>,
    stdout: String,
    requests: Vec<Request>,
}

fn run_with(args: &[&str], mut daemon: FakeDaemon) -> Outcome {
    let mut argv = vec!["ogh"];
    argv.extend_from_slice(args);
    let cli = Cli::try_parse_from(argv).expect("arguments parse");
    let mut out = Vec::new();
    let result = run(&cli, &mut daemon, &mut out);
    Outcome {
        result,
        stdout: String::from_utf8(out).expect("utf-8 output"),
        requests: daemon.requests,
    }
}

fn exit_code(outcome: &Outcome) -> u8 {
    outcome
        .result
        .as_ref()
        .expect_err("the command fails")
        .exit_code()
}

fn message(outcome: &Outcome) -> String {
    outcome
        .result
        .as_ref()
        .expect_err("the command fails")
        .to_string()
}

fn channel(level_percent: u8, muted: bool) -> Channel {
    Channel {
        level_percent,
        muted,
    }
}

fn mix() -> StreamMixSetting {
    StreamMixSetting {
        output: channel(80, false),
        game: channel(50, false),
        voice: channel(70, true),
    }
}

fn status() -> Status {
    Status {
        connection: Connection::Connected,
        headset: Headset::InUse,
        battery: Some(Battery {
            percent: 85,
            charge: Charge::Discharging,
        }),
        firmware: Some("1.2.3".to_string()),
        settings: DeviceSettings {
            profile: ProfileSettings {
                sidetone_percent: Some(40),
                balance: Some(6),
                noise_gate: Some(Gate::Night),
                headphone_eq: None,
                mic_eq: None,
                stream_mix: Some(mix()),
            },
            volume_percent: Some(30),
            led_brightness_percent: Some(80),
            sleep_after_minutes: Some(0),
            tones: Some(1),
        },
        active_profile: Some("gaming".to_string()),
        focus: FocusStatus {
            available: true,
            app: Some("game.exe".to_string()),
        },
        auto_switch: AutoSwitchStatus {
            enabled: true,
            active: true,
            error: None,
        },
        config_error: None,
    }
}

fn error(kind: ErrorKind, message: &str) -> Response {
    Response::Error(IpcError {
        kind,
        message: message.to_string(),
    })
}

fn usage_exit_code(args: &[&str]) -> i32 {
    let mut argv = vec!["ogh"];
    argv.extend_from_slice(args);
    match Cli::try_parse_from(argv) {
        Err(e) => e.exit_code(),
        Ok(_) => panic!("{args:?} parsed"),
    }
}

#[test]
fn sidetone_sends_the_set_request_and_prints_the_new_value() {
    let outcome = run_with(&["sidetone", "40"], FakeDaemon::replying([Response::Done]));
    outcome.result.expect("sidetone succeeds");
    assert_eq!(
        outcome.requests,
        [Request::Set {
            setting: Setting::Sidetone { percent: 40 }
        }]
    );
    assert_eq!(outcome.stdout, "sidetone 40\n");
}

#[test]
fn percent_above_100_is_a_usage_error_exit_2() {
    assert_eq!(usage_exit_code(&["sidetone", "101"]), 2);
    assert_eq!(usage_exit_code(&["volume", "101"]), 2);
    assert_eq!(usage_exit_code(&["brightness", "-1"]), 2);
    assert_eq!(usage_exit_code(&["mix", "game", "101"]), 2);
    assert_eq!(usage_exit_code(&["sidetone", "loud"]), 2);
}

#[test]
fn other_ranges_are_usage_errors_exit_2() {
    assert_eq!(usage_exit_code(&["balance", "13"]), 2);
    assert_eq!(usage_exit_code(&["tones", "3"]), 2);
    assert_eq!(usage_exit_code(&["sleep", "60"]), 2);
    assert_eq!(usage_exit_code(&["gate", "loud"]), 2);
    assert_eq!(usage_exit_code(&["mix", "chat", "50"]), 2);
    assert_eq!(usage_exit_code(&["nonsense"]), 2);
}

#[test]
fn every_setting_command_sends_its_setting_and_confirms() {
    let cases: [(&[&str], Setting, &str); 8] = [
        (
            &["balance", "6"],
            Setting::Balance { value: 6 },
            "balance 6",
        ),
        (
            &["gate", "night"],
            Setting::NoiseGate(Gate::Night),
            "gate night",
        ),
        (
            &["volume", "30"],
            Setting::Volume { percent: 30 },
            "volume 30",
        ),
        (
            &["brightness", "80"],
            Setting::LedBrightness { percent: 80 },
            "brightness 80",
        ),
        (
            &["sleep", "never"],
            Setting::SleepAfter { minutes: 0 },
            "sleep never",
        ),
        (
            &["sleep", "15"],
            Setting::SleepAfter { minutes: 15 },
            "sleep 15",
        ),
        (&["tones", "2"], Setting::Tones { value: 2 }, "tones 2"),
        (&["gate", "off"], Setting::NoiseGate(Gate::Off), "gate off"),
    ];
    for (args, setting, line) in cases {
        let outcome = run_with(args, FakeDaemon::replying([Response::Done]));
        outcome.result.expect("the setting succeeds");
        assert_eq!(outcome.requests, [Request::Set { setting }], "{args:?}");
        assert_eq!(outcome.stdout, format!("{line}\n"), "{args:?}");
    }
}

#[test]
fn status_json_matches_the_ipc_status() {
    let received = status();
    let outcome = run_with(
        &["status", "--json"],
        FakeDaemon::replying([Response::Status(received.clone())]),
    );
    outcome.result.expect("status succeeds");
    assert_eq!(outcome.requests, [Request::Status]);
    let printed: Value = serde_json::from_str(&outcome.stdout).expect("json");
    assert_eq!(printed, serde_json::to_value(&received).expect("json"));
    assert_eq!(printed["headset"], "in_use");
    assert_eq!(outcome.stdout.matches("\"headset\"").count(), 1);
}

#[test]
fn status_human_shows_headset_battery_profile_and_autoswitch() {
    let outcome = run_with(
        &["status"],
        FakeDaemon::replying([Response::Status(status())]),
    );
    outcome.result.expect("status succeeds");
    let text = outcome.stdout;
    for expected in [
        "connection:  connected",
        "headset:     in_use",
        "battery:     85% (discharging)",
        "profile:     gaming",
        "auto-switch: on, active",
        "focus:       game.exe",
        "sidetone:    40%",
        "noise gate:  night",
        "sleep:       never",
        "stream mix:  output 80%, game 50%, voice 70% muted",
    ] {
        assert!(text.contains(expected), "{expected:?} missing from\n{text}");
    }
}

#[test]
fn status_human_says_unknown_for_what_the_daemon_does_not_know() {
    let mut unknown = status();
    unknown.headset = Headset::Unknown;
    unknown.battery = None;
    unknown.active_profile = None;
    unknown.focus = FocusStatus {
        available: false,
        app: None,
    };
    unknown.auto_switch = AutoSwitchStatus {
        enabled: false,
        active: false,
        error: None,
    };
    let outcome = run_with(
        &["status"],
        FakeDaemon::replying([Response::Status(unknown)]),
    );
    outcome.result.expect("status succeeds");
    for expected in [
        "headset:     unknown",
        "battery:     unknown",
        "profile:     none",
        "auto-switch: off",
        "focus:       unavailable",
    ] {
        assert!(
            outcome.stdout.contains(expected),
            "{expected:?} missing from\n{}",
            outcome.stdout
        );
    }
}

#[test]
fn daemon_not_running_exits_6() {
    let outcome = run_with(
        &["status"],
        FakeDaemon::failing(TransportError::DaemonNotRunning),
    );
    assert_eq!(exit_code(&outcome), 6);
    assert_eq!(
        message(&outcome),
        "ogh-daemon is not running; start it or enable its service"
    );
}

#[test]
fn a_version_mismatch_exits_6_with_its_own_message() {
    let outcome = run_with(
        &["status"],
        FakeDaemon::failing(TransportError::VersionMismatch { daemon: 9 }),
    );
    assert_eq!(exit_code(&outcome), 6);
    assert_eq!(
        message(&outcome),
        TransportError::VersionMismatch { daemon: 9 }.to_string()
    );
}

#[test]
fn a_missing_endpoint_exits_6_after_the_standard_text() {
    let error = CliError::Endpoint(TransportError::XdgRuntimeDirUnset);
    assert_eq!(error.exit_code(), 6);
    assert_eq!(
        error.to_string(),
        "ogh-daemon is not running; start it or enable its service (XDG_RUNTIME_DIR is not set)"
    );
}

#[test]
fn other_transport_errors_exit_5() {
    let outcome = run_with(
        &["status"],
        FakeDaemon::failing(TransportError::Io(
            "ogh-daemon closed the connection".into(),
        )),
    );
    assert_eq!(exit_code(&outcome), 5);
}

#[test]
fn ghub_running_exits_4() {
    let outcome = run_with(
        &["sidetone", "40"],
        FakeDaemon::replying([error(ErrorKind::GhubRunning, "G Hub is running")]),
    );
    assert_eq!(exit_code(&outcome), 4);
    assert_eq!(message(&outcome), "G Hub is running");
}

#[test]
fn not_found_exits_3() {
    let outcome = run_with(
        &["sidetone", "40"],
        FakeDaemon::replying([error(ErrorKind::NotConnected, "no base station found")]),
    );
    assert_eq!(exit_code(&outcome), 3);
    assert_eq!(message(&outcome), "no base station found");
}

#[test]
fn every_other_error_kind_exits_5() {
    for kind in [
        ErrorKind::VersionMismatch,
        ErrorKind::BadRequest,
        ErrorKind::HeadsetOff,
        ErrorKind::InvalidValue,
        ErrorKind::NoSuchProfile,
        ErrorKind::ConfigUnavailable,
        ErrorKind::Device,
    ] {
        let outcome = run_with(
            &["profile", "apply", "x"],
            FakeDaemon::replying([error(kind, "refused")]),
        );
        assert_eq!(exit_code(&outcome), 5, "{kind:?}");
        assert_eq!(message(&outcome), "refused");
    }
}

#[test]
fn an_unexpected_response_exits_5() {
    let outcome = run_with(&["status"], FakeDaemon::replying([Response::Done]));
    assert_eq!(exit_code(&outcome), 5);
}

#[cfg(target_os = "linux")]
#[test]
fn access_denied_explains_udev_on_linux() {
    let outcome = run_with(
        &["status"],
        FakeDaemon::replying([error(
            ErrorKind::NotConnected,
            "cannot open the base station: Permission denied (os error 13)",
        )]),
    );
    assert_eq!(exit_code(&outcome), 3);
    let text = message(&outcome);
    assert!(text.contains("udev"), "{text}");
    assert!(text.contains("046d:0b0b"), "{text}");
}

#[test]
fn other_not_connected_messages_get_no_permission_hint() {
    let outcome = run_with(
        &["status"],
        FakeDaemon::replying([error(ErrorKind::NotConnected, "no base station found")]),
    );
    assert!(!message(&outcome).contains("udev"));
}

fn bands(eq: &Eq) -> Vec<(u16, Option<f32>, f32)> {
    eq.bands
        .iter()
        .map(|b| (b.freq_hz, b.q, b.gain_db))
        .collect()
}

fn card(equalizer: &eq::Equalizer) -> Vec<(u16, Option<f32>, f32)> {
    equalizer
        .bands
        .iter()
        .map(|b| (b.freq_hz, b.q, b.gain_db))
        .collect()
}

fn sent_eq(outcome: &Outcome) -> Eq {
    match &outcome.requests[..] {
        [
            Request::Set {
                setting: Setting::HeadphoneEq(eq) | Setting::MicEq(eq),
            },
        ] => eq.clone(),
        other => panic!("expected one EQ set, got {other:?}"),
    }
}

#[test]
fn eq_preset_gaming_sends_the_curated_card() {
    let outcome = run_with(
        &["eq", "headphone", "preset", "gaming"],
        FakeDaemon::replying([Response::Done]),
    );
    outcome.result.as_ref().expect("preset succeeds");
    assert!(matches!(
        outcome.requests[..],
        [Request::Set {
            setting: Setting::HeadphoneEq(_)
        }]
    ));
    assert_eq!(bands(&sent_eq(&outcome)), card(&eq::HEADPHONE_GAMING));
    assert_eq!(outcome.stdout, "eq headphone preset gaming\n");
}

#[test]
fn every_curated_preset_sends_its_card() {
    let cases: [(&str, &str, &eq::Equalizer); 6] = [
        ("headphone", "default", &eq::HEADPHONE_DEFAULT),
        ("headphone", "media", &eq::HEADPHONE_MEDIA),
        ("mic", "default", &eq::MIC_DEFAULT),
        ("mic", "competition", &eq::MIC_COMPETITION),
        ("mic", "broadcast", &eq::MIC_BROADCAST),
        ("headphone", "gaming", &eq::HEADPHONE_GAMING),
    ];
    for (target, preset, expected) in cases {
        let outcome = run_with(
            &["eq", target, "preset", preset],
            FakeDaemon::replying([Response::Done]),
        );
        outcome.result.as_ref().expect("preset succeeds");
        assert_eq!(
            bands(&sent_eq(&outcome)),
            card(expected),
            "{target} {preset}"
        );
    }
}

#[test]
fn a_preset_of_the_other_instance_is_a_usage_error_without_a_request() {
    for args in [
        ["eq", "headphone", "preset", "competition"],
        ["eq", "headphone", "preset", "broadcast"],
        ["eq", "mic", "preset", "gaming"],
        ["eq", "mic", "preset", "media"],
    ] {
        let outcome = run_with(&args, FakeDaemon::default());
        assert_eq!(exit_code(&outcome), 2, "{args:?}");
        assert!(outcome.requests.is_empty(), "{args:?}");
    }
}

#[test]
fn eq_gains_sends_a_graphic_card_on_the_instances_frequencies() {
    let outcome = run_with(
        &[
            "eq", "mic", "gains", "-1", "0", "0", "0", "0", "0", "0", "0", "1", "2",
        ],
        FakeDaemon::replying([Response::Done]),
    );
    outcome.result.as_ref().expect("gains succeed");
    assert!(matches!(
        outcome.requests[..],
        [Request::Set {
            setting: Setting::MicEq(_)
        }]
    ));
    let sent = sent_eq(&outcome);
    let expected: Vec<EqBand> = eq::MIC_DEFAULT
        .bands
        .iter()
        .zip([-1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0])
        .map(|(band, gain_db)| EqBand {
            freq_hz: band.freq_hz,
            q: None,
            gain_db,
        })
        .collect();
    assert_eq!(sent.bands, expected);
    assert_eq!(outcome.stdout, "eq mic gains -1 0 0 0 0 0 0 0 1 2\n");
}

#[test]
fn eq_gains_on_the_headphones_use_their_frequencies_without_q() {
    let outcome = run_with(
        &[
            "eq",
            "headphone",
            "gains",
            "1",
            "2",
            "3",
            "4",
            "5",
            "6",
            "-1",
            "-2",
            "-3",
            "-4",
        ],
        FakeDaemon::replying([Response::Done]),
    );
    outcome.result.as_ref().expect("gains succeed");
    assert!(matches!(
        outcome.requests[..],
        [Request::Set {
            setting: Setting::HeadphoneEq(_)
        }]
    ));
    let expected: Vec<(u16, Option<f32>, f32)> = eq::HEADPHONE_DEFAULT
        .bands
        .iter()
        .zip([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, -1.0, -2.0, -3.0, -4.0])
        .map(|(band, gain_db)| (band.freq_hz, None, gain_db))
        .collect();
    assert_eq!(bands(&sent_eq(&outcome)), expected);
    assert_eq!(
        outcome.stdout,
        "eq headphone gains 1 2 3 4 5 6 -1 -2 -3 -4\n"
    );
}

#[test]
fn eq_gains_needs_ten_values_in_range() {
    let nine = [
        "eq", "mic", "gains", "0", "0", "0", "0", "0", "0", "0", "0", "0",
    ];
    assert_eq!(usage_exit_code(&nine), 2);
    let eleven = [
        "eq", "mic", "gains", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0",
    ];
    assert_eq!(usage_exit_code(&eleven), 2);
    let high = [
        "eq", "mic", "gains", "6.5", "0", "0", "0", "0", "0", "0", "0", "0", "0",
    ];
    assert_eq!(usage_exit_code(&high), 2);
    let low = [
        "eq",
        "headphone",
        "gains",
        "0",
        "0",
        "0",
        "0",
        "0",
        "0",
        "0",
        "0",
        "0",
        "-6.5",
    ];
    assert_eq!(usage_exit_code(&low), 2);
    let edges = [
        "ogh",
        "eq",
        "headphone",
        "gains",
        "-6",
        "6",
        "0",
        "0",
        "0",
        "0",
        "0",
        "0",
        "0",
        "0",
    ];
    assert!(Cli::try_parse_from(edges).is_ok());
}

#[test]
fn profile_link_lowercases_nothing_itself() {
    let outcome = run_with(
        &["profile", "link", "Gaming", "Game.EXE"],
        FakeDaemon::replying([Response::Done]),
    );
    outcome.result.expect("link succeeds");
    assert_eq!(
        outcome.requests,
        [Request::LinkApp {
            profile: "Gaming".to_string(),
            app: "Game.EXE".to_string(),
        }]
    );
    assert_eq!(outcome.stdout, "linked Game.EXE to Gaming\n");
}

#[test]
fn profile_commands_send_their_requests_and_confirm() {
    let cases: [(&[&str], Request, &str); 5] = [
        (
            &["profile", "apply", "gaming"],
            Request::ApplyProfile {
                name: "gaming".into(),
            },
            "applied profile gaming",
        ),
        (
            &["profile", "save", "gaming"],
            Request::SaveProfile {
                name: "gaming".into(),
            },
            "saved profile gaming",
        ),
        (
            &["profile", "delete", "gaming"],
            Request::DeleteProfile {
                name: "gaming".into(),
            },
            "deleted profile gaming",
        ),
        (
            &["profile", "default", "gaming"],
            Request::SetDefaultProfile {
                name: "gaming".into(),
            },
            "default profile gaming",
        ),
        (
            &["profile", "unlink", "game.exe"],
            Request::UnlinkApp {
                app: "game.exe".into(),
            },
            "unlinked game.exe",
        ),
    ];
    for (args, request, line) in cases {
        let outcome = run_with(args, FakeDaemon::replying([Response::Done]));
        outcome.result.expect("the profile command succeeds");
        assert_eq!(outcome.requests, [request], "{args:?}");
        assert_eq!(outcome.stdout, format!("{line}\n"), "{args:?}");
    }
}

fn profiles() -> Vec<ProfileInfo> {
    vec![
        ProfileInfo {
            name: "gaming".to_string(),
            apps: vec!["game.exe".to_string(), "other.exe".to_string()],
            is_default: false,
            settings: ProfileSettings::default(),
        },
        ProfileInfo {
            name: "media".to_string(),
            apps: vec![],
            is_default: true,
            settings: ProfileSettings::default(),
        },
    ]
}

#[test]
fn profile_list_marks_the_default_and_lists_linked_apps() {
    let outcome = run_with(
        &["profile", "list"],
        FakeDaemon::replying([Response::Profiles(profiles())]),
    );
    outcome.result.expect("list succeeds");
    assert_eq!(outcome.requests, [Request::ListProfiles]);
    assert_eq!(
        outcome.stdout,
        "  gaming (apps: game.exe, other.exe)\n* media\n"
    );
}

#[test]
fn profile_list_with_no_profiles_prints_nothing() {
    let outcome = run_with(
        &["profile", "list"],
        FakeDaemon::replying([Response::Profiles(vec![])]),
    );
    outcome.result.expect("list succeeds");
    assert_eq!(outcome.stdout, "");
}

#[test]
fn profile_list_json_prints_the_profiles() {
    let outcome = run_with(
        &["profile", "list", "--json"],
        FakeDaemon::replying([Response::Profiles(profiles())]),
    );
    outcome.result.expect("list succeeds");
    let printed: Value = serde_json::from_str(&outcome.stdout).expect("json");
    assert_eq!(printed, serde_json::to_value(profiles()).expect("json"));
}

#[test]
fn autoswitch_sends_the_flag() {
    for (word, enabled) in [("on", true), ("off", false)] {
        let outcome = run_with(
            &["autoswitch", word],
            FakeDaemon::replying([Response::Done]),
        );
        outcome.result.expect("autoswitch succeeds");
        assert_eq!(outcome.requests, [Request::SetAutoSwitch { enabled }]);
        assert_eq!(outcome.stdout, format!("auto-switch {word}\n"));
    }
}

#[test]
fn features_prints_one_line_per_feature() {
    let features = vec![
        Feature {
            index: 3,
            id: 0x1004,
            name: Some("battery".to_string()),
            version: 1,
        },
        Feature {
            index: 7,
            id: 0xabcd,
            name: None,
            version: 2,
        },
    ];
    let outcome = run_with(
        &["features"],
        FakeDaemon::replying([Response::Features(features)]),
    );
    outcome.result.expect("features succeeds");
    assert_eq!(outcome.stdout, "3 0x1004 battery 1\n7 0xabcd ? 2\n");
}

fn mix_set(outcome: &Outcome) -> StreamMixSetting {
    match &outcome.requests[..] {
        [
            Request::Status,
            Request::Set {
                setting: Setting::StreamMix(mix),
            },
        ] => *mix,
        other => panic!("expected status then a stream mix set, got {other:?}"),
    }
}

fn run_mix(args: &[&str], status: Status) -> Outcome {
    run_with(
        args,
        FakeDaemon::replying([Response::Status(status), Response::Done]),
    )
}

#[test]
fn mix_changes_only_the_named_channels_level() {
    let outcome = run_mix(&["mix", "game", "20"], status());
    outcome.result.as_ref().expect("mix succeeds");
    assert_eq!(
        mix_set(&outcome),
        StreamMixSetting {
            game: channel(20, false),
            ..mix()
        }
    );
    assert_eq!(outcome.stdout, "mix game 20\n");
}

#[test]
fn mix_mute_and_unmute_leave_the_level_alone() {
    let outcome = run_mix(&["mix", "output", "mute"], status());
    outcome.result.as_ref().expect("mute succeeds");
    assert_eq!(
        mix_set(&outcome),
        StreamMixSetting {
            output: channel(80, true),
            ..mix()
        }
    );
    assert_eq!(outcome.stdout, "mix output muted\n");

    let outcome = run_mix(&["mix", "voice", "unmute"], status());
    outcome.result.as_ref().expect("unmute succeeds");
    assert_eq!(
        mix_set(&outcome),
        StreamMixSetting {
            voice: channel(70, false),
            ..mix()
        }
    );
    assert_eq!(outcome.stdout, "mix voice unmuted\n");
}

fn without_mix(connection: Connection) -> Status {
    let mut status = status();
    status.connection = connection;
    status.settings.profile.stream_mix = None;
    status
}

#[test]
fn mix_without_a_known_stream_mix_reports_the_connection() {
    let cases = [
        (Connection::NotFound, 3, "base station not found"),
        (Connection::GhubRunning, 4, "G Hub is running"),
        (
            Connection::AccessDenied {
                message: "denied".into(),
            },
            5,
            "access denied: denied",
        ),
        (
            Connection::Error {
                message: "broken".into(),
            },
            5,
            "error: broken",
        ),
        (Connection::Connected, 5, "stream mix is not known yet"),
    ];
    for (connection, code, text) in cases {
        let outcome = run_with(
            &["mix", "game", "20"],
            FakeDaemon::replying([Response::Status(without_mix(connection.clone()))]),
        );
        assert_eq!(exit_code(&outcome), code, "{connection:?}");
        assert!(message(&outcome).starts_with(text), "{connection:?}");
        assert_eq!(outcome.requests, [Request::Status]);
    }
}

fn watch_daemon(notifications: Vec<Result<Notification, TransportError>>) -> FakeDaemon {
    FakeDaemon {
        notifications,
        ..FakeDaemon::default()
    }
}

#[test]
fn watch_prints_one_line_per_status_change_and_ends_when_the_daemon_closes() {
    let mut docked = status();
    docked.headset = Headset::Docked;
    docked.battery = Some(Battery {
        percent: 90,
        charge: Charge::Charging,
    });
    docked.active_profile = None;
    let daemon = watch_daemon(vec![
        Ok(Notification::Status(status())),
        Ok(Notification::Status(docked)),
    ]);
    let outcome = run_with(&["watch"], daemon);
    outcome.result.as_ref().expect("watch ends cleanly");
    assert!(outcome.requests.is_empty());
    assert_eq!(
        outcome.stdout,
        "connection connected | headset in_use | battery 85% | profile gaming\n\
         connection connected | headset docked | battery 90% | profile none\n"
    );
}

#[test]
fn watch_json_prints_single_line_statuses() {
    let daemon = watch_daemon(vec![Ok(Notification::Status(status()))]);
    let outcome = run_with(&["watch", "--json"], daemon);
    outcome.result.as_ref().expect("watch ends cleanly");
    assert_eq!(outcome.stdout.lines().count(), 1);
    let printed: Value = serde_json::from_str(outcome.stdout.trim()).expect("json");
    assert_eq!(printed, serde_json::to_value(status()).expect("json"));
}

#[test]
fn watch_maps_a_broken_stream_like_any_transport_error() {
    let daemon = watch_daemon(vec![
        Ok(Notification::Status(status())),
        Err(TransportError::Io("broken pipe".to_string())),
    ]);
    let outcome = run_with(&["watch"], daemon);
    assert_eq!(exit_code(&outcome), 5);
    assert_eq!(outcome.stdout.lines().count(), 1);
}

#[test]
fn watch_exits_6_when_the_daemon_is_not_running() {
    let daemon = FakeDaemon {
        subscribe_error: Some(TransportError::DaemonNotRunning),
        ..FakeDaemon::default()
    };
    let outcome = run_with(&["watch"], daemon);
    assert_eq!(exit_code(&outcome), 6);
}
