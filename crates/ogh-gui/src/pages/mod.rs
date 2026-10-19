pub(crate) mod device;
pub mod eq;
pub(crate) mod mixer;

use eframe::egui::{self, Slider, Ui};
use ogh_ipc::{Request, Setting};

use crate::app::{GuiState, Page};
use crate::describe::{balance_label, gain_label, parse_gain};

pub(crate) const NOT_READ_YET: &str = "not read yet";

pub(crate) fn set(setting: Setting) -> Request {
    Request::Set { setting }
}

pub(crate) fn show(ui: &mut Ui, state: &mut GuiState, requests: &mut Vec<Request>) {
    ui.heading(state.page.title());
    ui.add_space(8.0);
    match state.page {
        Page::Mixer => mixer::show(ui, state, requests),
        Page::Device => device::show(ui, state, requests),
        Page::Equalizer => eq::equalizer(ui, state, requests),
        Page::Microphone => eq::microphone(ui, state, requests),
        Page::Profiles => {
            ui.label("Not available yet.");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LevelControl {
    Sidetone,
    Balance,
    StreamOutput,
    StreamGame,
    StreamVoice,
    Volume,
    LedBrightness,
    EqGain(eq::Target, u8),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Scale {
    Percent,
    Balance,
    Gain,
}

impl Scale {
    fn max(self) -> u8 {
        match self {
            Scale::Percent => 100,
            Scale::Balance => 12,
            Scale::Gain => eq::GAIN_STEPS,
        }
    }

    fn slider<'v>(self, value: &'v mut u8, label: &str) -> Slider<'v> {
        let slider = Slider::new(value, 0..=self.max()).step_by(1.0).text(label);
        match self {
            Scale::Percent => slider.suffix("%"),
            Scale::Balance => slider
                .custom_formatter(|value, _| balance_label(value as u8))
                .custom_parser(|text| text.split_whitespace().next()?.parse().ok()),
            Scale::Gain => slider
                .vertical()
                .custom_formatter(|step, _| gain_label(eq::step_to_gain(step as u8)))
                .custom_parser(|text| parse_gain(text).map(eq::gain_to_step_value)),
        }
    }
}

/// The slider the pointer is holding, with the value it shows. While held, that value
/// wins over incoming statuses so the handle does not jump back under the pointer.
#[derive(Debug, Default)]
pub(crate) struct HeldLevel(Option<(LevelControl, u8)>);

pub(crate) struct Level<'a> {
    pub control: LevelControl,
    pub label: &'a str,
    pub scale: Scale,
    pub current: Option<u8>,
    pub enabled: bool,
}

impl HeldLevel {
    /// Forgets a hold once the pointer is up, for when its slider was not drawn at the
    /// release (another page, daemon down, or a value that became unknown). On the release
    /// frame itself the drawn slider still needs the hold, because egui applies the release
    /// position as a click.
    pub(crate) fn release_if_idle(&mut self, ui: &Ui) {
        if self.0.is_none() {
            return;
        }
        let (down, released) =
            ui.input(|input| (input.pointer.any_down(), input.pointer.any_released()));
        if down {
            return;
        }
        if !released {
            self.0 = None;
        }
        ui.ctx().request_repaint();
    }

    /// Draws one level slider and returns the new value when the user changed it.
    pub(crate) fn slider(&mut self, ui: &mut Ui, level: Level<'_>) -> Option<u8> {
        ui.horizontal(|ui| {
            let Some(current) = level.current else {
                ui.add_enabled(false, level.scale.slider(&mut 0, level.label));
                ui.weak(NOT_READ_YET);
                return None;
            };
            let shown = match self.0 {
                Some((control, value)) if control == level.control => value,
                _ => current,
            };
            let mut value = shown;
            let response =
                ui.add_enabled(level.enabled, level.scale.slider(&mut value, level.label));
            if response.is_pointer_button_down_on() {
                self.0 = Some((level.control, value));
            } else if self.0.is_some_and(|(control, _)| control == level.control) {
                self.0 = None;
            }
            (value != shown).then_some(value)
        })
        .inner
    }
}

/// A combo box over fixed choices; returns the choice the user picked when it differs.
pub(crate) fn choice(
    ui: &mut Ui,
    label: &str,
    current: Option<u8>,
    options: &[u8],
    text: fn(u8) -> String,
    enabled: bool,
) -> Option<u8> {
    let mut picked = None;
    ui.add_enabled_ui(enabled && current.is_some(), |ui| {
        let selected = current.map_or_else(|| NOT_READ_YET.to_string(), text);
        egui::ComboBox::from_label(label)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for &option in options {
                    let chosen = Some(option) == current;
                    if ui.selectable_label(chosen, text(option)).clicked() && !chosen {
                        picked = Some(option);
                    }
                }
            });
    });
    picked
}
