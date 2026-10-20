use ogh_ipc::{Eq, EqBand};
use ogh_protocol::features::advanced_para_eq::{
    Equalizer, HEADPHONE_DEFAULT, HEADPHONE_GAMING, HEADPHONE_MEDIA, MIC_BROADCAST,
    MIC_COMPETITION, MIC_DEFAULT,
};

/// The Q G Hub writes for curated presets and new parametric cards.
pub(crate) const CURATED_Q: f32 = 0.6875;

pub fn headphone() -> [(&'static str, Eq); 3] {
    [
        ("Default", from_card(&HEADPHONE_DEFAULT)),
        ("Gaming", from_card(&HEADPHONE_GAMING)),
        ("Media", from_card(&HEADPHONE_MEDIA)),
    ]
}

pub fn mic() -> [(&'static str, Eq); 3] {
    [
        ("Default", from_card(&MIC_DEFAULT)),
        ("Competition", from_card(&MIC_COMPETITION)),
        ("Broadcast", from_card(&MIC_BROADCAST)),
    ]
}

pub(crate) fn from_card(card: &Equalizer) -> Eq {
    Eq {
        bands: card
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
