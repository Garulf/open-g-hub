use eframe::egui::{Button, DragValue, RadioButton, Response, Ui};
use ogh_ipc::{Eq, Gate, ProfileSettings, Request, Setting};
use ogh_protocol::features::advanced_para_eq::{
    HEADPHONE_DEFAULT, HEADPHONE_MIN_FREQ_HZ, MAX_FREQ_HZ, MAX_GAIN_DB, MAX_Q, MIC_DEFAULT,
    MIC_MIN_FREQ_HZ, MIN_GAIN_DB, MIN_Q,
};

use super::{Level, LevelControl, NOT_READ_YET, Scale, set};
use crate::app::GuiState;
use crate::describe::{frequency_label, gate_label};
use crate::presets::{self, CURATED_Q, from_card};

const STEPS_PER_DB: f32 = 2.0;
pub(crate) const GAIN_STEPS: u8 = ((MAX_GAIN_DB - MIN_GAIN_DB) * STEPS_PER_DB) as u8;
const GATES: [Gate; 4] = [Gate::Off, Gate::Home, Gate::Night, Gate::Tournament];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqMode {
    Graphic,
    Parametric,
}

/// Converts between G Hub's two EQ modes, keeping the gains. Graphic goes back to the
/// flat card's frequencies; parametric keeps the frequencies and takes the curated Q.
pub fn to_mode(eq: &Eq, mode: EqMode, flat: &Eq) -> Eq {
    let mut converted = eq.clone();
    for (band, flat) in converted.bands.iter_mut().zip(&flat.bands) {
        match mode {
            EqMode::Graphic => {
                band.q = None;
                band.freq_hz = flat.freq_hz;
            }
            EqMode::Parametric => band.q = Some(CURATED_Q),
        }
    }
    converted
}

fn mode_of(eq: &Eq) -> EqMode {
    match eq.bands.first() {
        Some(band) if band.q.is_none() => EqMode::Graphic,
        _ => EqMode::Parametric,
    }
}

pub(crate) fn gain_to_step(gain_db: f32) -> u8 {
    ((gain_db - MIN_GAIN_DB) * STEPS_PER_DB)
        .round()
        .clamp(0.0, f32::from(GAIN_STEPS)) as u8
}

pub(crate) fn gain_to_step_value(gain_db: f64) -> f64 {
    (gain_db - f64::from(MIN_GAIN_DB)) * f64::from(STEPS_PER_DB)
}

pub(crate) fn step_to_gain(step: u8) -> f32 {
    MIN_GAIN_DB + f32::from(step) / STEPS_PER_DB
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Target {
    Headphone,
    Mic,
}

impl Target {
    fn min_freq_hz(self) -> u16 {
        match self {
            Target::Headphone => HEADPHONE_MIN_FREQ_HZ,
            Target::Mic => MIC_MIN_FREQ_HZ,
        }
    }

    fn presets(self) -> [(&'static str, Eq); 3] {
        match self {
            Target::Headphone => presets::headphone(),
            Target::Mic => presets::mic(),
        }
    }

    fn flat(self) -> Eq {
        match self {
            Target::Headphone => from_card(&HEADPHONE_DEFAULT),
            Target::Mic => from_card(&MIC_DEFAULT),
        }
    }

    fn setting(self, eq: Eq) -> Setting {
        match self {
            Target::Headphone => Setting::HeadphoneEq(eq),
            Target::Mic => Setting::MicEq(eq),
        }
    }

    fn current(self, profile: &ProfileSettings) -> Option<&Eq> {
        match self {
            Target::Headphone => profile.headphone_eq.as_ref(),
            Target::Mic => profile.mic_eq.as_ref(),
        }
    }
}

pub(crate) fn equalizer(ui: &mut Ui, state: &mut GuiState, requests: &mut Vec<Request>) {
    editor(ui, state, Target::Headphone, requests);
}

pub(crate) fn microphone(ui: &mut Ui, state: &mut GuiState, requests: &mut Vec<Request>) {
    let enabled = state.shared.can_write();
    let current = state
        .shared
        .status
        .as_ref()
        .and_then(|status| status.settings.profile.noise_gate);
    let gate = ui
        .push_id(("noise_gate", Target::Mic), |ui| {
            noise_gate(ui, current, enabled)
        })
        .inner;
    if let Some(gate) = gate {
        requests.push(set(Setting::NoiseGate(gate)));
    }
    ui.add_space(8.0);
    ui.strong("Microphone EQ");
    editor(ui, state, Target::Mic, requests);
}

fn noise_gate(ui: &mut Ui, current: Option<Gate>, enabled: bool) -> Option<Gate> {
    let mut picked = None;
    ui.strong("Noise gate");
    ui.horizontal(|ui| {
        for gate in GATES {
            let chosen = Some(gate) == current;
            let radio = RadioButton::new(chosen, gate_label(gate));
            let clicked = ui
                .add_enabled(enabled && current.is_some(), radio)
                .clicked();
            if clicked && !chosen {
                picked = Some(gate);
            }
        }
        if current.is_none() {
            ui.weak(NOT_READ_YET);
        }
    });
    picked
}

fn editor(ui: &mut Ui, state: &mut GuiState, target: Target, requests: &mut Vec<Request>) {
    ui.push_id(("eq", target), |ui| {
        editor_body(ui, state, target, requests)
    });
}

fn editor_body(ui: &mut Ui, state: &mut GuiState, target: Target, requests: &mut Vec<Request>) {
    let enabled = state.shared.can_write();
    let current = state
        .shared
        .status
        .as_ref()
        .and_then(|status| target.current(&status.settings.profile))
        .cloned();

    ui.horizontal(|ui| {
        for (name, preset) in target.presets() {
            if ui.add_enabled(enabled, Button::new(name)).clicked() {
                requests.push(set(target.setting(preset)));
            }
        }
    });
    let Some(eq) = current else {
        ui.weak(format!("EQ {NOT_READ_YET}"));
        return;
    };

    if let Some(mode) = mode_switch(ui, mode_of(&eq), enabled) {
        requests.push(set(target.setting(to_mode(&eq, mode, &target.flat()))));
    }
    ui.add_space(8.0);
    if let Some(edited) = bands(ui, state, target, &eq, enabled) {
        requests.push(set(target.setting(edited)));
    }
}

fn mode_switch(ui: &mut Ui, current: EqMode, enabled: bool) -> Option<EqMode> {
    let mut picked = None;
    ui.horizontal(|ui| {
        for (mode, label) in [
            (EqMode::Graphic, "Graphic"),
            (EqMode::Parametric, "Parametric"),
        ] {
            let chosen = mode == current;
            let clicked = ui
                .add_enabled(enabled, Button::selectable(chosen, label))
                .clicked();
            if clicked && !chosen {
                picked = Some(mode);
            }
        }
    });
    picked
}

fn bands(ui: &mut Ui, state: &mut GuiState, target: Target, eq: &Eq, enabled: bool) -> Option<Eq> {
    let parametric = mode_of(eq) == EqMode::Parametric;
    let mut edited = eq.clone();
    eframe::egui::ScrollArea::horizontal().show(ui, |ui| {
        ui.spacing_mut().slider_width = 140.0;
        ui.horizontal_top(|ui| {
            for index in 0..eq.bands.len() {
                ui.push_id(("eq", target, index), |ui| {
                    ui.vertical(|ui| {
                        band(ui, state, target, &mut edited, index, parametric, enabled);
                    });
                });
            }
        });
    });
    (edited != *eq).then_some(edited)
}

fn band(
    ui: &mut Ui,
    state: &mut GuiState,
    target: Target,
    eq: &mut Eq,
    index: usize,
    parametric: bool,
    enabled: bool,
) {
    let shown = eq.bands[index].clone();
    let label = frequency_label(shown.freq_hz);
    let gain = state.held.slider(
        ui,
        Level {
            control: LevelControl::EqGain(target, index as u8),
            label: &label,
            scale: Scale::Gain,
            current: Some(gain_to_step(shown.gain_db)),
            enabled,
        },
    );
    if let Some(step) = gain {
        eq.bands[index].gain_db = step_to_gain(step);
    }
    if !parametric {
        return;
    }

    let (low, high) = freq_bounds(eq, index, target.min_freq_hz());
    let mut freq = shown.freq_hz;
    let freq_before = freq;
    let response = ui.add_enabled(
        enabled,
        DragValue::new(&mut freq)
            .range(low..=high)
            .clamp_existing_to_range(false)
            .update_while_editing(false)
            .speed((f64::from(shown.freq_hz) / 100.0).max(1.0))
            .suffix(" Hz"),
    );
    name(ui, &response, format!("Band {} frequency", index + 1));
    if freq != freq_before {
        eq.bands[index].freq_hz = freq.clamp(low, high);
    }

    let mut q = shown.q.unwrap_or(CURATED_Q);
    let q_before = q;
    let response = ui.add_enabled(
        enabled,
        DragValue::new(&mut q)
            .range(MIN_Q..=MAX_Q)
            .clamp_existing_to_range(false)
            .update_while_editing(false)
            .speed(0.01)
            .max_decimals(5)
            .prefix("Q "),
    );
    name(ui, &response, format!("Band {} Q", index + 1));
    if q != q_before {
        eq.bands[index].q = Some(q.clamp(MIN_Q, MAX_Q));
    }
}

/// A band may move anywhere strictly between its neighbours.
fn freq_bounds(eq: &Eq, index: usize, min_freq_hz: u16) -> (u16, u16) {
    let low = match index.checked_sub(1) {
        Some(prev) => eq.bands[prev].freq_hz.saturating_add(1),
        None => min_freq_hz,
    };
    let high = match eq.bands.get(index + 1) {
        Some(next) => next.freq_hz.saturating_sub(1),
        None => MAX_FREQ_HZ,
    };
    (low, high.max(low))
}

fn name(ui: &Ui, response: &Response, label: String) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_label(label);
    });
}
