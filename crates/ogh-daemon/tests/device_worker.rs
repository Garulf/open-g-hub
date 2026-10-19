use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use ogh_daemon::device_worker::{DeviceCommand, DeviceUpdate, WorkerConfig, spawn};
use ogh_daemon::snapshot::DeviceWrite;
use ogh_device::testing::{LAUNCH_OPEN_REPLIES, ScriptedTransport, frame_from_hex};
use ogh_device::{DeviceError, Event, FeatureId, FeatureInfo, Session, Transport};
use ogh_ipc::Connection;
use ogh_protocol::features::advanced_para_eq::{
    EqInstance, HEADPHONE_GAMING, MIC_BROADCAST, set_custom_request,
};
use ogh_protocol::features::auto_sleep::{AutoSleep, SleepAfter};
use ogh_protocol::features::battery::{Battery, ChargeStatus};
use ogh_protocol::features::headset_mix::GameVoiceBalance;
use ogh_protocol::features::mixer_audio::{
    MixChannel, MixLevel, PINNED_INPUT, StreamMix, set_request as set_mix_request,
};
use ogh_protocol::features::noise_gate::NoiseGate;
use ogh_protocol::features::sidetone::Sidetone;
use ogh_protocol::features::tones::Tones;
use ogh_protocol::features::volume::{Volume, VolumeLevel};
use ogh_protocol::{A50X_PREFIX, Decoded, FRAME_LEN, GHUB_SW_ID, Message, decode, encode};

const REQUEST_TIMEOUT: Duration = Duration::from_millis(150);
const WAIT: Duration = Duration::from_secs(3);
const QUIET: Duration = Duration::from_millis(200);
const DISCOVERY_WRITES: usize = 4;

const CONFIG: WorkerConfig = WorkerConfig {
    reconnect: Duration::from_millis(20),
    guard_poll: Duration::from_millis(50),
    event_poll_ms: 10,
    headset_read_timeout: Duration::from_millis(30),
};

fn index_of(id: FeatureId) -> u8 {
    static TABLE: OnceLock<Vec<FeatureInfo>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        Session::with_transport(ScriptedTransport::with_launch_discovery())
            .expect("launch discovery opens")
            .features()
            .to_vec()
    });
    table
        .iter()
        .find(|feature| feature.id == id)
        .unwrap_or_else(|| panic!("{id:?} is in the launch table"))
        .index
}

#[derive(Clone)]
enum Answer {
    Params(Vec<u8>),
    ByRequest(fn(&[u8]) -> Vec<u8>),
    Error(u8),
    Silent,
}

struct State {
    discovery_writes: usize,
    answers: HashMap<(FeatureId, u8), Answer>,
    inbox: VecDeque<[u8; FRAME_LEN]>,
    writes: Vec<Message>,
    failed: bool,
}

/// A device that answers requests from a table instead of a fixed script, so
/// the tests do not depend on the order the worker reads in.
#[derive(Clone)]
struct FakeDevice {
    state: Arc<Mutex<State>>,
}

fn mix() -> StreamMix {
    let channel = |level, muted| MixChannel {
        level: MixLevel(level),
        muted,
    };
    StreamMix {
        stream: channel(16, false),
        inputs: [
            PINNED_INPUT,
            channel(20, false),
            PINNED_INPUT,
            channel(10, false),
        ],
    }
}

fn eq_reply(request: &[u8]) -> Vec<u8> {
    let instance = EqInstance::from_byte(request[0]).expect("a known EQ instance");
    let equalizer = match instance {
        EqInstance::Headphone => HEADPHONE_GAMING,
        EqInstance::Mic => MIC_BROADCAST,
    };
    set_custom_request(0, instance, &equalizer).unwrap().params[2..].to_vec()
}

fn mix_reply(_: &[u8]) -> Vec<u8> {
    set_mix_request(0, &mix()).unwrap().params[1..].to_vec()
}

impl FakeDevice {
    /// Answers every read the worker makes. The headset is docked.
    fn new() -> Self {
        let device = Self {
            state: Arc::new(Mutex::new(State {
                discovery_writes: 0,
                answers: HashMap::new(),
                inbox: VecDeque::new(),
                writes: Vec::new(),
                failed: false,
            })),
        };
        device.answer(FeatureId::MIC_TEST, 6, Answer::Params(vec![5]));
        device.answer(FeatureId::BATTERY_SOC, 0, Answer::Params(vec![80, 80, 1]));
        device.answer(
            FeatureId::HEADSET_SIDETONE,
            0,
            Answer::Params(vec![0, 0, 0, 3]),
        );
        device.answer(FeatureId::HEADSET_MIX, 0, Answer::Params(vec![6]));
        device.answer(FeatureId::MIXER_MIC, 1, Answer::Params(vec![1]));
        device.answer(FeatureId::ADVANCED_PARA_EQ, 1, Answer::ByRequest(eq_reply));
        device.answer(FeatureId::MIXER_AUDIO, 5, Answer::ByRequest(mix_reply));
        device.answer(FeatureId::VOLUME_SET, 0, Answer::Params(vec![10, 0, 10]));
        device.answer(FeatureId::LED_BRIGHTNESS, 0, Answer::Params(vec![100]));
        device.answer(FeatureId::AUTO_SLEEP, 0, Answer::Params(vec![0x3c, 15]));
        device.answer(FeatureId::HEADSET_TONES, 0, Answer::Params(vec![1]));
        device.answer(
            FeatureId::DEVICE_INFO,
            1,
            Answer::Params(vec![1, 2, 0, 18, 4, b'm', b'a', b'i', b'n', 0]),
        );
        device.answer(FeatureId::HEADSET_SIDETONE, 1, Answer::Params(vec![]));
        device.answer(FeatureId::HEADSET_MIX, 1, Answer::Params(vec![]));
        device.answer(FeatureId::MIXER_MIC, 2, Answer::Params(vec![]));
        device
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn answer(&self, id: FeatureId, function: u8, answer: Answer) {
        self.lock().answers.insert((id, function), answer);
    }

    fn silence_headset_side(&self) {
        for (id, function) in [
            (FeatureId::BATTERY_SOC, 0),
            (FeatureId::HEADSET_SIDETONE, 0),
            (FeatureId::HEADSET_MIX, 0),
            (FeatureId::MIXER_MIC, 1),
        ] {
            self.answer(id, function, Answer::Silent);
        }
    }

    fn push_event(&self, id: FeatureId, params: &[u8]) {
        let msg = Message {
            feature_index: index_of(id),
            function: 0,
            sw_id: 0,
            params: params.to_vec(),
        };
        let frame = encode(A50X_PREFIX, 0, &msg).unwrap();
        self.lock().inbox.push_back(frame);
    }

    fn fail(&self) {
        self.lock().failed = true;
    }

    /// Requests written to the device so far, discovery included.
    fn written(&self) -> Vec<Message> {
        self.lock().writes.clone()
    }

    fn reply_for(state: &State, request: &Message) -> Option<[u8; FRAME_LEN]> {
        let id = state
            .answers
            .keys()
            .map(|(id, _)| *id)
            .find(|id| index_of(*id) == request.feature_index)?;
        let params = match state.answers.get(&(id, request.function))? {
            Answer::Params(params) => params.clone(),
            Answer::ByRequest(build) => build(&request.params),
            Answer::Silent => return None,
            Answer::Error(code) => {
                let body = [
                    0xff,
                    request.feature_index,
                    request.function << 4 | request.sw_id,
                    *code,
                ];
                let mut frame = [0u8; FRAME_LEN];
                frame[..2].copy_from_slice(A50X_PREFIX);
                frame[2] = 1 + body.len() as u8;
                frame[4..4 + body.len()].copy_from_slice(&body);
                return Some(frame);
            }
        };
        let msg = Message {
            feature_index: request.feature_index,
            function: request.function,
            sw_id: request.sw_id,
            params,
        };
        Some(encode(A50X_PREFIX, 0, &msg).unwrap())
    }
}

impl Transport for FakeDevice {
    fn write(&mut self, frame: &[u8; FRAME_LEN]) -> Result<(), DeviceError> {
        let mut state = self.lock();
        if state.failed {
            return Err(DeviceError::Disconnected);
        }
        let Ok(Decoded::Reply { msg, .. }) = decode(A50X_PREFIX, frame) else {
            panic!("the worker wrote something that is not a request");
        };
        assert_eq!(msg.sw_id, GHUB_SW_ID);
        state.writes.push(msg.clone());
        let reply = if state.discovery_writes < DISCOVERY_WRITES {
            let launch = LAUNCH_OPEN_REPLIES[state.discovery_writes];
            state.discovery_writes += 1;
            Some(frame_from_hex(launch))
        } else {
            Self::reply_for(&state, &msg)
        };
        state.inbox.extend(reply);
        Ok(())
    }

    fn read(&mut self, timeout_ms: i32) -> Result<Option<[u8; FRAME_LEN]>, DeviceError> {
        let deadline =
            Instant::now() + Duration::from_millis(u64::try_from(timeout_ms).unwrap_or(0));
        loop {
            {
                let mut state = self.lock();
                if state.failed {
                    return Err(DeviceError::Disconnected);
                }
                if let Some(frame) = state.inbox.pop_front() {
                    return Ok(Some(frame));
                }
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
}

fn session_on(device: &FakeDevice) -> Result<Session<FakeDevice>, DeviceError> {
    Session::with_transport_and_timeout(device.clone(), REQUEST_TIMEOUT)
}

struct Harness {
    commands: Sender<DeviceCommand>,
    updates: Receiver<DeviceUpdate>,
    handle: JoinHandle<()>,
    opens: Arc<AtomicUsize>,
}

impl Harness {
    fn start(devices: Vec<FakeDevice>) -> Self {
        Self::start_with_guard(devices, || Ok(false))
    }

    fn start_with_guard(
        devices: Vec<FakeDevice>,
        guard: impl Fn() -> Result<bool, DeviceError> + Send + 'static,
    ) -> Self {
        let (commands, command_rx) = unbounded();
        let (update_tx, updates) = unbounded();
        let opens = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&opens);
        let mut devices: VecDeque<FakeDevice> = devices.into();
        let handle = spawn(
            move || {
                counter.fetch_add(1, Ordering::SeqCst);
                match devices.pop_front() {
                    Some(device) => session_on(&device),
                    None => Err(DeviceError::NotFound),
                }
            },
            guard,
            CONFIG,
            command_rx,
            update_tx,
        );
        Self {
            commands,
            updates,
            handle,
            opens,
        }
    }

    fn next(&self) -> DeviceUpdate {
        self.updates
            .recv_timeout(WAIT)
            .expect("the worker sent an update in time")
    }

    fn assert_quiet(&self) {
        assert_eq!(
            self.updates.recv_timeout(QUIET),
            Err(crossbeam_channel::RecvTimeoutError::Timeout)
        );
    }

    fn connected(&self) -> ogh_daemon::snapshot::DeviceSnapshot {
        match self.next() {
            DeviceUpdate::Connected { snapshot, .. } => snapshot,
            other => panic!("expected Connected, got {other:?}"),
        }
    }

    fn finish(self) {
        self.commands.send(DeviceCommand::Shutdown).unwrap();
        self.handle.join().expect("the worker thread ended cleanly");
    }
}

fn index_pair(msg: &Message) -> (u8, u8) {
    (msg.feature_index, msg.function)
}

#[test]
fn connect_reads_a_snapshot() {
    let device = FakeDevice::new();
    device.answer(FeatureId::MIC_TEST, 6, Answer::Params(vec![0]));
    let harness = Harness::start(vec![device]);

    let DeviceUpdate::Connected { features, snapshot } = harness.next() else {
        panic!("expected Connected first");
    };

    assert!(features.iter().any(|f| f.id == FeatureId::HEADSET_SIDETONE));
    assert_eq!(snapshot.headset_available, Some(true));
    assert_eq!(
        snapshot.battery,
        Some(Battery {
            level_a: 80,
            level_b: 80,
            status: ChargeStatus::Charging
        })
    );
    assert_eq!(snapshot.sidetone, Some(Sidetone(3)));
    assert_eq!(snapshot.balance, Some(GameVoiceBalance(6)));
    assert_eq!(snapshot.noise_gate, Some(NoiseGate::Home));
    assert_eq!(snapshot.headphone_eq, Some(HEADPHONE_GAMING));
    assert_eq!(snapshot.mic_eq, Some(MIC_BROADCAST));
    assert_eq!(snapshot.stream_mix, Some(mix()));
    assert_eq!(
        snapshot.volume,
        Some(Volume {
            scaled: 10,
            muted: false,
            level: VolumeLevel(10)
        })
    );
    assert_eq!(snapshot.led_brightness, Some(100));
    assert_eq!(
        snapshot.auto_sleep,
        Some(AutoSleep {
            prefix: 0x3c,
            sleep_after: SleepAfter(15)
        })
    );
    assert_eq!(snapshot.tones, Some(Tones(1)));
    assert_eq!(snapshot.firmware.as_deref(), Some("1.2.18"));
    harness.finish();
}

#[test]
fn headset_off_reads_are_none_not_errors() {
    let device = FakeDevice::new();
    device.silence_headset_side();
    let harness = Harness::start(vec![device]);

    let snapshot = harness.connected();

    assert_eq!(snapshot.headset_available, Some(false));
    assert_eq!(snapshot.battery, None);
    assert_eq!(snapshot.sidetone, None);
    assert_eq!(snapshot.balance, None);
    assert_eq!(snapshot.noise_gate, None);
    assert_eq!(snapshot.stream_mix, Some(mix()));
    assert_eq!(snapshot.headphone_eq, Some(HEADPHONE_GAMING));
    harness.finish();
}

#[test]
fn a_rejected_read_leaves_its_field_empty() {
    let device = FakeDevice::new();
    device.answer(FeatureId::VOLUME_SET, 0, Answer::Error(5));
    device.answer(FeatureId::HEADSET_TONES, 0, Answer::Params(vec![]));
    let harness = Harness::start(vec![device]);

    let snapshot = harness.connected();

    assert_eq!(snapshot.volume, None);
    assert_eq!(snapshot.tones, None);
    assert_eq!(snapshot.led_brightness, Some(100));
    harness.finish();
}

#[test]
fn unplug_then_replug_reconnects() {
    let first = FakeDevice::new();
    let second = FakeDevice::new();
    second.answer(
        FeatureId::HEADSET_SIDETONE,
        0,
        Answer::Params(vec![0, 0, 0, 5]),
    );
    let harness = Harness::start(vec![first.clone(), second]);
    harness.connected();

    first.fail();

    assert!(matches!(
        harness.next(),
        DeviceUpdate::Closed(Connection::Error { .. })
    ));
    let snapshot = harness.connected();
    assert_eq!(snapshot.sidetone, Some(Sidetone(5)));
    assert_eq!(harness.opens.load(Ordering::SeqCst), 2);
    harness.finish();
}

#[test]
fn ghub_appearing_closes_the_session() {
    let ghub = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&ghub);
    let harness =
        Harness::start_with_guard(vec![FakeDevice::new(), FakeDevice::new()], move || {
            Ok(flag.load(Ordering::SeqCst))
        });
    harness.connected();
    assert_eq!(harness.opens.load(Ordering::SeqCst), 1);

    let started = Instant::now();
    ghub.store(true, Ordering::SeqCst);

    assert_eq!(
        harness.next(),
        DeviceUpdate::Closed(Connection::GhubRunning)
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    harness.assert_quiet();
    assert_eq!(harness.opens.load(Ordering::SeqCst), 1);

    ghub.store(false, Ordering::SeqCst);
    harness.connected();
    assert_eq!(harness.opens.load(Ordering::SeqCst), 2);
    harness.finish();
}

#[test]
fn ghub_already_running_is_reported_once_without_opening() {
    let harness = Harness::start_with_guard(vec![FakeDevice::new()], || Ok(true));

    assert_eq!(
        harness.next(),
        DeviceUpdate::Closed(Connection::GhubRunning)
    );
    harness.assert_quiet();
    assert_eq!(harness.opens.load(Ordering::SeqCst), 0);
    harness.finish();
}

#[test]
fn an_unreadable_process_list_keeps_a_session_open() {
    let broken = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&broken);
    let harness = Harness::start_with_guard(vec![FakeDevice::new()], move || {
        if flag.load(Ordering::SeqCst) {
            Err(DeviceError::Io("no process list".into()))
        } else {
            Ok(false)
        }
    });
    harness.connected();

    broken.store(true, Ordering::SeqCst);

    harness.assert_quiet();
    harness.finish();
}

#[test]
fn a_missing_device_is_reported_once() {
    let harness = Harness::start(Vec::new());

    assert_eq!(harness.next(), DeviceUpdate::Closed(Connection::NotFound));
    harness.assert_quiet();
    assert!(harness.opens.load(Ordering::SeqCst) > 1);
    harness.finish();
}

#[test]
fn writes_while_closed_are_answered() {
    let harness = Harness::start(Vec::new());
    assert_eq!(harness.next(), DeviceUpdate::Closed(Connection::NotFound));

    harness
        .commands
        .send(DeviceCommand::Write {
            id: 9,
            writes: vec![DeviceWrite::Sidetone(Sidetone(1))],
        })
        .unwrap();

    assert_eq!(
        harness.next(),
        DeviceUpdate::WriteDone {
            id: 9,
            result: Err(DeviceError::NotFound)
        }
    );
    harness.finish();
}

#[test]
fn write_batch_stops_at_first_error() {
    let device = FakeDevice::new();
    device.answer(FeatureId::HEADSET_MIX, 1, Answer::Error(5));
    let harness = Harness::start(vec![device.clone()]);
    harness.connected();
    let before = device.written().len();

    harness
        .commands
        .send(DeviceCommand::Write {
            id: 4,
            writes: vec![
                DeviceWrite::Sidetone(Sidetone(2)),
                DeviceWrite::Balance(GameVoiceBalance(3)),
                DeviceWrite::NoiseGate(NoiseGate::Night),
            ],
        })
        .unwrap();

    let balance_index = index_of(FeatureId::HEADSET_MIX);
    assert_eq!(
        harness.next(),
        DeviceUpdate::WriteDone {
            id: 4,
            result: Err(DeviceError::Rejected {
                feature_index: balance_index,
                code: 5
            })
        }
    );
    let sent: Vec<_> = device.written()[before..].iter().map(index_pair).collect();
    assert_eq!(
        sent,
        vec![
            (index_of(FeatureId::HEADSET_SIDETONE), 1),
            (balance_index, 1)
        ]
    );
    harness.finish();
}

#[test]
fn a_successful_batch_reports_ok_once() {
    let device = FakeDevice::new();
    let harness = Harness::start(vec![device.clone()]);
    harness.connected();

    harness
        .commands
        .send(DeviceCommand::Write {
            id: 1,
            writes: vec![
                DeviceWrite::Sidetone(Sidetone(2)),
                DeviceWrite::NoiseGate(NoiseGate::Night),
            ],
        })
        .unwrap();

    assert_eq!(
        harness.next(),
        DeviceUpdate::WriteDone {
            id: 1,
            result: Ok(())
        }
    );
    harness.assert_quiet();
    harness.finish();
}

#[test]
fn an_out_of_range_write_sends_nothing() {
    let device = FakeDevice::new();
    let harness = Harness::start(vec![device.clone()]);
    harness.connected();
    let before = device.written().len();

    harness
        .commands
        .send(DeviceCommand::Write {
            id: 2,
            writes: vec![DeviceWrite::Balance(GameVoiceBalance(99))],
        })
        .unwrap();

    let DeviceUpdate::WriteDone { id: 2, result } = harness.next() else {
        panic!("expected WriteDone");
    };
    assert!(matches!(result, Err(DeviceError::InvalidValue(_))));
    assert_eq!(device.written().len(), before);
    harness.finish();
}

#[test]
fn events_are_forwarded_in_order() {
    let device = FakeDevice::new();
    let harness = Harness::start(vec![device.clone()]);
    harness.connected();

    device.push_event(FeatureId::BATTERY_SOC, &[70, 70, 0]);
    device.push_event(FeatureId::HEADSET_SIDETONE, &[0, 0, 0, 1]);
    device.push_event(FeatureId::HEADSET_TONES, &[2]);

    assert_eq!(
        harness.next(),
        DeviceUpdate::Event(Event::Battery(Battery {
            level_a: 70,
            level_b: 70,
            status: ChargeStatus::Discharging
        }))
    );
    assert_eq!(
        harness.next(),
        DeviceUpdate::Event(Event::Sidetone(Sidetone(1)))
    );
    assert_eq!(harness.next(), DeviceUpdate::Event(Event::Tones(Tones(2))));
    harness.assert_quiet();
    harness.finish();
}

#[test]
fn availability_true_triggers_refresh() {
    let device = FakeDevice::new();
    device.silence_headset_side();
    let harness = Harness::start(vec![device.clone()]);
    let first = harness.connected();
    assert_eq!(first.sidetone, None);
    device.answer(FeatureId::BATTERY_SOC, 0, Answer::Params(vec![90, 90, 0]));
    device.answer(
        FeatureId::HEADSET_SIDETONE,
        0,
        Answer::Params(vec![0, 0, 0, 4]),
    );
    device.answer(FeatureId::HEADSET_MIX, 0, Answer::Params(vec![2]));
    device.answer(FeatureId::MIXER_MIC, 1, Answer::Params(vec![4]));

    device.push_event(FeatureId::MIC_TEST, &[0]);

    assert_eq!(
        harness.next(),
        DeviceUpdate::Event(Event::HeadsetAvailability(true))
    );
    let DeviceUpdate::Refreshed(snapshot) = harness.next() else {
        panic!("expected Refreshed after the availability event");
    };
    assert_eq!(snapshot.headset_available, Some(true));
    assert_eq!(snapshot.sidetone, Some(Sidetone(4)));
    assert_eq!(snapshot.balance, Some(GameVoiceBalance(2)));
    assert_eq!(snapshot.noise_gate, Some(NoiseGate::Tournament));
    assert_eq!(
        snapshot.battery,
        Some(Battery {
            level_a: 90,
            level_b: 90,
            status: ChargeStatus::Discharging
        })
    );
    assert_eq!(snapshot.stream_mix, first.stream_mix);

    device.push_event(FeatureId::MIC_TEST, &[0]);
    assert_eq!(
        harness.next(),
        DeviceUpdate::Event(Event::HeadsetAvailability(true))
    );
    harness.assert_quiet();
    harness.finish();
}

#[test]
fn shutdown_ends_the_thread_promptly() {
    let harness = Harness::start(vec![FakeDevice::new()]);
    harness.connected();
    let started = Instant::now();

    harness.finish();

    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn dropping_the_command_sender_ends_the_thread() {
    let harness = Harness::start(Vec::new());
    harness.next();
    let Harness {
        commands, handle, ..
    } = harness;

    drop(commands);

    handle.join().expect("the worker thread ended cleanly");
}
