//! Typed messages for each feature the A50 X base station exposes.
//!
//! Builders take the feature index found during discovery. Parsers take the
//! params of a reply or an event. Only calls G Hub was seen making have a
//! builder here.

pub mod advanced_para_eq;
pub mod auto_sleep;
pub mod battery;
pub mod bt_host_info;
pub mod device_info;
pub mod device_name;
pub mod headset_mix;
pub mod led_brightness;
pub mod mic_test;
pub mod mixer_audio;
pub mod noise_gate;
pub mod sidetone;
pub mod tones;
pub mod volume;

use crate::error::{ParseError, require_len};
use crate::feature_id::FeatureId;
use crate::frame::Message;

use advanced_para_eq::CustomEq;
use auto_sleep::AutoSleep;
use battery::Battery;
use headset_mix::GameVoiceBalance;
use mic_test::MicTestState;
use mixer_audio::StreamMix;
use noise_gate::NoiseGate;
use sidetone::Sidetone;
use tones::Tones;
use volume::Volume;

const EVENT_SW_ID: u8 = 0;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Battery(Battery),
    /// From the mic test state: `false` when the headset is docked or off.
    HeadsetAvailability(bool),
    Sidetone(Sidetone),
    Balance(GameVoiceBalance),
    BluetoothDevices(u8),
    MixerInputs(u32),
    NoiseGate(NoiseGate),
    MixerSettings(StreamMix),
    Volume(Volume),
    AutoSleep(AutoSleep),
    Tones(Tones),
    CustomEq(CustomEq),
}

/// `None` for replies, for events no capture showed (iProfile and Memfault
/// are captured but not modelled) and for params that do not parse. Events
/// carry their own function numbers: the mixer sends input state as fn0 and
/// its settings as fn2.
pub fn parse_event(feature: FeatureId, msg: &Message) -> Option<Event> {
    if msg.sw_id != EVENT_SW_ID {
        return None;
    }
    let params = msg.params.as_slice();
    match (feature, msg.function) {
        (FeatureId::BATTERY_SOC, 0) => battery::parse(params).ok().map(Event::Battery),
        (FeatureId::MIC_TEST, 0) => mic_test::parse_state(params)
            .ok()
            .map(|state: MicTestState| Event::HeadsetAvailability(state.is_headset_available())),
        (FeatureId::HEADSET_SIDETONE, 0) => sidetone::parse_level(params).ok().map(Event::Sidetone),
        (FeatureId::HEADSET_MIX, 0) => headset_mix::parse_balance(params).ok().map(Event::Balance),
        (FeatureId::BT_HOST_INFO, 0) => bt_host_info::parse_connected_count(params)
            .ok()
            .map(Event::BluetoothDevices),
        (FeatureId::MIXER_AUDIO, 0) => mixer_audio::parse_input_state(params)
            .ok()
            .map(Event::MixerInputs),
        (FeatureId::MIXER_AUDIO, 2) => mixer_audio::parse_settings(params)
            .ok()
            .map(Event::MixerSettings),
        (FeatureId::MIXER_MIC, 0) => noise_gate::parse_current(params).ok().map(Event::NoiseGate),
        (FeatureId::VOLUME_SET, 0) => volume::parse(params).ok().map(Event::Volume),
        (FeatureId::AUTO_SLEEP, 0) => auto_sleep::parse(params).ok().map(Event::AutoSleep),
        (FeatureId::HEADSET_TONES, 0) => tones::parse(params).ok().map(Event::Tones),
        (FeatureId::ADVANCED_PARA_EQ, 0) => advanced_para_eq::parse_custom_eq(params)
            .ok()
            .map(Event::CustomEq),
        _ => None,
    }
}

/// `roundf(max * percent / 100)`, the agent's percent mapping for a device
/// scale of `max + 1` steps. Values above 100 count as 100.
pub(crate) fn percent_to_level(percent: u8, max: u8) -> u8 {
    let percent = u16::from(percent.min(100));
    ((percent * u16::from(max) + 50) / 100) as u8
}

/// Inverse of `percent_to_level`, rounded to the nearest percent.
pub(crate) fn level_to_percent(level: u8, max: u8) -> u8 {
    let max = u16::from(max);
    let level = u16::from(level).min(max);
    ((level * 100 + max / 2) / max) as u8
}

pub(crate) fn bool_byte(byte: u8) -> Result<bool, ParseError> {
    match byte {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(ParseError::UnknownValue(other)),
    }
}

pub(crate) fn first_byte(params: &[u8]) -> Result<u8, ParseError> {
    require_len(params, 1)?;
    Ok(params[0])
}

/// Reads a length byte at `at` followed by that many bytes of UTF-8.
pub(crate) fn length_prefixed_str(params: &[u8], at: usize) -> Result<String, ParseError> {
    require_len(params, at + 1)?;
    let len = usize::from(params[at]);
    require_len(params, at + 1 + len)?;
    let bytes = &params[at + 1..at + 1 + len];
    String::from_utf8(bytes.to_vec()).map_err(|_| ParseError::InvalidUtf8)
}
