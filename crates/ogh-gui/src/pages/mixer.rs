use eframe::egui::{Checkbox, Ui};
use ogh_ipc::{Channel, Request, Setting, StreamMixSetting};

use super::{Level, LevelControl, Scale, set};
use crate::app::GuiState;

pub(crate) fn show(ui: &mut Ui, state: &mut GuiState, requests: &mut Vec<Request>) {
    let enabled = state.shared.can_write();
    let profile = state
        .shared
        .status
        .as_ref()
        .map(|status| status.settings.profile.clone())
        .unwrap_or_default();

    let sidetone = state.held.slider(
        ui,
        Level {
            control: LevelControl::Sidetone,
            label: "Sidetone",
            scale: Scale::Percent,
            current: profile.sidetone_percent,
            enabled,
        },
    );
    if let Some(percent) = sidetone {
        requests.push(set(Setting::Sidetone { percent }));
    }

    let balance = state.held.slider(
        ui,
        Level {
            control: LevelControl::Balance,
            label: "Game/Voice balance",
            scale: Scale::Balance,
            current: profile.balance,
            enabled,
        },
    );
    if let Some(value) = balance {
        requests.push(set(Setting::Balance { value }));
    }

    ui.add_space(8.0);
    ui.strong("Stream mix");
    match profile.stream_mix {
        Some(mix) => stream_mix(ui, state, mix, enabled, requests),
        None => unknown_stream_mix(ui, state),
    }
}

const CHANNELS: [(LevelControl, &str); 3] = [
    (LevelControl::StreamOutput, "Output"),
    (LevelControl::StreamGame, "Game"),
    (LevelControl::StreamVoice, "Voice"),
];

fn stream_mix(
    ui: &mut Ui,
    state: &mut GuiState,
    mix: StreamMixSetting,
    enabled: bool,
    requests: &mut Vec<Request>,
) {
    let mut changed = mix;
    for (control, label) in CHANNELS {
        let channel = channel_mut(&mut changed, control);
        ui.horizontal(|ui| {
            let level = state.held.slider(
                ui,
                Level {
                    control,
                    label,
                    scale: Scale::Percent,
                    current: Some(channel.level_percent),
                    enabled,
                },
            );
            if let Some(percent) = level {
                channel.level_percent = percent;
            }
            ui.add_enabled(enabled, Checkbox::new(&mut channel.muted, "Mute"));
        });
    }
    if changed != mix {
        requests.push(set(Setting::StreamMix(changed)));
    }
}

fn unknown_stream_mix(ui: &mut Ui, state: &mut GuiState) {
    for (control, label) in CHANNELS {
        ui.horizontal(|ui| {
            state.held.slider(
                ui,
                Level {
                    control,
                    label,
                    scale: Scale::Percent,
                    current: None,
                    enabled: false,
                },
            );
            ui.add_enabled(false, Checkbox::new(&mut false, "Mute"));
        });
    }
}

fn channel_mut(mix: &mut StreamMixSetting, control: LevelControl) -> &mut Channel {
    match control {
        LevelControl::StreamGame => &mut mix.game,
        LevelControl::StreamVoice => &mut mix.voice,
        _ => &mut mix.output,
    }
}
