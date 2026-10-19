use ogh_ipc::Headset;
use ogh_protocol::features::Event;
use ogh_protocol::features::advanced_para_eq::{EqInstance, Equalizer};
use ogh_protocol::features::auto_sleep::{AutoSleep, SleepAfter};
use ogh_protocol::features::battery::{Battery, ChargeStatus};
use ogh_protocol::features::headset_mix::GameVoiceBalance;
use ogh_protocol::features::mixer_audio::StreamMix;
use ogh_protocol::features::noise_gate::NoiseGate;
use ogh_protocol::features::sidetone::Sidetone;
use ogh_protocol::features::tones::Tones;
use ogh_protocol::features::volume::{Volume, VolumeLevel};

/// The first byte of every captured sleep timer reply and write.
const AUTO_SLEEP_PREFIX: u8 = 0x3c;

/// What the daemon last learned from the device. `None` means not read yet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceSnapshot {
    pub battery: Option<Battery>,
    pub headset_available: Option<bool>,
    pub sidetone: Option<Sidetone>,
    pub balance: Option<GameVoiceBalance>,
    pub noise_gate: Option<NoiseGate>,
    pub headphone_eq: Option<Equalizer>,
    pub mic_eq: Option<Equalizer>,
    pub stream_mix: Option<StreamMix>,
    pub volume: Option<Volume>,
    pub led_brightness: Option<u8>,
    pub auto_sleep: Option<AutoSleep>,
    pub tones: Option<Tones>,
    pub firmware: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DeviceWrite {
    Sidetone(Sidetone),
    Balance(GameVoiceBalance),
    NoiseGate(NoiseGate),
    Eq(EqInstance, Equalizer),
    StreamMix(StreamMix),
    Volume(VolumeLevel),
    LedBrightness(u8),
    SleepAfter(SleepAfter),
    Tones(Tones),
}

impl DeviceSnapshot {
    pub fn apply_event(&mut self, event: &Event) {
        match event {
            Event::Battery(battery) => self.battery = Some(*battery),
            Event::HeadsetAvailability(available) => self.headset_available = Some(*available),
            Event::Sidetone(sidetone) => self.sidetone = Some(*sidetone),
            Event::Balance(balance) => self.balance = Some(*balance),
            Event::NoiseGate(gate) => self.noise_gate = Some(*gate),
            Event::MixerSettings(mix) => self.stream_mix = Some(*mix),
            Event::Volume(volume) => self.volume = Some(*volume),
            Event::AutoSleep(auto_sleep) => self.auto_sleep = Some(*auto_sleep),
            Event::Tones(tones) => self.tones = Some(*tones),
            Event::CustomEq(custom) => self.set_eq(custom.instance, custom.equalizer),
            Event::BluetoothDevices(_) | Event::MixerInputs(_) => {}
        }
    }

    /// For writes the device does not echo as an event, such as LED
    /// brightness. Touches only the field the write targets.
    pub fn record_write(&mut self, write: &DeviceWrite) {
        match write {
            DeviceWrite::Sidetone(sidetone) => self.sidetone = Some(*sidetone),
            DeviceWrite::Balance(balance) => self.balance = Some(*balance),
            DeviceWrite::NoiseGate(gate) => self.noise_gate = Some(*gate),
            DeviceWrite::Eq(instance, equalizer) => self.set_eq(*instance, *equalizer),
            DeviceWrite::StreamMix(mix) => self.stream_mix = Some(*mix),
            DeviceWrite::Volume(level) => {
                let mut volume = self.volume.unwrap_or(Volume {
                    scaled: 0,
                    muted: false,
                    level: *level,
                });
                volume.level = *level;
                self.volume = Some(volume);
            }
            DeviceWrite::LedBrightness(percent) => self.led_brightness = Some(*percent),
            DeviceWrite::SleepAfter(sleep_after) => {
                let mut auto_sleep = self.auto_sleep.unwrap_or(AutoSleep {
                    prefix: AUTO_SLEEP_PREFIX,
                    sleep_after: *sleep_after,
                });
                auto_sleep.sleep_after = *sleep_after;
                self.auto_sleep = Some(auto_sleep);
            }
            DeviceWrite::Tones(tones) => self.tones = Some(*tones),
        }
    }

    /// A docked headset reports itself unavailable while its battery charges,
    /// so unavailable alone does not say docked.
    pub fn headset(&self) -> Headset {
        match self.headset_available {
            None => Headset::Unknown,
            Some(true) => Headset::InUse,
            Some(false) => match self.battery.map(|battery| battery.status) {
                Some(ChargeStatus::Charging | ChargeStatus::Full) => Headset::Docked,
                _ => Headset::Off,
            },
        }
    }

    fn set_eq(&mut self, instance: EqInstance, equalizer: Equalizer) {
        match instance {
            EqInstance::Headphone => self.headphone_eq = Some(equalizer),
            EqInstance::Mic => self.mic_eq = Some(equalizer),
        }
    }
}
