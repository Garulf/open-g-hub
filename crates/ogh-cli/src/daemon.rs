use ogh_ipc::{Client, Endpoint, Notification, Request, Response, TransportError};

pub type NotificationStream = Box<dyn Iterator<Item = Result<Notification, TransportError>>>;

/// What the commands need from the daemon, so tests can stand in for it.
pub trait Daemon {
    fn call(&mut self, request: &Request) -> Result<Response, TransportError>;

    /// Notifications until the daemon closes the connection. After an error item the stream ends.
    fn subscribe(&mut self) -> Result<NotificationStream, TransportError>;
}

/// The real daemon, connected on first use so that a missing daemon is an ordinary call error.
pub struct DaemonConnection {
    endpoint: Endpoint,
    client: Option<Client>,
}

impl DaemonConnection {
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            client: None,
        }
    }

    fn connect(&mut self) -> Result<Client, TransportError> {
        match self.client.take() {
            Some(client) => Ok(client),
            None => Client::connect(&self.endpoint),
        }
    }
}

impl Daemon for DaemonConnection {
    fn call(&mut self, request: &Request) -> Result<Response, TransportError> {
        let mut client = self.connect()?;
        let response = client.call(request)?;
        self.client = Some(client);
        Ok(response)
    }

    fn subscribe(&mut self) -> Result<NotificationStream, TransportError> {
        let subscription = self.connect()?.subscribe()?;
        Ok(Box::new(Notifications {
            subscription: Some(subscription),
        }))
    }
}

struct Notifications {
    subscription: Option<ogh_ipc::Subscription>,
}

impl Iterator for Notifications {
    type Item = Result<Notification, TransportError>;

    fn next(&mut self) -> Option<Self::Item> {
        let subscription = self.subscription.as_mut()?;
        match subscription.next() {
            Ok(Some(notification)) => Some(Ok(notification)),
            Ok(None) => {
                self.subscription = None;
                None
            }
            Err(error) => {
                self.subscription = None;
                Some(Err(error))
            }
        }
    }
}
