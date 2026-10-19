mod common;

use common::{index_in, open_scripted, send_built};
use ogh_device::FeatureId;
use ogh_protocol::features::advanced_para_eq::{
    self, EqBand, EqInstance, Equalizer, MAX_GAIN_DB, MAX_Q, MIN_GAIN_DB, MIN_Q,
};
use ogh_protocol::features::auto_sleep::{self, SleepAfter};
use ogh_protocol::features::headset_mix::{self, GameVoiceBalance};
use ogh_protocol::features::led_brightness;
use ogh_protocol::features::mixer_audio::{self, MixChannel, MixInput, MixLevel, StreamMix};
use ogh_protocol::features::noise_gate::{self, NoiseGate};
use ogh_protocol::features::sidetone::{self, Sidetone};
use ogh_protocol::features::tones::{self, Tones};
use ogh_protocol::features::volume::{self, VolumeLevel};
use ogh_protocol::{Message, RangeError};

/// Sends what `build` makes for each value through a scripted session; the
/// session's checks (allowlist and param rules) must let it out unchanged.
fn send_extremes<V: Copy + std::fmt::Debug>(
    id: FeatureId,
    values: &[V],
    build: impl Fn(u8, V) -> Result<Message, RangeError>,
) {
    let (mut session, device) = open_scripted();
    let index = index_in(&session, id);
    for &value in values {
        let msg = build(index, value).unwrap_or_else(|e| panic!("{id:?} {value:?}: {e}"));
        send_built(&mut session, &device, id, msg, &format!("{id:?} {value:?}"));
    }
}

fn card(freqs: [u16; 10], q: Option<f32>, gain_db: f32) -> Equalizer {
    Equalizer {
        bands: freqs.map(|freq_hz| EqBand {
            freq_hz,
            q,
            gain_db,
        }),
    }
}

const HEADPHONE_FREQS: [u16; 10] = [20, 50, 125, 250, 500, 1000, 2500, 5000, 10000, 20000];
const MIC_FREQS: [u16; 10] = [60, 150, 250, 400, 800, 1500, 2500, 5000, 10000, 20000];

#[test]
fn eq_q_and_gain_extremes_leave_the_session() {
    for instance in [EqInstance::Headphone, EqInstance::Mic] {
        let freqs = match instance {
            EqInstance::Headphone => HEADPHONE_FREQS,
            EqInstance::Mic => MIC_FREQS,
        };
        let cards: Vec<_> = [MIN_Q, MAX_Q]
            .into_iter()
            .flat_map(|q| [MIN_GAIN_DB, MAX_GAIN_DB].map(|gain| card(freqs, Some(q), gain)))
            .collect();
        send_extremes(FeatureId::ADVANCED_PARA_EQ, &cards, |index, card| {
            advanced_para_eq::set_custom_request(index, instance, &card)
        });
    }
}

#[test]
fn eq_frequency_extremes_leave_the_session() {
    let mut mic_first_band = MIC_FREQS;
    mic_first_band[0] = 60;
    let cases = [
        (EqInstance::Mic, mic_first_band),
        (EqInstance::Headphone, HEADPHONE_FREQS),
    ];
    for (instance, freqs) in cases {
        let cards = [card(freqs, None, 0.0), card(freqs, Some(1.0), 0.0)];
        send_extremes(FeatureId::ADVANCED_PARA_EQ, &cards, |index, card| {
            advanced_para_eq::set_custom_request(index, instance, &card)
        });
    }
}

#[test]
fn eq_graphic_and_parametric_cards_leave_the_session() {
    for instance in [EqInstance::Headphone, EqInstance::Mic] {
        let freqs = match instance {
            EqInstance::Headphone => advanced_para_eq::HEADPHONE_DEFAULT,
            EqInstance::Mic => advanced_para_eq::MIC_DEFAULT,
        };
        let graphic = Equalizer {
            bands: freqs.bands.map(|band| EqBand { q: None, ..band }),
        };
        send_extremes(
            FeatureId::ADVANCED_PARA_EQ,
            &[freqs, graphic],
            |index, card| advanced_para_eq::set_custom_request(index, instance, &card),
        );
    }
}

#[test]
fn mixer_extremes_leave_the_session() {
    let mut mixes = Vec::new();
    for muted in [false, true] {
        for level in [0, mixer_audio::MAX_LEVEL] {
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
            mixes.push(mix);
        }
    }
    send_extremes(FeatureId::MIXER_AUDIO, &mixes, |index, mix| {
        mixer_audio::set_request(index, &mix)
    });
}

#[test]
fn volume_extremes_leave_the_session() {
    send_extremes(FeatureId::VOLUME_SET, &[0, 21], |index, level| {
        volume::set_request(index, VolumeLevel(level))
    });
}

#[test]
fn brightness_extremes_leave_the_session() {
    send_extremes(
        FeatureId::LED_BRIGHTNESS,
        &[0, 100],
        led_brightness::set_request,
    );
}

#[test]
fn sleep_after_values_leave_the_session() {
    send_extremes(FeatureId::AUTO_SLEEP, &[0, 15, 30], |index, minutes| {
        auto_sleep::set_request(index, SleepAfter(minutes))
    });
}

#[test]
fn tones_values_leave_the_session() {
    send_extremes(FeatureId::HEADSET_TONES, &[0, 1, 2], |index, tones| {
        tones::set_request(index, Tones(tones))
    });
}

#[test]
fn every_settable_noise_gate_leaves_the_session() {
    send_extremes(FeatureId::MIXER_MIC, &[0, 1, 2, 4], |index, gate| {
        noise_gate::set_request(index, NoiseGate::from_byte(gate))
    });
}

#[test]
fn sidetone_percent_extremes_leave_the_session() {
    send_extremes(FeatureId::HEADSET_SIDETONE, &[0, 100], |index, percent| {
        sidetone::set_request(index, Sidetone::from_percent(percent))
    });
}

#[test]
fn balance_extremes_leave_the_session() {
    send_extremes(FeatureId::HEADSET_MIX, &[0, 12], |index, balance| {
        headset_mix::set_request(index, GameVoiceBalance(balance))
    });
}
