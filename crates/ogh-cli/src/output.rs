//! Text renderings of what the daemon reports.

use ogh_ipc::{
    Battery, Charge, Connection, Feature, Gate, Headset, ProfileInfo, Status, StreamMixSetting,
};

const UNKNOWN: &str = "unknown";
const LABEL_WIDTH: usize = "auto-switch:".len() + 1;

pub fn headset_name(headset: Headset) -> &'static str {
    match headset {
        Headset::InUse => "in_use",
        Headset::Docked => "docked",
        Headset::Off => "off",
        Headset::Unknown => UNKNOWN,
    }
}

pub fn gate_name(gate: Gate) -> &'static str {
    match gate {
        Gate::Off => "off",
        Gate::Home => "home",
        Gate::Night => "night",
        Gate::Tournament => "tournament",
    }
}

pub fn connection_text(connection: &Connection) -> String {
    match connection {
        Connection::Connected => "connected".to_string(),
        Connection::NotFound => "base station not found".to_string(),
        Connection::GhubRunning => "G Hub is running".to_string(),
        Connection::AccessDenied { message } => format!("access denied: {message}"),
        Connection::Error { message } => format!("error: {message}"),
    }
}

fn charge_text(charge: Charge) -> String {
    match charge {
        Charge::Discharging => "discharging".to_string(),
        Charge::Charging => "charging".to_string(),
        Charge::Full => "full".to_string(),
        Charge::Other(code) => format!("status {code}"),
    }
}

fn battery_text(battery: Option<Battery>) -> String {
    battery.map_or_else(
        || UNKNOWN.to_string(),
        |b| format!("{}% ({})", b.percent, charge_text(b.charge)),
    )
}

fn sleep_text(minutes: u8) -> String {
    match minutes {
        0 => "never".to_string(),
        minutes => format!("{minutes} min"),
    }
}

fn percent_text(value: Option<u8>) -> String {
    value.map_or_else(|| UNKNOWN.to_string(), |v| format!("{v}%"))
}

fn value_text<T: ToString>(value: Option<T>) -> String {
    value.map_or_else(|| UNKNOWN.to_string(), |v| v.to_string())
}

fn stream_mix_text(mix: &StreamMixSetting) -> String {
    [
        ("output", mix.output),
        ("game", mix.game),
        ("voice", mix.voice),
    ]
    .map(|(name, channel)| {
        let muted = if channel.muted { " muted" } else { "" };
        format!("{name} {}%{muted}", channel.level_percent)
    })
    .join(", ")
}

fn auto_switch_text(status: &Status) -> String {
    let auto = &status.auto_switch;
    let mut text = if auto.enabled { "on" } else { "off" }.to_string();
    if auto.active {
        text.push_str(", active");
    }
    if let Some(error) = &auto.error {
        text.push_str(&format!(", error: {error}"));
    }
    text
}

fn focus_text(status: &Status) -> String {
    match (&status.focus.available, &status.focus.app) {
        (false, _) => "unavailable".to_string(),
        (true, Some(app)) => app.clone(),
        (true, None) => "none".to_string(),
    }
}

pub fn status_human(status: &Status) -> String {
    let settings = &status.settings;
    let profile = &settings.profile;
    let mut rows = vec![
        ("connection", connection_text(&status.connection)),
        ("headset", headset_name(status.headset).to_string()),
        ("battery", battery_text(status.battery)),
        ("firmware", value_text(status.firmware.as_ref())),
        (
            "profile",
            status
                .active_profile
                .clone()
                .unwrap_or_else(|| "none".to_string()),
        ),
        ("auto-switch", auto_switch_text(status)),
        ("focus", focus_text(status)),
        ("sidetone", percent_text(profile.sidetone_percent)),
        ("balance", value_text(profile.balance)),
        (
            "noise gate",
            profile
                .noise_gate
                .map_or_else(|| UNKNOWN.to_string(), |g| gate_name(g).to_string()),
        ),
        ("volume", percent_text(settings.volume_percent)),
        ("brightness", percent_text(settings.led_brightness_percent)),
        (
            "sleep",
            settings
                .sleep_after_minutes
                .map_or_else(|| UNKNOWN.to_string(), sleep_text),
        ),
        ("tones", value_text(settings.tones)),
        (
            "stream mix",
            profile
                .stream_mix
                .as_ref()
                .map_or_else(|| UNKNOWN.to_string(), stream_mix_text),
        ),
    ];
    if let Some(error) = &status.config_error {
        rows.push(("config", error.clone()));
    }
    rows.iter()
        .map(|(label, value)| format!("{:<LABEL_WIDTH$}{value}", format!("{label}:")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One line for `watch`: the parts of the status that change.
pub fn status_line(status: &Status) -> String {
    format!(
        "connection {} | headset {} | battery {} | profile {}",
        connection_text(&status.connection),
        headset_name(status.headset),
        status
            .battery
            .map_or_else(|| UNKNOWN.to_string(), |b| format!("{}%", b.percent)),
        status.active_profile.as_deref().unwrap_or("none"),
    )
}

pub fn features_table(features: &[Feature]) -> String {
    features
        .iter()
        .map(|f| {
            format!(
                "{} 0x{:04x} {} {}",
                f.index,
                f.id,
                f.name.as_deref().unwrap_or("?"),
                f.version
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn profiles_table(profiles: &[ProfileInfo]) -> String {
    profiles
        .iter()
        .map(|p| {
            let mark = if p.is_default { '*' } else { ' ' };
            if p.apps.is_empty() {
                format!("{mark} {}", p.name)
            } else {
                format!("{mark} {} (apps: {})", p.name, p.apps.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
