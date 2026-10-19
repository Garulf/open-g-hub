use ogh_ipc::ProfileSettings;

use super::Controller;
use crate::convert;
use crate::snapshot::DeviceSnapshot;

/// How far the default profile has been filled from the headset.
pub(super) enum DefaultCapture {
    Unchecked,
    /// Saved while the headset-side settings were unknown; the named profile
    /// gets its missing fields from the first snapshot that knows them.
    Partial(String),
    Done,
}

impl Controller {
    /// A default profile with no settings takes what the headset holds, so
    /// the first profile switch has something to return to. Settings the
    /// headset only reports while in use are filled in from the first
    /// snapshot that has them. A failed save or unreadable config tries again
    /// at the next connect or refresh.
    pub(super) fn create_default_profile(&mut self) {
        let Ok(config) = self.store.config() else {
            return;
        };
        let settings = convert::profile_settings(&self.snapshot);
        let headset_known = headset_settings_known(&self.snapshot);
        match &self.default_capture {
            DefaultCapture::Done => {}
            DefaultCapture::Unchecked => {
                let name = config.default_profile.clone();
                let is_empty = config
                    .profiles
                    .get(&name)
                    .is_none_or(|profile| profile.settings == ProfileSettings::default());
                if !is_empty {
                    self.default_capture = DefaultCapture::Done;
                } else if self.store.save_profile(&name, settings).is_ok() {
                    self.default_capture = if headset_known {
                        DefaultCapture::Done
                    } else {
                        DefaultCapture::Partial(name)
                    };
                }
            }
            DefaultCapture::Partial(name) => {
                if !headset_known {
                    return;
                }
                let Some(saved) = config.profiles.get(name) else {
                    self.default_capture = DefaultCapture::Done;
                    return;
                };
                let completed = fill_missing(saved.settings.clone(), settings);
                let name = name.clone();
                if self.store.save_profile(&name, completed).is_ok() {
                    self.default_capture = DefaultCapture::Done;
                }
            }
        }
    }
}

fn headset_settings_known(snapshot: &DeviceSnapshot) -> bool {
    snapshot.sidetone.is_some() && snapshot.balance.is_some() && snapshot.noise_gate.is_some()
}

fn fill_missing(saved: ProfileSettings, captured: ProfileSettings) -> ProfileSettings {
    ProfileSettings {
        sidetone_percent: saved.sidetone_percent.or(captured.sidetone_percent),
        balance: saved.balance.or(captured.balance),
        noise_gate: saved.noise_gate.or(captured.noise_gate),
        headphone_eq: saved.headphone_eq.or(captured.headphone_eq),
        mic_eq: saved.mic_eq.or(captured.mic_eq),
        stream_mix: saved.stream_mix.or(captured.stream_mix),
    }
}
