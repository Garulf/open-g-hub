use std::mem::discriminant;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, unbounded};
use ogh_ipc::{Client, Endpoint, Notification, ProfileInfo, Request, Response, Status};

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

        let subscription = Subscriber {
            endpoint: endpoint.clone(),
            retry,
            emitter: emitter.clone(),
            requests: requests.clone(),
            stale: Arc::clone(&stale),
        };
        thread::spawn(move || subscription.run());
        let sender = RequestSender {
            endpoint,
            emitter,
            stale,
            client: None,
        };
        thread::spawn(move || sender.run(&queued));

        DaemonLink { requests, events }
    }

    pub fn send(&self, request: Request) {
        let _ = self.requests.send(request);
    }

    /// Everything that arrived since the last call; never blocks.
    pub fn events(&self) -> Vec<LinkEvent> {
        self.events.try_iter().collect()
    }
}

/// Drops each `Set` that a later `Set` of the same setting supersedes. Nothing else is dropped
/// and the survivors keep their order.
pub fn coalesce(queue: Vec<Request>) -> Vec<Request> {
    let superseded = |index: usize, setting| {
        queue[index + 1..].iter().any(|later| match later {
            Request::Set { setting: later } => discriminant(later) == discriminant(setting),
            _ => false,
        })
    };
    let keep: Vec<bool> = queue
        .iter()
        .enumerate()
        .map(|(index, request)| match request {
            Request::Set { setting } => !superseded(index, setting),
            _ => true,
        })
        .collect();
    queue
        .into_iter()
        .zip(keep)
        .filter_map(|(request, keep)| keep.then_some(request))
        .collect()
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
}

impl Subscriber {
    fn run(self) {
        let mut reported = false;
        loop {
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

impl RequestSender {
    fn run(mut self, queued: &Receiver<Request>) {
        while let Ok(first) = queued.recv() {
            let mut batch = vec![first];
            batch.extend(queued.try_iter());
            for request in coalesce(batch) {
                if self.handle(request).is_break() {
                    return;
                }
            }
        }
    }

    fn handle(&mut self, request: Request) -> ControlFlow<()> {
        let refreshes = refreshes_profiles(&request);
        match self.call(&request)? {
            Some(Response::Error(error)) => {
                self.emitter.emit(LinkEvent::RequestFailed(error.message))
            }
            Some(Response::Profiles(profiles)) => self.emitter.emit(LinkEvent::Profiles(profiles)),
            Some(Response::Done) if refreshes => self.handle(Request::ListProfiles),
            _ => ControlFlow::Continue(()),
        }
    }

    /// `Continue(None)` when the daemon could not be reached; the failure is already reported.
    fn call(&mut self, request: &Request) -> ControlFlow<(), Option<Response>> {
        if self.stale.swap(false, Ordering::SeqCst) {
            self.client = None;
        }
        let response = self
            .client()
            .and_then(|client| client.call(request).map_err(|_| ()));
        match response {
            Ok(response) => ControlFlow::Continue(Some(response)),
            Err(()) => {
                self.client = None;
                self.emitter
                    .emit(LinkEvent::RequestFailed(NOT_RUNNING.to_string()))?;
                ControlFlow::Continue(None)
            }
        }
    }

    fn client(&mut self) -> Result<&mut Client, ()> {
        if self.client.is_none() {
            self.client = Some(Client::connect(&self.endpoint).map_err(|_| ())?);
        }
        self.client.as_mut().ok_or(())
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
    fn other_requests_between_two_sets_are_kept() {
        let queue = vec![
            headphone_eq(1.0),
            Request::ApplyProfile { name: name("x") },
            headphone_eq(2.0),
        ];
        assert_eq!(
            coalesce(queue),
            vec![Request::ApplyProfile { name: name("x") }, headphone_eq(2.0)]
        );
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
