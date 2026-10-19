use ogh_daemon::convert::{device_settings, diff, profile_settings, write_for};
use ogh_daemon::snapshot::{DeviceSnapshot, DeviceWrite};
use ogh_ipc::{Channel, ErrorKind, Gate, Headset, ProfileSettings, Setting, StreamMixSetting};
use ogh_protocol::features::Event;
use ogh_protocol::features::advanced_para_eq::{
    CustomEq, EqInstance, HEADPHONE_DEFAULT, HEADPHONE_GAMING, MIC_BROADCAST, MIC_DEFAULT,
};
use ogh_protocol::features::auto_sleep::{AutoSleep, SleepAfter};
use ogh_protocol::features::battery::{Battery, ChargeStatus};
use ogh_protocol::features::headset_mix::GameVoiceBalance;
use ogh_protocol::features::mixer_audio::{
    MixChannel, MixInput, MixLevel, PINNED_INPUT, StreamMix,
};
use ogh_protocol::features::noise_gate::NoiseGate;
use ogh_protocol::features::sidetone::Sidetone;
use ogh_protocol::features::tones::Tones;
use ogh_protocol::features::volume::{Volume, VolumeLevel};

fn channel(level: u8, muted: bool) -> MixChannel {
    MixChannel {
        level: MixLevel(level),
        muted,
    }
}

fn mix() -> StreamMix {
    StreamMix {
        stream: channel(16, false),
        inputs: [
            PINNED_INPUT,
            channel(20, false),
            channel(9, true),
            channel(10, false),
        ],
    }
}

fn battery(status: ChargeStatus) -> Battery {
    Battery {
        level_a: 80,
        level_b: 80,
        status,
    }
}

fn full_snapshot() -> DeviceSnapshot {
    DeviceSnapshot {
        battery: Some(battery(ChargeStatus::Discharging)),
        headset_available: Some(true),
        sidetone: Some(Sidetone(2)),
        balance: Some(GameVoiceBalance(6)),
        noise_gate: Some(NoiseGate::Home),
        headphone_eq: Some(HEADPHONE_GAMING),
        mic_eq: Some(MIC_DEFAULT),
        stream_mix: Some(mix()),
        volume: Some(Volume {
            scaled: 16,
            muted: false,
            level: VolumeLevel(11),
        }),
        led_brightness: Some(100),
        auto_sleep: Some(AutoSleep {
            prefix: 0x3c,
            sleep_after: SleepAfter(15),
        }),
        tones: Some(Tones(1)),
        firmware: Some("1.2.3".to_string()),
    }
}

fn error_kind<T: std::fmt::Debug>(result: Result<T, ogh_ipc::IpcError>) -> ErrorKind {
    result.unwrap_err().kind
}

#[test]
fn equal_settings_produce_no_writes() {
    let snapshot = full_snapshot();
    let target = profile_settings(&snapshot);
    assert_eq!(diff(&snapshot, &target).unwrap(), vec![]);
}

#[test]
fn every_level_survives_a_percent_round_trip() {
    for level in 0..=6 {
        let mut snapshot = full_snapshot();
        snapshot.sidetone = Some(Sidetone(level));
        let target = profile_settings(&snapshot);
        assert_eq!(
            diff(&snapshot, &target).unwrap(),
            vec![],
            "sidetone {level}"
        );
    }
    for level in 0..=32 {
        let mut snapshot = full_snapshot();
        snapshot.stream_mix.as_mut().unwrap().stream = channel(level, false);
        snapshot.stream_mix.as_mut().unwrap().inputs[1] = channel(level, true);
        snapshot.stream_mix.as_mut().unwrap().inputs[3] = channel(level, false);
        let target = profile_settings(&snapshot);
        assert_eq!(diff(&snapshot, &target).unwrap(), vec![], "mix {level}");
    }
}

#[test]
fn sidetone_percent_compares_in_device_levels() {
    let snapshot = full_snapshot();
    let same = ProfileSettings {
        sidetone_percent: Some(40),
        ..Default::default()
    };
    assert_eq!(diff(&snapshot, &same).unwrap(), vec![]);

    let louder = ProfileSettings {
        sidetone_percent: Some(60),
        ..Default::default()
    };
    assert_eq!(
        diff(&snapshot, &louder).unwrap(),
        vec![DeviceWrite::Sidetone(Sidetone(4))]
    );
}

#[test]
fn eq_compares_by_wire_bytes() {
    let snapshot = full_snapshot();
    let mut eq = profile_settings(&snapshot).headphone_eq.unwrap();
    eq.bands[3].gain_db += 0.001;
    let target = ProfileSettings {
        headphone_eq: Some(eq.clone()),
        ..Default::default()
    };
    assert_eq!(diff(&snapshot, &target).unwrap(), vec![]);

    eq.bands[3].gain_db += 0.5;
    let expected_gain = eq.bands[3].gain_db;
    let target = ProfileSettings {
        headphone_eq: Some(eq),
        ..Default::default()
    };
    let writes = diff(&snapshot, &target).unwrap();
    assert_eq!(writes.len(), 1);
    let DeviceWrite::Eq(EqInstance::Headphone, written) = &writes[0] else {
        panic!("expected a headphone EQ write, got {:?}", writes[0]);
    };
    assert_eq!(written.bands[3].gain_db, expected_gain);
}

#[test]
fn eq_that_cannot_be_encoded_is_invalid_value() {
    let snapshot = full_snapshot();
    let mut eq = profile_settings(&snapshot).mic_eq.unwrap();
    eq.bands[0].gain_db = 9.0;
    let target = ProfileSettings {
        mic_eq: Some(eq),
        ..Default::default()
    };
    assert_eq!(
        error_kind(diff(&snapshot, &target)),
        ErrorKind::InvalidValue
    );

    let mut short = profile_settings(&snapshot).mic_eq.unwrap();
    short.bands.pop();
    assert_eq!(
        error_kind(write_for(&Setting::MicEq(short), &snapshot)),
        ErrorKind::InvalidValue
    );
}

#[test]
fn stream_mix_keeps_pinned_inputs() {
    let snapshot = full_snapshot();
    let setting = StreamMixSetting {
        output: Channel {
            level_percent: 100,
            muted: true,
        },
        game: Channel {
            level_percent: 50,
            muted: false,
        },
        voice: Channel {
            level_percent: 0,
            muted: true,
        },
    };
    let DeviceWrite::StreamMix(written) =
        write_for(&Setting::StreamMix(setting), &snapshot).unwrap()
    else {
        panic!("expected a stream mix write");
    };
    let current = snapshot.stream_mix.unwrap();
    assert_eq!(
        written.input(MixInput::MicOut),
        current.input(MixInput::MicOut)
    );
    assert_eq!(
        written.input(MixInput::Bluetooth),
        current.input(MixInput::Bluetooth)
    );
    assert_eq!(written.stream, channel(32, true));
    assert_eq!(written.input(MixInput::Game), channel(16, false));
    assert_eq!(written.input(MixInput::Voice), channel(0, true));
}

#[test]
fn stream_mix_without_a_current_mix_is_not_connected() {
    let setting = StreamMixSetting {
        output: Channel {
            level_percent: 50,
            muted: false,
        },
        game: Channel {
            level_percent: 50,
            muted: false,
        },
        voice: Channel {
            level_percent: 50,
            muted: false,
        },
    };
    assert_eq!(
        error_kind(write_for(
            &Setting::StreamMix(setting),
            &DeviceSnapshot::default()
        )),
        ErrorKind::NotConnected
    );
}

#[test]
fn unknown_current_value_is_written() {
    let snapshot = DeviceSnapshot::default();
    let target = ProfileSettings {
        sidetone_percent: Some(40),
        balance: Some(6),
        noise_gate: Some(Gate::Night),
        ..Default::default()
    };
    assert_eq!(
        diff(&snapshot, &target).unwrap(),
        vec![
            DeviceWrite::Sidetone(Sidetone(2)),
            DeviceWrite::Balance(GameVoiceBalance(6)),
            DeviceWrite::NoiseGate(NoiseGate::Night),
        ]
    );
}

#[test]
fn writes_follow_the_order_of_profile_fields() {
    let snapshot = DeviceSnapshot {
        stream_mix: Some(mix()),
        ..Default::default()
    };
    let mut target = profile_settings(&full_snapshot());
    target.sidetone_percent = Some(100);
    target.stream_mix = Some(StreamMixSetting {
        output: Channel {
            level_percent: 0,
            muted: false,
        },
        game: Channel {
            level_percent: 0,
            muted: false,
        },
        voice: Channel {
            level_percent: 0,
            muted: false,
        },
    });
    let order: Vec<&str> = diff(&snapshot, &target)
        .unwrap()
        .iter()
        .map(|write| match write {
            DeviceWrite::Sidetone(_) => "sidetone",
            DeviceWrite::Balance(_) => "balance",
            DeviceWrite::NoiseGate(_) => "noise_gate",
            DeviceWrite::Eq(EqInstance::Headphone, _) => "headphone_eq",
            DeviceWrite::Eq(EqInstance::Mic, _) => "mic_eq",
            DeviceWrite::StreamMix(_) => "stream_mix",
            _ => "other",
        })
        .collect();
    assert_eq!(
        order,
        [
            "sidetone",
            "balance",
            "noise_gate",
            "headphone_eq",
            "mic_eq",
            "stream_mix"
        ]
    );
}

#[test]
fn out_of_range_percent_is_invalid_value() {
    let snapshot = full_snapshot();
    let error = write_for(&Setting::Sidetone { percent: 101 }, &snapshot).unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidValue);
    assert!(error.message.contains("sidetone"), "{}", error.message);

    let target = ProfileSettings {
        sidetone_percent: Some(101),
        ..Default::default()
    };
    assert_eq!(
        error_kind(diff(&snapshot, &target)),
        ErrorKind::InvalidValue
    );

    for setting in [
        Setting::Volume { percent: 101 },
        Setting::LedBrightness { percent: 101 },
        Setting::Balance { value: 13 },
        Setting::SleepAfter { minutes: 60 },
        Setting::Tones { value: 3 },
    ] {
        assert_eq!(
            error_kind(write_for(&setting, &snapshot)),
            ErrorKind::InvalidValue,
            "{setting:?}"
        );
    }
}

#[test]
fn settings_convert_to_device_writes() {
    let snapshot = full_snapshot();
    let cases = [
        (
            Setting::Sidetone { percent: 100 },
            DeviceWrite::Sidetone(Sidetone(6)),
        ),
        (
            Setting::Balance { value: 12 },
            DeviceWrite::Balance(GameVoiceBalance(12)),
        ),
        (
            Setting::NoiseGate(Gate::Tournament),
            DeviceWrite::NoiseGate(NoiseGate::Tournament),
        ),
        (
            Setting::Volume { percent: 50 },
            DeviceWrite::Volume(VolumeLevel(11)),
        ),
        (
            Setting::LedBrightness { percent: 30 },
            DeviceWrite::LedBrightness(30),
        ),
        (
            Setting::SleepAfter { minutes: 30 },
            DeviceWrite::SleepAfter(SleepAfter(30)),
        ),
        (Setting::Tones { value: 2 }, DeviceWrite::Tones(Tones(2))),
    ];
    for (setting, expected) in cases {
        assert_eq!(write_for(&setting, &snapshot).unwrap(), expected);
    }
}

#[test]
fn docked_needs_battery_status() {
    let mut snapshot = DeviceSnapshot::default();
    assert_eq!(snapshot.headset(), Headset::Unknown);

    snapshot.battery = Some(battery(ChargeStatus::Charging));
    assert_eq!(snapshot.headset(), Headset::Unknown);

    snapshot.headset_available = Some(false);
    assert_eq!(snapshot.headset(), Headset::Docked);
    snapshot.battery = Some(battery(ChargeStatus::Full));
    assert_eq!(snapshot.headset(), Headset::Docked);
    snapshot.battery = Some(battery(ChargeStatus::Discharging));
    assert_eq!(snapshot.headset(), Headset::Off);
    snapshot.battery = Some(battery(ChargeStatus::Other(2)));
    assert_eq!(snapshot.headset(), Headset::Off);
    snapshot.battery = None;
    assert_eq!(snapshot.headset(), Headset::Off);

    snapshot.battery = Some(battery(ChargeStatus::Charging));
    snapshot.headset_available = Some(true);
    assert_eq!(snapshot.headset(), Headset::InUse);
}

#[test]
fn events_update_the_snapshot() {
    let mut snapshot = DeviceSnapshot::default();
    let untouched = snapshot.clone();

    snapshot.apply_event(&Event::BluetoothDevices(2));
    snapshot.apply_event(&Event::MixerInputs(0b1010));
    assert_eq!(snapshot, untouched);

    let volume = Volume {
        scaled: 8,
        muted: true,
        level: VolumeLevel(5),
    };
    let auto_sleep = AutoSleep {
        prefix: 0x3c,
        sleep_after: SleepAfter(30),
    };
    for event in [
        Event::Battery(battery(ChargeStatus::Full)),
        Event::HeadsetAvailability(false),
        Event::Sidetone(Sidetone(3)),
        Event::Balance(GameVoiceBalance(4)),
        Event::NoiseGate(NoiseGate::Night),
        Event::MixerSettings(mix()),
        Event::Volume(volume),
        Event::AutoSleep(auto_sleep),
        Event::Tones(Tones(2)),
        Event::CustomEq(CustomEq {
            instance: EqInstance::Headphone,
            slot: 3,
            leading: 0,
            equalizer: HEADPHONE_DEFAULT,
        }),
    ] {
        snapshot.apply_event(&event);
    }
    assert_eq!(snapshot.battery, Some(battery(ChargeStatus::Full)));
    assert_eq!(snapshot.headset_available, Some(false));
    assert_eq!(snapshot.sidetone, Some(Sidetone(3)));
    assert_eq!(snapshot.balance, Some(GameVoiceBalance(4)));
    assert_eq!(snapshot.noise_gate, Some(NoiseGate::Night));
    assert_eq!(snapshot.stream_mix, Some(mix()));
    assert_eq!(snapshot.volume, Some(volume));
    assert_eq!(snapshot.auto_sleep, Some(auto_sleep));
    assert_eq!(snapshot.tones, Some(Tones(2)));
    assert_eq!(snapshot.headphone_eq, Some(HEADPHONE_DEFAULT));
    assert_eq!(snapshot.mic_eq, None);

    snapshot.apply_event(&Event::CustomEq(CustomEq {
        instance: EqInstance::Mic,
        slot: 3,
        leading: 0,
        equalizer: MIC_BROADCAST,
    }));
    assert_eq!(snapshot.mic_eq, Some(MIC_BROADCAST));
    assert_eq!(snapshot.headphone_eq, Some(HEADPHONE_DEFAULT));
}

#[test]
fn record_write_updates_only_the_targeted_field() {
    let before = full_snapshot();

    let mut snapshot = before.clone();
    snapshot.record_write(&DeviceWrite::LedBrightness(40));
    assert_eq!(snapshot.led_brightness, Some(40));
    assert_eq!(
        DeviceSnapshot {
            led_brightness: before.led_brightness,
            ..snapshot
        },
        before
    );

    let mut snapshot = before.clone();
    snapshot.record_write(&DeviceWrite::Volume(VolumeLevel(3)));
    assert_eq!(snapshot.volume.unwrap().level, VolumeLevel(3));
    assert_eq!(snapshot.volume.unwrap().scaled, 16);

    let mut snapshot = before.clone();
    snapshot.record_write(&DeviceWrite::SleepAfter(SleepAfter(0)));
    assert_eq!(snapshot.auto_sleep.unwrap().sleep_after, SleepAfter(0));
    assert_eq!(snapshot.auto_sleep.unwrap().prefix, 0x3c);

    let mut snapshot = before.clone();
    snapshot.record_write(&DeviceWrite::Eq(EqInstance::Mic, MIC_BROADCAST));
    assert_eq!(snapshot.mic_eq, Some(MIC_BROADCAST));
    assert_eq!(snapshot.headphone_eq, before.headphone_eq);

    let mut snapshot = before.clone();
    snapshot.record_write(&DeviceWrite::Tones(Tones(0)));
    snapshot.record_write(&DeviceWrite::Sidetone(Sidetone(5)));
    snapshot.record_write(&DeviceWrite::Balance(GameVoiceBalance(1)));
    snapshot.record_write(&DeviceWrite::NoiseGate(NoiseGate::Off));
    assert_eq!(snapshot.tones, Some(Tones(0)));
    assert_eq!(snapshot.sidetone, Some(Sidetone(5)));
    assert_eq!(snapshot.balance, Some(GameVoiceBalance(1)));
    assert_eq!(snapshot.noise_gate, Some(NoiseGate::Off));
}

#[test]
fn snapshot_converts_to_ipc_settings() {
    let snapshot = full_snapshot();
    let settings = device_settings(&snapshot);
    assert_eq!(settings.volume_percent, Some(52));
    assert_eq!(settings.led_brightness_percent, Some(100));
    assert_eq!(settings.sleep_after_minutes, Some(15));
    assert_eq!(settings.tones, Some(1));
    assert_eq!(settings.profile, profile_settings(&snapshot));
    assert_eq!(settings.profile.sidetone_percent, Some(33));
    assert_eq!(settings.profile.balance, Some(6));
    assert_eq!(settings.profile.noise_gate, Some(Gate::Home));
    assert_eq!(settings.profile.headphone_eq.unwrap().bands.len(), 10);
    let stream_mix = settings.profile.stream_mix.unwrap();
    assert_eq!(stream_mix.output.level_percent, 50);
    assert_eq!(stream_mix.game.level_percent, 63);
    assert!(!stream_mix.voice.muted);

    assert_eq!(
        device_settings(&DeviceSnapshot::default()),
        ogh_ipc::DeviceSettings::default()
    );
}
