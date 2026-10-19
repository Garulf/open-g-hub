//! Text and JSON renderings of what the commands read.

use ogh_device::{Event, FeatureInfo};
use ogh_protocol::features::battery::{Battery, ChargeStatus};
use ogh_protocol::features::headset_mix::GameVoiceBalance;
use ogh_protocol::features::noise_gate::NoiseGate;
use ogh_protocol::features::sidetone::Sidetone;
use serde_json::{Value, json};

const UNKNOWN: &str = "unknown";
const LABEL_WIDTH: usize = "noise gate:".len();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadsetState {
    InUse,
    Docked,
    Disconnected,
}

impl HeadsetState {
    fn name(self) -> &'static str {
        match self {
            Self::InUse => "in-use",
            Self::Docked => "docked",
            Self::Disconnected => "disconnected",
        }
    }
}

/// Everything `ogh status` reports. A `None` is a read that timed out while
/// the headset was not in use.
pub struct Status {
    pub in_use: bool,
    pub battery: Option<Battery>,
    pub sidetone: Option<Sidetone>,
    pub balance: Option<GameVoiceBalance>,
    pub noise_gate: Option<NoiseGate>,
    pub firmware: String,
}

impl Status {
    /// Docked is told apart from off by the battery status alone, because
    /// the device sends the same frames for both.
    pub fn headset(&self) -> HeadsetState {
        if self.in_use {
            HeadsetState::InUse
        } else if self.battery.is_some_and(|b| is_on_dock(b.status)) {
            HeadsetState::Docked
        } else {
            HeadsetState::Disconnected
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "headset": self.headset().name(),
            "battery_percent": self.battery.map(|b| b.percent()),
            "charging": self.battery.map(|b| b.status == ChargeStatus::Charging),
            "docked": self.docked(),
            "headset_connected": self.in_use,
            "sidetone": self.sidetone.map(Sidetone::to_percent),
            "balance": self.balance.map(|GameVoiceBalance(raw)| raw),
            "noise_gate": self.noise_gate.map(noise_gate_name),
            "firmware": self.firmware,
        })
    }

    pub fn to_human(&self) -> String {
        let rows = [
            ("headset", self.headset().name().to_string()),
            ("battery", self.battery.map_or_else(unknown, battery_text)),
            (
                "sidetone",
                self.sidetone
                    .map_or_else(unknown, |s| format!("{}%", s.to_percent())),
            ),
            (
                "balance",
                self.balance
                    .map_or_else(unknown, |GameVoiceBalance(raw)| raw.to_string()),
            ),
            (
                "noise gate",
                self.noise_gate
                    .map_or_else(unknown, |gate| noise_gate_name(gate).to_string()),
            ),
            ("firmware", self.firmware.clone()),
        ];
        rows.iter()
            .map(|(label, value)| format!("{:<LABEL_WIDTH$} {value}", format!("{label}:")))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn docked(&self) -> Option<bool> {
        if self.in_use {
            Some(false)
        } else {
            self.battery.map(|b| is_on_dock(b.status))
        }
    }
}

fn unknown() -> String {
    UNKNOWN.to_string()
}

fn is_on_dock(status: ChargeStatus) -> bool {
    matches!(status, ChargeStatus::Charging | ChargeStatus::Full)
}

pub fn noise_gate_name(gate: NoiseGate) -> &'static str {
    match gate {
        NoiseGate::Off => "off",
        NoiseGate::Home => "home",
        NoiseGate::Night => "night",
        NoiseGate::Tournament => "tournament",
    }
}

fn charge_status_name(status: ChargeStatus) -> String {
    match status {
        ChargeStatus::Discharging => "discharging".to_string(),
        ChargeStatus::Charging => "charging".to_string(),
        ChargeStatus::Full => "full".to_string(),
        ChargeStatus::Other(code) => format!("other-{code}"),
    }
}

fn battery_text(battery: Battery) -> String {
    let percent = battery.percent();
    match battery.status {
        ChargeStatus::Discharging => format!("{percent}%"),
        status => format!("{percent}% {}", charge_status_name(status)),
    }
}

/// One line per feature, from Root up, with a header.
pub fn features_table(features: &[FeatureInfo]) -> String {
    let rows: Vec<[String; 4]> = features
        .iter()
        .map(|f| {
            [
                f.index.to_string(),
                format!("0x{:04X}", f.id.0),
                f.id.name().unwrap_or(UNKNOWN).to_string(),
                f.version.to_string(),
            ]
        })
        .collect();
    let header = ["index", "id", "name", "version"].map(String::from);
    let widths: [usize; 4] = std::array::from_fn(|column| {
        std::iter::once(&header)
            .chain(&rows)
            .map(|row| row[column].len())
            .max()
            .unwrap_or(0)
    });
    std::iter::once(&header)
        .chain(&rows)
        .map(|row| {
            let cells: Vec<String> = row
                .iter()
                .zip(widths)
                .map(|(cell, width)| format!("{cell:<width$}"))
                .collect();
            cells.join("  ").trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn event_line(event: &Event) -> String {
    match event {
        Event::HeadsetAvailability(true) => "headset: in-use".to_string(),
        Event::HeadsetAvailability(false) => "headset: not in use".to_string(),
        Event::Battery(battery) => format!("battery: {}", battery_text(*battery)),
        Event::Sidetone(sidetone) => format!("sidetone: {}%", sidetone.to_percent()),
        Event::Balance(GameVoiceBalance(raw)) => format!("balance: {raw}"),
        Event::BluetoothDevices(count) => format!("bluetooth devices: {count}"),
        Event::MixerInputs(mask) => format!("mixer inputs: {mask:#04x}"),
        Event::NoiseGate(gate) => format!("noise gate: {}", noise_gate_name(*gate)),
    }
}

pub fn event_json(event: &Event) -> Value {
    match event {
        Event::HeadsetAvailability(in_use) => json!({"event": "headset", "in_use": in_use}),
        Event::Battery(battery) => json!({
            "event": "battery",
            "percent": battery.percent(),
            "status": charge_status_name(battery.status),
        }),
        Event::Sidetone(sidetone) => json!({"event": "sidetone", "percent": sidetone.to_percent()}),
        Event::Balance(GameVoiceBalance(raw)) => json!({"event": "balance", "value": raw}),
        Event::BluetoothDevices(count) => json!({"event": "bluetooth_devices", "count": count}),
        Event::MixerInputs(mask) => json!({"event": "mixer_inputs", "mask": mask}),
        Event::NoiseGate(gate) => json!({"event": "noise_gate", "mode": noise_gate_name(*gate)}),
    }
}
