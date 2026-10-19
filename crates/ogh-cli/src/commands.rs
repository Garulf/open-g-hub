use std::io::Write;
use std::num::ParseIntError;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::{Parser, Subcommand};
use ogh_device::{DeviceError, FeatureId, Session, Transport};
use ogh_protocol::features::battery::{self, Battery};
use ogh_protocol::features::device_info;
use ogh_protocol::features::headset_mix::{self, GameVoiceBalance};
use ogh_protocol::features::noise_gate::{self, NoiseGate};
use ogh_protocol::features::sidetone::{self, Sidetone};
use ogh_protocol::{Message, ParseError};

use crate::error::CliError;
use crate::output::{self, Status};

const POLL_MS: i32 = 500;
const AWAY_READ_TIMEOUT: Duration = Duration::from_millis(1000);
const MAX_PERCENT: u8 = 100;
const MAIN_FIRMWARE_ENTITY: u8 = 0;

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
        let value: u8 = text
            .parse()
            .map_err(|e: ParseIntError| format!("{e}; expected a whole number from 0 to 100"))?;
        if value > MAX_PERCENT {
            return Err(format!("{value} is above {MAX_PERCENT}"));
        }
        Ok(Self(value))
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// List the features the base station offers
    Features,
    /// Show headset state, battery and settings
    Status {
        /// Print one JSON object
        #[arg(long)]
        json: bool,
    },
    /// Print events as they arrive until Ctrl-C
    Watch {
        /// Print one JSON object per line
        #[arg(long)]
        json: bool,
    },
    /// Set the sidetone level, 0 to 100
    Sidetone { value: Percent },
}

/// What a run may be stopped by besides an error.
#[derive(Default)]
pub struct RunOptions {
    /// Set to end `watch` after the event being waited for.
    pub stop: Arc<AtomicBool>,
    /// End `watch` after this many events.
    pub max_events: Option<usize>,
}

pub fn run<T: Transport>(
    command: &Command,
    session: &mut Session<T>,
    out: &mut impl Write,
    options: &RunOptions,
) -> Result<(), CliError> {
    match command {
        Command::Features => emit(out, &output::features_table(session.features())),
        Command::Status { json } => status(session, out, *json),
        Command::Watch { json } => watch(session, out, *json, options),
        Command::Sidetone { value } => set_sidetone(session, out, *value),
    }
}

fn emit(out: &mut impl Write, text: &str) -> Result<(), CliError> {
    writeln!(out, "{text}")?;
    out.flush()?;
    Ok(())
}

fn status<T: Transport>(
    session: &mut Session<T>,
    out: &mut impl Write,
    json: bool,
) -> Result<(), CliError> {
    let status = read_status(session)?;
    if json {
        emit(out, &status.to_json().to_string())
    } else {
        emit(out, &status.to_human())
    }
}

fn read_status<T: Transport>(session: &mut Session<T>) -> Result<Status, DeviceError> {
    let mut in_use = session.headset_available()?;
    let battery = headset_side(session, &mut in_use, read_battery)?;
    let sidetone = headset_side(session, &mut in_use, read_sidetone)?;
    let balance = headset_side(session, &mut in_use, read_balance)?;
    let noise_gate = headset_side(session, &mut in_use, read_noise_gate)?;
    let firmware = read_firmware(session)?;
    Ok(Status {
        in_use,
        battery,
        sidetone,
        balance,
        noise_gate,
        firmware,
    })
}

/// A read that gets no answer means "no value", not a failure of the
/// command, when the headset is not in use. The device saying the headset is
/// not connected always does, and ends `in_use`: the headset just went away.
/// A silent headset is given `AWAY_READ_TIMEOUT` instead of the full request
/// timeout.
fn headset_side<T: Transport, V>(
    session: &mut Session<T>,
    in_use: &mut bool,
    read: impl FnOnce(&mut Session<T>) -> Result<V, DeviceError>,
) -> Result<Option<V>, DeviceError> {
    let full_timeout = session.request_timeout();
    if !*in_use {
        session.set_request_timeout(full_timeout.min(AWAY_READ_TIMEOUT));
    }
    let result = read(session);
    session.set_request_timeout(full_timeout);
    match result {
        Ok(value) => Ok(Some(value)),
        Err(DeviceError::HeadsetDisconnected) => {
            *in_use = false;
            Ok(None)
        }
        Err(DeviceError::Timeout) if !*in_use => Ok(None),
        Err(error) => Err(error),
    }
}

fn read<T: Transport, V>(
    session: &mut Session<T>,
    feature: FeatureId,
    build: impl FnOnce(u8) -> Message,
    parse: impl FnOnce(&[u8]) -> Result<V, ParseError>,
) -> Result<V, DeviceError> {
    let params = session.call(feature, build)?;
    parse(&params).map_err(|e| DeviceError::Io(format!("unexpected reply: {e}")))
}

fn read_battery<T: Transport>(session: &mut Session<T>) -> Result<Battery, DeviceError> {
    read(
        session,
        FeatureId::BATTERY_SOC,
        battery::get_request,
        battery::parse,
    )
}

fn read_sidetone<T: Transport>(session: &mut Session<T>) -> Result<Sidetone, DeviceError> {
    read(
        session,
        FeatureId::HEADSET_SIDETONE,
        sidetone::get_request,
        sidetone::parse_level,
    )
}

fn read_balance<T: Transport>(session: &mut Session<T>) -> Result<GameVoiceBalance, DeviceError> {
    read(
        session,
        FeatureId::HEADSET_MIX,
        headset_mix::get_request,
        headset_mix::parse_balance,
    )
}

fn read_noise_gate<T: Transport>(session: &mut Session<T>) -> Result<NoiseGate, DeviceError> {
    read(
        session,
        FeatureId::MIXER_MIC,
        noise_gate::current_request,
        noise_gate::parse_current,
    )
}

fn read_firmware<T: Transport>(session: &mut Session<T>) -> Result<String, DeviceError> {
    let version = read(
        session,
        FeatureId::DEVICE_INFO,
        |index| device_info::firmware_version_request(index, MAIN_FIRMWARE_ENTITY),
        device_info::parse_firmware_version,
    )?;
    Ok(format!(
        "{}.{}.{}",
        version.major, version.minor, version.build
    ))
}

fn watch<T: Transport>(
    session: &mut Session<T>,
    out: &mut impl Write,
    json: bool,
    options: &RunOptions,
) -> Result<(), CliError> {
    let mut printed = 0;
    while !options.stop.load(Ordering::SeqCst) && options.max_events.is_none_or(|max| printed < max)
    {
        let Some(event) = session.next_event(POLL_MS)? else {
            continue;
        };
        let line = if json {
            output::event_json(&event).to_string()
        } else {
            output::event_line(&event)
        };
        emit(out, &line)?;
        printed += 1;
    }
    Ok(())
}

fn set_sidetone<T: Transport>(
    session: &mut Session<T>,
    out: &mut impl Write,
    Percent(percent): Percent,
) -> Result<(), CliError> {
    let feature = FeatureId::HEADSET_SIDETONE;
    let index = session
        .features()
        .iter()
        .find(|f| f.id == feature)
        .map(|f| f.index)
        .ok_or(DeviceError::FeatureMissing(feature))?;
    let request = sidetone::set_request(index, Sidetone::from_percent(percent))
        .map_err(|e| DeviceError::Io(format!("cannot encode the sidetone level: {e}")))?;
    session.call(feature, |_| request)?;
    let level = read_sidetone(session)?;
    emit(out, &format!("sidetone: {}%", level.to_percent()))
}
