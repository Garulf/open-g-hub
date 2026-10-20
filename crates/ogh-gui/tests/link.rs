use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use ogh_gui::link::{DaemonLink, LinkEvent};
use ogh_ipc::transport::{Connection, ConnectionCloser};
use ogh_ipc::{
    AutoSwitchStatus, Client, Endpoint, ErrorKind, FocusStatus, Headset, Hello, IPC_VERSION,
    IpcError, ProfileInfo, ProfileSettings, Request, Response, Server, Setting, Status,
};
use tempfile::TempDir;

const WAIT: Duration = Duration::from_secs(5);
const RETRY: Duration = Duration::from_millis(20);

type Handler = Arc<dyn Fn(&Request) -> Response + Send + Sync>;

fn status(profile: &str) -> Status {
    Status {
        connection: ogh_ipc::Connection::Connected,
        headset: Headset::InUse,
        battery: None,
        firmware: None,
        settings: Default::default(),
        active_profile: Some(profile.to_string()),
        focus: FocusStatus {
            available: true,
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

fn profile(name: &str) -> ProfileInfo {
    ProfileInfo {
        name: name.to_string(),
        apps: Vec::new(),
        is_default: false,
        settings: ProfileSettings::default(),
    }
}

fn sidetone(percent: u8) -> Request {
    Request::Set {
        setting: Setting::Sidetone { percent },
    }
}

fn answer_done() -> Handler {
    Arc::new(|request| match request {
        Request::ListProfiles => Response::Profiles(Vec::new()),
        _ => Response::Done,
    })
}

/// A scripted daemon: answers `Hello`, sends `statuses` after `Subscribe`, answers everything
/// else through `handler`, and logs each of those requests before answering.
struct FakeDaemon {
    endpoint: Endpoint,
    requests: Receiver<Request>,
    stopped: Arc<AtomicBool>,
    closers: Arc<Mutex<Vec<ConnectionCloser>>>,
    accept: Option<JoinHandle<()>>,
}

impl FakeDaemon {
    fn start(endpoint: &Endpoint, statuses: Vec<Status>, handler: Handler) -> FakeDaemon {
        let server = Server::bind(endpoint).unwrap();
        let (log, requests) = unbounded();
        let stopped = Arc::new(AtomicBool::new(false));
        let closers = Arc::new(Mutex::new(Vec::new()));
        let accept = {
            let stopped = Arc::clone(&stopped);
            let closers = Arc::clone(&closers);
            thread::spawn(move || {
                while let Ok(connection) = server.accept() {
                    if stopped.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Some(closer) = connection.closer() {
                        closers
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push(closer);
                    }
                    let (statuses, handler, log) =
                        (statuses.clone(), Arc::clone(&handler), log.clone());
                    thread::spawn(move || serve(connection, &statuses, &*handler, &log));
                }
            })
        };
        FakeDaemon {
            endpoint: endpoint.clone(),
            requests,
            stopped,
            closers,
            accept: Some(accept),
        }
    }

    /// Ends every open connection and releases the endpoint.
    fn stop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        for closer in self
            .closers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            closer.close();
        }
        let _ = Client::connect(&self.endpoint);
        if let Some(accept) = self.accept.take() {
            accept.join().unwrap();
        }
    }

    fn next_request(&self) -> Request {
        self.requests
            .recv_timeout(WAIT)
            .expect("a request on the wire")
    }
}

fn serve(
    mut connection: Connection,
    statuses: &[Status],
    handler: &(dyn Fn(&Request) -> Response + Send + Sync),
    log: &Sender<Request>,
) {
    while let Ok(Some(request)) = connection.recv::<Request>() {
        let sent = match &request {
            Request::Hello { .. } => connection.send(&Response::Hello(Hello {
                version: IPC_VERSION,
                daemon_version: "test".to_string(),
            })),
            Request::Subscribe => {
                let mut sent = connection.send(&Response::Done);
                for status in statuses {
                    sent = sent.and_then(|()| {
                        connection.send(&ogh_ipc::Notification::Status(status.clone()))
                    });
                }
                sent
            }
            _ => {
                let _ = log.send(request.clone());
                connection.send(&handler(&request))
            }
        };
        if sent.is_err() {
            return;
        }
    }
}

struct Harness {
    link: DaemonLink,
    woken: Receiver<()>,
    seen: Vec<LinkEvent>,
    _dir: TempDir,
}

impl Harness {
    fn endpoint() -> (TempDir, Endpoint) {
        let dir = TempDir::new().unwrap();
        let endpoint = endpoint_in(&dir);
        (dir, endpoint)
    }

    /// The link starts after the daemon is listening, so no outage precedes the first status.
    fn with_daemon(statuses: Vec<Status>, handler: Handler) -> (Harness, FakeDaemon, Endpoint) {
        let (dir, endpoint) = Harness::endpoint();
        let daemon = FakeDaemon::start(&endpoint, statuses, handler);
        let harness = Harness::start(dir, &endpoint);
        (harness, daemon, endpoint)
    }

    fn start(dir: TempDir, endpoint: &Endpoint) -> Harness {
        let (wake, woken) = unbounded();
        let link = DaemonLink::start_with_retry(endpoint.clone(), RETRY, move || {
            let _ = wake.send(());
        });
        Harness {
            link,
            woken,
            seen: Vec::new(),
            _dir: dir,
        }
    }

    fn next_event(&mut self) -> LinkEvent {
        while self.seen.is_empty() {
            self.woken
                .recv_timeout(WAIT)
                .expect("the link to wake the window");
            self.seen = self.link.events();
            self.seen.reverse();
        }
        self.seen.pop().unwrap()
    }

    fn events_until(&mut self, done: impl Fn(&[LinkEvent]) -> bool) -> Vec<LinkEvent> {
        let mut events = Vec::new();
        while !done(&events) {
            events.push(self.next_event());
        }
        events
    }
}

#[cfg(unix)]
fn endpoint_in(dir: &TempDir) -> Endpoint {
    Endpoint::Path(dir.path().join("daemon.sock"))
}

#[cfg(windows)]
fn endpoint_in(_dir: &TempDir) -> Endpoint {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    Endpoint::PipeName(format!("ogh-gui-test-{}-{n}", std::process::id()))
}

fn count(events: &[LinkEvent], kind: impl Fn(&LinkEvent) -> bool) -> usize {
    events.iter().filter(|event| kind(event)).count()
}

fn is_status(event: &LinkEvent) -> bool {
    matches!(event, LinkEvent::Status(_))
}

fn is_profiles(event: &LinkEvent) -> bool {
    matches!(event, LinkEvent::Profiles(_))
}

fn is_down(event: &LinkEvent) -> bool {
    matches!(event, LinkEvent::DaemonDown(_))
}

#[test]
fn status_notifications_become_events() {
    let (mut harness, _daemon, _) =
        Harness::with_daemon(vec![status("a"), status("b")], answer_done());

    let events = harness
        .events_until(|events| count(events, is_status) == 2 && count(events, is_profiles) == 1);

    let statuses: Vec<_> = events
        .into_iter()
        .filter_map(|event| match event {
            LinkEvent::Status(status) => status.active_profile,
            _ => None,
        })
        .collect();
    assert_eq!(statuses, ["a", "b"]);
}

#[test]
fn daemon_down_is_reported_once_per_outage() {
    let (dir, endpoint) = Harness::endpoint();
    let mut harness = Harness::start(dir, &endpoint);

    let first = harness.next_event();
    assert!(is_down(&first), "{first:?}");
    thread::sleep(RETRY * 5);

    assert_eq!(harness.link.events(), Vec::new());
}

#[test]
fn reconnects_after_the_daemon_restarts() {
    let (mut harness, mut first, endpoint) = Harness::with_daemon(vec![status("a")], answer_done());
    let mut events = harness
        .events_until(|events| count(events, is_status) == 1 && count(events, is_profiles) == 1);

    first.stop();
    events.extend(harness.events_until(|events| count(events, is_down) == 1));
    let _second = FakeDaemon::start(&endpoint, vec![status("b")], answer_done());
    events.extend(
        harness.events_until(|events| {
            count(events, is_status) == 1 && count(events, is_profiles) == 1
        }),
    );

    let shape: Vec<_> = events
        .iter()
        .filter(|event| !is_profiles(event))
        .map(|event| match event {
            LinkEvent::Status(status) => status.active_profile.clone().unwrap(),
            LinkEvent::DaemonDown(_) => "down".to_string(),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(shape, ["a", "down", "b"]);
}

#[test]
fn queued_sets_of_one_kind_collapse_to_the_last() {
    let (release, gate) = unbounded::<()>();
    let held = AtomicBool::new(false);
    let handler: Handler = Arc::new(move |request| match request {
        Request::ListProfiles => Response::Profiles(Vec::new()),
        Request::Set { .. } => {
            if !held.swap(true, Ordering::SeqCst) {
                gate.recv_timeout(WAIT).expect("the gate to open");
            }
            Response::Done
        }
        _ => Response::Done,
    });
    let (mut harness, daemon, _) = Harness::with_daemon(Vec::new(), handler);
    harness.events_until(|events| count(events, is_profiles) == 1);

    harness.link.send(sidetone(1));
    while daemon.next_request() != sidetone(1) {}
    for percent in 2..=21 {
        harness.link.send(sidetone(percent));
    }
    release.send(()).unwrap();

    let mut later = Vec::new();
    loop {
        let request = daemon.next_request();
        if request == sidetone(21) {
            later.push(request);
            break;
        }
        later.push(request);
    }
    assert!(later.len() <= 2, "{later:?}");
}

#[test]
fn daemon_errors_become_request_failed() {
    let handler: Handler = Arc::new(|request| match request {
        Request::ApplyProfile { .. } => Response::Error(IpcError {
            kind: ErrorKind::NoSuchProfile,
            message: "no profile named x".to_string(),
        }),
        _ => Response::Profiles(Vec::new()),
    });
    let (mut harness, _daemon, _) = Harness::with_daemon(Vec::new(), handler);
    harness.events_until(|events| count(events, is_profiles) == 1);

    harness.link.send(Request::ApplyProfile {
        name: "x".to_string(),
    });

    assert_eq!(
        harness.next_event(),
        LinkEvent::RequestFailed("no profile named x".to_string())
    );
}

#[test]
fn profile_changes_refresh_the_list() {
    let lists = AtomicUsize::new(0);
    let handler: Handler = Arc::new(move |request| match request {
        Request::ListProfiles if lists.fetch_add(1, Ordering::SeqCst) == 0 => {
            Response::Profiles(Vec::new())
        }
        Request::ListProfiles => Response::Profiles(vec![profile("kept")]),
        _ => Response::Done,
    });
    let (mut harness, daemon, _) = Harness::with_daemon(Vec::new(), handler);
    harness.events_until(|events| count(events, is_profiles) == 1);
    assert_eq!(daemon.next_request(), Request::ListProfiles);

    harness.link.send(Request::DeleteProfile {
        name: "gone".to_string(),
    });

    assert_eq!(
        daemon.next_request(),
        Request::DeleteProfile {
            name: "gone".to_string()
        }
    );
    assert_eq!(daemon.next_request(), Request::ListProfiles);
    assert_eq!(
        harness.next_event(),
        LinkEvent::Profiles(vec![profile("kept")])
    );
}

#[test]
fn dropping_the_link_stops_its_threads_during_an_outage() {
    let (dir, endpoint) = Harness::endpoint();
    let mut harness = Harness::start(dir, &endpoint);
    let first = harness.next_event();
    assert!(is_down(&first), "{first:?}");

    let Harness { link, woken, .. } = harness;
    drop(link);

    loop {
        match woken.recv_timeout(WAIT) {
            Ok(()) => {}
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => panic!("the link threads are still running"),
        }
    }
}
