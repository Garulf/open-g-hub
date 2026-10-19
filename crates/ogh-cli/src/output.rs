//! Text and JSON renderings of what the commands read.

use ogh_device::{Event, FeatureInfo};
use ogh_protocol::features::advanced_para_eq::{CustomEq, EqInstance};
use ogh_protocol::features::auto_sleep::{AutoSleep, SleepAfter};
use ogh_protocol::features::battery::{Battery, ChargeStatus};
use ogh_protocol::features::headset_mix::GameVoiceBalance;
use ogh_protocol::features::mixer_audio::{MixChannel, MixInput, StreamMix};
use ogh_protocol::features::noise_gate::NoiseGate;
use ogh_protocol::features::sidetone::Sidetone;
use ogh_protocol::features::tones::Tones;
use ogh_protocol::features::volume::Volume;
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
                self.noise_gate.map_or_else(unknown, noise_gate_name),
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

pub fn noise_gate_name(gate: NoiseGate) -> String {
    match gate {
        NoiseGate::Off => "off".to_string(),
        NoiseGate::Home => "home".to_string(),
        NoiseGate::Night => "night".to_string(),
        NoiseGate::Tournament => "tournament".to_string(),
        NoiseGate::Other(code) => format!("other-{code}"),
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
        Event::MixerSettings(mix) => format!("stream mix: {}", stream_mix_text(mix)),
        Event::Volume(volume) => format!("volume: {}", volume_text(*volume)),
        Event::AutoSleep(AutoSleep { sleep_after, .. }) => {
            format!("sleep after: {}", sleep_after_text(*sleep_after))
        }
        Event::Tones(tones) => format!("tones: {}", tones_name(*tones)),
        Event::CustomEq(eq) => format!(
            "{} eq: custom slot {} changed",
            eq_instance_name(eq.instance),
            eq.slot
        ),
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
        Event::MixerSettings(mix) => stream_mix_json(mix),
        Event::Volume(volume) => json!({
            "event": "volume",
            "percent": volume.level.to_percent(),
            "muted": volume.muted,
        }),
        Event::AutoSleep(AutoSleep { sleep_after, .. }) => json!({
            "event": "sleep_after",
            "minutes": (*sleep_after != SleepAfter::NEVER).then_some(sleep_after.0),
        }),
        Event::Tones(tones) => json!({"event": "tones", "value": tones.0}),
        Event::CustomEq(eq) => custom_eq_json(eq),
    }
}

fn input_name(input: MixInput) -> &'static str {
    match input {
        MixInput::MicOut => "mic_out",
        MixInput::Game => "game",
        MixInput::Bluetooth => "bluetooth",
        MixInput::Voice => "voice",
    }
}

fn channel_text(name: &str, channel: MixChannel) -> String {
    let muted = if channel.muted { " muted" } else { "" };
    format!("{name} {}%{muted}", channel.level.to_percent())
}

fn stream_mix_text(mix: &StreamMix) -> String {
    let inputs = MixInput::ALL
        .map(|input| channel_text(&input_name(input).replace('_', " "), mix.input(input)));
    format!(
        "{}, {}",
        channel_text("stream", mix.stream),
        inputs.join(", ")
    )
}

fn channel_json(channel: MixChannel) -> Value {
    json!({"percent": channel.level.to_percent(), "muted": channel.muted})
}

fn stream_mix_json(mix: &StreamMix) -> Value {
    let inputs: serde_json::Map<String, Value> = MixInput::ALL
        .iter()
        .map(|&input| {
            (
                input_name(input).to_string(),
                channel_json(mix.input(input)),
            )
        })
        .collect();
    json!({"event": "stream_mix", "stream": channel_json(mix.stream), "inputs": inputs})
}

fn volume_text(volume: Volume) -> String {
    let muted = if volume.muted { " muted" } else { "" };
    format!("{}%{muted}", volume.level.to_percent())
}

fn sleep_after_text(sleep_after: SleepAfter) -> String {
    if sleep_after == SleepAfter::NEVER {
        "never".to_string()
    } else {
        format!("{} min", sleep_after.0)
    }
}

fn tones_name(tones: Tones) -> String {
    if tones == Tones::ALL {
        "all".to_string()
    } else {
        tones.0.to_string()
    }
}

fn eq_instance_name(instance: EqInstance) -> &'static str {
    match instance {
        EqInstance::Mic => "mic",
        EqInstance::Headphone => "headphone",
    }
}

fn custom_eq_json(eq: &CustomEq) -> Value {
    let bands: Vec<Value> = eq
        .equalizer
        .bands
        .iter()
        .map(|band| json!({"freq_hz": band.freq_hz, "q": band.q, "gain_db": band.gain_db}))
        .collect();
    json!({
        "event": "eq",
        "instance": eq_instance_name(eq.instance),
        "slot": eq.slot,
        "bands": bands,
    })
}
