use crate::error::{Allowed, ParseError, RangeError, require_len};
use crate::frame::{Message, request};

const GET_CUSTOM: u8 = 1;
const SET_CUSTOM: u8 = 2;
/// Slots 0 to 2 are read-only factory presets (getEQInfos); G Hub writes
/// every preset pick and custom edit into slot 3.
pub const CUSTOM_SLOT: u8 = 3;
/// Meaning unknown (guessed: a pre-EQ gain); 0 in every captured write.
const LEADING_BYTE: u8 = 0;
pub const BAND_COUNT: usize = 10;
const BAND_LEN: usize = 5;
const HEADER_LEN: usize = 3;
const BANDS_LEN: usize = BAND_COUNT * BAND_LEN;
const CARD_LEN: usize = HEADER_LEN + BANDS_LEN;
/// The fn1 reply is a leading byte, then the ten bands.
const REPLY_LEN: usize = 1 + BANDS_LEN;

pub const MIN_GAIN_DB: f32 = -6.0;
pub const MAX_GAIN_DB: f32 = 6.0;
/// The device's gain grid: raw = (dB + 6) * 20, 0 to 240.
const GAIN_STEPS_PER_DB: f32 = 20.0;
/// Raw 120 is 0 dB. Decoding subtracts it before dividing, so a decoded gain
/// is the f32 nearest its exact value (raw 177 gives the same f32 as 2.85).
const ZERO_DB_RAW: f32 = -MIN_GAIN_DB * GAIN_STEPS_PER_DB;
/// The G Hub editor shows Q 0.031 to 7.969, which round to raw 1 and 255.
pub const MIN_Q: f32 = 0.031;
pub const MAX_Q: f32 = 7.969;
const Q_STEPS: f32 = 32.0;
/// Graphic mode writes Q raw 0 on every band.
const GRAPHIC_Q: u8 = 0;
pub const MAX_FREQ_HZ: u16 = 20000;
/// The G Hub editor's lowest headphone frequency.
pub const HEADPHONE_MIN_FREQ_HZ: u16 = 20;
/// The G Hub editor's lowest mic frequency; G Hub wrote 60 Hz.
pub const MIC_MIN_FREQ_HZ: u16 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EqInstance {
    Mic = 0,
    Headphone = 1,
}

impl EqInstance {
    pub fn from_byte(byte: u8) -> Result<Self, ParseError> {
        match byte {
            0 => Ok(Self::Mic),
            1 => Ok(Self::Headphone),
            other => Err(ParseError::UnknownValue(other)),
        }
    }

    pub fn min_freq_hz(self) -> u16 {
        match self {
            Self::Mic => MIC_MIN_FREQ_HZ,
            Self::Headphone => HEADPHONE_MIN_FREQ_HZ,
        }
    }
}

/// One band. `q` is `None` in G Hub's graphic mode, which writes Q 0;
/// curated presets and advanced mode carry a Q.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqBand {
    pub freq_hz: u16,
    pub q: Option<f32>,
    pub gain_db: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Equalizer {
    pub bands: [EqBand; BAND_COUNT],
}

/// The fn2 params, which the fn0 "custom EQ changed" event repeats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CustomEq {
    pub instance: EqInstance,
    pub slot: u8,
    /// 0 in every capture; meaning unknown.
    pub leading: u8,
    pub equalizer: Equalizer,
}

const PLACEHOLDER_BAND: EqBand = EqBand {
    freq_hz: 0,
    q: None,
    gain_db: 0.0,
};

/// The Q G Hub writes for curated presets and new advanced cards (raw 22).
const CURATED_Q: f32 = 0.6875;
const HEADPHONE_FREQS: [u16; BAND_COUNT] = [20, 50, 125, 250, 500, 1000, 2500, 5000, 10000, 20000];
const MIC_FREQS: [u16; BAND_COUNT] = [80, 150, 250, 400, 800, 1500, 2500, 5000, 10000, 19000];

const fn curated(freqs: [u16; BAND_COUNT], gains_db: [f32; BAND_COUNT]) -> Equalizer {
    let mut bands = [PLACEHOLDER_BAND; BAND_COUNT];
    let mut i = 0;
    while i < BAND_COUNT {
        bands[i] = EqBand {
            freq_hz: freqs[i],
            q: Some(CURATED_Q),
            gain_db: gains_db[i],
        };
        i += 1;
    }
    Equalizer { bands }
}

/// G Hub's curated presets as it writes them into the custom slot.
pub const HEADPHONE_DEFAULT: Equalizer = curated(HEADPHONE_FREQS, [0.0; BAND_COUNT]);
pub const HEADPHONE_GAMING: Equalizer = curated(
    HEADPHONE_FREQS,
    [-5.0, -4.5, -3.5, -2.0, 0.0, 2.0, 3.0, 3.0, 3.0, 3.0],
);
pub const HEADPHONE_MEDIA: Equalizer = curated(
    HEADPHONE_FREQS,
    [4.0, 3.0, 2.0, -0.5, -0.5, 1.0, 2.0, 1.5, 0.5, -0.5],
);
/// GUESS: mic Default was never picked in a capture. The headphone Default
/// pick and the other mic picks write their preset with Q 0x16, so this is
/// taken to be the flat Q 0x16 card G Hub wrote at eq-mic line 29.
pub const MIC_DEFAULT: Equalizer = curated(MIC_FREQS, [0.0; BAND_COUNT]);
pub const MIC_BROADCAST: Equalizer = curated(
    MIC_FREQS,
    [4.0, 4.0, 2.5, 1.5, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0],
);
pub const MIC_COMPETITION: Equalizer = curated(
    MIC_FREQS,
    [-3.0, -3.0, -1.5, 0.0, 0.0, 1.0, 1.5, 1.5, 1.5, 2.0],
);

/// fn1 getCustomEQ: `<instance> 03`, as G Hub reads the custom slot.
pub fn get_custom_request(feature_index: u8, instance: EqInstance) -> Message {
    request(feature_index, GET_CUSTOM, &[instance as u8, CUSTOM_SLOT])
}

/// fn2 setCustomEQ into the custom slot, the only call G Hub uses to change
/// an EQ. The card takes the first band's mode: every band must then be
/// graphic (`q: None`) or every band parametric, as in every captured card.
/// Frequencies must rise strictly from band to band. Gains round to the
/// nearest step of the device's 0.05 dB grid and Q to the nearest 1/32; how
/// G Hub rounds a gain between grid points is unknown, so such a value may
/// land one step away from what G Hub writes.
pub fn set_custom_request(
    feature_index: u8,
    instance: EqInstance,
    equalizer: &Equalizer,
) -> Result<Message, RangeError> {
    let graphic = equalizer.bands[0].q.is_none();
    let mut params = Vec::with_capacity(CARD_LEN);
    params.extend([instance as u8, CUSTOM_SLOT, LEADING_BYTE]);
    let mut min_freq_hz = instance.min_freq_hz();
    for band in &equalizer.bands {
        RangeError::check_between(
            "EQ frequency (Hz)",
            f32::from(band.freq_hz),
            f32::from(min_freq_hz),
            f32::from(MAX_FREQ_HZ),
        )?;
        min_freq_hz = band.freq_hz.saturating_add(1);
        params.extend(band.freq_hz.to_be_bytes());
        params.push(q_raw(graphic, band.q)?);
        params.extend(gain_raw(band.gain_db)?.to_be_bytes());
    }
    Ok(request(feature_index, SET_CUSTOM, &params))
}

fn q_raw(graphic: bool, q: Option<f32>) -> Result<u8, RangeError> {
    match (graphic, q) {
        (true, None) => Ok(GRAPHIC_Q),
        (true, Some(q)) => Err(RangeError {
            what: "EQ Q on a graphic card",
            value: q,
            allowed: Allowed::Between { min: 0.0, max: 0.0 },
        }),
        (false, q) => {
            let q = q.unwrap_or(0.0);
            RangeError::check_between("EQ Q", q, MIN_Q, MAX_Q)?;
            Ok((q * Q_STEPS).round() as u8)
        }
    }
}

fn gain_raw(gain_db: f32) -> Result<u16, RangeError> {
    RangeError::check_between("EQ gain (dB)", gain_db, MIN_GAIN_DB, MAX_GAIN_DB)?;
    Ok((gain_db * GAIN_STEPS_PER_DB + ZERO_DB_RAW).round() as u16)
}

/// Parses a fn0 "custom EQ changed" event or fn2 params:
/// `<instance> <slot> <leading>` then ten `<freq u16> <q> <gain u16>`.
pub fn parse_custom_eq(params: &[u8]) -> Result<CustomEq, ParseError> {
    require_len(params, CARD_LEN)?;
    let instance = EqInstance::from_byte(params[0])?;
    Ok(CustomEq {
        instance,
        slot: params[1],
        leading: params[2],
        equalizer: parse_bands(&params[HEADER_LEN..CARD_LEN]),
    })
}

/// Parses a fn1 reply: a leading byte, then ten `<freq u16> <q> <gain u16>`.
pub fn parse_custom_reply(params: &[u8]) -> Result<Equalizer, ParseError> {
    require_len(params, REPLY_LEN)?;
    Ok(parse_bands(&params[1..REPLY_LEN]))
}

fn parse_bands(bands: &[u8]) -> Equalizer {
    let mut parsed = [PLACEHOLDER_BAND; BAND_COUNT];
    for (band, raw) in parsed.iter_mut().zip(bands.as_chunks::<BAND_LEN>().0) {
        let gain_raw = u16::from_be_bytes([raw[3], raw[4]]);
        *band = EqBand {
            freq_hz: u16::from_be_bytes([raw[0], raw[1]]),
            q: (raw[2] != GRAPHIC_Q).then(|| f32::from(raw[2]) / Q_STEPS),
            gain_db: (f32::from(gain_raw) - ZERO_DB_RAW) / GAIN_STEPS_PER_DB,
        };
    }
    Equalizer { bands: parsed }
}
