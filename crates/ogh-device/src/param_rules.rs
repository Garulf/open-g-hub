//! Parameter checks for allowlisted calls whose safety depends on their
//! params (rulings R22 and R31). Each write accepts only the shape G Hub was
//! seen sending, so a raw `Session::request` cannot reach values the
//! builders refuse. A call with no rule here is checked on (feature,
//! function) alone.

use crate::FeatureId;

/// The only custom EQ slot on the A50 X: getEQInfos reports `0x31`, three
/// read-only factory presets (0..2) and one custom slot.
const CUSTOM_EQ_SLOT: u8 = 3;
const EQ_INSTANCE_MIC: u8 = 0;
const EQ_INSTANCE_HEADPHONE: u8 = 1;
const EQ_BANDS: usize = 10;
const EQ_BAND_LEN: usize = 5;
/// 0 dB is raw 120 on a 0.05 dB grid, so +6 dB is 240.
const EQ_MAX_GAIN_RAW: u16 = 240;
const EQ_MAX_FREQ_HZ: u16 = 20000;
const EQ_HEADPHONE_MIN_FREQ_HZ: u16 = 20;
const EQ_MIC_MIN_FREQ_HZ: u16 = 60;

const MIXER_STREAM_OUTPUT: u8 = 0;
const MIXER_INPUTS: u8 = 4;
const MIXER_MAX_LEVEL: u8 = 32;

const VOLUME_PREFIX: u8 = 0xff;
const VOLUME_MAX_LEVEL: u8 = 21;
const BRIGHTNESS_MAX: u8 = 100;
const SLEEP_PREFIX: u8 = 0x3c;
const SLEEP_MINUTES: &[u8] = &[0, 15, 30];
const TONES: &[u8] = &[0, 1, 2];
const NOISE_GATES: &[u8] = &[0, 1, 2, 4];

struct ParamRule {
    feature: FeatureId,
    function: u8,
    accepts: fn(&[u8]) -> bool,
}

const RULES: &[ParamRule] = &[
    ParamRule {
        feature: FeatureId::ADVANCED_PARA_EQ,
        function: 2,
        accepts: set_custom_eq,
    },
    ParamRule {
        feature: FeatureId::MIXER_AUDIO,
        function: 6,
        accepts: set_mixer_settings,
    },
    ParamRule {
        feature: FeatureId::VOLUME_SET,
        function: 1,
        accepts: set_volume_level,
    },
    ParamRule {
        feature: FeatureId::LED_BRIGHTNESS,
        function: 1,
        accepts: set_brightness,
    },
    ParamRule {
        feature: FeatureId::AUTO_SLEEP,
        function: 1,
        accepts: set_sleep_timer,
    },
    ParamRule {
        feature: FeatureId::HEADSET_TONES,
        function: 1,
        accepts: set_tones,
    },
    ParamRule {
        feature: FeatureId::MIXER_MIC,
        function: 2,
        accepts: set_noise_gate,
    },
];

/// `<instance> 03 00` then ten `<freq u16> <q> <gain u16>`; factory slots
/// must never be written. Any Q byte is accepted: G Hub wrote 0 (graphic
/// mode) and the editor's range covers 1 to 255.
fn set_custom_eq(params: &[u8]) -> bool {
    let [instance, slot, leading, bands @ ..] = params else {
        return false;
    };
    let min_freq = match *instance {
        EQ_INSTANCE_MIC => EQ_MIC_MIN_FREQ_HZ,
        EQ_INSTANCE_HEADPHONE => EQ_HEADPHONE_MIN_FREQ_HZ,
        _ => return false,
    };
    let (bands, rest) = bands.as_chunks::<EQ_BAND_LEN>();
    *slot == CUSTOM_EQ_SLOT
        && *leading == 0
        && bands.len() == EQ_BANDS
        && rest.is_empty()
        && bands
            .iter()
            .all(|&[freq_hi, freq_lo, _q, gain_hi, gain_lo]| {
                let freq = u16::from_be_bytes([freq_hi, freq_lo]);
                (min_freq..=EQ_MAX_FREQ_HZ).contains(&freq)
                    && u16::from_be_bytes([gain_hi, gain_lo]) <= EQ_MAX_GAIN_RAW
            })
}

/// `00 <level> <mute> 04` then inputs 0 to 3 in order, each `<index>
/// <level> <mute>`. Leaving an input out (routing) was never captured.
fn set_mixer_settings(params: &[u8]) -> bool {
    let [output, level, mute, count, inputs @ ..] = params else {
        return false;
    };
    let (records, rest) = inputs.as_chunks::<3>();
    *output == MIXER_STREAM_OUTPUT
        && mixer_channel(*level, *mute)
        && *count == MIXER_INPUTS
        && records.len() == usize::from(MIXER_INPUTS)
        && rest.is_empty()
        && records
            .iter()
            .zip(0..)
            .all(|(&[index, level, mute], expected)| {
                index == expected && mixer_channel(level, mute)
            })
}

fn mixer_channel(level: u8, mute: u8) -> bool {
    level <= MIXER_MAX_LEVEL && mute <= 1
}

fn set_volume_level(params: &[u8]) -> bool {
    matches!(params, [VOLUME_PREFIX, level] if *level <= VOLUME_MAX_LEVEL)
}

fn set_brightness(params: &[u8]) -> bool {
    matches!(params, [percent] if *percent <= BRIGHTNESS_MAX)
}

fn set_sleep_timer(params: &[u8]) -> bool {
    matches!(params, [SLEEP_PREFIX, minutes] if SLEEP_MINUTES.contains(minutes))
}

fn set_tones(params: &[u8]) -> bool {
    matches!(params, [state] if TONES.contains(state))
}

fn set_noise_gate(params: &[u8]) -> bool {
    matches!(params, [gate] if NOISE_GATES.contains(gate))
}

pub(crate) fn permits(feature: FeatureId, function: u8, params: &[u8]) -> bool {
    RULES
        .iter()
        .filter(|rule| rule.feature == feature && rule.function == function)
        .all(|rule| (rule.accepts)(params))
}
