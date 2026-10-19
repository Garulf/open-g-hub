#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};

use ogh_ipc::transport::Connection;
use ogh_ipc::{
    AutoSwitchStatus, Battery, Charge, Client, DeviceSettings, Endpoint, FocusStatus, Headset,
    Hello, IPC_VERSION, Notification, Request, Response, Server, Status, TransportError,
};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> TestDir {
        let dir = std::env::temp_dir().join(format!("ogh-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TestDir(dir)
    }

    fn socket(&self) -> PathBuf {
        self.0.join("d.sock")
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn hello_reply(version: u32) -> Response {
    Response::Hello(Hello {
        version,
        daemon_version: "test".to_string(),
    })
}

fn status() -> Status {
    Status {
        connection: ogh_ipc::Connection::Connected,
        headset: Headset::InUse,
        battery: Some(Battery {
            percent: 80,
            charge: Charge::Discharging,
        }),
        firmware: Some("1.2.3".into()),
        settings: DeviceSettings::default(),
        active_profile: None,
        focus: FocusStatus {
            available: false,
            app: None,
        },
        auto_switch: AutoSwitchStatus {
            enabled: false,
            active: false,
            error: None,
        },
        config_error: None,
    }
}

fn serve_once<F>(server: Server, handler: F) -> JoinHandle<()>
where
    F: FnOnce(Connection) + Send + 'static,
{
    thread::spawn(move || handler(server.accept().unwrap()))
}

#[test]
fn client_and_server_exchange_lines() {
    let dir = TestDir::new("exchange");
    let endpoint = Endpoint::Path(dir.socket());
    let server = Server::bind(&endpoint).unwrap();
    let canned = status();
    let expected = canned.clone();
    let handle = serve_once(server, move |mut conn| {
        let hello: Request = conn.recv().unwrap().unwrap();
        assert_eq!(
            hello,
            Request::Hello {
                version: IPC_VERSION
            }
        );
        conn.send(&hello_reply(IPC_VERSION)).unwrap();
        while let Some(request) = conn.recv::<Request>().unwrap() {
            assert_eq!(request, Request::Status);
            conn.send(&Response::Status(canned.clone())).unwrap();
        }
    });

    let mut client = Client::connect(&endpoint).unwrap();
    for _ in 0..2 {
        assert_eq!(
            client.call(&Request::Status).unwrap(),
            Response::Status(expected.clone())
        );
    }
    drop(client);
    handle.join().unwrap();
}

#[test]
fn stale_socket_file_is_replaced() {
    let dir = TestDir::new("stale");
    let path = dir.socket();
    drop(UnixListener::bind(&path).unwrap());
    assert!(path.exists());

    let server = Server::bind(&Endpoint::Path(path.clone())).unwrap();
    assert!(fs::metadata(&path).unwrap().file_type().is_socket());
    drop(server);
}

#[test]
fn regular_file_at_the_path_is_not_removed() {
    let dir = TestDir::new("regular");
    let path = dir.socket();
    fs::write(&path, b"precious").unwrap();

    assert!(matches!(
        Server::bind(&Endpoint::Path(path.clone())),
        Err(TransportError::Io(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), b"precious");
}

#[test]
fn second_bind_while_serving_is_already_running() {
    let dir = TestDir::new("twice");
    let endpoint = Endpoint::Path(dir.socket());
    let _first = Server::bind(&endpoint).unwrap();
    match Server::bind(&endpoint) {
        Err(TransportError::AlreadyRunning) => {}
        other => panic!("expected AlreadyRunning, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn socket_dir_is_private() {
    let dir = TestDir::new("private");
    let sub = dir.0.join("open-g-hub");
    let path = sub.join("d.sock");
    let _server = Server::bind(&Endpoint::Path(path.clone())).unwrap();
    assert_eq!(mode(&sub), 0o700);
    assert_eq!(mode(&path) & 0o077, 0);
}

#[test]
fn wide_existing_dir_is_tightened() {
    let dir = TestDir::new("wide");
    fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o755)).unwrap();
    let _server = Server::bind(&Endpoint::Path(dir.socket())).unwrap();
    assert_eq!(mode(&dir.0), 0o700);
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn connect_without_daemon_is_daemon_not_running() {
    let dir = TestDir::new("absent");
    let endpoint = Endpoint::Path(dir.socket());
    assert!(matches!(
        Client::connect(&endpoint),
        Err(TransportError::DaemonNotRunning)
    ));

    drop(UnixListener::bind(dir.socket()).unwrap());
    assert!(matches!(
        Client::connect(&endpoint),
        Err(TransportError::DaemonNotRunning)
    ));
}

#[test]
fn version_mismatch_is_reported() {
    let dir = TestDir::new("version");
    let endpoint = Endpoint::Path(dir.socket());
    let server = Server::bind(&endpoint).unwrap();
    let handle = serve_once(server, |mut conn| {
        let _: Option<Request> = conn.recv().unwrap();
        conn.send(&hello_reply(2)).unwrap();
    });

    match Client::connect(&endpoint) {
        Err(TransportError::VersionMismatch { daemon: 2 }) => {}
        other => panic!("expected VersionMismatch, got {:?}", other.map(|_| ())),
    }
    handle.join().unwrap();
}

#[test]
fn closing_without_a_reply_is_an_io_error() {
    let dir = TestDir::new("hangup");
    let endpoint = Endpoint::Path(dir.socket());
    let server = Server::bind(&endpoint).unwrap();
    let handle = serve_once(server, |mut conn| {
        let _: Option<Request> = conn.recv().unwrap();
        conn.send(&hello_reply(IPC_VERSION)).unwrap();
        let _: Option<Request> = conn.recv().unwrap();
    });

    let mut client = Client::connect(&endpoint).unwrap();
    assert!(matches!(
        client.call(&Request::Features),
        Err(TransportError::Io(_))
    ));
    handle.join().unwrap();
}

#[test]
fn subscription_receives_notifications_from_a_cloned_writer() {
    let dir = TestDir::new("subscribe");
    let endpoint = Endpoint::Path(dir.socket());
    let server = Server::bind(&endpoint).unwrap();
    let pushed = status();
    let expected = Notification::Status(pushed.clone());
    let handle = serve_once(server, move |mut conn| {
        let _: Option<Request> = conn.recv().unwrap();
        conn.send(&hello_reply(IPC_VERSION)).unwrap();
        let subscribe: Request = conn.recv().unwrap().unwrap();
        assert_eq!(subscribe, Request::Subscribe);
        conn.send(&Response::Done).unwrap();
        let mut writer = conn.try_clone_writer().unwrap();
        let pusher = thread::spawn(move || {
            writer.send(&Notification::Status(pushed)).unwrap();
        });
        pusher.join().unwrap();
        assert!(conn.recv::<Request>().unwrap().is_none());
    });

    let client = Client::connect(&endpoint).unwrap();
    let mut subscription = client.subscribe().unwrap();
    assert_eq!(subscription.next().unwrap(), Some(expected));
    drop(subscription);
    handle.join().unwrap();
}

#[test]
fn subscription_ends_when_the_daemon_closes() {
    let dir = TestDir::new("subend");
    let endpoint = Endpoint::Path(dir.socket());
    let server = Server::bind(&endpoint).unwrap();
    let handle = serve_once(server, |mut conn| {
        let _: Option<Request> = conn.recv().unwrap();
        conn.send(&hello_reply(IPC_VERSION)).unwrap();
        let _: Option<Request> = conn.recv().unwrap();
        conn.send(&Response::Done).unwrap();
    });

    let mut subscription = Client::connect(&endpoint).unwrap().subscribe().unwrap();
    assert_eq!(subscription.next().unwrap(), None);
    handle.join().unwrap();
}

#[test]
fn endpoint_without_xdg_runtime_dir_is_an_error() {
    assert!(matches!(
        Endpoint::from_runtime_dir(None),
        Err(TransportError::XdgRuntimeDirUnset)
    ));
    assert!(matches!(
        Endpoint::from_runtime_dir(Some(OsString::new())),
        Err(TransportError::XdgRuntimeDirUnset)
    ));
    assert_eq!(
        Endpoint::from_runtime_dir(Some(OsString::from("/run/user/1000"))).unwrap(),
        Endpoint::Path(PathBuf::from("/run/user/1000/open-g-hub/daemon.sock"))
    );
}

#[test]
fn error_messages_name_the_daemon() {
    assert_eq!(
        TransportError::AlreadyRunning.to_string(),
        "ogh-daemon is already running"
    );
    assert_eq!(
        TransportError::DaemonNotRunning.to_string(),
        "ogh-daemon is not running"
    );
}
