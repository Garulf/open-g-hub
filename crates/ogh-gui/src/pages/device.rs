use eframe::egui::{Checkbox, Ui};
use ogh_ipc::{Request, Setting};

use super::{Level, LevelControl, Scale, choice, set};
use crate::app::GuiState;
use crate::describe::{auto_switch_text, sleep_label, tones_label};

const SLEEP_MINUTES: [u8; 3] = [0, 15, 30];
const TONES: [u8; 3] = [0, 1, 2];

pub(crate) fn show(ui: &mut Ui, state: &mut GuiState, requests: &mut Vec<Request>) {
    let enabled = state.shared.can_write();
    let settings = state
        .shared
        .status
        .as_ref()
        .map(|status| status.settings.clone())
        .unwrap_or_default();

    let volume = state.held.slider(
        ui,
        Level {
            control: LevelControl::Volume,
            label: "Volume",
            scale: Scale::Percent,
            current: settings.volume_percent,
            enabled,
        },
    );
    if let Some(percent) = volume {
        requests.push(set(Setting::Volume { percent }));
    }

    let brightness = state.held.slider(
        ui,
        Level {
            control: LevelControl::LedBrightness,
            label: "LED brightness",
            scale: Scale::Percent,
            current: settings.led_brightness_percent,
            enabled,
        },
    );
    if let Some(percent) = brightness {
        requests.push(set(Setting::LedBrightness { percent }));
    }

    let sleep = choice(
        ui,
        "Sleep After",
        settings.sleep_after_minutes,
        &SLEEP_MINUTES,
        sleep_label,
        enabled,
    );
    if let Some(minutes) = sleep {
        requests.push(set(Setting::SleepAfter { minutes }));
    }

    let tones = choice(ui, "Tones", settings.tones, &TONES, tones_label, enabled);
    if let Some(value) = tones {
        requests.push(set(Setting::Tones { value }));
    }

    ui.add_space(8.0);
    auto_switch(ui, state, requests);
}

fn auto_switch(ui: &mut Ui, state: &GuiState, requests: &mut Vec<Request>) {
    let auto = state
        .shared
        .status
        .as_ref()
        .map(|status| &status.auto_switch);
    let mut enabled = auto.is_some_and(|auto| auto.enabled);
    let checkbox = Checkbox::new(&mut enabled, "Switch audio devices on dock");
    if ui.add_enabled(auto.is_some(), checkbox).changed() {
        requests.push(Request::SetAutoSwitch { enabled });
    }
    match auto {
        Some(auto) => ui.weak(auto_switch_text(auto)),
        None => ui.weak(super::NOT_READ_YET),
    };
}
