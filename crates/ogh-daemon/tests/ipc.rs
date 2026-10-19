#![cfg(unix)]

use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use ogh_daemon::config::Store;
use ogh_daemon::controller::{Controller, Input};
use ogh_daemon::device_worker::{DeviceCommand, DeviceUpdate};
use ogh_daemon::ipc_server;
use ogh_daemon::snapshot::DeviceSnapshot;
use ogh_device::{Event, FeatureId, FeatureInfo};
use ogh_ipc::{
    Client, CodecError, Connection, Endpoint, ErrorKind, IPC_VERSION, MAX_LINE_BYTES, Notification,
    Request, Response, Server, Setting, Status, Subscription, read_line,
};
use ogh_protocol::features::sidetone::Sidetone;
use tempfile::TempDir;

const WAIT: Duration = Duration::from_secs(5);

struct Daemon {
    endpoint: Endpoint,
    socket: PathBuf,
    inputs: Sender<Input>,
    device: Receiver<DeviceCommand>,
    _controller: Option<JoinHandle<()>>,
    _server: JoinHandle<()>,
    _dir: TempDir,
}

impl Daemon {
    fn start() -> Daemon {
        Daemon::start_with(true)
    }

    /// The controller's input receiver is gone, as when the daemon is shutting down.
    fn start_without_controller() -> Daemon {
        Daemon::start_with(false)
    }

    fn start_with(controller_runs: bool) -> Daemon {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("config.toml"));
        let (devices, device) = unbounded();
        let (inputs, input_rx) = unbounded();
        let controller = controller_runs.then(|| {
            let controller = Controller::new(store, devices, Instant::now);
            thread::spawn(move || controller.run(input_rx))
        });

        let socket = dir.path().join("daemon.sock");
        let endpoint = Endpoint::Path(socket.clone());
        let server = Server::bind(&endpoint).unwrap();
        let server = ipc_server::serve(server, inputs.clone());
        Daemon {
            endpoint,
            socket,
            inputs,
            device,
            _controller: controller,
            _server: server,
            _dir: dir,
        }
    }

    fn client(&self) -> Client {
        Client::connect(&self.endpoint).unwrap()
    }

    fn raw(&self) -> Raw {
        let stream = UnixStream::connect(&self.socket).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        Raw {
            reader: BufReader::new(stream.try_clone().unwrap()),
            stream,
        }
    }

    fn connect_device(&self, sidetone: u8) {
        let snapshot = DeviceSnapshot {
            headset_available: Some(true),
            sidetone: Some(Sidetone(sidetone)),
            ..DeviceSnapshot::default()
        };
        self.inputs
            .send(Input::Device(DeviceUpdate::Connected {
                features: vec![FeatureInfo {
                    index: 3,
                    id: FeatureId(0x0001),
                    flags: 0,
                    version: 2,
                }],
                snapshot,
            }))
            .unwrap();
    }

    fn finish_next_write(&self) {
        match self.device.recv_timeout(WAIT).expect("a write command") {
            DeviceCommand::Write { id, .. } => self
                .inputs
                .send(Input::Device(DeviceUpdate::WriteDone {
                    id,
                    result: Ok(()),
                }))
                .unwrap(),
            other => panic!("expected a write, got {other:?}"),
        }
    }
}

struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Raw {
    fn send_line(&mut self, line: &str) {
        use std::io::Write;
        self.stream.write_all(line.as_bytes()).unwrap();
        self.stream.write_all(b"\n").unwrap();
    }

    fn reply(&mut self) -> Response {
        read_line(&mut self.reader)
            .expect("a readable line")
            .expect("a reply, not a closed connection")
    }

    fn notification(&mut self) -> Notification {
        read_line(&mut self.reader)
            .expect("a readable line")
            .expect("a notification, not a closed connection")
    }

    fn assert_closed(&mut self) {
        let next: Option<Response> = read_line(&mut self.reader).expect("a clean close");
        assert_eq!(next, None);
    }

    fn send_unterminated(&mut self, bytes: &[u8]) {
        use std::io::Write;
        let _ = self.stream.write_all(bytes);
    }

    fn hello(&mut self) {
        self.send_line(&format!(r#"{{"cmd":"hello","version":{IPC_VERSION}}}"#));
        assert!(matches!(self.reply(), Response::Hello(_)));
    }
}

fn expect_error(response: Response, kind: ErrorKind) {
    match response {
        Response::Error(error) => assert_eq!(error.kind, kind, "{}", error.message),
        other => panic!("expected {kind:?}, got {other:?}"),
    }
}

fn watch(mut subscription: Subscription) -> Receiver<Notification> {
    let (tx, rx) = unbounded();
    thread::spawn(move || {
        while let Ok(Some(notification)) = subscription.next() {
            if tx.send(notification).is_err() {
                return;
            }
        }
    });
    rx
}

fn next_status(notifications: &Receiver<Notification>) -> Status {
    match notifications.recv_timeout(WAIT).expect("a notification") {
        Notification::Status(status) => status,
    }
}

fn status_until(
    notifications: &Receiver<Notification>,
    wanted: impl Fn(&Status) -> bool,
) -> Status {
    let deadline = Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match notifications.recv_timeout(left) {
            Ok(Notification::Status(status)) if wanted(&status) => return status,
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => panic!("no matching status in time"),
            Err(RecvTimeoutError::Disconnected) => panic!("subscription ended"),
        }
    }
}

#[test]
fn status_round_trip_over_the_socket() {
    let daemon = Daemon::start();
    let mut client = daemon.client();

    match client.call(&Request::Status).unwrap() {
        Response::Status(status) => assert_eq!(status.connection, Connection::NotFound),
        other => panic!("expected a status, got {other:?}"),
    }
    expect_error(
        client.call(&Request::Features).unwrap(),
        ErrorKind::NotConnected,
    );
}

#[test]
fn bad_json_gets_bad_request_and_the_connection_stays_usable() {
    let daemon = Daemon::start();
    let mut raw = daemon.raw();

    raw.send_line("this is not json");
    expect_error(raw.reply(), ErrorKind::BadRequest);
    raw.hello();

    raw.send_line(r#"{"cmd":"no_such_command"}"#);
    expect_error(raw.reply(), ErrorKind::BadRequest);
    raw.send_line("{");
    expect_error(raw.reply(), ErrorKind::BadRequest);

    raw.send_line(r#"{"cmd":"status"}"#);
    assert!(matches!(raw.reply(), Response::Status(_)));
}

#[test]
fn missing_hello_is_rejected() {
    let daemon = Daemon::start();

    let mut raw = daemon.raw();
    raw.send_line(r#"{"cmd":"status"}"#);
    expect_error(raw.reply(), ErrorKind::BadRequest);
    raw.assert_closed();

    let mut raw = daemon.raw();
    raw.send_line(r#"{"cmd":"hello","version":999}"#);
    expect_error(raw.reply(), ErrorKind::VersionMismatch);
    raw.assert_closed();
}

#[test]
fn subscription_receives_a_status_after_a_set() {
    let daemon = Daemon::start();
    daemon.connect_device(0);
    let notifications = watch(daemon.client().subscribe().unwrap());
    assert_eq!(
        next_status(&notifications)
            .settings
            .profile
            .sidetone_percent,
        Some(0)
    );

    let mut setter = daemon.client();
    let set = thread::spawn(move || {
        setter.call(&Request::Set {
            setting: Setting::Sidetone { percent: 100 },
        })
    });
    daemon.finish_next_write();
    assert_eq!(set.join().unwrap().unwrap(), Response::Done);

    let status = status_until(&notifications, |status| {
        status.settings.profile.sidetone_percent == Some(100)
    });
    assert_eq!(status.connection, Connection::Connected);
}

#[test]
fn client_disconnect_mid_subscription_does_not_stop_the_server() {
    let daemon = Daemon::start();
    let mut leaving = daemon.raw();
    leaving.hello();
    leaving.send_line(r#"{"cmd":"subscribe"}"#);
    assert_eq!(leaving.reply(), Response::Done);
    assert!(matches!(leaving.notification(), Notification::Status(_)));
    let staying = watch(daemon.client().subscribe().unwrap());
    next_status(&staying);
    drop(leaving);

    daemon.connect_device(0);
    status_until(&staying, |status| {
        status.connection == Connection::Connected
    });

    let mut client = daemon.client();
    assert!(matches!(
        client.call(&Request::Status).unwrap(),
        Response::Status(_)
    ));
}

#[test]
fn a_subscription_the_controller_dropped_ends_in_eof() {
    const CHANGES: usize = 3000;
    let daemon = Daemon::start();
    daemon.connect_device(0);
    let mut stalled = daemon.raw();
    stalled.hello();
    stalled.send_line(r#"{"cmd":"subscribe"}"#);
    assert_eq!(stalled.reply(), Response::Done);

    for change in 0..CHANGES {
        let level = Sidetone(1 + (change % 2) as u8);
        daemon
            .inputs
            .send(Input::Device(DeviceUpdate::Event(Event::Sidetone(level))))
            .unwrap();
    }

    let mut received = 0;
    loop {
        match read_line::<_, Notification>(&mut stalled.reader) {
            Ok(Some(_)) => received += 1,
            Ok(None) => break,
            Err(CodecError::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                panic!("the connection stayed open after {received} notifications")
            }
            Err(_) => break,
        }
    }
    assert!(received < CHANGES, "{received} notifications, none dropped");
}

#[test]
fn an_oversized_line_gets_bad_request_and_the_connection_closes() {
    let daemon = Daemon::start();
    let mut raw = daemon.raw();
    raw.hello();

    raw.send_unterminated(&vec![b'a'; MAX_LINE_BYTES + 10]);
    expect_error(raw.reply(), ErrorKind::BadRequest);
    raw.assert_closed();

    let mut client = daemon.client();
    assert!(matches!(
        client.call(&Request::Status).unwrap(),
        Response::Status(_)
    ));
}

#[test]
fn requests_after_the_controller_is_gone_get_shutting_down() {
    let daemon = Daemon::start_without_controller();
    let mut raw = daemon.raw();
    raw.hello();

    raw.send_line(r#"{"cmd":"status"}"#);
    match raw.reply() {
        Response::Error(error) => {
            assert_eq!(error.kind, ErrorKind::Device);
            assert_eq!(error.message, "ogh-daemon is shutting down");
        }
        other => panic!("expected an error, got {other:?}"),
    }
    raw.assert_closed();

    let mut subscriber = daemon.raw();
    subscriber.hello();
    subscriber.send_line(r#"{"cmd":"subscribe"}"#);
    expect_error(subscriber.reply(), ErrorKind::Device);
    subscriber.assert_closed();
}
