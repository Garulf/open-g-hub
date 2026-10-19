use std::io::Write;
use std::str::FromStr;

use clap::{Parser, Subcommand, ValueEnum};
use ogh_ipc::{
    Channel, Connection, Eq, EqBand, ErrorKind, Gate, IpcError, Request, Response, Setting, Status,
    StreamMixSetting,
};
use ogh_protocol::features::advanced_para_eq::{self as eq, Equalizer};

use crate::autostart::{self, AutostartCommand};
use crate::daemon::Daemon;
use crate::error::CliError;
use crate::output;

const MAX_PERCENT: u8 = 100;
const MAX_BALANCE: u8 = 12;
const MAX_TONES: u8 = 2;
const SLEEP_NEVER: &str = "never";
const SLEEP_CHOICES: [u8; 2] = [15, 30];

#[derive(Parser)]
#[command(name = "ogh", version, about = "Control an Astro A50 X base station")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// A whole number from 0 to 100, as on G Hub's sliders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Percent(pub u8);

impl FromStr for Percent {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let value = whole_number(text, MAX_PERCENT)?;
        Ok(Self(value))
    }
}

fn whole_number(text: &str, max: u8) -> Result<u8, String> {
    let value: u8 = text
        .parse()
        .map_err(|e| format!("{e}; expected a whole number from 0 to {max}"))?;
    if value > max {
        return Err(format!("{value} is above {max}"));
    }
    Ok(value)
}

/// The Game/Voice balance, 0 to 12 with 6 the centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Balance(pub u8);

impl FromStr for Balance {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        Ok(Self(whole_number(text, MAX_BALANCE)?))
    }
}

/// The tones setting: 0, 1 or 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tones(pub u8);

impl FromStr for Tones {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        Ok(Self(whole_number(text, MAX_TONES)?))
    }
}

/// Sleep After: `never`, `15` or `30` minutes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SleepAfter(pub u8);

impl FromStr for SleepAfter {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        if text == SLEEP_NEVER {
            return Ok(Self(0));
        }
        match text.parse::<u8>() {
            Ok(minutes) if SLEEP_CHOICES.contains(&minutes) => Ok(Self(minutes)),
            _ => Err(format!("expected {SLEEP_NEVER}, 15 or 30")),
        }
    }
}

impl SleepAfter {
    fn name(self) -> String {
        match self.0 {
            0 => SLEEP_NEVER.to_string(),
            minutes => minutes.to_string(),
        }
    }
}

/// One EQ gain in dB, within what the device takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gain(pub f32);

impl FromStr for Gain {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let range = format!("{} to {} dB", eq::MIN_GAIN_DB, eq::MAX_GAIN_DB);
        let value: f32 = text
            .parse()
            .map_err(|_| format!("{text:?} is not a number; expected {range}"))?;
        if !(eq::MIN_GAIN_DB..=eq::MAX_GAIN_DB).contains(&value) {
            return Err(format!("{value} is outside {range}"));
        }
        // Normalises -0 so it prints as 0.
        Ok(Self(value + 0.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum GateArg {
    Off,
    Home,
    Night,
    Tournament,
}

impl GateArg {
    fn gate(self) -> Gate {
        match self {
            Self::Off => Gate::Off,
            Self::Home => Gate::Home,
            Self::Night => Gate::Night,
            Self::Tournament => Gate::Tournament,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum MixChannel {
    Output,
    Game,
    Voice,
}

impl MixChannel {
    fn name(self) -> &'static str {
        match self {
            Self::Output => "output",
            Self::Game => "game",
            Self::Voice => "voice",
        }
    }

    fn pick(self, mix: &mut StreamMixSetting) -> &mut Channel {
        match self {
            Self::Output => &mut mix.output,
            Self::Game => &mut mix.game,
            Self::Voice => &mut mix.voice,
        }
    }
}

/// What to do to one stream mix channel: a level, `mute` or `unmute`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixAction {
    Level(Percent),
    Mute,
    Unmute,
}

impl FromStr for MixAction {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        match text {
            "mute" => Ok(Self::Mute),
            "unmute" => Ok(Self::Unmute),
            level => level
                .parse()
                .map(Self::Level)
                .map_err(|e| format!("{e}; or use mute / unmute")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum EqTarget {
    Headphone,
    Mic,
}

impl EqTarget {
    fn name(self) -> &'static str {
        match self {
            Self::Headphone => "headphone",
            Self::Mic => "mic",
        }
    }

    fn setting(self, eq: Eq) -> Setting {
        match self {
            Self::Headphone => Setting::HeadphoneEq(eq),
            Self::Mic => Setting::MicEq(eq),
        }
    }

    fn flat(self) -> &'static Equalizer {
        match self {
            Self::Headphone => &eq::HEADPHONE_DEFAULT,
            Self::Mic => &eq::MIC_DEFAULT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum EqPreset {
    Default,
    Gaming,
    Media,
    Competition,
    Broadcast,
}

impl EqPreset {
    fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Gaming => "gaming",
            Self::Media => "media",
            Self::Competition => "competition",
            Self::Broadcast => "broadcast",
        }
    }

    fn card(self, target: EqTarget) -> Option<&'static Equalizer> {
        match (target, self) {
            (EqTarget::Headphone, Self::Default) => Some(&eq::HEADPHONE_DEFAULT),
            (EqTarget::Headphone, Self::Gaming) => Some(&eq::HEADPHONE_GAMING),
            (EqTarget::Headphone, Self::Media) => Some(&eq::HEADPHONE_MEDIA),
            (EqTarget::Mic, Self::Default) => Some(&eq::MIC_DEFAULT),
            (EqTarget::Mic, Self::Competition) => Some(&eq::MIC_COMPETITION),
            (EqTarget::Mic, Self::Broadcast) => Some(&eq::MIC_BROADCAST),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Toggle {
    On,
    Off,
}

#[derive(Subcommand)]
pub enum EqCommand {
    /// Use one of G Hub's curated cards: default, gaming or media for the headphones;
    /// default, competition or broadcast for the mic
    Preset { name: EqPreset },
    /// Set a graphic card: ten gains in dB, -6 to 6, on the instance's curated frequencies
    Gains {
        #[arg(num_args = 10, allow_negative_numbers = true, value_name = "GAIN")]
        gains: Vec<Gain>,
    },
}

#[derive(Subcommand)]
pub enum ProfileCommand {
    /// List the profiles
    List {
        /// Print the profiles as JSON
        #[arg(long)]
        json: bool,
    },
    /// Apply a profile to the headset now
    Apply { name: String },
    /// Save the headset's current settings as a profile
    Save { name: String },
    /// Delete a profile
    Delete { name: String },
    /// Choose the profile used when no linked app has focus
    Default { name: String },
    /// Switch to a profile when an app has focus
    Link { name: String, app: String },
    /// Stop switching profiles for an app
    Unlink { app: String },
}

#[derive(Subcommand)]
pub enum Command {
    /// List the features the base station offers
    Features,
    /// Show the daemon's view of the headset and settings
    Status {
        /// Print the status as JSON
        #[arg(long)]
        json: bool,
    },
    /// Print a line each time the status changes, until Ctrl-C
    Watch {
        /// Print one JSON object per line
        #[arg(long)]
        json: bool,
    },
    /// Set the sidetone level, 0 to 100
    Sidetone { value: Percent },
    /// Set the Game/Voice balance, 0 to 12 (6 is the centre)
    Balance { value: Balance },
    /// Set the noise gate
    Gate { mode: GateArg },
    /// Set the headset volume, 0 to 100
    Volume { value: Percent },
    /// Set the LED brightness, 0 to 100
    Brightness { value: Percent },
    /// Set Sleep After: never, 15 or 30 minutes
    Sleep { after: SleepAfter },
    /// Set the tones: 0, 1 or 2
    Tones { value: Tones },
    /// Set a stream mix channel's level (0 to 100), or mute / unmute it
    Mix {
        channel: MixChannel,
        action: MixAction,
    },
    /// Set the headphone or mic EQ
    Eq {
        target: EqTarget,
        #[command(subcommand)]
        command: EqCommand,
    },
    /// Manage profiles
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Turn automatic profile switching on or off
    Autoswitch { state: Toggle },
    /// Start ogh-daemon at logon (Windows)
    Autostart {
        #[command(subcommand)]
        command: AutostartCommand,
    },
}

pub fn run(cli: &Cli, client: &mut impl Daemon, out: &mut impl Write) -> Result<(), CliError> {
    match &cli.command {
        Command::Features => features(client, out),
        Command::Status { json } => status(client, out, *json),
        Command::Watch { json } => watch(client, out, *json),
        Command::Sidetone { value } => set(
            client,
            out,
            Setting::Sidetone { percent: value.0 },
            format!("sidetone {}", value.0),
        ),
        Command::Balance { value } => set(
            client,
            out,
            Setting::Balance { value: value.0 },
            format!("balance {}", value.0),
        ),
        Command::Gate { mode } => set(
            client,
            out,
            Setting::NoiseGate(mode.gate()),
            format!("gate {}", output::gate_name(mode.gate())),
        ),
        Command::Volume { value } => set(
            client,
            out,
            Setting::Volume { percent: value.0 },
            format!("volume {}", value.0),
        ),
        Command::Brightness { value } => set(
            client,
            out,
            Setting::LedBrightness { percent: value.0 },
            format!("brightness {}", value.0),
        ),
        Command::Sleep { after } => set(
            client,
            out,
            Setting::SleepAfter { minutes: after.0 },
            format!("sleep {}", after.name()),
        ),
        Command::Tones { value } => set(
            client,
            out,
            Setting::Tones { value: value.0 },
            format!("tones {}", value.0),
        ),
        Command::Mix { channel, action } => mix(client, out, *channel, *action),
        Command::Eq { target, command } => eq_command(client, out, *target, command),
        Command::Profile { command } => profile(client, out, command),
        Command::Autostart { command } => autostart::run(*command, out),
        Command::Autoswitch { state } => {
            let enabled = *state == Toggle::On;
            expect_done(client, Request::SetAutoSwitch { enabled })?;
            let word = if enabled { "on" } else { "off" };
            emit(out, &format!("auto-switch {word}"))
        }
    }
}

fn emit(out: &mut impl Write, text: &str) -> Result<(), CliError> {
    writeln!(out, "{text}")?;
    out.flush()?;
    Ok(())
}

/// Prints a table, or nothing at all when it has no rows.
fn emit_rows(out: &mut impl Write, text: &str) -> Result<(), CliError> {
    if text.is_empty() {
        return Ok(());
    }
    emit(out, text)
}

fn call(client: &mut impl Daemon, request: Request) -> Result<Response, CliError> {
    match client.call(&request)? {
        Response::Error(error) => Err(CliError::Daemon(error)),
        response => Ok(response),
    }
}

fn unexpected() -> CliError {
    CliError::Local("ogh-daemon sent an unexpected reply".to_string())
}

fn expect_done(client: &mut impl Daemon, request: Request) -> Result<(), CliError> {
    match call(client, request)? {
        Response::Done => Ok(()),
        _ => Err(unexpected()),
    }
}

fn read_status(client: &mut impl Daemon) -> Result<Status, CliError> {
    match call(client, Request::Status)? {
        Response::Status(status) => Ok(status),
        _ => Err(unexpected()),
    }
}

fn features(client: &mut impl Daemon, out: &mut impl Write) -> Result<(), CliError> {
    match call(client, Request::Features)? {
        Response::Features(features) => emit_rows(out, &output::features_table(&features)),
        _ => Err(unexpected()),
    }
}

fn status(client: &mut impl Daemon, out: &mut impl Write, json: bool) -> Result<(), CliError> {
    let status = read_status(client)?;
    if json {
        let text = serde_json::to_string_pretty(&status)
            .map_err(|e| CliError::Local(format!("cannot render the status: {e}")))?;
        emit(out, &text)
    } else {
        emit(out, &output::status_human(&status))
    }
}

fn watch(client: &mut impl Daemon, out: &mut impl Write, json: bool) -> Result<(), CliError> {
    for notification in client.subscribe()? {
        let ogh_ipc::Notification::Status(status) = notification?;
        let line = if json {
            serde_json::to_string(&status)
                .map_err(|e| CliError::Local(format!("cannot render the status: {e}")))?
        } else {
            output::status_line(&status)
        };
        emit(out, &line)?;
    }
    Ok(())
}

fn set(
    client: &mut impl Daemon,
    out: &mut impl Write,
    setting: Setting,
    confirmation: String,
) -> Result<(), CliError> {
    expect_done(client, Request::Set { setting })?;
    emit(out, &confirmation)
}

fn mix(
    client: &mut impl Daemon,
    out: &mut impl Write,
    channel: MixChannel,
    action: MixAction,
) -> Result<(), CliError> {
    let status = read_status(client)?;
    let mut stream_mix = status
        .settings
        .profile
        .stream_mix
        .ok_or_else(|| missing_stream_mix(&status.connection))?;
    let target = channel.pick(&mut stream_mix);
    let confirmation = match action {
        MixAction::Level(Percent(percent)) => {
            target.level_percent = percent;
            format!("mix {} {percent}", channel.name())
        }
        MixAction::Mute => {
            target.muted = true;
            format!("mix {} muted", channel.name())
        }
        MixAction::Unmute => {
            target.muted = false;
            format!("mix {} unmuted", channel.name())
        }
    };
    set(client, out, Setting::StreamMix(stream_mix), confirmation)
}

fn missing_stream_mix(connection: &Connection) -> CliError {
    let (kind, message) = match connection {
        Connection::Connected => (ErrorKind::Device, "stream mix is not known yet".to_string()),
        Connection::NotFound => (ErrorKind::NotConnected, output::connection_text(connection)),
        Connection::GhubRunning => (ErrorKind::GhubRunning, output::connection_text(connection)),
        other => (ErrorKind::Device, output::connection_text(other)),
    };
    CliError::Daemon(IpcError { kind, message })
}

fn eq_command(
    client: &mut impl Daemon,
    out: &mut impl Write,
    target: EqTarget,
    command: &EqCommand,
) -> Result<(), CliError> {
    let (card, confirmation) = match command {
        EqCommand::Preset { name } => {
            let card = name.card(target).ok_or_else(|| {
                CliError::Usage(format!(
                    "{} is not a {} EQ preset",
                    name.name(),
                    target.name()
                ))
            })?;
            let confirmation = format!("eq {} preset {}", target.name(), name.name());
            (preset_bands(card), confirmation)
        }
        EqCommand::Gains { gains } => {
            let confirmation = format!(
                "eq {} gains {}",
                target.name(),
                gains
                    .iter()
                    .map(|Gain(gain)| gain.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            (graphic_bands(target.flat(), gains), confirmation)
        }
    };
    set(
        client,
        out,
        target.setting(Eq { bands: card }),
        confirmation,
    )
}

fn preset_bands(card: &Equalizer) -> Vec<EqBand> {
    card.bands
        .iter()
        .map(|band| EqBand {
            freq_hz: band.freq_hz,
            q: band.q,
            gain_db: band.gain_db,
        })
        .collect()
}

fn graphic_bands(frequencies: &Equalizer, gains: &[Gain]) -> Vec<EqBand> {
    frequencies
        .bands
        .iter()
        .zip(gains)
        .map(|(band, Gain(gain_db))| EqBand {
            freq_hz: band.freq_hz,
            q: None,
            gain_db: *gain_db,
        })
        .collect()
}

fn profile(
    client: &mut impl Daemon,
    out: &mut impl Write,
    command: &ProfileCommand,
) -> Result<(), CliError> {
    let (request, confirmation) = match command {
        ProfileCommand::List { json } => return profile_list(client, out, *json),
        ProfileCommand::Apply { name } => (
            Request::ApplyProfile { name: name.clone() },
            format!("applied profile {name}"),
        ),
        ProfileCommand::Save { name } => (
            Request::SaveProfile { name: name.clone() },
            format!("saved profile {name}"),
        ),
        ProfileCommand::Delete { name } => (
            Request::DeleteProfile { name: name.clone() },
            format!("deleted profile {name}"),
        ),
        ProfileCommand::Default { name } => (
            Request::SetDefaultProfile { name: name.clone() },
            format!("default profile {name}"),
        ),
        ProfileCommand::Link { name, app } => (
            Request::LinkApp {
                profile: name.clone(),
                app: app.clone(),
            },
            format!("linked {app} to {name}"),
        ),
        ProfileCommand::Unlink { app } => (
            Request::UnlinkApp { app: app.clone() },
            format!("unlinked {app}"),
        ),
    };
    expect_done(client, request)?;
    emit(out, &confirmation)
}

fn profile_list(
    client: &mut impl Daemon,
    out: &mut impl Write,
    json: bool,
) -> Result<(), CliError> {
    let Response::Profiles(profiles) = call(client, Request::ListProfiles)? else {
        return Err(unexpected());
    };
    if json {
        let text = serde_json::to_string_pretty(&profiles)
            .map_err(|e| CliError::Local(format!("cannot render the profiles: {e}")))?;
        emit(out, &text)
    } else {
        emit_rows(out, &output::profiles_table(&profiles))
    }
}
