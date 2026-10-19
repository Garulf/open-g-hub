use std::collections::HashSet;
use std::mem::discriminant;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, unbounded};
use ogh_ipc::{
    Client, Endpoint, Notification, ProfileInfo, Request, Response, Status, TransportError,
};

pub const RECONNECT: Duration = Duration::from_secs(1);

const NOT_RUNNING: &str = "ogh-daemon is not running";

#[derive(Debug, Clone, PartialEq)]
pub enum LinkEvent {
    Status(Box<Status>),
    Profiles(Vec<ProfileInfo>),
    DaemonDown(String),
    RequestFailed(String),
}

/// The window's only path to the daemon. Two threads do the blocking work: one holds the
/// subscription and forwards statuses, the other sends queued requests over its own client.
/// Every event is followed by a call to `wake`.
pub struct DaemonLink {
    requests: Sender<Request>,
    events: Receiver<LinkEvent>,
    shutdown: Arc<AtomicBool>,
}

impl DaemonLink {
    pub fn start(endpoint: Endpoint, wake: impl Fn() + Send + Sync + 'static) -> DaemonLink {
        DaemonLink::start_with_retry(endpoint, RECONNECT, wake)
    }

    pub fn start_with_retry(
        endpoint: Endpoint,
        retry: Duration,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> DaemonLink {
        let (requests, queued) = unbounded();
        let (event_tx, events) = unbounded();
        let emitter = Emitter {
            events: event_tx,
            wake: Arc::new(wake),
        };
        let stale = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));

        let subscription = Subscriber {
            endpoint: endpoint.clone(),
            retry,
            emitter: emitter.clone(),
            requests: requests.clone(),
            stale: Arc::clone(&stale),
            shutdown: Arc::clone(&shutdown),
        };
        thread::spawn(move || subscription.run());
        let sender = RequestSender {
            endpoint,
            emitter,
            stale,
            client: None,
        };
        thread::spawn(move || sender.run(&queued));

        DaemonLink {
            requests,
            events,
            shutdown,
        }
    }

    pub fn send(&self, request: Request) {
        let _ = self.requests.send(request);
    }

    /// Everything that arrived since the last call; never blocks.
    pub fn events(&self) -> Vec<LinkEvent> {
        self.events.try_iter().collect()
    }
}

impl Drop for DaemonLink {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

/// Drops each `Set` that a later `Set` of the same setting supersedes. Any other request is a
/// barrier: the daemon saves the device state as it is when the save arrives, so the sets
/// before it must still be sent. Survivors keep their order.
pub fn coalesce(queue: Vec<Request>) -> Vec<Request> {
    let mut later_sets = HashSet::new();
    let mut kept: Vec<Request> = queue
        .into_iter()
        .rev()
        .filter(|request| match request {
            Request::Set { setting } => later_sets.insert(discriminant(setting)),
            _ => {
                later_sets.clear();
                true
            }
        })
        .collect();
    kept.reverse();
    kept
}

#[derive(Clone)]
struct Emitter {
    events: Sender<LinkEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Emitter {
    /// `Break` once the link has been dropped and nobody reads events any more.
    fn emit(&self, event: LinkEvent) -> ControlFlow<()> {
        if self.events.send(event).is_err() {
            return ControlFlow::Break(());
        }
        (self.wake)();
        ControlFlow::Continue(())
    }
}

struct Subscriber {
    endpoint: Endpoint,
    retry: Duration,
    emitter: Emitter,
    requests: Sender<Request>,
    stale: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
}

impl Subscriber {
    fn run(self) {
        let mut reported = false;
        while !self.shutdown.load(Ordering::SeqCst) {
            let outage = match self.stream(&mut reported) {
                ControlFlow::Break(()) => return,
                ControlFlow::Continue(outage) => outage,
            };
            self.stale.store(true, Ordering::SeqCst);
            if !reported {
                reported = true;
                if self.emitter.emit(LinkEvent::DaemonDown(outage)).is_break() {
                    return;
                }
            }
            thread::sleep(self.retry);
        }
    }

    /// Streams statuses until the connection fails; the error text is the outage's reason.
    fn stream(&self, reported: &mut bool) -> ControlFlow<(), String> {
        let mut subscription = match Client::connect(&self.endpoint).and_then(Client::subscribe) {
            Ok(subscription) => subscription,
            Err(error) => return ControlFlow::Continue(error.to_string()),
        };
        *reported = false;
        if self.requests.send(Request::ListProfiles).is_err() {
            return ControlFlow::Break(());
        }
        loop {
            match subscription.next() {
                Ok(Some(Notification::Status(status))) => {
                    self.emitter.emit(LinkEvent::Status(Box::new(status)))?;
                }
                Ok(None) => {
                    return ControlFlow::Continue("ogh-daemon closed the connection".to_string());
                }
                Err(error) => return ControlFlow::Continue(error.to_string()),
            }
        }
    }
}

struct RequestSender {
    endpoint: Endpoint,
    emitter: Emitter,
    stale: Arc<AtomicBool>,
    client: Option<Client>,
}

/// Why the rest of a batch is abandoned.
enum Stop {
    Unreachable,
    LinkDropped,
}

impl RequestSender {
    fn run(mut self, queued: &Receiver<Request>) {
        while let Ok(first) = queued.recv() {
            let mut batch = vec![first];
            batch.extend(queued.try_iter());
            for request in coalesce(batch) {
                match self.handle(request) {
                    Ok(()) => {}
                    Err(Stop::Unreachable) => break,
                    Err(Stop::LinkDropped) => return,
                }
            }
        }
    }

    fn handle(&mut self, request: Request) -> Result<(), Stop> {
        let refreshes = refreshes_profiles(&request);
        match self.call(&request)? {
            Response::Error(error) => self.report(LinkEvent::RequestFailed(error.message)),
            Response::Profiles(profiles) => self.report(LinkEvent::Profiles(profiles)),
            Response::Done if refreshes => self.handle(Request::ListProfiles),
            _ => Ok(()),
        }
    }

    /// An unreachable daemon is reported once here, and the caller drops the rest of its batch.
    fn call(&mut self, request: &Request) -> Result<Response, Stop> {
        if self.stale.swap(false, Ordering::SeqCst) {
            self.client = None;
        }
        self.exchange(request).map_err(|_| {
            match self.report(LinkEvent::RequestFailed(NOT_RUNNING.to_string())) {
                Ok(()) => Stop::Unreachable,
                Err(stop) => stop,
            }
        })
    }

    /// The client is kept only after a successful call, so a failed one reconnects next time.
    fn exchange(&mut self, request: &Request) -> Result<Response, TransportError> {
        let mut client = match self.client.take() {
            Some(client) => client,
            None => Client::connect(&self.endpoint)?,
        };
        let response = client.call(request)?;
        self.client = Some(client);
        Ok(response)
    }

    fn report(&self, event: LinkEvent) -> Result<(), Stop> {
        match self.emitter.emit(event) {
            ControlFlow::Continue(()) => Ok(()),
            ControlFlow::Break(()) => Err(Stop::LinkDropped),
        }
    }
}

fn refreshes_profiles(request: &Request) -> bool {
    matches!(
        request,
        Request::ApplyProfile { .. }
            | Request::SaveProfile { .. }
            | Request::DeleteProfile { .. }
            | Request::SetDefaultProfile { .. }
            | Request::LinkApp { .. }
            | Request::UnlinkApp { .. }
    )
}

#[cfg(test)]
mod tests {
    use ogh_ipc::{Eq, EqBand, Setting};

    use super::*;

    fn sidetone(percent: u8) -> Request {
        Request::Set {
            setting: Setting::Sidetone { percent },
        }
    }

    fn balance(value: u8) -> Request {
        Request::Set {
            setting: Setting::Balance { value },
        }
    }

    fn headphone_eq(gain_db: f32) -> Request {
        Request::Set {
            setting: Setting::HeadphoneEq(Eq {
                bands: vec![EqBand {
                    freq_hz: 100,
                    q: None,
                    gain_db,
                }],
            }),
        }
    }

    fn name(name: &str) -> String {
        name.to_string()
    }

    #[test]
    fn a_later_set_of_the_same_setting_replaces_the_earlier_one() {
        let queue = vec![sidetone(10), balance(3), sidetone(40)];
        assert_eq!(coalesce(queue), vec![balance(3), sidetone(40)]);
    }

    #[test]
    fn a_set_before_a_profile_request_is_kept() {
        let queue = vec![
            headphone_eq(1.0),
            Request::ApplyProfile { name: name("x") },
            headphone_eq(2.0),
        ];
        assert_eq!(coalesce(queue.clone()), queue);
    }

    #[test]
    fn a_set_before_a_save_is_kept_so_the_save_sees_it() {
        let queue = vec![
            sidetone(1),
            Request::SaveProfile { name: name("p") },
            sidetone(2),
        ];
        assert_eq!(coalesce(queue.clone()), queue);
    }

    #[test]
    fn sets_are_collapsed_within_each_run_between_barriers() {
        let queue = vec![
            sidetone(1),
            sidetone(2),
            Request::SaveProfile { name: name("p") },
            sidetone(3),
            sidetone(4),
        ];
        assert_eq!(
            coalesce(queue),
            vec![
                sidetone(2),
                Request::SaveProfile { name: name("p") },
                sidetone(4)
            ]
        );
    }

    #[test]
    fn an_unreachable_daemon_fails_a_batch_once() {
        let dir = tempfile::TempDir::new().unwrap();
        let (event_tx, events) = unbounded();
        let sender = RequestSender {
            endpoint: Endpoint::Path(dir.path().join("daemon.sock")),
            emitter: Emitter {
                events: event_tx,
                wake: Arc::new(|| {}),
            },
            stale: Arc::new(AtomicBool::new(false)),
            client: None,
        };
        let (requests, queued) = unbounded();
        for request in [
            Request::ApplyProfile { name: name("a") },
            Request::SaveProfile { name: name("b") },
            Request::DeleteProfile { name: name("c") },
        ] {
            requests.send(request).unwrap();
        }
        drop(requests);

        sender.run(&queued);

        let events: Vec<_> = events.try_iter().collect();
        assert_eq!(events, [LinkEvent::RequestFailed(NOT_RUNNING.to_string())]);
    }

    #[test]
    fn repeated_non_set_requests_are_never_dropped() {
        let queue = vec![
            Request::SaveProfile { name: name("a") },
            Request::SaveProfile { name: name("a") },
        ];
        assert_eq!(coalesce(queue.clone()), queue);
    }
}
