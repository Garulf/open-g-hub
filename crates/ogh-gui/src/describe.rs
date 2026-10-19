use ogh_ipc::{AutoSwitchStatus, Charge, Connection, Gate, Headset, Status};

use crate::Shared;

const NOT_RUNNING: &str = "ogh-daemon is not running";

pub fn headline(shared: &Shared) -> String {
    let Some(status) = &shared.status else {
        return NOT_RUNNING.to_string();
    };
    match &status.connection {
        Connection::Connected => connected_headline(status),
        Connection::NotFound => "A50 X not found".to_string(),
        Connection::GhubRunning => "G Hub is running; quit it to use open-g-hub".to_string(),
        Connection::AccessDenied { message } => format!("No access to the A50 X: {message}"),
        Connection::Error { message } => format!("A50 X error: {message}"),
    }
}

fn connected_headline(status: &Status) -> String {
    let mut text = format!("A50 X: {}", headset_word(status.headset));
    if let Some(battery) = status.battery {
        text.push_str(&format!(
            ", {}% ({})",
            battery.percent,
            charge_word(battery.charge)
        ));
    }
    text
}

fn headset_word(headset: Headset) -> &'static str {
    match headset {
        Headset::InUse => "in use",
        Headset::Docked => "docked",
        Headset::Off => "off",
        Headset::Unknown => "unknown",
    }
}

fn charge_word(charge: Charge) -> String {
    match charge {
        Charge::Discharging => "discharging".to_string(),
        Charge::Charging => "charging".to_string(),
        Charge::Full => "full".to_string(),
        Charge::Other(code) => format!("status {code}"),
    }
}

pub fn sleep_label(minutes: u8) -> String {
    match minutes {
        0 => "Never".to_string(),
        minutes => format!("{minutes} min"),
    }
}

pub fn tones_label(value: u8) -> String {
    match value {
        1 => "1 (All)".to_string(),
        value => value.to_string(),
    }
}

pub fn balance_label(value: u8) -> String {
    match value {
        6 => "6 (centre)".to_string(),
        value => value.to_string(),
    }
}

pub fn gate_label(gate: Gate) -> &'static str {
    match gate {
        Gate::Off => "Off",
        Gate::Home => "Home",
        Gate::Night => "Night",
        Gate::Tournament => "Tournament",
    }
}

pub fn auto_switch_text(auto: &AutoSwitchStatus) -> String {
    let mut text = if auto.enabled { "On" } else { "Off" }.to_string();
    if auto.active {
        text.push_str(", active");
    }
    if let Some(error) = &auto.error {
        text.push_str(&format!(", error: {error}"));
    }
    text
}
