use ogh_ipc::{
    Channel, Charge, DeviceSettings, Eq, EqBand, ErrorKind, Gate, IpcError, ProfileSettings,
    Setting, StreamMixSetting,
};
use ogh_protocol::RangeError;
use ogh_protocol::features::advanced_para_eq::{
    BAND_COUNT, EqBand as DeviceBand, EqInstance, Equalizer, set_custom_request,
};
use ogh_protocol::features::auto_sleep::{self, SleepAfter};
use ogh_protocol::features::battery::ChargeStatus;
use ogh_protocol::features::headset_mix::{self, GameVoiceBalance};
use ogh_protocol::features::mixer_audio::{MixChannel, MixInput, MixLevel, StreamMix};
use ogh_protocol::features::noise_gate::NoiseGate;
use ogh_protocol::features::sidetone::Sidetone;
use ogh_protocol::features::tones::{self, Tones};
use ogh_protocol::features::volume::VolumeLevel;

use crate::snapshot::{DeviceSnapshot, DeviceWrite};

/// The feature index only matters to the message, which is discarded after
/// its range checks.
const CHECK_ONLY_INDEX: u8 = 0;

fn invalid(message: impl Into<String>) -> IpcError {
    IpcError {
        kind: ErrorKind::InvalidValue,
        message: message.into(),
    }
}

fn percent(field: &str, value: u8) -> Result<u8, IpcError> {
    if value > 100 {
        return Err(invalid(format!("{field} must be 0 to 100, got {value}")));
    }
    Ok(value)
}

fn in_range<T>(result: Result<T, RangeError>) -> Result<(), IpcError> {
    result.map(drop).map_err(|error| invalid(error.to_string()))
}

fn sidetone_for(value: u8) -> Result<Sidetone, IpcError> {
    Ok(Sidetone::from_percent(percent("sidetone percent", value)?))
}

fn balance_for(value: u8) -> Result<GameVoiceBalance, IpcError> {
    let balance = GameVoiceBalance(value);
    in_range(headset_mix::set_request(CHECK_ONLY_INDEX, balance))?;
    Ok(balance)
}

fn gate_for(gate: Gate) -> NoiseGate {
    match gate {
        Gate::Off => NoiseGate::Off,
        Gate::Home => NoiseGate::Home,
        Gate::Night => NoiseGate::Night,
        Gate::Tournament => NoiseGate::Tournament,
    }
}

fn gate_to_ipc(gate: NoiseGate) -> Option<Gate> {
    match gate {
        NoiseGate::Off => Some(Gate::Off),
        NoiseGate::Home => Some(Gate::Home),
        NoiseGate::Night => Some(Gate::Night),
        NoiseGate::Tournament => Some(Gate::Tournament),
        NoiseGate::Other(_) => None,
    }
}

fn volume_for(value: u8) -> Result<VolumeLevel, IpcError> {
    Ok(VolumeLevel::from_percent(percent("volume percent", value)?))
}

fn sleep_after_for(minutes: u8) -> Result<SleepAfter, IpcError> {
    let sleep_after = SleepAfter(minutes);
    in_range(auto_sleep::set_request(CHECK_ONLY_INDEX, sleep_after))?;
    Ok(sleep_after)
}

fn tones_for(value: u8) -> Result<Tones, IpcError> {
    let tones = Tones(value);
    in_range(tones::set_request(CHECK_ONLY_INDEX, tones))?;
    Ok(tones)
}

fn equalizer_for(field: &str, instance: EqInstance, eq: &Eq) -> Result<Equalizer, IpcError> {
    eq.validate(instance.min_freq_hz())
        .map_err(|message| invalid(format!("{field}: {message}")))?;
    let bands: Vec<DeviceBand> = eq
        .bands
        .iter()
        .map(|band| DeviceBand {
            freq_hz: band.freq_hz,
            q: band.q,
            gain_db: band.gain_db,
        })
        .collect();
    let bands: [DeviceBand; BAND_COUNT] = bands
        .try_into()
        .map_err(|_| invalid(format!("{field}: expected {BAND_COUNT} bands")))?;
    let equalizer = Equalizer { bands };
    set_custom_request(CHECK_ONLY_INDEX, instance, &equalizer)
        .map_err(|error| invalid(format!("{field}: {error}")))?;
    Ok(equalizer)
}

fn equalizer_to_ipc(equalizer: &Equalizer) -> Eq {
    Eq {
        bands: equalizer
            .bands
            .iter()
            .map(|band| EqBand {
                freq_hz: band.freq_hz,
                q: band.q,
                gain_db: band.gain_db,
            })
            .collect(),
    }
}

/// The EQ as the device would receive it. Gains and Q land on the device's
/// grid, so equal bytes mean equal device state where `f32`s may differ.
fn eq_bytes(instance: EqInstance, equalizer: &Equalizer) -> Option<Vec<u8>> {
    set_custom_request(CHECK_ONLY_INDEX, instance, equalizer)
        .ok()
        .map(|message| message.params)
}

fn eq_changed(instance: EqInstance, current: Option<&Equalizer>, target: &Equalizer) -> bool {
    current.and_then(|current| eq_bytes(instance, current)) != eq_bytes(instance, target)
}

fn channel_for(field: &str, channel: Channel) -> Result<MixChannel, IpcError> {
    Ok(MixChannel {
        level: MixLevel::from_percent(percent(field, channel.level_percent)?),
        muted: channel.muted,
    })
}

fn channel_to_ipc(channel: MixChannel) -> Channel {
    Channel {
        level_percent: channel.level.to_percent(),
        muted: channel.muted,
    }
}

/// Output, game and voice go onto the current mix; the pinned inputs keep
/// whatever the device holds.
fn stream_mix_for(
    setting: &StreamMixSetting,
    current: &DeviceSnapshot,
) -> Result<StreamMix, IpcError> {
    let output = channel_for("stream mix output percent", setting.output)?;
    let game = channel_for("stream mix game percent", setting.game)?;
    let voice = channel_for("stream mix voice percent", setting.voice)?;
    let mut mix = current.stream_mix.ok_or_else(|| IpcError {
        kind: ErrorKind::NotConnected,
        message: "the stream mix has not been read from the headset".to_string(),
    })?;
    mix.stream = output;
    *mix.input_mut(MixInput::Game) = game;
    *mix.input_mut(MixInput::Voice) = voice;
    Ok(mix)
}

pub fn write_for(setting: &Setting, current: &DeviceSnapshot) -> Result<DeviceWrite, IpcError> {
    Ok(match setting {
        Setting::Sidetone { percent } => DeviceWrite::Sidetone(sidetone_for(*percent)?),
        Setting::Balance { value } => DeviceWrite::Balance(balance_for(*value)?),
        Setting::NoiseGate(gate) => DeviceWrite::NoiseGate(gate_for(*gate)),
        Setting::HeadphoneEq(eq) => DeviceWrite::Eq(
            EqInstance::Headphone,
            equalizer_for("headphone_eq", EqInstance::Headphone, eq)?,
        ),
        Setting::MicEq(eq) => DeviceWrite::Eq(
            EqInstance::Mic,
            equalizer_for("mic_eq", EqInstance::Mic, eq)?,
        ),
        Setting::StreamMix(mix) => DeviceWrite::StreamMix(stream_mix_for(mix, current)?),
        Setting::Volume { percent } => DeviceWrite::Volume(volume_for(*percent)?),
        Setting::LedBrightness { percent: value } => {
            DeviceWrite::LedBrightness(percent("led brightness percent", *value)?)
        }
        Setting::SleepAfter { minutes } => DeviceWrite::SleepAfter(sleep_after_for(*minutes)?),
        Setting::Tones { value } => DeviceWrite::Tones(tones_for(*value)?),
    })
}

/// Writes for the target fields that differ from `current`, in the order of
/// the fields of `ProfileSettings`. A value `current` does not know yet
/// counts as different. A target stream mix needs a known current mix to
/// merge onto, so without one this fails with `NotConnected`.
pub fn diff(
    current: &DeviceSnapshot,
    target: &ProfileSettings,
) -> Result<Vec<DeviceWrite>, IpcError> {
    let mut writes = Vec::new();
    if let Some(value) = target.sidetone_percent {
        let sidetone = sidetone_for(value)?;
        if current.sidetone != Some(sidetone) {
            writes.push(DeviceWrite::Sidetone(sidetone));
        }
    }
    if let Some(value) = target.balance {
        let balance = balance_for(value)?;
        if current.balance != Some(balance) {
            writes.push(DeviceWrite::Balance(balance));
        }
    }
    if let Some(gate) = target.noise_gate {
        let gate = gate_for(gate);
        if current.noise_gate != Some(gate) {
            writes.push(DeviceWrite::NoiseGate(gate));
        }
    }
    if let Some(eq) = &target.headphone_eq {
        let instance = EqInstance::Headphone;
        let equalizer = equalizer_for("headphone_eq", instance, eq)?;
        if eq_changed(instance, current.headphone_eq.as_ref(), &equalizer) {
            writes.push(DeviceWrite::Eq(instance, equalizer));
        }
    }
    if let Some(eq) = &target.mic_eq {
        let instance = EqInstance::Mic;
        let equalizer = equalizer_for("mic_eq", instance, eq)?;
        if eq_changed(instance, current.mic_eq.as_ref(), &equalizer) {
            writes.push(DeviceWrite::Eq(instance, equalizer));
        }
    }
    if let Some(setting) = &target.stream_mix {
        let mix = stream_mix_for(setting, current)?;
        if current.stream_mix != Some(mix) {
            writes.push(DeviceWrite::StreamMix(mix));
        }
    }
    Ok(writes)
}

pub fn battery(snapshot: &DeviceSnapshot) -> Option<ogh_ipc::Battery> {
    snapshot.battery.map(|battery| ogh_ipc::Battery {
        percent: battery.percent(),
        charge: match battery.status {
            ChargeStatus::Discharging => Charge::Discharging,
            ChargeStatus::Charging => Charge::Charging,
            ChargeStatus::Full => Charge::Full,
            ChargeStatus::Other(byte) => Charge::Other(byte),
        },
    })
}

/// Every profile field the snapshot knows. A noise gate mode outside the four
/// G Hub offers has no IPC name and is left out.
pub fn profile_settings(snapshot: &DeviceSnapshot) -> ProfileSettings {
    ProfileSettings {
        sidetone_percent: snapshot.sidetone.map(Sidetone::to_percent),
        balance: snapshot.balance.map(|balance| balance.0),
        noise_gate: snapshot.noise_gate.and_then(gate_to_ipc),
        headphone_eq: snapshot.headphone_eq.as_ref().map(equalizer_to_ipc),
        mic_eq: snapshot.mic_eq.as_ref().map(equalizer_to_ipc),
        stream_mix: snapshot.stream_mix.map(|mix| StreamMixSetting {
            output: channel_to_ipc(mix.stream),
            game: channel_to_ipc(mix.input(MixInput::Game)),
            voice: channel_to_ipc(mix.input(MixInput::Voice)),
        }),
    }
}

pub fn device_settings(snapshot: &DeviceSnapshot) -> DeviceSettings {
    DeviceSettings {
        profile: profile_settings(snapshot),
        volume_percent: snapshot.volume.map(|volume| volume.level.to_percent()),
        led_brightness_percent: snapshot.led_brightness,
        sleep_after_minutes: snapshot.auto_sleep.map(|sleep| sleep.sleep_after.0),
        tones: snapshot.tones.map(|tones| tones.0),
    }
}
