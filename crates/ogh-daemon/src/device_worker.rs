use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use ogh_device::{DeviceError, Event, FeatureId, FeatureInfo, Session, Transport};
use ogh_ipc::Connection;
use ogh_protocol::features::advanced_para_eq::{self, EqInstance};
use ogh_protocol::features::{
    auto_sleep, battery, device_info, headset_mix, led_brightness, mixer_audio, noise_gate,
    sidetone, tones, volume,
};
use ogh_protocol::{Message, ParseError};

use crate::snapshot::{DeviceSnapshot, DeviceWrite};

const MAIN_FIRMWARE_ENTITY: u8 = 0;

#[derive(Debug, Clone, PartialEq)]
pub enum DeviceCommand {
    Write { id: u64, writes: Vec<DeviceWrite> },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeviceUpdate {
    Connected {
        features: Vec<FeatureInfo>,
        snapshot: DeviceSnapshot,
    },
    Event(Event),
    Refreshed(DeviceSnapshot),
    WriteDone {
        id: u64,
        result: Result<(), DeviceError>,
    },
    Closed(Connection),
}

#[derive(Debug, Clone, Copy)]
pub struct WorkerConfig {
    /// Wait between attempts to open the device.
    pub reconnect: Duration,
    /// How often an open session checks whether G Hub's agent started.
    pub guard_poll: Duration,
    pub event_poll_ms: i32,
    /// Request timeout for reads the headset answers. A silent headset is
    /// off or docked, so waiting longer would only stall the snapshot.
    pub headset_read_timeout: Duration,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            reconnect: Duration::from_secs(2),
            guard_poll: Duration::from_secs(5),
            event_poll_ms: 100,
            headset_read_timeout: Duration::from_secs(1),
        }
    }
}

/// Production passes `Session::open` and `guard::ghub_agent_running`.
pub fn spawn<T: Transport + Send + 'static>(
    open: impl FnMut() -> Result<Session<T>, DeviceError> + Send + 'static,
    guard: impl Fn() -> Result<bool, DeviceError> + Send + 'static,
    config: WorkerConfig,
    commands: Receiver<DeviceCommand>,
    updates: Sender<DeviceUpdate>,
) -> JoinHandle<()> {
    let worker = Worker {
        open,
        guard,
        config,
        commands,
        updates,
        last_closed: None,
        reason: DeviceError::NotFound,
    };
    thread::spawn(move || worker.run())
}

enum Exit {
    Stop,
    Closed(DeviceError),
}

struct Worker<O, G> {
    open: O,
    guard: G,
    config: WorkerConfig,
    commands: Receiver<DeviceCommand>,
    updates: Sender<DeviceUpdate>,
    /// The last `Closed` sent, so a missing device is reported once.
    last_closed: Option<Connection>,
    /// Why the device is closed now, for writes that arrive meanwhile.
    reason: DeviceError,
}

impl<T, O, G> Worker<O, G>
where
    T: Transport,
    O: FnMut() -> Result<Session<T>, DeviceError>,
    G: Fn() -> Result<bool, DeviceError>,
{
    fn run(mut self) {
        loop {
            let Some(session) = self.wait_for_session() else {
                return;
            };
            match self.serve(session) {
                Exit::Stop => return,
                Exit::Closed(error) => {
                    if !self.report_closed(error) {
                        return;
                    }
                }
            }
        }
    }

    fn wait_for_session(&mut self) -> Option<Session<T>> {
        loop {
            match self.try_open() {
                Ok(session) => return Some(session),
                Err(error) => {
                    if !self.report_closed(error) {
                        return None;
                    }
                }
            }
            if !self.wait_for_reconnect_tick() {
                return None;
            }
        }
    }

    /// Fails closed like `Session::open`: a guard that is true or cannot be
    /// read means the device is not touched.
    fn try_open(&mut self) -> Result<Session<T>, DeviceError> {
        match (self.guard)() {
            Ok(false) => (self.open)(),
            Ok(true) => Err(DeviceError::GHubRunning),
            Err(error) => Err(error),
        }
    }

    /// `false` when the worker should stop.
    fn report_closed(&mut self, error: DeviceError) -> bool {
        let connection = connection_for(&error);
        self.reason = error;
        if self.last_closed.as_ref() == Some(&connection) {
            return true;
        }
        self.last_closed = Some(connection.clone());
        self.send(DeviceUpdate::Closed(connection))
    }

    /// Waits out `reconnect`, refusing writes meanwhile. `false` on shutdown.
    fn wait_for_reconnect_tick(&mut self) -> bool {
        let deadline = Instant::now() + self.config.reconnect;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.commands.recv_timeout(remaining) {
                Ok(DeviceCommand::Shutdown) | Err(RecvTimeoutError::Disconnected) => return false,
                Ok(DeviceCommand::Write { id, .. }) => {
                    let result = Err(self.reason.clone());
                    if !self.send(DeviceUpdate::WriteDone { id, result }) {
                        return false;
                    }
                }
                Err(RecvTimeoutError::Timeout) => return true,
            }
        }
    }

    fn serve(&mut self, mut session: Session<T>) -> Exit {
        let mut snapshot = DeviceSnapshot::default();
        if let Err(error) = read_snapshot(&mut session, &mut snapshot, &self.config) {
            return Exit::Closed(error);
        }
        self.last_closed = None;
        let connected = DeviceUpdate::Connected {
            features: session.features().to_vec(),
            snapshot: snapshot.clone(),
        };
        if !self.send(connected) {
            return Exit::Stop;
        }
        let mut last_guard = Instant::now();
        loop {
            if let Some(exit) = self.run_commands(&mut session, &mut snapshot) {
                return exit;
            }
            if let Some(exit) = self.forward_event(&mut session, &mut snapshot) {
                return exit;
            }
            if last_guard.elapsed() >= self.config.guard_poll {
                last_guard = Instant::now();
                if matches!((self.guard)(), Ok(true)) {
                    return Exit::Closed(DeviceError::GHubRunning);
                }
            }
        }
    }

    fn run_commands(
        &mut self,
        session: &mut Session<T>,
        snapshot: &mut DeviceSnapshot,
    ) -> Option<Exit> {
        loop {
            match self.commands.try_recv() {
                Ok(DeviceCommand::Write { id, writes }) => {
                    let result = apply_writes(session, snapshot, &writes);
                    let fatal = result.as_ref().err().filter(|e| ends_session(e)).cloned();
                    if !self.send(DeviceUpdate::WriteDone { id, result }) {
                        return Some(Exit::Stop);
                    }
                    if let Some(error) = fatal {
                        return Some(Exit::Closed(error));
                    }
                }
                Ok(DeviceCommand::Shutdown) | Err(TryRecvError::Disconnected) => {
                    return Some(Exit::Stop);
                }
                Err(TryRecvError::Empty) => return None,
            }
        }
    }

    fn forward_event(
        &mut self,
        session: &mut Session<T>,
        snapshot: &mut DeviceSnapshot,
    ) -> Option<Exit> {
        let event = match session.next_event(self.config.event_poll_ms) {
            Ok(Some(event)) => event,
            Ok(None) => return None,
            Err(error) => return Some(Exit::Closed(error)),
        };
        let was_available = snapshot.headset_available == Some(true);
        snapshot.apply_event(&event);
        let became_available = !was_available && snapshot.headset_available == Some(true);
        if !self.send(DeviceUpdate::Event(event)) {
            return Some(Exit::Stop);
        }
        if !became_available {
            return None;
        }
        if let Err(error) = read_headset_side(session, snapshot, &self.config) {
            return Some(Exit::Closed(error));
        }
        if !self.send(DeviceUpdate::Refreshed(snapshot.clone())) {
            return Some(Exit::Stop);
        }
        None
    }

    /// `false` once the receiver is gone.
    fn send(&self, update: DeviceUpdate) -> bool {
        self.updates.send(update).is_ok()
    }
}

fn connection_for(error: &DeviceError) -> Connection {
    match error {
        DeviceError::NotFound => Connection::NotFound,
        DeviceError::GHubRunning => Connection::GhubRunning,
        DeviceError::AccessDenied(message) => Connection::AccessDenied {
            message: message.clone(),
        },
        other => Connection::Error {
            message: other.to_string(),
        },
    }
}

/// Errors that mean the device is gone or unusable, as opposed to one
/// request failing.
fn ends_session(error: &DeviceError) -> bool {
    matches!(
        error,
        DeviceError::Disconnected | DeviceError::Io(_) | DeviceError::AccessDenied(_)
    )
}

fn apply_writes(
    session: &mut Session<impl Transport>,
    snapshot: &mut DeviceSnapshot,
    writes: &[DeviceWrite],
) -> Result<(), DeviceError> {
    for write in writes {
        send_write(session, write)?;
        snapshot.record_write(write);
    }
    Ok(())
}

fn send_write(
    session: &mut Session<impl Transport>,
    write: &DeviceWrite,
) -> Result<(), DeviceError> {
    match write {
        DeviceWrite::Sidetone(level) => session.try_call(FeatureId::HEADSET_SIDETONE, |index| {
            sidetone::set_request(index, *level)
        }),
        DeviceWrite::Balance(balance) => session.try_call(FeatureId::HEADSET_MIX, |index| {
            headset_mix::set_request(index, *balance)
        }),
        DeviceWrite::NoiseGate(gate) => session.try_call(FeatureId::MIXER_MIC, |index| {
            noise_gate::set_request(index, *gate)
        }),
        DeviceWrite::Eq(instance, equalizer) => session
            .try_call(FeatureId::ADVANCED_PARA_EQ, |index| {
                advanced_para_eq::set_custom_request(index, *instance, equalizer)
            }),
        DeviceWrite::StreamMix(mix) => session.try_call(FeatureId::MIXER_AUDIO, |index| {
            mixer_audio::set_request(index, mix)
        }),
        DeviceWrite::Volume(level) => session.try_call(FeatureId::VOLUME_SET, |index| {
            volume::set_request(index, *level)
        }),
        DeviceWrite::LedBrightness(percent) => session
            .try_call(FeatureId::LED_BRIGHTNESS, |index| {
                led_brightness::set_request(index, *percent)
            }),
        DeviceWrite::SleepAfter(sleep_after) => session.try_call(FeatureId::AUTO_SLEEP, |index| {
            auto_sleep::set_request(index, *sleep_after)
        }),
        DeviceWrite::Tones(setting) => session.try_call(FeatureId::HEADSET_TONES, |index| {
            tones::set_request(index, *setting)
        }),
    }
    .map(drop)
}

/// A read that fails for any reason but a lost device leaves `None`: the
/// feature may be missing, the headset away, or the reply unusable.
fn read_field<V>(
    session: &mut Session<impl Transport>,
    feature: FeatureId,
    build: impl FnOnce(u8) -> Message,
    parse: impl FnOnce(&[u8]) -> Result<V, ParseError>,
) -> Result<Option<V>, DeviceError> {
    match session.call(feature, build) {
        Ok(params) => Ok(parse(&params).ok()),
        Err(error) if ends_session(&error) => Err(error),
        Err(_) => Ok(None),
    }
}

fn read_snapshot(
    session: &mut Session<impl Transport>,
    snapshot: &mut DeviceSnapshot,
    config: &WorkerConfig,
) -> Result<(), DeviceError> {
    snapshot.headset_available = match session.headset_available() {
        Ok(available) => Some(available),
        Err(error) if ends_session(&error) => return Err(error),
        Err(_) => None,
    };
    read_headset_side(session, snapshot, config)?;
    snapshot.headphone_eq = read_eq(session, EqInstance::Headphone)?;
    snapshot.mic_eq = read_eq(session, EqInstance::Mic)?;
    snapshot.stream_mix = read_field(
        session,
        FeatureId::MIXER_AUDIO,
        mixer_audio::get_settings_request,
        mixer_audio::parse_settings_reply,
    )?;
    snapshot.volume = read_field(
        session,
        FeatureId::VOLUME_SET,
        volume::get_request,
        volume::parse,
    )?;
    snapshot.led_brightness = read_field(
        session,
        FeatureId::LED_BRIGHTNESS,
        led_brightness::get_request,
        led_brightness::parse,
    )?;
    snapshot.auto_sleep = read_field(
        session,
        FeatureId::AUTO_SLEEP,
        auto_sleep::get_request,
        auto_sleep::parse,
    )?;
    snapshot.tones = read_field(
        session,
        FeatureId::HEADSET_TONES,
        tones::get_request,
        tones::parse,
    )?;
    snapshot.firmware = read_field(
        session,
        FeatureId::DEVICE_INFO,
        |index| device_info::firmware_version_request(index, MAIN_FIRMWARE_ENTITY),
        device_info::parse_firmware_version,
    )?
    .map(|version| format!("{}.{}.{}", version.major, version.minor, version.build));
    Ok(())
}

fn read_eq(
    session: &mut Session<impl Transport>,
    instance: EqInstance,
) -> Result<Option<advanced_para_eq::Equalizer>, DeviceError> {
    read_field(
        session,
        FeatureId::ADVANCED_PARA_EQ,
        |index| advanced_para_eq::get_custom_request(index, instance),
        advanced_para_eq::parse_custom_reply,
    )
}

/// Battery, sidetone, balance and noise gate are answered by the headset, so
/// they run on the short timeout. A value that cannot be read keeps what the
/// snapshot already held.
fn read_headset_side(
    session: &mut Session<impl Transport>,
    snapshot: &mut DeviceSnapshot,
    config: &WorkerConfig,
) -> Result<(), DeviceError> {
    let full_timeout = session.request_timeout();
    session.set_request_timeout(full_timeout.min(config.headset_read_timeout));
    let result = read_headset_fields(session, snapshot);
    session.set_request_timeout(full_timeout);
    result
}

fn read_headset_fields(
    session: &mut Session<impl Transport>,
    snapshot: &mut DeviceSnapshot,
) -> Result<(), DeviceError> {
    let battery = read_field(
        session,
        FeatureId::BATTERY_SOC,
        battery::get_request,
        battery::parse,
    )?;
    let sidetone = read_field(
        session,
        FeatureId::HEADSET_SIDETONE,
        sidetone::get_request,
        sidetone::parse_level,
    )?;
    let balance = read_field(
        session,
        FeatureId::HEADSET_MIX,
        headset_mix::get_request,
        headset_mix::parse_balance,
    )?;
    let noise_gate = read_field(
        session,
        FeatureId::MIXER_MIC,
        noise_gate::current_request,
        noise_gate::parse_current,
    )?;
    snapshot.battery = battery.or(snapshot.battery);
    snapshot.sidetone = sidetone.or(snapshot.sidetone);
    snapshot.balance = balance.or(snapshot.balance);
    snapshot.noise_gate = noise_gate.or(snapshot.noise_gate);
    Ok(())
}
