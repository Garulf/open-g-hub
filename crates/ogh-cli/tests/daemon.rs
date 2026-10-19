#![cfg(unix)]

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use ogh_cli::daemon::{Daemon, DaemonConnection};
use ogh_ipc::{
    AutoSwitchStatus, Connection, DeviceSettings, Endpoint, FocusStatus, Headset, Hello,
    IPC_VERSION, Notification, Request, Response, Server, Status, TransportError,
};
use tempfile::TempDir;

const DEADLINE: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(200);

type Step = Box<dyn FnOnce(ogh_ipc::transport::Connection) + Send>;

fn step(f: impl FnOnce(ogh_ipc::transport::Connection) + Send + 'static) -> Step {
    Box::new(f)
}

/// Serves one scripted step per accepted connection, announcing each accept on the channel.
fn serve(endpoint: &Endpoint, steps: Vec<Step>) -> mpsc::Receiver<()> {
    let server = Server::bind(endpoint).expect("bind");
    let (accepted, accepts) = mpsc::channel();
    thread::spawn(move || {
        for step in steps {
            let connection = server.accept().expect("accept");
            let _ = accepted.send(());
            step(connection);
        }
    });
    accepts
}

fn endpoint(dir: &TempDir) -> Endpoint {
    Endpoint::Path(dir.path().join("daemon.sock"))
}

fn answer_hello(connection: &mut ogh_ipc::transport::Connection) {
    let request: Option<Request> = connection.recv().expect("hello request");
    assert!(matches!(request, Some(Request::Hello { .. })));
    connection
        .send(&Response::Hello(Hello {
            version: IPC_VERSION,
            daemon_version: "test".to_string(),
        }))
        .expect("hello reply");
}

fn expect_request(connection: &mut ogh_ipc::transport::Connection, expected: Request) {
    let request: Option<Request> = connection.recv().expect("request");
    assert_eq!(request, Some(expected));
}

fn status(headset: Headset) -> Status {
    Status {
        connection: Connection::Connected,
        headset,
        battery: None,
        firmware: None,
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

/// Runs `body` on its own thread so a hang fails the test instead of stalling it.
fn bounded<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    let (done, result) = mpsc::channel();
    thread::spawn(move || {
        let _ = done.send(body());
    });
    result.recv_timeout(DEADLINE).expect("finished in time")
}

#[test]
fn the_first_call_connects_and_nothing_before_it() {
    let dir = TempDir::new().expect("tempdir");
    let endpoint = endpoint(&dir);
    let accepts = serve(
        &endpoint,
        vec![step(|mut c| {
            answer_hello(&mut c);
            expect_request(&mut c, Request::Status);
            c.send(&Response::Done).expect("reply");
        })],
    );
    bounded(move || {
        let mut daemon = DaemonConnection::new(endpoint);
        assert!(accepts.recv_timeout(QUIET).is_err(), "connected too early");
        let reply = daemon.call(&Request::Status).expect("call");
        assert_eq!(reply, Response::Done);
        assert!(accepts.try_recv().is_ok());
    });
}

#[test]
fn the_connection_is_reused_across_calls() {
    let dir = TempDir::new().expect("tempdir");
    let endpoint = endpoint(&dir);
    let accepts = serve(
        &endpoint,
        vec![step(|mut c| {
            answer_hello(&mut c);
            for _ in 0..2 {
                expect_request(&mut c, Request::Features);
                c.send(&Response::Done).expect("reply");
            }
        })],
    );
    bounded(move || {
        let mut daemon = DaemonConnection::new(endpoint);
        for _ in 0..2 {
            assert_eq!(daemon.call(&Request::Features), Ok(Response::Done));
        }
        assert_eq!(accepts.try_iter().count(), 1);
    });
}

#[test]
fn a_call_after_a_transport_error_reconnects() {
    let dir = TempDir::new().expect("tempdir");
    let endpoint = endpoint(&dir);
    let accepts = serve(
        &endpoint,
        vec![
            step(|mut c| {
                answer_hello(&mut c);
                expect_request(&mut c, Request::Status);
            }),
            step(|mut c| {
                answer_hello(&mut c);
                expect_request(&mut c, Request::Status);
                c.send(&Response::Done).expect("reply");
            }),
        ],
    );
    bounded(move || {
        let mut daemon = DaemonConnection::new(endpoint);
        assert!(matches!(
            daemon.call(&Request::Status),
            Err(TransportError::Io(_))
        ));
        assert_eq!(daemon.call(&Request::Status), Ok(Response::Done));
        assert_eq!(accepts.try_iter().count(), 2);
    });
}

#[test]
fn no_socket_means_the_daemon_is_not_running() {
    let dir = TempDir::new().expect("tempdir");
    let endpoint = endpoint(&dir);
    bounded(move || {
        let mut daemon = DaemonConnection::new(endpoint);
        assert_eq!(
            daemon.call(&Request::Status),
            Err(TransportError::DaemonNotRunning)
        );
        assert!(matches!(
            daemon.subscribe(),
            Err(TransportError::DaemonNotRunning)
        ));
    });
}

fn subscribing(
    notifications: Vec<Notification>,
    then: fn(&mut ogh_ipc::transport::Connection),
) -> Step {
    step(move |mut c| {
        answer_hello(&mut c);
        expect_request(&mut c, Request::Subscribe);
        c.send(&Response::Done).expect("subscribed");
        for notification in &notifications {
            c.send(notification).expect("notification");
        }
        then(&mut c);
    })
}

#[test]
fn notifications_arrive_in_order_and_end_when_the_server_closes() {
    let dir = TempDir::new().expect("tempdir");
    let endpoint = endpoint(&dir);
    serve(
        &endpoint,
        vec![subscribing(
            vec![
                Notification::Status(status(Headset::InUse)),
                Notification::Status(status(Headset::Docked)),
            ],
            |_| {},
        )],
    );
    bounded(move || {
        let mut daemon = DaemonConnection::new(endpoint);
        let mut stream = daemon.subscribe().expect("subscribe");
        assert_eq!(
            stream.next(),
            Some(Ok(Notification::Status(status(Headset::InUse))))
        );
        assert_eq!(
            stream.next(),
            Some(Ok(Notification::Status(status(Headset::Docked))))
        );
        assert_eq!(stream.next(), None);
        assert_eq!(stream.next(), None);
    });
}

#[test]
fn the_stream_ends_after_yielding_an_error() {
    let dir = TempDir::new().expect("tempdir");
    let endpoint = endpoint(&dir);
    serve(
        &endpoint,
        vec![subscribing(
            vec![Notification::Status(status(Headset::InUse))],
            |c| {
                c.send(&Response::Done).expect("not a notification");
                c.send(&Notification::Status(status(Headset::Off)))
                    .expect("late notification");
            },
        )],
    );
    bounded(move || {
        let mut daemon = DaemonConnection::new(endpoint);
        let mut stream = daemon.subscribe().expect("subscribe");
        assert_eq!(
            stream.next(),
            Some(Ok(Notification::Status(status(Headset::InUse))))
        );
        assert!(matches!(stream.next(), Some(Err(TransportError::Io(_)))));
        assert_eq!(stream.next(), None);
    });
}
