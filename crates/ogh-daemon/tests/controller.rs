use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use ogh_daemon::autoswitch::{AudioSwitchOutcome, Command};
use ogh_daemon::config::{AudioSwitchConfig, Store};
use ogh_daemon::controller::{Controller, FOCUS_SETTLE, Input};
use ogh_daemon::convert::profile_settings;
use ogh_daemon::device_worker::{DeviceCommand, DeviceUpdate};
use ogh_daemon::snapshot::{DeviceSnapshot, DeviceWrite};
use ogh_device::{DeviceError, Event, FeatureId, FeatureInfo};
use ogh_ipc::{
    Connection, ErrorKind, Headset, IpcError, Notification, ProfileInfo, ProfileSettings, Request,
    Response, Setting, Status,
};
use ogh_protocol::features::headset_mix::GameVoiceBalance;
use ogh_protocol::features::mixer_audio::{MixChannel, MixLevel, StreamMix};
use ogh_protocol::features::noise_gate::NoiseGate;
use ogh_protocol::features::sidetone::Sidetone;
use tempfile::TempDir;

const WAIT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(350);

#[derive(Clone)]
struct ManualClock(Arc<Mutex<Instant>>);

impl ManualClock {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Instant::now())))
    }

    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }

    fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

struct Harness {
    inputs: Sender<Input>,
    device: Receiver<DeviceCommand>,
    clock: ManualClock,
    thread: JoinHandle<()>,
    dir: TempDir,
}

impl Harness {
    fn start() -> Harness {
        Harness::start_with(|_| {})
    }

    fn start_with(prepare: impl FnOnce(&mut Store)) -> Harness {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(&dir.path().join("config.toml"));
        prepare(&mut store);
        Harness::start_store(dir, store)
    }

    fn start_store(dir: TempDir, store: Store) -> Harness {
        let (devices, device) = unbounded();
        let (inputs, input_rx) = unbounded();
        let clock = ManualClock::new();
        let controller = Controller::new(store, devices, {
            let clock = clock.clone();
            move || clock.now()
        });
        let thread = thread::spawn(move || controller.run(input_rx));
        Harness {
            inputs,
            device,
            clock,
            thread,
            dir,
        }
    }

    fn config_path(&self) -> PathBuf {
        self.dir.path().join("config.toml")
    }

    fn send(&self, request: Request) -> Receiver<Response> {
        let (reply, response) = unbounded();
        self.inputs.send(Input::Ipc { request, reply }).unwrap();
        response
    }

    fn request(&self, request: Request) -> Response {
        self.send(request).recv_timeout(WAIT).expect("a reply")
    }

    fn request_error(&self, request: Request) -> IpcError {
        match self.request(request) {
            Response::Error(error) => error,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn status(&self) -> Status {
        match self.request(Request::Status) {
            Response::Status(status) => status,
            other => panic!("expected a status, got {other:?}"),
        }
    }

    /// Every earlier input has been handled once a reply comes back.
    fn sync(&self) {
        self.status();
    }

    fn update(&self, update: DeviceUpdate) {
        self.inputs.send(Input::Device(update)).unwrap();
    }

    fn connect(&self, snapshot: DeviceSnapshot) {
        self.update(connected(snapshot));
        self.sync();
    }

    fn write(&self) -> (u64, Vec<DeviceWrite>) {
        match self.device.recv_timeout(WAIT).expect("a write command") {
            DeviceCommand::Write { id, writes } => (id, writes),
            other => panic!("expected a write, got {other:?}"),
        }
    }

    fn write_and_finish(&self) -> Vec<DeviceWrite> {
        let (id, writes) = self.write();
        self.update(DeviceUpdate::WriteDone { id, result: Ok(()) });
        writes
    }

    fn no_write(&self) {
        assert_eq!(
            self.device.recv_timeout(QUIET),
            Err(crossbeam_channel::RecvTimeoutError::Timeout)
        );
    }

    fn focus(&self, app: &str) {
        self.inputs
            .send(Input::Focus(Some(app.to_string())))
            .unwrap();
        self.sync();
    }

    fn let_focus_settle(&self) {
        self.clock.advance(FOCUS_SETTLE);
        self.sync();
    }

    fn profiles(&self) -> Vec<ProfileInfo> {
        match self.request(Request::ListProfiles) {
            Response::Profiles(profiles) => profiles,
            other => panic!("expected profiles, got {other:?}"),
        }
    }

    fn finish(self) {
        drop(self.inputs);
        self.thread.join().unwrap();
    }
}

fn connected(snapshot: DeviceSnapshot) -> DeviceUpdate {
    DeviceUpdate::Connected {
        features: vec![FeatureInfo {
            index: 3,
            id: FeatureId(0x0001),
            flags: 0,
            version: 2,
        }],
        snapshot,
    }
}

fn snapshot(sidetone: u8, gate: NoiseGate) -> DeviceSnapshot {
    DeviceSnapshot {
        headset_available: Some(true),
        sidetone: Some(Sidetone(sidetone)),
        noise_gate: Some(gate),
        ..DeviceSnapshot::default()
    }
}

fn sidetone_profile(percent: u8) -> ProfileSettings {
    ProfileSettings {
        sidetone_percent: Some(percent),
        ..ProfileSettings::default()
    }
}

fn expect_error(response: Response, kind: ErrorKind) {
    match response {
        Response::Error(error) => assert_eq!(error.kind, kind, "{}", error.message),
        other => panic!("expected {kind:?}, got {other:?}"),
    }
}

#[test]
fn set_replies_after_the_write_finishes() {
    let harness = Harness::start();
    harness.connect(snapshot(0, NoiseGate::Off));

    let first = harness.send(Request::Set {
        setting: Setting::Sidetone { percent: 100 },
    });
    let second = harness.send(Request::Set {
        setting: Setting::NoiseGate(ogh_ipc::Gate::Night),
    });
    let (first_id, first_writes) = harness.write();
    let (second_id, second_writes) = harness.write();
    assert_ne!(first_id, second_id);
    assert_eq!(first_writes, vec![DeviceWrite::Sidetone(Sidetone(6))]);
    assert_eq!(
        second_writes,
        vec![DeviceWrite::NoiseGate(NoiseGate::Night)]
    );
    assert!(
        first.recv_timeout(QUIET).is_err(),
        "replied before WriteDone"
    );

    harness.update(DeviceUpdate::WriteDone {
        id: second_id,
        result: Ok(()),
    });
    assert_eq!(second.recv_timeout(WAIT), Ok(Response::Done));
    assert!(
        first.recv_timeout(QUIET).is_err(),
        "replies are independent"
    );

    harness.update(DeviceUpdate::WriteDone {
        id: first_id,
        result: Err(DeviceError::HeadsetDisconnected),
    });
    expect_error(first.recv_timeout(WAIT).unwrap(), ErrorKind::HeadsetOff);

    assert_eq!(
        harness.status().settings.profile.noise_gate,
        Some(ogh_ipc::Gate::Night)
    );
    assert_eq!(harness.status().settings.profile.sidetone_percent, Some(0));
    harness.finish();
}

#[test]
fn set_while_disconnected_is_not_connected() {
    let harness = Harness::start();
    let error = harness.request_error(Request::Set {
        setting: Setting::Sidetone { percent: 50 },
    });
    assert_eq!(error.kind, ErrorKind::NotConnected);
    harness.no_write();

    harness.connect(snapshot(0, NoiseGate::Off));
    let error = harness.request_error(Request::Set {
        setting: Setting::Sidetone { percent: 101 },
    });
    assert_eq!(error.kind, ErrorKind::InvalidValue);
    harness.no_write();
    harness.finish();
}

#[test]
fn pending_write_fails_when_the_device_closes() {
    let harness = Harness::start();
    harness.connect(snapshot(0, NoiseGate::Off));

    let reply = harness.send(Request::Set {
        setting: Setting::Sidetone { percent: 100 },
    });
    harness.write();
    harness.update(DeviceUpdate::Closed(Connection::NotFound));
    expect_error(reply.recv_timeout(WAIT).unwrap(), ErrorKind::NotConnected);

    let status = harness.status();
    assert_eq!(status.connection, Connection::NotFound);
    assert_eq!(status.headset, Headset::Unknown);
    assert_eq!(status.settings.profile.sidetone_percent, None);
    harness.finish();
}

#[test]
fn ghub_running_is_reported_in_status_and_errors() {
    let harness = Harness::start();
    harness.update(DeviceUpdate::Closed(Connection::GhubRunning));

    assert_eq!(harness.status().connection, Connection::GhubRunning);
    let error = harness.request_error(Request::Set {
        setting: Setting::Sidetone { percent: 50 },
    });
    assert_eq!(error.kind, ErrorKind::GhubRunning);
    harness.no_write();
    harness.finish();
}

#[test]
fn focus_settles_before_applying() {
    let harness = Harness::start_with(|store| {
        store.save_profile("a", sidetone_profile(100)).unwrap();
        store.link_app("a", "a.exe").unwrap();
        store
            .save_profile(
                "b",
                ProfileSettings {
                    noise_gate: Some(ogh_ipc::Gate::Night),
                    ..ProfileSettings::default()
                },
            )
            .unwrap();
        store.link_app("b", "b.exe").unwrap();
    });
    harness.connect(snapshot(0, NoiseGate::Off));
    harness.no_write();

    harness.focus("a.exe");
    harness.clock.advance(Duration::from_millis(100));
    harness.focus("b.exe");
    harness.clock.advance(Duration::from_millis(100));
    harness.focus("a.exe");
    harness.no_write();
    assert_eq!(harness.status().focus.app, None);

    harness.let_focus_settle();
    assert_eq!(
        harness.write_and_finish(),
        vec![DeviceWrite::Sidetone(Sidetone(6))]
    );
    harness.no_write();

    let status = harness.status();
    assert_eq!(status.focus.app.as_deref(), Some("a.exe"));
    assert_eq!(status.active_profile.as_deref(), Some("a"));
    harness.finish();
}

#[test]
fn matching_settings_send_no_write() {
    let current = snapshot(4, NoiseGate::Home);
    let matching = profile_settings(&current);
    let harness = Harness::start_with(|store| {
        store.save_profile("default", matching.clone()).unwrap();
        store.save_profile("same", matching).unwrap();
        store.link_app("same", "a.exe").unwrap();
    });

    harness.connect(current.clone());
    harness.update(DeviceUpdate::Refreshed(current));
    harness.focus("a.exe");
    harness.let_focus_settle();
    assert_eq!(harness.status().active_profile.as_deref(), Some("same"));
    harness.no_write();
    harness.finish();
}

#[test]
fn unlinked_app_returns_to_default() {
    let harness = Harness::start_with(|store| {
        store.save_profile("default", sidetone_profile(0)).unwrap();
        store.save_profile("game", sidetone_profile(100)).unwrap();
        store.link_app("game", "a.exe").unwrap();
    });
    harness.connect(snapshot(0, NoiseGate::Off));

    harness.focus("A.exe");
    harness.let_focus_settle();
    assert_eq!(
        harness.write_and_finish(),
        vec![DeviceWrite::Sidetone(Sidetone(6))]
    );
    assert_eq!(harness.status().active_profile.as_deref(), Some("game"));

    assert_eq!(
        harness.request(Request::UnlinkApp {
            app: "a.exe".to_string()
        }),
        Response::Done
    );
    assert_eq!(
        harness.write_and_finish(),
        vec![DeviceWrite::Sidetone(Sidetone(0))]
    );
    assert_eq!(harness.status().active_profile.as_deref(), Some("default"));
    harness.finish();
}

#[test]
fn profile_reapplied_after_reconnect_and_after_headset_returns() {
    let harness = Harness::start_with(|store| {
        store
            .save_profile("default", sidetone_profile(100))
            .unwrap();
    });
    let unset = snapshot(0, NoiseGate::Off);

    harness.connect(unset.clone());
    assert_eq!(
        harness.write_and_finish(),
        vec![DeviceWrite::Sidetone(Sidetone(6))]
    );
    harness.no_write();

    harness.update(DeviceUpdate::Closed(Connection::NotFound));
    harness.connect(unset.clone());
    assert_eq!(
        harness.write_and_finish(),
        vec![DeviceWrite::Sidetone(Sidetone(6))]
    );

    harness.update(DeviceUpdate::Event(Event::HeadsetAvailability(false)));
    harness.update(DeviceUpdate::Event(Event::HeadsetAvailability(true)));
    harness.update(DeviceUpdate::Refreshed(unset));
    assert_eq!(
        harness.write_and_finish(),
        vec![DeviceWrite::Sidetone(Sidetone(6))]
    );
    harness.no_write();
    harness.finish();
}

#[test]
fn first_connect_creates_the_default_profile() {
    let harness = Harness::start();
    assert!(!harness.config_path().exists());

    let first = snapshot(3, NoiseGate::Home);
    harness.connect(first.clone());
    harness.no_write();

    let profiles = harness.profiles();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].name, "default");
    assert!(profiles[0].is_default);
    assert_eq!(profiles[0].settings, profile_settings(&first));
    assert!(harness.config_path().exists());

    harness.update(DeviceUpdate::Closed(Connection::NotFound));
    harness.update(connected(snapshot(1, NoiseGate::Off)));
    assert_eq!(harness.profiles()[0].settings, profile_settings(&first));
    assert_eq!(
        harness.write_and_finish(),
        vec![
            DeviceWrite::Sidetone(Sidetone(3)),
            DeviceWrite::NoiseGate(NoiseGate::Home)
        ]
    );
    harness.finish();
}

#[test]
fn config_error_reaches_status_and_profile_commands_fail() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "default_profile = [").unwrap();
    let harness = Harness::start_store(dir, Store::open(&path));

    assert!(harness.status().config_error.is_some());
    harness.connect(snapshot(0, NoiseGate::Off));
    let status = harness.status();
    assert!(status.config_error.is_some());
    assert_eq!(status.active_profile, None);
    harness.no_write();

    let requests = [
        Request::ListProfiles,
        Request::SaveProfile {
            name: "x".to_string(),
        },
        Request::DeleteProfile {
            name: "x".to_string(),
        },
        Request::SetDefaultProfile {
            name: "x".to_string(),
        },
        Request::LinkApp {
            profile: "x".to_string(),
            app: "a.exe".to_string(),
        },
        Request::UnlinkApp {
            app: "a.exe".to_string(),
        },
        Request::SetAutoSwitch { enabled: true },
        Request::ApplyProfile {
            name: "x".to_string(),
        },
    ];
    for request in requests {
        let error = harness.request_error(request.clone());
        assert_eq!(error.kind, ErrorKind::ConfigUnavailable, "{request:?}");
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "default_profile = ["
    );

    let reply = harness.send(Request::Set {
        setting: Setting::Sidetone { percent: 100 },
    });
    harness.write_and_finish();
    assert_eq!(reply.recv_timeout(WAIT), Ok(Response::Done));
    harness.finish();
}

#[test]
fn subscribers_get_status_on_change_and_dead_ones_are_dropped() {
    let harness = Harness::start();
    let (live_tx, live) = unbounded();
    let (dead_tx, dead) = unbounded();
    harness.inputs.send(Input::Subscribe(live_tx)).unwrap();
    harness.inputs.send(Input::Subscribe(dead_tx)).unwrap();
    drop(dead);

    let Notification::Status(first) = live.recv_timeout(WAIT).unwrap();
    assert_eq!(first.connection, Connection::NotFound);

    harness.update(connected(snapshot(2, NoiseGate::Off)));
    let Notification::Status(connected_status) = live.recv_timeout(WAIT).unwrap();
    assert_eq!(connected_status.connection, Connection::Connected);
    assert_eq!(connected_status.headset, Headset::InUse);

    harness.update(DeviceUpdate::Event(Event::Sidetone(Sidetone(5))));
    let Notification::Status(changed) = live.recv_timeout(WAIT).unwrap();
    assert_eq!(
        changed.settings.profile.sidetone_percent,
        Some(Sidetone(5).to_percent())
    );

    harness.update(DeviceUpdate::Event(Event::Sidetone(Sidetone(5))));
    harness.sync();
    assert!(
        live.try_recv().is_err(),
        "an unchanged status is not pushed"
    );

    harness.update(DeviceUpdate::Closed(Connection::GhubRunning));
    let Notification::Status(closed) = live.recv_timeout(WAIT).unwrap();
    assert_eq!(closed.connection, Connection::GhubRunning);
    harness.finish();
}

#[test]
fn features_list_the_connected_table() {
    let harness = Harness::start();
    expect_error(harness.request(Request::Features), ErrorKind::NotConnected);

    harness.connect(snapshot(0, NoiseGate::Off));
    match harness.request(Request::Features) {
        Response::Features(features) => {
            assert_eq!(features.len(), 1);
            assert_eq!(features[0].index, 3);
            assert_eq!(features[0].id, 0x0001);
            assert_eq!(features[0].version, 2);
        }
        other => panic!("expected features, got {other:?}"),
    }
    harness.finish();
}

#[test]
fn applied_profile_holds_until_the_focus_changes_app() {
    let harness = Harness::start_with(|store| {
        store.save_profile("default", sidetone_profile(0)).unwrap();
        store.save_profile("game", sidetone_profile(100)).unwrap();
    });
    harness.connect(snapshot(0, NoiseGate::Off));

    expect_error(
        harness.request(Request::ApplyProfile {
            name: "nope".to_string(),
        }),
        ErrorKind::NoSuchProfile,
    );
    let reply = harness.send(Request::ApplyProfile {
        name: "game".to_string(),
    });
    let (id, writes) = harness.write();
    assert_eq!(writes, vec![DeviceWrite::Sidetone(Sidetone(6))]);
    assert!(reply.recv_timeout(QUIET).is_err());
    harness.update(DeviceUpdate::WriteDone { id, result: Ok(()) });
    assert_eq!(reply.recv_timeout(WAIT), Ok(Response::Done));
    assert_eq!(harness.status().active_profile.as_deref(), Some("game"));

    harness.inputs.send(Input::Focus(None)).unwrap();
    harness.let_focus_settle();
    assert_eq!(harness.status().active_profile.as_deref(), Some("game"));
    harness.no_write();

    harness.focus("other.exe");
    harness.let_focus_settle();
    assert_eq!(harness.status().active_profile.as_deref(), Some("default"));
    assert_eq!(
        harness.write_and_finish(),
        vec![DeviceWrite::Sidetone(Sidetone(0))]
    );
    harness.finish();
}

#[test]
fn profile_switch_waits_for_writes_in_flight() {
    let harness = Harness::start_with(|store| {
        store.save_profile("default", sidetone_profile(0)).unwrap();
        store.save_profile("a", sidetone_profile(100)).unwrap();
        store.link_app("a", "a.exe").unwrap();
        store.save_profile("b", sidetone_profile(0)).unwrap();
        store.link_app("b", "b.exe").unwrap();
    });
    harness.connect(snapshot(0, NoiseGate::Off));

    harness.focus("a.exe");
    harness.let_focus_settle();
    let (id, writes) = harness.write();
    assert_eq!(writes, vec![DeviceWrite::Sidetone(Sidetone(6))]);

    harness.focus("b.exe");
    harness.let_focus_settle();
    assert_eq!(harness.status().active_profile.as_deref(), Some("b"));
    harness.no_write();

    harness.update(DeviceUpdate::WriteDone { id, result: Ok(()) });
    assert_eq!(
        harness.write_and_finish(),
        vec![DeviceWrite::Sidetone(Sidetone(0))]
    );
    harness.no_write();
    harness.finish();
}

#[test]
fn default_profile_creation_retries_after_a_failed_save() {
    let dir = TempDir::new().unwrap();
    let blocker = dir.path().join("blocked");
    let path = blocker.join("config.toml");
    let harness = Harness::start_store(dir, Store::open(&path));
    std::fs::write(&blocker, "").unwrap();

    let first = snapshot(3, NoiseGate::Home);
    harness.connect(first.clone());
    assert_eq!(harness.profiles()[0].settings, ProfileSettings::default());
    assert!(!path.exists());

    std::fs::remove_file(&blocker).unwrap();
    harness.update(DeviceUpdate::Closed(Connection::NotFound));
    harness.connect(first.clone());
    assert_eq!(harness.profiles()[0].settings, profile_settings(&first));
    assert!(path.exists());
    harness.finish();
}

#[test]
fn headset_changes_reach_the_auto_switch_and_its_outcome_shows_in_status() {
    let dir = TempDir::new().unwrap();
    let store = Store::open(&dir.path().join("config.toml"));
    let (devices, _device) = unbounded();
    let (commands, command_rx) = unbounded();
    let (inputs, input_rx) = unbounded();
    let controller = Controller::new(store, devices, Instant::now).with_auto_switch(commands);
    let thread = thread::spawn(move || controller.run(input_rx));
    let status = |inputs: &Sender<Input>| {
        let (reply, response) = unbounded();
        let request = Request::Status;
        inputs.send(Input::Ipc { request, reply }).unwrap();
        match response.recv_timeout(WAIT).expect("a reply") {
            Response::Status(status) => status,
            other => panic!("expected a status, got {other:?}"),
        }
    };
    let next_command = || command_rx.recv_timeout(WAIT).expect("a command");

    assert_eq!(
        next_command(),
        Command::Configure(AudioSwitchConfig::default())
    );
    assert!(!status(&inputs).auto_switch.active);

    inputs
        .send(Input::Device(connected(snapshot(0, NoiseGate::Off))))
        .unwrap();
    assert_eq!(
        next_command(),
        Command::Configure(AudioSwitchConfig::default())
    );
    assert_eq!(next_command(), Command::Headset(Headset::InUse));

    inputs
        .send(Input::AutoSwitch(AudioSwitchOutcome {
            active: true,
            error: Some("could not set render.console: gone".to_string()),
        }))
        .unwrap();
    let auto_switch = status(&inputs).auto_switch;
    assert!(auto_switch.active);
    assert_eq!(
        auto_switch.error.as_deref(),
        Some("could not set render.console: gone")
    );

    let (reply, response) = unbounded();
    let request = Request::SetAutoSwitch { enabled: true };
    inputs.send(Input::Ipc { request, reply }).unwrap();
    assert!(matches!(response.recv_timeout(WAIT), Ok(Response::Done)));
    assert_eq!(
        next_command(),
        Command::Configure(AudioSwitchConfig {
            enabled: true,
            ..AudioSwitchConfig::default()
        })
    );
    assert!(status(&inputs).auto_switch.enabled);

    inputs
        .send(Input::Device(DeviceUpdate::Closed(Connection::NotFound)))
        .unwrap();
    assert_eq!(next_command(), Command::Headset(Headset::Unknown));
    assert_eq!(
        command_rx.recv_timeout(QUIET),
        Err(crossbeam_channel::RecvTimeoutError::Timeout)
    );

    drop(inputs);
    thread.join().unwrap();
}

#[test]
fn manual_apply_during_automatic_writes_is_reapplied_after_they_finish() {
    let harness = Harness::start_with(|store| {
        store.save_profile("app", sidetone_profile(80)).unwrap();
        store.link_app("app", "a.exe").unwrap();
        store.save_profile("manual", sidetone_profile(50)).unwrap();
    });
    harness.connect(snapshot(3, NoiseGate::Off));

    harness.focus("a.exe");
    harness.let_focus_settle();
    let (id, writes) = harness.write();
    assert_eq!(writes, vec![DeviceWrite::Sidetone(Sidetone(5))]);

    assert_eq!(
        harness.request(Request::ApplyProfile {
            name: "manual".to_string()
        }),
        Response::Done
    );
    harness.no_write();

    harness.update(DeviceUpdate::WriteDone { id, result: Ok(()) });
    assert_eq!(
        harness.write_and_finish(),
        vec![DeviceWrite::Sidetone(Sidetone(3))]
    );
    harness.no_write();
    let status = harness.status();
    assert_eq!(status.active_profile.as_deref(), Some("manual"));
    assert_eq!(status.settings.profile.sidetone_percent, Some(50));
    harness.finish();
}

#[test]
fn save_profile_waits_for_writes_in_flight() {
    let harness = Harness::start();
    harness.connect(snapshot(0, NoiseGate::Off));

    let set = harness.send(Request::Set {
        setting: Setting::Sidetone { percent: 100 },
    });
    let (id, _) = harness.write();
    let save = harness.send(Request::SaveProfile {
        name: "loud".to_string(),
    });
    assert!(save.recv_timeout(QUIET).is_err(), "saved before WriteDone");

    harness.update(DeviceUpdate::WriteDone { id, result: Ok(()) });
    assert_eq!(set.recv_timeout(WAIT), Ok(Response::Done));
    assert_eq!(save.recv_timeout(WAIT), Ok(Response::Done));
    let loud = harness
        .profiles()
        .into_iter()
        .find(|profile| profile.name == "loud")
        .expect("the saved profile");
    assert_eq!(loud.settings.sidetone_percent, Some(100));
    harness.finish();
}

#[test]
fn save_profile_waiting_for_writes_fails_when_the_device_closes() {
    let harness = Harness::start();
    harness.connect(snapshot(0, NoiseGate::Off));

    let _set = harness.send(Request::Set {
        setting: Setting::Sidetone { percent: 100 },
    });
    harness.write();
    let save = harness.send(Request::SaveProfile {
        name: "loud".to_string(),
    });
    harness.update(DeviceUpdate::Closed(Connection::NotFound));
    expect_error(save.recv_timeout(WAIT).unwrap(), ErrorKind::NotConnected);
    assert!(
        harness
            .profiles()
            .iter()
            .all(|profile| profile.name != "loud")
    );
    harness.finish();
}

fn docked_with_mix() -> DeviceSnapshot {
    let channel = MixChannel {
        level: MixLevel(16),
        muted: false,
    };
    DeviceSnapshot {
        headset_available: Some(false),
        stream_mix: Some(StreamMix {
            stream: channel,
            inputs: [channel; 4],
        }),
        ..DeviceSnapshot::default()
    }
}

#[test]
fn default_profile_is_completed_once_the_headset_reports_its_settings() {
    let harness = Harness::start();
    let docked = docked_with_mix();
    harness.connect(docked.clone());
    let partial = profile_settings(&docked);
    assert!(partial.stream_mix.is_some());
    assert_eq!(harness.profiles()[0].settings, partial);

    let in_use = DeviceSnapshot {
        headset_available: Some(true),
        sidetone: Some(Sidetone(3)),
        balance: Some(GameVoiceBalance(6)),
        noise_gate: Some(NoiseGate::Home),
        ..docked
    };
    harness.update(DeviceUpdate::Refreshed(in_use.clone()));
    assert_eq!(harness.profiles()[0].settings, profile_settings(&in_use));
    harness.no_write();

    let changed = DeviceSnapshot {
        sidetone: Some(Sidetone(6)),
        ..in_use.clone()
    };
    harness.update(DeviceUpdate::Refreshed(changed));
    assert_eq!(harness.profiles()[0].settings, profile_settings(&in_use));
    assert_eq!(
        harness.write_and_finish(),
        vec![DeviceWrite::Sidetone(Sidetone(3))]
    );
    harness.finish();
}

#[test]
fn hand_edited_audio_names_reach_the_auto_switch_at_the_next_change() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    let mut store = Store::open(&path);
    store.save_profile("fps", sidetone_profile(50)).unwrap();
    let (devices, _device) = unbounded();
    let (commands, command_rx) = unbounded();
    let (inputs, input_rx) = unbounded();
    let controller = Controller::new(store, devices, Instant::now).with_auto_switch(commands);
    let thread = thread::spawn(move || controller.run(input_rx));
    let request = |request: Request| {
        let (reply, response) = unbounded();
        inputs.send(Input::Ipc { request, reply }).unwrap();
        response.recv_timeout(WAIT).expect("a reply")
    };
    assert_eq!(
        command_rx.recv_timeout(WAIT),
        Ok(Command::Configure(AudioSwitchConfig::default()))
    );

    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("A50 X Mic", "Desk Mic");
    std::fs::write(&path, text).unwrap();
    assert_eq!(
        request(Request::LinkApp {
            profile: "fps".to_string(),
            app: "cs2.exe".to_string(),
        }),
        Response::Done
    );
    assert_eq!(
        command_rx.recv_timeout(WAIT),
        Ok(Command::Configure(AudioSwitchConfig {
            mic_input: "Desk Mic".to_string(),
            ..AudioSwitchConfig::default()
        }))
    );
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("Desk Mic"), "{saved}");
    assert!(saved.contains("cs2.exe"), "{saved}");

    drop(inputs);
    thread.join().unwrap();
}

#[test]
fn deferred_save_captures_the_reapplied_profile() {
    let harness = Harness::start_with(|store| {
        store.save_profile("app", sidetone_profile(80)).unwrap();
        store.link_app("app", "a.exe").unwrap();
        store.save_profile("manual", sidetone_profile(50)).unwrap();
    });
    harness.connect(snapshot(3, NoiseGate::Off));

    harness.focus("a.exe");
    harness.let_focus_settle();
    let (id, writes) = harness.write();
    assert_eq!(writes, vec![DeviceWrite::Sidetone(Sidetone(5))]);
    assert_eq!(
        harness.request(Request::ApplyProfile {
            name: "manual".to_string()
        }),
        Response::Done
    );
    let save = harness.send(Request::SaveProfile {
        name: "x".to_string(),
    });

    harness.update(DeviceUpdate::WriteDone { id, result: Ok(()) });
    let (reapply, writes) = harness.write();
    assert_eq!(writes, vec![DeviceWrite::Sidetone(Sidetone(3))]);
    assert!(
        save.recv_timeout(QUIET).is_err(),
        "saved before the reapply"
    );

    harness.update(DeviceUpdate::WriteDone {
        id: reapply,
        result: Ok(()),
    });
    assert_eq!(save.recv_timeout(WAIT), Ok(Response::Done));
    let saved = harness
        .profiles()
        .into_iter()
        .find(|profile| profile.name == "x")
        .expect("the saved profile");
    assert_eq!(saved.settings.sidetone_percent, Some(50));
    harness.no_write();
    harness.finish();
}

#[test]
fn default_profile_creation_reconfigures_auto_switch() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    let store = Store::open(&path);
    let (devices, _device) = unbounded();
    let (commands, command_rx) = unbounded();
    let (inputs, input_rx) = unbounded();
    let controller = Controller::new(store, devices, Instant::now).with_auto_switch(commands);
    let thread = thread::spawn(move || controller.run(input_rx));
    assert_eq!(
        command_rx.recv_timeout(WAIT),
        Ok(Command::Configure(AudioSwitchConfig::default()))
    );

    std::fs::write(&path, "[audio_switch]\nenabled = true\n").unwrap();
    inputs
        .send(Input::Device(connected(snapshot(0, NoiseGate::Off))))
        .unwrap();
    let mut received = Vec::new();
    while let Ok(command) = command_rx.recv_timeout(QUIET) {
        received.push(command);
    }
    assert!(
        received.contains(&Command::Configure(AudioSwitchConfig {
            enabled: true,
            ..AudioSwitchConfig::default()
        })),
        "{received:?}"
    );
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("[profiles.default]"), "{saved}");

    drop(inputs);
    thread.join().unwrap();
}

#[test]
fn partial_default_is_not_filled_after_the_default_changes() {
    let harness = Harness::start_with(|store| {
        store
            .save_profile("other", ProfileSettings::default())
            .unwrap();
    });
    let docked = docked_with_mix();
    harness.connect(docked.clone());
    let partial = profile_settings(&docked);
    let settings_of = |name: &str| {
        harness
            .profiles()
            .into_iter()
            .find(|profile| profile.name == name)
            .expect("the profile")
            .settings
    };
    assert_eq!(settings_of("default"), partial);

    assert_eq!(
        harness.request(Request::SetDefaultProfile {
            name: "other".to_string()
        }),
        Response::Done
    );
    let in_use = DeviceSnapshot {
        headset_available: Some(true),
        sidetone: Some(Sidetone(3)),
        balance: Some(GameVoiceBalance(6)),
        noise_gate: Some(NoiseGate::Home),
        ..docked
    };
    harness.update(DeviceUpdate::Refreshed(in_use.clone()));
    assert_eq!(settings_of("default"), partial);
    assert_eq!(settings_of("other"), ProfileSettings::default());

    assert_eq!(
        harness.request(Request::SetDefaultProfile {
            name: "default".to_string()
        }),
        Response::Done
    );
    harness.update(DeviceUpdate::Refreshed(in_use));
    assert_eq!(settings_of("default"), partial, "the capture is finished");
    harness.finish();
}
