use eframe::egui::{Checkbox, Ui};
use ogh_ipc::{Channel, Request, Setting, StreamMixSetting};

use super::{Level, LevelControl, Scale, set, set_accessible_label};
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
    stream_mix(ui, state, profile.stream_mix, enabled, requests);
}

#[derive(Debug, Clone, Copy)]
enum Stream {
    Output,
    Game,
    Voice,
}

impl Stream {
    const ALL: [Stream; 3] = [Stream::Output, Stream::Game, Stream::Voice];

    fn control(self) -> LevelControl {
        match self {
            Stream::Output => LevelControl::StreamOutput,
            Stream::Game => LevelControl::StreamGame,
            Stream::Voice => LevelControl::StreamVoice,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Stream::Output => "Output",
            Stream::Game => "Game",
            Stream::Voice => "Voice",
        }
    }

    fn mute_label(self) -> &'static str {
        match self {
            Stream::Output => "Mute output",
            Stream::Game => "Mute game",
            Stream::Voice => "Mute voice",
        }
    }

    fn channel(self, mix: &mut StreamMixSetting) -> &mut Channel {
        match self {
            Stream::Output => &mut mix.output,
            Stream::Game => &mut mix.game,
            Stream::Voice => &mut mix.voice,
        }
    }
}

fn stream_mix(
    ui: &mut Ui,
    state: &mut GuiState,
    mix: Option<StreamMixSetting>,
    enabled: bool,
    requests: &mut Vec<Request>,
) {
    let mut changed = mix;
    for stream in Stream::ALL {
        let channel = changed.as_mut().map(|mix| stream.channel(mix));
        stream_row(ui, state, stream, channel, enabled);
    }
    if let Some(changed) = changed.filter(|changed| Some(*changed) != mix) {
        requests.push(set(Setting::StreamMix(changed)));
    }
}

fn stream_row(
    ui: &mut Ui,
    state: &mut GuiState,
    stream: Stream,
    channel: Option<&mut Channel>,
    enabled: bool,
) {
    ui.horizontal(|ui| {
        let level = state.held.slider(
            ui,
            Level {
                control: stream.control(),
                label: stream.label(),
                scale: Scale::Percent,
                current: channel.as_ref().map(|channel| channel.level_percent),
                enabled,
            },
        );
        let mute = match channel {
            Some(channel) => {
                if let Some(percent) = level {
                    channel.level_percent = percent;
                }
                ui.add_enabled(enabled, Checkbox::new(&mut channel.muted, "Mute"))
            }
            None => ui.add_enabled(false, Checkbox::new(&mut false, "Mute")),
        };
        set_accessible_label(ui, &mute, stream.mute_label());
    });
}
