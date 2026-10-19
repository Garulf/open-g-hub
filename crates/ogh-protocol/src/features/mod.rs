//! Typed messages for each feature the A50 X base station exposes.
//!
//! Builders take the feature index found during discovery. Parsers take the
//! params of a reply or an event. Only calls G Hub was seen making have a
//! builder here.

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

use crate::error::{ParseError, require_len};
use crate::feature_id::FeatureId;
use crate::frame::Message;

use battery::Battery;
use headset_mix::GameVoiceBalance;
use mic_test::MicTestState;
use noise_gate::NoiseGate;
use sidetone::Sidetone;

const EVENT_SW_ID: u8 = 0;
const EVENT_FUNCTION: u8 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Battery(Battery),
    /// From the mic test state: `false` when the headset is docked or off.
    HeadsetAvailability(bool),
    Sidetone(Sidetone),
    Balance(GameVoiceBalance),
    BluetoothDevices(u8),
    MixerInputs(u32),
    /// Never captured as an event; assumed to carry the value `getNoiseGate`
    /// returns.
    NoiseGate(NoiseGate),
}

/// `None` for replies, for events of features this crate does not model
/// (Memfault, iProfile, EQ, volume) and for params that do not parse.
pub fn parse_event(feature: FeatureId, msg: &Message) -> Option<Event> {
    if msg.sw_id != EVENT_SW_ID || msg.function != EVENT_FUNCTION {
        return None;
    }
    let params = msg.params.as_slice();
    match feature {
        FeatureId::BATTERY_SOC => battery::parse(params).ok().map(Event::Battery),
        FeatureId::MIC_TEST => mic_test::parse_state(params)
            .ok()
            .map(|state: MicTestState| Event::HeadsetAvailability(state.is_headset_available())),
        FeatureId::HEADSET_SIDETONE => sidetone::parse_level(params).ok().map(Event::Sidetone),
        FeatureId::HEADSET_MIX => headset_mix::parse_balance(params).ok().map(Event::Balance),
        FeatureId::BT_HOST_INFO => bt_host_info::parse_connected_count(params)
            .ok()
            .map(Event::BluetoothDevices),
        FeatureId::MIXER_AUDIO => mixer_audio::parse_input_state(params)
            .ok()
            .map(Event::MixerInputs),
        FeatureId::MIXER_MIC => noise_gate::parse_current(params).ok().map(Event::NoiseGate),
        _ => None,
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
