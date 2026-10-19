use std::io::{self, BufRead, Cursor, Read};

use ogh_ipc::*;

fn gate_status() -> Status {
    Status {
        connection: Connection::Connected,
        headset: Headset::Docked,
        battery: Some(Battery {
            percent: 80,
            charge: Charge::Other(7),
        }),
        firmware: Some("1.2.3".into()),
        settings: DeviceSettings {
            profile: ProfileSettings {
                sidetone_percent: Some(40),
                noise_gate: Some(Gate::Night),
                ..ProfileSettings::default()
            },
            volume_percent: Some(55),
            led_brightness_percent: None,
            sleep_after_minutes: Some(15),
            tones: Some(1),
        },
        active_profile: Some("game".into()),
        focus: FocusStatus {
            available: true,
            app: Some("game.exe".into()),
        },
        auto_switch: AutoSwitchStatus {
            enabled: true,
            active: false,
            error: None,
        },
        config_error: None,
    }
}

fn band(freq_hz: u16, q: Option<f32>) -> EqBand {
    EqBand {
        freq_hz,
        q,
        gain_db: 0.0,
    }
}

fn graphic_eq(first: u16) -> Eq {
    Eq {
        bands: (0..10).map(|i| band(first + i * 100, None)).collect(),
    }
}

fn channel(level_percent: u8, muted: bool) -> Channel {
    Channel {
        level_percent,
        muted,
    }
}

fn round_trip<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let mut out = Vec::new();
    write_line(&mut out, value).unwrap();
    assert_eq!(out.last(), Some(&b'\n'));
    let back: T = read_line(&mut Cursor::new(out)).unwrap().unwrap();
    assert_eq!(&back, value);
}

#[test]
fn request_wire_form_is_stable() {
    let set = Request::Set {
        setting: Setting::Sidetone { percent: 40 },
    };
    assert_eq!(
        serde_json::to_string(&set).unwrap(),
        r#"{"cmd":"set","setting":{"sidetone":{"percent":40}}}"#
    );
    assert_eq!(
        serde_json::to_string(&Request::Hello { version: 1 }).unwrap(),
        r#"{"cmd":"hello","version":1}"#
    );
}

#[test]
fn status_uses_one_headset_field() {
    let json = serde_json::to_string(&gate_status()).unwrap();
    assert!(json.contains(r#""headset":"docked""#));
    assert!(!json.contains("headset_connected"));
    assert!(!json.contains("in_use"));
}

#[test]
fn unknown_command_is_a_json_error_not_a_panic() {
    let mut input = Cursor::new(b"{\"cmd\":\"format_disk\"}\n".to_vec());
    let err = read_line::<_, Request>(&mut input).unwrap_err();
    assert!(matches!(err, CodecError::Json(_)), "{err:?}");
}

#[test]
fn unknown_field_on_a_struct_request_is_rejected() {
    let line = b"{\"cmd\":\"hello\",\"version\":1,\"extra\":1}\n".to_vec();
    let err = read_line::<_, Request>(&mut Cursor::new(line)).unwrap_err();
    assert!(matches!(err, CodecError::Json(_)), "{err:?}");
}

struct CountingReader<R> {
    inner: R,
    consumed: usize,
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.consumed += n;
        Ok(n)
    }
}

#[test]
fn line_over_limit_is_rejected_without_reading_past_it() {
    let mut reader = io::BufReader::with_capacity(
        4096,
        CountingReader {
            inner: io::repeat(b'a'),
            consumed: 0,
        },
    );
    let err = read_line::<_, Request>(&mut reader).unwrap_err();
    assert!(matches!(err, CodecError::TooLong), "{err:?}");
    let consumed = reader.get_ref().consumed;
    assert!(consumed <= MAX_LINE_BYTES + 1 + 4096, "read {consumed}");
}

#[test]
fn line_at_limit_is_not_too_long() {
    let mut line = vec![b' '; MAX_LINE_BYTES - 16];
    line.extend_from_slice(br#"{"cmd":"status"}"#);
    line.push(b'\n');
    let got: Request = read_line(&mut Cursor::new(line)).unwrap().unwrap();
    assert_eq!(got, Request::Status);
}

#[test]
fn one_byte_over_limit_is_too_long() {
    let mut line = vec![b' '; MAX_LINE_BYTES + 1];
    line.push(b'\n');
    let err = read_line::<_, Request>(&mut Cursor::new(line)).unwrap_err();
    assert!(matches!(err, CodecError::TooLong), "{err:?}");
}

#[test]
fn eof_is_none() {
    let got = read_line::<_, Request>(&mut Cursor::new(Vec::new())).unwrap();
    assert!(got.is_none());
}

#[test]
fn final_line_without_newline_is_parsed() {
    let mut input = Cursor::new(br#"{"cmd":"status"}"#.to_vec());
    let got: Request = read_line(&mut input).unwrap().unwrap();
    assert_eq!(got, Request::Status);
    assert!(read_line::<_, Request>(&mut input).unwrap().is_none());
}

#[test]
fn consecutive_lines_are_read_one_at_a_time() {
    let mut input = Cursor::new(b"{\"cmd\":\"status\"}\n{\"cmd\":\"features\"}\n".to_vec());
    assert_eq!(
        read_line::<_, Request>(&mut input).unwrap(),
        Some(Request::Status)
    );
    assert_eq!(
        read_line::<_, Request>(&mut input).unwrap(),
        Some(Request::Features)
    );
    assert!(input.fill_buf().unwrap().is_empty());
}

#[test]
fn eq_validate_accepts_graphic_and_parametric() {
    assert_eq!(graphic_eq(20).validate(20), Ok(()));
    let parametric = Eq {
        bands: (0..10).map(|i| band(60 + i * 100, Some(1.0))).collect(),
    };
    assert_eq!(parametric.validate(60), Ok(()));
}

#[test]
fn eq_validate_rejects_nine_bands_and_falling_frequencies() {
    let mut nine = graphic_eq(20);
    nine.bands.pop();
    assert!(nine.validate(20).is_err());

    let mut falling = graphic_eq(20);
    falling.bands[5].freq_hz = falling.bands[4].freq_hz;
    assert!(falling.validate(20).is_err());
    falling.bands[5].freq_hz = 1;
    assert!(falling.validate(20).is_err());
}

#[test]
fn eq_validate_rejects_low_first_band_and_mixed_modes() {
    assert!(graphic_eq(59).validate(60).is_err());

    let mut mixed = graphic_eq(20);
    mixed.bands[3].q = Some(1.0);
    assert!(mixed.validate(20).is_err());
}

#[test]
fn every_request_round_trips() {
    let requests = vec![
        Request::Hello {
            version: IPC_VERSION,
        },
        Request::Status,
        Request::Features,
        Request::Set {
            setting: Setting::Sidetone { percent: 40 },
        },
        Request::Set {
            setting: Setting::Balance { value: 6 },
        },
        Request::Set {
            setting: Setting::NoiseGate(Gate::Tournament),
        },
        Request::Set {
            setting: Setting::HeadphoneEq(graphic_eq(20)),
        },
        Request::Set {
            setting: Setting::MicEq(Eq {
                bands: (0..10).map(|i| band(60 + i * 100, Some(0.5))).collect(),
            }),
        },
        Request::Set {
            setting: Setting::StreamMix(StreamMixSetting {
                output: channel(50, false),
                game: channel(60, true),
                voice: channel(70, false),
            }),
        },
        Request::Set {
            setting: Setting::Volume { percent: 30 },
        },
        Request::Set {
            setting: Setting::LedBrightness { percent: 80 },
        },
        Request::Set {
            setting: Setting::SleepAfter { minutes: 15 },
        },
        Request::Set {
            setting: Setting::Tones { value: 1 },
        },
        Request::ListProfiles,
        Request::ApplyProfile { name: "a".into() },
        Request::SaveProfile { name: "a".into() },
        Request::DeleteProfile { name: "a".into() },
        Request::SetDefaultProfile { name: "a".into() },
        Request::LinkApp {
            profile: "a".into(),
            app: "b.exe".into(),
        },
        Request::UnlinkApp {
            app: "b.exe".into(),
        },
        Request::SetAutoSwitch { enabled: true },
        Request::Subscribe,
    ];
    for request in &requests {
        round_trip(request);
    }
}

#[test]
fn every_response_and_notification_round_trips() {
    let responses = vec![
        Response::Hello(Hello {
            version: IPC_VERSION,
            daemon_version: "0.1.0".into(),
        }),
        Response::Status(gate_status()),
        Response::Features(vec![Feature {
            index: 3,
            id: 0x8010,
            name: None,
            version: 2,
        }]),
        Response::Profiles(vec![ProfileInfo {
            name: "game".into(),
            apps: vec!["game.exe".into()],
            is_default: true,
            settings: ProfileSettings {
                headphone_eq: Some(graphic_eq(20)),
                ..ProfileSettings::default()
            },
        }]),
        Response::Done,
        Response::Error(IpcError {
            kind: ErrorKind::InvalidValue,
            message: "bad".into(),
        }),
    ];
    for response in &responses {
        round_trip(response);
    }
    round_trip(&Notification::Status(gate_status()));
}

#[test]
fn status_variants_round_trip() {
    for connection in [
        Connection::Connected,
        Connection::NotFound,
        Connection::GhubRunning,
        Connection::AccessDenied {
            message: "x".into(),
        },
        Connection::Error {
            message: "y".into(),
        },
    ] {
        round_trip(&connection);
    }
    for headset in [
        Headset::InUse,
        Headset::Docked,
        Headset::Off,
        Headset::Unknown,
    ] {
        round_trip(&headset);
    }
    for charge in [
        Charge::Discharging,
        Charge::Charging,
        Charge::Full,
        Charge::Other(9),
    ] {
        round_trip(&charge);
    }
    for kind in [
        ErrorKind::VersionMismatch,
        ErrorKind::BadRequest,
        ErrorKind::NotConnected,
        ErrorKind::GhubRunning,
        ErrorKind::HeadsetOff,
        ErrorKind::InvalidValue,
        ErrorKind::NoSuchProfile,
        ErrorKind::ConfigUnavailable,
        ErrorKind::Device,
    ] {
        round_trip(&kind);
    }
}

#[test]
fn empty_profile_settings_serialize_as_empty_object() {
    assert_eq!(
        serde_json::to_string(&ProfileSettings::default()).unwrap(),
        "{}"
    );
    let parsed: ProfileSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(parsed, ProfileSettings::default());
}
