use ogh_ipc::{Connection, Endpoint, ProfileInfo, Status, TransportError};

use crate::link::LinkEvent;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shared {
    pub status: Option<Status>,
    pub profiles: Vec<ProfileInfo>,
    pub daemon_down: Option<String>,
    pub last_error: Option<String>,
    /// The link is up but has reported neither a status nor an outage yet.
    pub connecting: bool,
}

impl Shared {
    /// The state before the link reports anything; an endpoint that cannot be resolved
    /// means the link never starts.
    pub fn starting(endpoint: &Result<Endpoint, TransportError>) -> Shared {
        match endpoint {
            Ok(_) => Shared {
                connecting: true,
                ..Shared::default()
            },
            Err(error) => Shared {
                daemon_down: Some(error.to_string()),
                ..Shared::default()
            },
        }
    }

    pub fn apply(&mut self, event: LinkEvent) {
        match event {
            LinkEvent::Status(status) => {
                self.status = Some(*status);
                self.daemon_down = None;
                self.connecting = false;
            }
            LinkEvent::Profiles(profiles) => self.profiles = profiles,
            LinkEvent::DaemonDown(message) => {
                self.status = None;
                self.daemon_down = Some(message);
                self.connecting = false;
            }
            LinkEvent::RequestFailed(message) => self.last_error = Some(message),
        }
    }

    pub fn can_write(&self) -> bool {
        self.status
            .as_ref()
            .is_some_and(|status| status.connection == Connection::Connected)
    }
}
